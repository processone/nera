# nera Moonshot: A Modern Erlang for the Next Decade

> *"The goal is not to replace the BEAM. The goal is to make Erlang's ideas
> accessible to people who would never install the BEAM."*

## Vision

nera is a native compiler for Erlang that produces small, fast,
self-contained binaries. It is not a reimplementation of OTP. It is a new
tool for a new set of problems — problems where Erlang's model is the
right answer but the BEAM's operational footprint is not.

The BEAM is an extraordinary runtime. It powers telecom switches, messaging
platforms, and financial systems. It will continue to do so. nera exists
for the other 90% of software — CLI tools, microservices, serverless
functions, embedded systems, WebAssembly modules — where developers choose
Go or Rust today not because the concurrency model is better, but because
the deployment story is simpler.

**nera's thesis**: Erlang's ideas (immutable data, pattern matching,
lightweight processes, let-it-crash supervision) are more valuable than
any single runtime. By compiling to native code, those ideas become
available everywhere a C binary can run.

---

## Principles

### 1. Trust is earned in small increments

Nobody will rewrite a production system in nera. People will try it for a
weekend project, a CLI tool, a Lambda function. If it works — if the binary
is small, starts fast, runs correctly, and the error messages make sense —
they will try something bigger. Trust compounds.

**Implication**: correctness and developer experience come before features.
Every release must be more reliable than the last. A compiler that handles
20 features correctly beats one that handles 200 with edge cases.

### 2. Do not carry 100% of the legacy

Erlang/OTP carries 30+ years of evolution. Some of that is wisdom. Some is
accumulated weight. nera can learn from Erlang's design without
inheriting every decision:

- **Records** are supported because real code uses them, but the long-term
  answer is maps with compile-time key checking
- **The preprocessor** (`-ifdef`, `-define`, macros) is supported for
  compatibility, but modern code should need it less
- **Distribution protocol** is explicitly out of scope — use gRPC, NATS,
  or message queues. The BEAM's distribution is tightly coupled to its
  runtime in ways that don't translate to native code
- **Hot code loading** is not a goal — containers and blue-green deploys
  have largely replaced this need outside telecom
- **Full OTP behaviours** are aspirational but not gating — a `gen_server`
  skeleton that covers 80% of use cases is better than nothing while
  waiting for 100%

### 3. Erlang syntax, modern tooling

The syntax is Erlang. No new language. Existing Erlang developers can read
and write nera code immediately. But the tooling around it should feel
like 2026, not 1998:

- One binary (`nera`) that builds, formats, tests, and runs
- Error messages that point at your code, not a stack of compiler internals
- Fast compilation — seconds, not minutes
- LSP support so every editor is a first-class citizen

### 4. Interop over isolation

The BEAM is a world unto itself. nera should be a good citizen of the
native ecosystem:

- Link with C/Rust libraries directly
- Produce libraries that C/Rust programs can call
- Use the OS thread scheduler when you need it, nera's scheduler when
  you need lightweight processes
- Read JSON, speak HTTP, open files — with the same ease as Go or Python

---

## Roadmap

### Phase 0: Foundation (current — solidifying)

*Status: substantially complete. Focus is on hardening.*

**What exists today:**
- Full Erlang parser (expressions, patterns, guards, comprehensions,
  binaries, records, funs, try/catch/after, receive/after)
- IR lowering with pattern compilation, closures, tail call optimization
- Cranelift codegen producing native binaries
- Work-stealing scheduler with preemptive yield (reduction counting)
- Per-process heaps with generational GC
- Message passing with deep copy
- Process links, monitors, registered names, process dictionary
- Timer wheel for `receive...after` timeouts
- stdlib: `lists`, `maps` (hot paths in Rust, higher-order in Erlang)
- 548 unit tests, 182 end-to-end tests

**What needs hardening:**
- Fuzz the parser and runtime (afl-rs, cargo-fuzz)
- Property-based testing for term encoding/decoding
- Stress test the scheduler under high process counts (10k+)
- Memory leak detection in long-running processes
- Audit all unsafe code in the runtime

**Exit criteria**: nera can compile and correctly run any program within
its documented feature set, without crashes, hangs, or silent
miscompilation. Users can read FEATURES.md and trust it.

---

### Phase 1: Developer Experience (months 1-4)

*The goal: make someone's first 30 minutes with nera delightful.*

#### 1.1 Error Messages That Teach

Current error messages are functional but terse. The bar is set by Rust
and Elm.

```
error[E0042]: no clause matching {error, timeout}
  --> src/client.erl:47:5
   |
47 |     case connect(Host, Port) of
   |     ^^^^ this case expression
48 |         {ok, Socket} -> send(Socket, Data);
49 |         {error, refused} -> retry(Host, Port)
   |
   = note: the value {error, timeout} does not match any branch
   = hint: add a catch-all clause: _ -> handle_error(...)
```

Deliverables:
- [ ] Source spans preserved through IR to codegen
- [ ] Colored, context-rich error output on terminal
- [ ] Warnings for common mistakes (unused variables, unreachable clauses)
- [ ] Runtime error messages include source location and term values

#### 1.2 `nera fmt` — Source Formatter

*Already on the roadmap. Promote to Phase 1 priority.*

An opinionated formatter that ends all style debates. One command, one
style, no configuration (with an escape hatch for line width).

```bash
nera fmt src/
nera fmt --check src/    # CI mode — exit 1 if unformatted
```

#### 1.3 `nera test` — Built-in Test Runner

Convention-based test discovery and execution.

```bash
nera test                      # find and run all *_test.erl
nera test --filter "ring_*"    # glob filtering
nera test --parallel 8         # concurrent test execution
```

Test files export `test_*() -> ok` functions. Assertion failures produce
readable diffs:

```
FAIL test_lookup/0 (cache_test.erl:23)

  Expected: {ok, <<"alice">>}
  Got:      {error, not_found}
```

#### 1.4 `nera new` — Project Scaffolding

```bash
nera new my_project
# Creates:
#   my_project/
#     src/
#       main.erl      # -module(main). -export([start/0]).
#     test/
#       main_test.erl
#     nera.toml       # project configuration
```

The `nera.toml` file replaces the need for rebar.config, Makefile, and
shell scripts:

```toml
[project]
name = "my_project"
entry = "main:start"

[build]
output = "my_project"

[dependencies]
# Future: hex.pm or git deps
```

#### 1.5 Watch Mode

```bash
nera watch               # rebuild on file change
nera watch --test        # rebuild + run tests on change
```

Fast feedback loops are the foundation of developer joy.

---

### Phase 2: Language Completeness (months 3-8)

*The goal: compile real-world Erlang modules without surprises.*

#### 2.1 Binary Pattern Matching (bit-level)

**Status: ✅ Done** — Bit-level binary pattern matching is fully implemented.

Currently byte-aligned only. Real Erlang code — especially protocol
parsers — depends on bit-level matching:

```erlang
parse_header(<<Version:4, IHL:4, DSCP:6, ECN:2, Rest/binary>>) ->
    {Version, IHL, DSCP, ECN, Rest}.
```

This is table stakes for anyone coming from network/telecom Erlang.

#### 2.2 Binary Comprehensions

**Status: ✅ Done** — Binary comprehensions are implemented.

```erlang
<< <<(X*2):16>> || <<X:16>> <= Bin >>.
```

Follows naturally from list comprehension support + binary pattern work.

#### 2.3 Maps Comprehensions (EEP-58)

```erlang
#{K => V*2 || K := V <- Map}.
```

Modern Erlang feature. Supporting it signals that nera tracks the
language, not just legacy.

#### 2.4 Binary Strings by Default

*Design doc: STRING_MIGRATION.md*

Three-phase migration:
1. `--binary-strings` flag + `-compile(binary_strings)` pragma
2. `~"interpolation syntax: ~p"` (always binary)
3. Binary strings by default (opt out with `-compile(list_strings)`)

This is a genuine improvement over standard Erlang. Binaries are compact,
fast, and what most programs actually want.

#### 2.5 Maybe Expression (EEP-49)

```erlang
maybe
    {ok, A} ?= fetch(X),
    {ok, B} ?= fetch(Y),
    {ok, A + B}
end
```

Adopted in OTP 25. Modern Erlang code will increasingly use this.

#### 2.6 Nominal Types for Records

Records are tuples with names. This works but loses type safety. Without
adding a full type system, nera can enforce that record tags are checked
at compile time:

```
error[E0088]: record type mismatch
  --> src/db.erl:31:12
   |
31 |     User#session.token
   |          ^^^^^^^ expected #user{}, found #session{}
```

---

### Phase 3: The Type Story (months 6-14)

*The goal: optional, gradual, useful types — not an academic exercise.*

This is the single most impactful feature nera could offer. The approach
is pragmatic, not purist.

#### 3.1 Spec Enforcement

Today, `-spec` declarations are parsed and ignored. Step one is to check
them:

```erlang
-spec add(integer(), integer()) -> integer().
add(A, B) -> A + B.

-spec double(integer()) -> integer().
double(X) -> add(X, "hello").  %% ERROR: "hello" is not integer()
```

This alone — making `-spec` mean something — would differentiate nera
from every other Erlang toolchain. Dialyzer finds bugs post-hoc and
slowly. nera can find them at compile time, fast.

#### 3.2 Local Type Inference

Within a function, infer types from usage without requiring annotations:

```erlang
foo(X) ->
    Y = X + 1,        %% X :: number(), Y :: number()
    Z = atom_to_list(Y).  %% ERROR: atom_to_list expects atom(), got number()
```

No `-spec` required for local variables. The inference is strictly
intra-function — no whole-program analysis, no fixpoints, no long
compile times.

#### 3.3 Typed Maps (Lightweight Structs)

```erlang
-type user() :: #{
    name := binary(),
    age := integer(),
    email => binary()     %% optional
}.

-spec create_user(binary(), integer()) -> user().
create_user(Name, Age) ->
    #{name => Name, age => Age}.
```

Maps with type annotations checked at compile time give the ergonomics of
structs without adding a new data type. Required keys (`:=`) vs optional
keys (`=>`) are already in the syntax.

#### 3.4 Union Narrowing in Case/Receive

```erlang
-spec handle(ok | {error, term()}) -> integer().
handle(ok) -> 0;
handle({error, Reason}) ->
    %% Reason :: term() — narrowed from the union
    log(Reason),
    1.
```

Pattern matching already narrows types. Making the compiler aware of this
produces better error messages and catches impossible branches.

#### 3.5 Non-Goals for Types

- **No full Hindley-Milner**: Erlang's semantics (dynamic dispatch, hot
  code loading history, message passing of arbitrary terms) make global
  inference impractical and arguably undesirable
- **No mandatory annotations**: types are always optional. Unannotated
  code compiles and runs exactly as before
- **No breaking changes**: all existing valid Erlang remains valid

---

### Phase 4: Concurrency Hardening (months 4-10)

*The goal: nera processes are reliable enough for production services.*

#### 4.1 Supervision Trees

A minimal but correct supervisor implementation:

```erlang
-module(my_sup).
-behaviour(supervisor).

init([]) ->
    {ok, {{one_for_one, 5, 10}, [
        {worker1, {worker, start_link, []}, permanent, 5000, worker, [worker]}
    ]}}.
```

This requires:
- `gen_server` (basic message loop + call/cast/reply)
- `supervisor` (child specs, restart strategies: one_for_one,
  one_for_all, rest_for_one)
- `proc_lib` (proper process init)
- `sys` (debug/trace hooks — can be stubs initially)

The implementation lives in Erlang (Tier 3 stdlib), not Rust. This is
important — it proves that nera can compile OTP-style code, not just
toy examples.

#### 4.2 Selective Receive

**Status: ✅ Done** — Selective receive is fully implemented using a three-BIF protocol (receive_peek, receive_next, receive_accept). gen_server call/reply patterns work correctly.

#### 4.3 Process Priority

Three levels: `high`, `normal`, `low`. High-priority processes get
scheduled before normal; low-priority only when no normal work is
queued. Needed for supervisor responsiveness.

#### 4.4 System Limits and Backpressure

- Configurable max process count
- Mailbox size warnings/limits
- Memory high-water mark with GC pressure

These are operational necessities. A service that silently grows to 10GB
before OOM-killing is not trustworthy.

#### 4.5 ETS (Erlang Term Storage)

**Status: ✅ Done** — ETS is fully implemented with HashMap-backed tables, mmap-backed region pools, and comprehensive API support.

Shared mutable storage accessible by all processes. Critical for caches,
counters, and configuration. Implementation in Rust using concurrent
hash maps.

The API surface is large. Start with:
- `ets:new/2`, `ets:insert/2`, `ets:lookup/2`, `ets:delete/2`
- `set` and `ordered_set` types
- `public`, `protected`, `private` access
- `ets:match/2`, `ets:select/2` later

---

### Phase 5: Ecosystem & Interop (months 8-16)

*The goal: nera programs can use libraries and talk to the world.*

#### 5.1 Hex.pm Dependency Resolution

```toml
[dependencies]
jiffy = "1.1.1"       # Pure Erlang deps only (no NIFs)
cowlib = "2.12.1"
```

`nera build` downloads, compiles, and links pure-Erlang hex packages.
NIF-based packages require an explicit FFI bridge (see 5.3).

Constraints:
- Only pure Erlang packages (no C NIFs, no rebar plugins)
- No support for parse transforms (use nera's own extensions)
- Dependency resolution via a bundled solver (PubGrub algorithm)

This is high leverage. The hex.pm ecosystem has thousands of pure-Erlang
libraries for JSON, HTTP, crypto, templating, etc.

#### 5.2 C FFI (Explicit, Safe)

```erlang
-nif([
    {compress, 2, "zlib_compress", [{binary, integer}, binary]},
    {decompress, 1, "zlib_decompress", [{binary}, binary]}
]).
```

*Design doc: NERA_NIF_DESIGN.md*

Declare external C functions with types. nera generates the
marshaling code. No `erl_nif.h`, no NIF boilerplate — just a C function
and a declaration.

#### 5.3 Rust FFI (Zero-Cost)

For Rust libraries, the FFI can be tighter:

```toml
[dependencies.native]
serde_json = "1.0"    # Rust crate, linked directly
```

With a thin wrapper crate that exposes `extern "C"` functions, nera can
link Rust libraries with no overhead. This opens access to the entire
crates.io ecosystem for performance-critical operations.

#### 5.4 HTTP Client & Server

A built-in HTTP stack (Tier 2, Rust implementation):

```erlang
{ok, #{status := 200, body := Body}} =
    httpc:request(get, "https://api.example.com/data", []).
```

And a simple server:

```erlang
httpd:start(8080, fun(Req) ->
    #{method := <<"GET">>, path := <<"/health">>} = Req,
    {200, #{}, <<"ok">>}
end).
```

Erlang developers expect `httpc`/`httpd` to exist. Building on hyper
(Rust) gives us HTTP/2 and TLS for free.

#### 5.5 JSON, Base64, Crypto (Tier 2 Stdlib)

*Design doc: STDLIB_DESIGN.md*

These are wrappers around proven Rust crates:

| Module   | Rust crate    | Priority |
|----------|---------------|----------|
| json     | serde_json    | P0       |
| base64   | base64        | P0       |
| re       | regex         | P1       |
| crypto   | ring          | P1       |
| rand     | fastrand      | P1       |
| unicode  | unicode-*     | P2       |
| zlib     | flate2        | P2       |

Each module has a clean Erlang API. The Rust implementation is invisible
to the user.

---

### Phase 6: Platform Targets (months 10-18)

*The goal: nera binaries run everywhere.*

#### 6.1 Cross-Compilation Matrix

```bash
nera build --target x86_64-linux-musl    # static Linux binary
nera build --target aarch64-linux-musl   # ARM64 Linux (Raspberry Pi, AWS Graviton)
nera build --target x86_64-macos         # Intel Mac
nera build --target aarch64-macos        # Apple Silicon (current default)
nera build --target x86_64-windows-msvc  # Windows
```

Cranelift supports these targets. The work is in the linker
configuration, runtime platform abstraction, and CI matrix.

Static musl binaries are the priority — they run on any Linux without
glibc version issues, perfect for containers.

#### 6.2 Minimal Docker Images

```dockerfile
FROM scratch
COPY my_service /my_service
ENTRYPOINT ["/my_service"]
```

A statically-linked nera binary in a `scratch` container. No OS, no
runtime, no dependencies. Image size: 5-15 MB.

This is a concrete, measurable advantage over BEAM deployment.

#### 6.3 WebAssembly (WASI)

```bash
nera build --target wasm32-wasi -o handler.wasm
```

Cranelift has a WASM backend. The runtime needs adaptation:
- Single-threaded scheduler (WASM has no threads yet — threads proposal
  is still maturing)
- WASI for I/O instead of POSIX
- No process spawning beyond cooperative green threads

Use cases:
- Edge computing (Cloudflare Workers, Fastly Compute)
- Browser-based Erlang playgrounds
- Plugin systems (load Erlang logic into any WASM host)

This is genuinely novel. No other tool compiles Erlang to WASM.

#### 6.4 AWS Lambda / Serverless

```bash
nera build --target x86_64-linux-musl -o bootstrap
zip function.zip bootstrap
aws lambda create-function --runtime provided.al2023 ...
```

Target, not measured: Erlang in Lambda with a cold start under 10 ms. Today
only a local demo exists; no Lambda runtime is qualified, and no Lambda cold
start or cost has been measured.

*Status and qualification steps: [SERVERLESS_ERLANG.md](SERVERLESS_ERLANG.md)*

---

### Phase 7: The Editor Story (months 12-20)

*The goal: nera feels like a modern language in your editor.*

#### 7.1 Language Server Protocol (LSP)

`nera lsp` starts a language server providing:

- **Go to definition** (functions, records, macros)
- **Find references**
- **Hover information** (types, specs, docs)
- **Inline diagnostics** (errors, warnings, type mismatches)
- **Completions** (module functions, record fields, variable names)
- **Format on save** (delegates to `nera fmt`)

The LSP is built into the `nera` binary. No separate installation. Works
with VS Code, Neovim, Emacs, Helix, Zed — anything that speaks LSP.

#### 7.2 VS Code Extension

A first-party extension wrapping the LSP:

- Syntax highlighting (TextMate grammar)
- Inline error display
- Test runner integration (run test at cursor)
- Debug adapter protocol (DAP) for future debugger

#### 7.3 REPL

```bash
nera repl
> 1 + 2.
3
> lists:seq(1, 5).
[1,2,3,4,5]
> F = fun(X) -> X * X end.
#Fun<...>
> lists:map(F, lists:seq(1, 5)).
[1,4,9,16,25]
```

An interactive shell that compiles expressions to native code on the fly
using Cranelift's JIT mode. Essential for exploration and learning.

---

### Phase 8: Showcase & Community (ongoing)

*The goal: people see nera solving real problems.*

#### 8.1 Reference Applications

| Application      | LOC   | Demonstrates                           |
|------------------|-------|----------------------------------------|
| `kv`             | ~100  | gen_server, ETS, supervision           |
| `chat`           | ~300  | TCP, processes, message routing        |
| `httpbin`        | ~400  | HTTP server, JSON, routing             |
| `mini_irc`       | ~500  | Full IRC protocol, channels, nicks     |
| `build_tool`     | ~800  | File I/O, process pool, dependency DAG |

Each application is documented, tested, and demonstrates a core
nera capability. They serve as both validation and tutorials.

#### 8.2 Benchmarks

Publish honest, reproducible benchmarks comparing:

- **Startup time**: nera vs BEAM vs Go vs Rust
- **Binary size**: nera vs BEAM release vs Go
- **Throughput**: message passing, JSON parsing, HTTP handling
- **Memory**: per-process overhead, GC pause times

Do not cherry-pick. Show where nera wins (startup, binary size) and
where the BEAM wins (sustained throughput, massive process counts). Honesty
builds trust faster than marketing.

#### 8.3 Website & Documentation

- **nera.dev** (or similar) — landing page, getting started guide,
  feature comparison
- **Interactive playground** — WASM-based (Phase 6.3), try nera in
  the browser
- **Cookbook** — recipes for common tasks (HTTP client, JSON parsing,
  file processing, TCP server, CLI argument parsing)
- **Migration guide** — "Porting Erlang to nera: what works, what
  doesn't, what's different"

#### 8.4 Community Building

- GitHub Discussions for questions and ideas
- Monthly progress reports (blog posts or GitHub releases)
- Conference talks (Code BEAM, ElixirConf, RustConf crossover)
- Encourage contributions with "good first issue" labels and
  contributor guide

---

## What We Explicitly Do NOT Build

Clarity about scope is as important as the roadmap itself.

| Feature                | Why not                                           |
|------------------------|---------------------------------------------------|
| Hot code loading       | Containers and deploys have replaced this need    |
| Distribution protocol  | Use gRPC/NATS/MQTT — protocol-agnostic is better  |
| Full OTP application   | Application controller is BEAM-specific machinery |
| NIFs (BEAM-compatible) | Different FFI model — see nera NIF design         |
| Ports                  | Use FFI or OS pipes directly                      |
| BEAM file loading      | We compile from source, not bytecode              |
| Mnesia                 | Use SQLite, Postgres, or ETS for local state      |
| wx / GUI toolkit       | Out of scope entirely                             |
| Elixir compilation     | Elixir's macro system requires the BEAM compiler  |
| Parse transforms       | Compile-time metaprogramming via other means      |
| Jinterface / C nodes   | Distribution is out of scope                      |

These decisions are not permanent. They reflect the belief that doing
fewer things well builds more trust than doing many things poorly.

---

## Success Metrics

How we know nera is on track:

### Year 1
- [ ] 10 people have built something real with nera (CLI tools, services)
- [x] All documented features pass a comprehensive test suite (548 unit + 182 e2e tests)
- [ ] Binary size under 5 MB for a hello-world HTTP server
- [ ] Cold start under 5 ms
- [ ] `nera build` time under 2 seconds for 1000-line programs
- [ ] Zero known miscompilation bugs open for more than 1 week

### Year 2
- [ ] 100+ GitHub stars (community interest signal)
- [ ] At least one external contributor beyond the core team
- [x] Working `gen_server` and `supervisor`
- [ ] Type checking catches real bugs in test programs
- [ ] LSP provides go-to-definition and inline errors
- [ ] Cross-compilation to Linux from macOS works

### Year 3
- [ ] Used in at least one production system (even a small one)
- [ ] WASM target produces working programs
- [ ] hex.pm pure-Erlang packages compile
- [ ] Community-contributed stdlib modules
- [ ] Conference talk or publication

---

## The Competitive Landscape

Understanding where nera sits relative to alternatives:

| Tool         | Strengths                           | nera advantage                     |
|--------------|-------------------------------------|-------------------------------------|
| BEAM/OTP     | Battle-tested, full OTP, hot reload | Binary size, startup, deployment    |
| Gleam        | Type safety, BEAM + JS targets      | Erlang syntax, existing code compat |
| Go           | Simple, fast compile, great stdlib  | Pattern matching, fault tolerance   |
| Rust          | Performance, safety, ecosystem     | Higher-level concurrency, simpler   |
| Elixir/BEAM  | Productivity, Phoenix, community   | Native compilation, no VM overhead  |
| LFE          | Lisp on BEAM, macros               | Native binaries, broader appeal     |
| Caramel      | OCaml to BEAM                      | Source-level Erlang compatibility   |

nera occupies a unique position: **Erlang's semantics with Go's
deployment model**. No other tool offers this combination.

---

## Timeline Summary

```
            Year 1                    Year 2                   Year 3
    ┌─────────────────────┐   ┌─────────────────────┐   ┌──────────────────┐
    │ Phase 0: Hardening  │   │ Phase 4: Concurrency │   │ Phase 7: Editor  │
    │ Phase 1: DevEx      │   │ Phase 5: Ecosystem   │   │ Phase 8: Commun. │
    │ Phase 2: Language    │   │ Phase 6: Platforms   │   │                  │
    │ Phase 3: Types start │   │ Phase 3: Types cont. │   │                  │
    └─────────────────────┘   └─────────────────────┘   └──────────────────┘

    Focus: trust &             Focus: power &             Focus: reach &
    correctness                capability                 community
```

Phases overlap. Work proceeds on multiple fronts. But the priority is
always: **make what exists today more reliable before adding what's
new.**

---

## Closing Thought

The Erlang community does not need saving. It needs expansion. The ideas
in Erlang — process isolation, pattern matching, immutable data, fault
tolerance — are as relevant today as they were in 1986. What's changed is
that developers now expect a single binary, fast startup, small footprint,
and modern tooling.

nera is the bridge. Not a replacement for the BEAM, but a way to bring
Erlang's ideas to places the BEAM cannot or will not go. If we build it
with care, with honesty about what works and what doesn't, and with
respect for the decades of engineering that came before — it could become
the tool that introduces a new generation to Erlang's way of thinking.

That would be the real moonshot.
