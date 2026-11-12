# EEP-Informed Roadmap — Nera Post-v1 Strategy

**Historical analysis (April 2026), not a verified release checklist.** Use
[PROJECT_STATUS.md](PROJECT_STATUS.md) for the October baseline. In particular,
the original `gen_server` timeout probe stopped at an exception-resumption
defect. After that repair, the probe confirms late mailbox replies in Nera
that BEAM suppresses. The remaining alias gap is tracked in project status.

> A 12–18 month strategic outlook derived from cross-referencing all Final and
> Accepted Erlang Enhancement Proposals (R12B through OTP 29) against Nera's
> current feature coverage.

## TL;DR

- **Nera is compatible with modern OTP syntax on the critical path.** `try/catch Class:Reason:Stacktrace` (EEP-47), `maybe ... end` (EEP-49), named funs (EEP-37), `?FUNCTION_NAME/ARITY` (EEP-45), and maps (EEP-43) are all in place. Real-world libraries written against OTP 21+ should parse.
- **One latent correctness issue is worth pausing for**: [A1 — EEP-53 Process Aliases](#a1-eep-53-process-aliases--correctness-gap). Without them, every `gen_server:call/3` with a timeout carries a race that can corrupt the next `receive` in the caller.
- **Two stdlib gaps block v1.0 adoption**: [A2 — `re`](#a2-eep-11-re-module--stdlib-dealbreaker) and [A3 — `json`](#a3-eep-68-json-module--stdlib-dealbreaker). Both are already planned as Tier 2 extensions in [STDLIB_DESIGN.md](STDLIB_DESIGN.md); EEP history tells us exactly how to build them.
- **A batch of cheap syntax catchups** ([Section B](#section-b--syntax-catchup-for-v10)) brings Nera to OTP 29 parity in a single work cycle.
- **Nera deliberately diverges from EEP-79 (native records)** in favor of the [Moonshot](MOONSHOT.md) direction: maps with compile-time key checking. This is not a gap, it's a choice.
- **Phase 6/7 reactor work continues**. Only A1 justifies pausing; everything else sequences *after* the current reactor optimization cycle.

See [Section 7 — Phase 6/7 impact](#section-7--phase-67-impact) for the concrete sequence.

---

## Why this document exists

Erlang has evolved through ~80 EEPs since R12B. Some are syntax sugar, some are runtime semantics, some are stdlib fill-ins, and a few are correctness-critical features that masquerade as conveniences. Nera has implemented many of them organically while building out the language, but it hasn't systematically checked what it's missing, and the gaps matter differently depending on category:

- A missing **syntax feature** (EEP-58 map comprehensions) breaks parsing of code that uses it. Mechanical to fix.
- A missing **stdlib module** (EEP-68 `json`, EEP-11 `re`) forces users to write their own, which nobody will do for a weekend project.
- A missing **runtime feature** (EEP-53 process aliases) can silently corrupt program state without any parse error or crash — just occasionally-wrong behavior.

This document inventories those gaps, proposes a sequence for closing them, and names the EEPs Nera should deliberately *not* adopt. It complements rather than replaces:

- [MOONSHOT.md](MOONSHOT.md) — vision and non-goals (hot code loading, distributed Erlang)
- [STDLIB_DESIGN.md](STDLIB_DESIGN.md) — three-tier stdlib architecture
- [ROADMAP.md](../ROADMAP.md) — current P0/P1/P2 diagnostic work
- [FLAGSHIP_ROADMAP.md](FLAGSHIP_ROADMAP.md) — feature-level roadmap

---

## Meta-lessons from EEP history

Before cataloging gaps, five patterns from the EEP process that should shape how Nera evolves:

### 1. Scheduler-aware BIFs are non-negotiable

EEP-11 (PCRE, R12B-3) is explicit: *"the BIF stops execution after a certain number of iterations to avoid stalling the scheduler — saving the state so execution can resume when the process is rescheduled."* This is the canonical pattern for every Tier 2 Rust extension. Nera's [STDLIB_DESIGN.md](STDLIB_DESIGN.md) already echoes it ("bounded work per call, no blocking on scheduler threads, proper reduction accounting"), but the lesson is to *verify*, not just document. Every BIF that could plausibly run for more than a few thousand reductions on realistic input is a potential fairness bug.

### 2. The type system is a 15-year arc

EEP-8 `-spec`/`-type` (R13B03, 2009) → EEP-61 `dynamic()` (OTP 26, 2023) → EEP-69 nominal types (OTP 28, 2024). Fifteen years, three EEPs, each adding one capability layer on top of the previous. The trajectory is: *accept syntax → build inference → add escape hatches → tighten nominal identity*. Nera currently parses `-spec` and `-type` but discards them in IR lowering. A v1.0 decision is due: commit to the arc, or defer explicitly.

### 3. OTP 21 (2018) is the modern compatibility baseline

EEP-47 (catch stacktrace pattern) is *the* test for whether a library targets the last five years. Nera already passes this test. Anything written since ~2019 that touches error handling uses `catch C:R:S -> ...`, and that code compiles today on Nera.

### 4. Syntax evolution accelerated in OTP 25–29

`maybe` (25), map comprehensions (26), sigils and triple-quoted strings (27), strict/zip generators (28), multi-valued and assigning comprehensions (29), native records (29). This is roughly one syntax EEP per release, not one per decade. The implication for Nera: treat the EEP syntax list as a moving compatibility target, not a one-time checklist.

### 5. Stdlib gaps eventually force official inclusion

Erlang lived without JSON in the standard library for ~20 years. Third-party libraries filled the gap (`jsx`, `jiffy`, `jsone`, ...) but none became canonical. OTP 27 (2024) finally shipped `json` (EEP-68) because the fragmentation was causing real pain. Lesson for Nera: don't wait. If a module is obviously going to be needed by 90% of users (`json`, `re`, `crypto`, `string`, `file`), ship it pre-v1 even if the implementation is a thin wrapper around a Rust crate.

### 6. Process aliases are a correctness feature, not a convenience

This one deserves its own lesson because it's easy to misread EEP-53. The proposal reads like a minor ergonomic improvement ("lightweight send destination that auto-deactivates"), but the underlying bug it fixes is severe. The classic `gen_server:call` pattern before EEP-53:

```erlang
call(Server, Req, Timeout) ->
    Ref = make_ref(),
    Server ! {'$call', self(), Ref, Req},
    receive
        {Ref, Reply} -> Reply
    after Timeout -> exit(timeout)
    end.
```

If `Server` replies slowly and the caller hits the timeout, the `Ref`-tagged reply arrives in the caller's mailbox *after* the caller has moved on. The next unrelated `receive` that happens to match `{Ref, _}` (or worse, the next `receive` that scans past this orphan message) pays a cost, and selective receive in Erlang is linear in mailbox depth. With enough timeouts under load, the caller's mailbox fills with stale replies and `receive` performance collapses. EEP-53 fixes this at the runtime level: a deactivated alias causes the mailbox to drop the stale message on arrival.

Any BEAM-compatible runtime without process aliases ships this bug. Nera currently does.

---

## Verified coverage table

Status flags based on direct source-code reads of [crates/nera-syntax](../crates/nera-syntax), [crates/nera-ir](../crates/nera-ir), and [crates/nera-runtime](../crates/nera-runtime) as of 2026-04-15:

- ✓ — implemented
- ⚠ — partial or parser-only
- ✗ — not implemented
- ⊘ — deliberately declined

| EEP | OTP | Feature | Status | Notes |
|-----|-----|---------|--------|-------|
| 4   | R12B | Bit-level binaries | ✓ | Full support, incl. pattern matching |
| 6   | R12B | `tuple_size`/`byte_size` guard BIFs | ✓ | |
| 8   | R13B03 | `-spec`/`-type`/`-opaque`/`-callback` | ⚠ | Parser accepts, IR discards |
| 11  | R12B-3 | `re` module (PCRE) | ✗ | Planned as Tier 2 in STDLIB_DESIGN.md |
| 23  | R12B-4 | Variables in `fun M:F/A` | ✓ | |
| 24  | R12B-5 | F/N syntax in all attributes | ✓ | |
| 26  | R13A | Tail-recursive `andalso`/`orelse` | ✓ | |
| 30  | R13A | `erlang:max/2`, `min/2` as guard BIFs | ✓ | |
| 31  | R14A | `binary` module | ⚠ | Has `split`/`part`/`at`/`copy`; missing `match`/`replace` |
| 36  | R15B | Line numbers in exceptions | ✓ | |
| 37  | R15B | Named funs `fun Name(...) -> ... end` | ✓ | `parse_named_fun_clauses` in parser |
| 43  | 17 | Maps | ✓ | HAMT representation |
| 44  | 19 | `-warning(Text)`/`-error(Text)` directives | ✗ | |
| 45  | 19 | `?FUNCTION_NAME`/`?FUNCTION_ARITY` macros | ✓ | |
| 47  | 21 | `catch Class:Reason:Stacktrace` | ✓ | Tested; constraint check (stacktrace not previously bound) is a pending P1 |
| 48  | 24 | Docs chunk storage | ⊘ | Nera has no .beam chunks; see C4 below |
| 49  | 25 | `maybe ... end` + `?=` | ✓ | `parse_maybe_body`, 4 tests |
| 50  | 24 | Maps-based `sets` module | ✗ | |
| 51  | 23 | Digit separators `1_000_000` | ✗ | |
| 52  | 23 | Expression keys in map/binary patterns | ? | Needs verification in pattern compiler |
| 53  | 24 | **Process aliases** | ✗ | **A1 — correctness gap** |
| 54  | 24 | BIF `error_info` metadata | ~ | Nera has rich diagnostics via codespan-reporting; may not match OTP's machine-readable BIF error format |
| 56  | 24 | Significant children in supervisor | ✗ | |
| 58  | 26 | Map comprehensions `#{K => V \|\| K := V <- M}` | ✗ | |
| 59  | 27 | `-doc`/`-moduledoc` attributes | ✗ | See C4 |
| 61  | 26 | `dynamic()` built-in type | ✗ | Blocked on type system (see C1) |
| 64  | 27 | Triple-quoted strings | ✗ | |
| 66  | 27 | Sigils `~"..."`, `~b"..."`, etc. | ✗ | |
| 68  | 27 | `json` module | ✗ | **A3 — stdlib dealbreaker** |
| 69  | 28 | Nominal types | ✗ | Blocked on type system |
| 70  | 28 | Strict generators | ✗ | |
| 73  | 28 | Zip generators `&&` | ✗ | |
| 76  | 28 | Priority messages | ✗ | Runtime mailbox change; see C2 |
| 77  | 29 | Assignment in comprehensions | ✗ | |
| 78  | 29 | Multi-valued comprehensions | ✗ | |
| 79  | 29 | Native records | ⊘ | Nera diverges toward maps-with-compile-time-key-checking; see C3 |

---

## Section A — Urgent gaps

These three items are the only EEP-derived work that warrants consideration against the current Phase 6/7 reactor optimization timeline.

### A1. EEP-53 Process Aliases — correctness gap

**The bug in plain English.** Every `gen_server:call(Pid, Msg, Timeout)` in Nera today leaks a reply message into the caller's mailbox if the server responds after the timeout. Under load with slow servers, the caller's mailbox fills with stale `{Ref, Reply}` tuples, and subsequent `receive` expressions — even unrelated ones — pay O(mailbox-depth) per match attempt.

**Why EEP-53 fixes it.** A process alias is a reference that can be used as a one-shot send destination. When the caller times out, it deactivates the alias, and the mailbox drops any subsequent send to that alias *at arrival time* rather than enqueueing it.

**Implementation sketch.**
- New term kind in [term.rs](../crates/nera-runtime/src/term.rs): `AliasRef { pid: Pid, id: u64 }`, tagged like an existing `Ref` but carrying an alias id.
- Alias table on each process (small `SmallVec<u64, 4>` of active alias ids): activate on creation, deactivate on explicit unalias or on process exit.
- Mailbox send path in [process.rs](../crates/nera-runtime/src/process.rs) and [scheduler.rs](../crates/nera-runtime/src/scheduler.rs): before enqueueing to a process targeted by an alias, check the alias table. Drop silently if inactive.
- BIFs in [ffi/apply.rs](../crates/nera-runtime/src/ffi/apply.rs): `erlang:alias/0`, `erlang:alias/1`, `erlang:unalias/1`, `erlang:monitor/3` with the `{alias, reply_demonitor}` option.
- [erlang/stdlib/gen_server.erl](../erlang/stdlib/gen_server.erl): rewrite `call/3` to use `monitor(process, Pid, [{alias, reply_demonitor}])` instead of a bare `make_ref()`.

**Verification.** Write a test: spawn a server that takes 500 ms to reply, call it with a 100 ms timeout in a loop, and after the loop issue an unrelated `receive after 0`. Pre-fix: the receive is slow (or sees stale messages). Post-fix: the receive is fast and the mailbox is empty.

**Timing recommendation.** Land *after* the current Phase 7 park-duration instrumentation task completes — not mid-task, to avoid confusing reactor profiling results with an unrelated correctness fix. The correctness fix itself is expected to have negligible hot-path impact (one extra table check per send), so it does not conflict with the ongoing optimization goals.

### A2. EEP-11 `re` module — stdlib dealbreaker

**Why it matters.** Any non-trivial Erlang code uses regex. Without `re`, Nera cannot compile most HTTP routers, log parsers, config loaders, or text-processing utilities. This is a v1.0 blocker, not a v2 nice-to-have.

**Implementation sketch.**
- New Tier 2 file `crates/nera-runtime/src/ffi/re.rs`.
- Wrap the Rust `regex` crate. Maintain compiled pattern cache indexed by pattern binary (avoid recompiling on each call).
- **Scheduler-aware iteration** (the EEP-11 lesson): for long inputs, match in bounded chunks, saving the iterator state between yields. The `regex` crate doesn't expose pausable matching natively, so the first cut can be a reduction budget check *before* each match and a yield-via-trap if the budget is insufficient.
- Register BIFs in [ffi/apply.rs](../crates/nera-runtime/src/ffi/apply.rs) for the `re` namespace: `compile`, `run`, `replace`, `split`.

**Timing.** After A1 and after Phase 7 reactor work completes. Must land before v1.0 tag.

### A3. EEP-68 `json` module — stdlib dealbreaker

**Why it matters.** Nera's [MOONSHOT.md](MOONSHOT.md) positions it explicitly for "CLI tools, microservices, serverless functions" — the exact categories where JSON is a baseline expectation. Without `json`, Nera cannot host a single-file Lambda handler without pulling a third-party library the user has to compile themselves.

**Implementation sketch.**
- New Tier 2 file `crates/nera-runtime/src/ffi/json.rs`.
- Use `serde_json` with a custom `Serialize`/`Deserialize` bridge for Erlang terms. Map: atoms ↔ strings (with a convention for `true`/`false`/`null`), maps ↔ objects, lists ↔ arrays, binaries ↔ strings.
- Expose the EEP-68 API surface: `json:encode/1`, `json:decode/1`, and the advanced callback-based API (`json:encode/2`, `json:decode_continue/2`) for streaming.
- Document the atom↔string convention explicitly; it's the only place Nera's `json` can silently disagree with OTP's.

**Timing.** After A2. Must land before v1.0 tag.

---

## Section B — Syntax catchup for v1.0

Compile-time mechanical work that brings Nera from "OTP 25-compatible" to "OTP 29-compatible." Can be batched as a single "modern syntax" work package after Phase 7 reactor work concludes.

| Tag | EEP | OTP | Feature | Primary files |
|-----|-----|-----|---------|---------------|
| B1  | 51 | 23 | Digit separators `1_000_000` | [lexer](../crates/nera-syntax/src/lexer.rs) / token regex |
| B2  | 58 | 26 | Map comprehensions | [parser.rs](../crates/nera-syntax/src/parser.rs), [nera-ir/src/lower/mod.rs](../crates/nera-ir/src/lower/mod.rs) |
| B3  | 52 | 23 | Expression keys in map/binary patterns | [nera-ir/src/pattern.rs](../crates/nera-ir/src/pattern.rs) (verify first) |
| B4  | 64 | 27 | Triple-quoted strings | lexer + indentation stripping |
| B5  | 66 | 27 | Sigils `~"..."`, `~b"..."` | lexer + term factory |
| B6  | 70 | 28 | Strict generators `<:-` | parser + lowering |
| B7  | 73 | 28 | Zip generators `&&` | parser + lowering |
| B8  | 77 | 29 | Assignment in comprehensions | parser |
| B9  | 78 | 29 | Multi-valued comprehensions | lower to flatmap |
| B10 | 44 | 19 | `-warning(Text)`/`-error(Text)` | [preproc.rs](../crates/nera-syntax/src/preproc.rs) |
| B11 | 31 | R14A | `binary:match`/`binary:replace` | [ffi/binary_ops.rs](../crates/nera-runtime/src/ffi/binary_ops.rs) |
| B12 | 50 | 24 | `sets` v2 (maps-based) | new `erlang/stdlib/sets.erl` (Tier 3) |
| B13 | 56 | 24 | Significant children in supervisor | [erlang/stdlib/supervisor.erl](../erlang/stdlib/supervisor.erl) |
| B14 | 54 | 24 | BIF `error_info` metadata | runtime exception raise path in [ffi/](../crates/nera-runtime/src/ffi/) |

**B3 is a special case.** EEP-52 (expression keys in patterns) may already work in Nera's pattern compiler — Maranget decision trees don't inherently require literal keys. Verify with `F = fun(K, M) -> #{K := V} = M, V end, F(a, #{a => 1})` before assuming it's a gap.

**B12 (`sets` v2).** A thin Tier 3 Erlang module wrapping maps. Ship it as `erlang/stdlib/sets.erl` implementing the EEP-50 `sets:new([{version, 2}])` API. OTP's `sets` v1 (based on `gb_trees`) is not worth porting; v2 is simpler and faster for modern workloads.

**B14 (`error_info` metadata).** Nera's diagnostics are already ahead of OTP in rendering quality (codespan-reporting, underlined spans, colored output). What's missing is the *machine-readable* format that EEP-54 defines — the structured `{error_info, #{...}}` term that OTP attaches to exceptions so tooling can format argument-specific messages. Add this to the exception raise path so that `erlang:hd([])` and friends produce `{badarg, [..., {error_info, #{...}}]}` in the stacktrace.

---

## Section C — Strategic arcs (12–18 months)

### C1. Type system — commit to the arc

**Demonstrator update (2026-10-14):**
[P1 of the XMPP component roadmap](XMPP_COMPONENT_ROADMAP.md#p1-enforce-the-selected-erlang-type-contracts)
requires blocking contract checks and limited local inference for an explicitly
selected Erlang subset. That requirement takes precedence over the enforcement
deferral below for this demonstrator. General gradual typing and whole-program
inference remain separate work; this does not claim an implemented checker.

**Recommendation: parse into IR for v1.0, enforce-later.**

EEP-8 (2009) → EEP-61 (2023) → EEP-69 (2024) is fifteen years of incremental work. Nera doesn't need to replicate all of it in one release, but it should stop *discarding* type information. Concrete v1.0 work:

1. Extend [nera-syntax/src/ast.rs](../crates/nera-syntax/src/ast.rs) with full `-spec`, `-type`, `-opaque`, `-callback` representations.
2. Extend [nera-ir/src/lower/mod.rs](../crates/nera-ir/src/lower/mod.rs) to preserve type information as IR metadata (not discarded).
3. Emit warnings for simple type violations: calling a function with the wrong arity, returning a value that doesn't match a simple `-spec` return type (simple = literal atom lists, basic primitives).
4. **Do not** build inference, gradual typing, or nominal types in v1.0.

Post-v1, the gradual typing layer (EEP-61 `dynamic()`) becomes a standalone project. Nominal types (EEP-69) are post-post-v1.

**Why not just defer to Dialyzer?** Dialyzer operates on `.beam` files. Nera doesn't produce those. Running OTP's Dialyzer against Nera source is possible but brittle (it relies on OTP's stdlib specs for Nera's stdlib, which may drift). More importantly: the [MCP server](../crates/nera/src/lsp/mod.rs) that's part of Nera's AI-native story benefits from having type information *in the compiler*, not in an external tool, so the compiler can answer "what's the return type of this function?" directly.

### C2. Priority messages (EEP-76)

**Recommendation: prototype post-v1, measure, decide.**

Priority messages are a runtime mailbox change — they affect Nera's hot path (the one being optimized in Phase 6/7). Adopting them without careful measurement risks regressing OneCached throughput.

Post-v1 work:
1. Build a priority-mailbox branch on top of the stabilized Phase 7 reactor.
2. Run the OneCached 9-benchmark baseline. Compare.
3. If the priority path doesn't regress the non-priority path by more than ~5%, merge.
4. If it does, ship priority messages as an opt-in process flag (`erlang:process_flag(priority_msgs, true)`) with a documented cost.

### C3. EEP-79 Native Records — deliberate divergence

**Recommendation: do not adopt.**

OTP 29's EEP-79 introduces native records as a first-class data type. This is the opposite direction from Nera's [MOONSHOT.md](MOONSHOT.md):

> Records are supported because real code uses them, but the long-term answer is maps with compile-time key checking.

Nera will continue treating `-record` as a compile-time tuple macro (the pre-EEP-79 semantics) for compatibility, while investing in compile-time map-key validation as the modern alternative. This divergence should be documented explicitly so future contributors don't silently import EEP-79 semantics as a "missing feature."

Concrete follow-up: a separate design doc on compile-time map access validation, once the type system work (C1) is underway. The two are complementary — if map keys have types, the compiler can reject `M#{missing_key := V}` statically without needing native records at all.

### C4. Documentation (EEP-48 + EEP-59)

**Recommendation: accept `-doc`/`-moduledoc` in the parser, skip chunk storage, ship a `nera docs` subcommand.**

EEP-48 (Docs chunks in `.beam` files) doesn't apply: Nera emits native ELF/Mach-O, not `.beam`. Three options for how to handle `-doc`:

1. **Skip entirely** — fail to parse modern code that uses `-doc`. Unacceptable.
2. **Embed in the output binary** — add an ELF section or custom linker script. Complex, hard to extract.
3. **Extract at compile time** — parser accepts `-doc`/`-moduledoc`, the compiler writes an optional sidecar (`mymodule.doc.md` or `.doc.json`) next to the `.beam` -equivalent IR dump. A `nera docs <module>` subcommand renders it to Markdown or HTML.

Option 3 is simplest and aligns with Nera's "single binary plus optional tooling" philosophy.

---

## Section D — Explicit non-goals

### D1. Hot code loading

Already out-of-scope per [MOONSHOT.md](MOONSHOT.md). Any EEP relating to module upgrade, code replacement, or runtime re-linking is declined. Nera's deployment model is immutable binaries; hot code loading is incompatible with that model and with AOT compilation to native code.

### D2. Distributed Erlang

Already out-of-scope per [MOONSHOT.md](MOONSHOT.md). `net_kernel`, `rpc`, `global`, `pg`, and the Erlang distribution protocol are not implemented. Users reach for gRPC, NATS, or message queues for inter-node communication. Any EEP that touches distribution semantics is declined.

### D3. `.beam` file compatibility

Nera emits native code. EEPs that define `.beam` file chunk formats (EEP-48 docs chunks, `beam_lib` metadata) are declined as specified but their *content* may still be handled at the source level (e.g., EEP-59 `-doc` parsing per C4).

---

## Section E — Scheduler-correctness audit

The EEP-11 lesson ("every long-running BIF must yield") applies to every Rust BIF Nera has already shipped, not just the ones still to be written. Before v1.0, audit:

- [ffi/binary_ops.rs](../crates/nera-runtime/src/ffi/binary_ops.rs) — large `binary:copy`, `binary:split` on multi-MB inputs
- [ffi/list_ops.rs](../crates/nera-runtime/src/ffi/) — `lists:sort`, `lists:reverse`, `lists:flatten` on long lists
- [ffi/map_ops.rs](../crates/nera-runtime/src/ffi/) — full-map iteration in `maps:fold`, `maps:filter`, `maps:to_list`
- Any future `re` and `json` code (A2, A3 — designed with reduction budgets from day one)

**Deliverable**: for each BIF that can plausibly run for more than ~2000 reductions on realistic input, add a reduction budget check and trap/yield mechanism. Document the pattern in [STDLIB_DESIGN.md](STDLIB_DESIGN.md) so future Tier 2 additions follow it.

---

## Section 7 — Phase 6/7 impact

Concrete sequencing against current in-flight work:

```
Current: Phase 7 park-duration instrumentation (in progress)
        │
        ▼
A1 — EEP-53 Process Aliases ← pause after current task, land correctness fix
        │
        ▼
Phase 7 continues — reactor optimization completion
        │
        ▼
B1–B14 — syntax catchup + stdlib polishing (single work cycle)
        │
        ▼
A2 — EEP-11 re module (Tier 2)
        │
        ▼
A3 — EEP-68 json module (Tier 2)
        │
        ▼
Scheduler-correctness audit (Section E)
        │
        ▼
────── v1.0 tag ──────
        │
        ▼
C1 — type system arc kickoff
C2 — priority messages prototype
C3 — compile-time map validation design doc
C4 — nera docs subcommand
```

**The only decision that affects the current sprint** is the A1 pause. Everything else sequences *after* Phase 7 reactor work concludes, so the immediate question for the team is: can A1 land in the week after the current park-duration instrumentation task? If yes, do it. If no, schedule it explicitly so it doesn't slip past v1.0.

---

## Verification

Each urgent gap has a runnable Erlang snippet that must compile and behave correctly on post-fix Nera.

**A1 (EEP-53 aliases)** — correctness regression test:

```erlang
-module(alias_test).
-behaviour(gen_server).
-export([start_link/0, slow_call/0, init/1, handle_call/3]).
init([]) -> {ok, []}.
handle_call(slow_op, _From, S) ->
    timer:sleep(500),
    {reply, late_reply, S}.
start_link() -> gen_server:start_link({local, ?MODULE}, ?MODULE, [], []).
slow_call() ->
    {ok, _} = start_link(),
    [catch gen_server:call(?MODULE, slow_op, 100) || _ <- lists:seq(1, 50)],
    %% After 50 timeouts, the caller's mailbox must be empty:
    receive _ -> fail after 0 -> ok end.
```

Pre-fix: the final `receive` finds stale `{Ref, late_reply}` tuples. Post-fix: the mailbox is empty because the alias dropped them.

**A2 (EEP-11 `re`)** — basic correctness plus scheduler fairness:

```erlang
{match, [{0, 5}]} = re:run(<<"hello world">>, <<"\\w+">>).

%% Fairness: a regex-heavy process should not starve another process.
Spawn = fun(Fun) -> spawn(fun() -> Fun() end) end,
Heavy = Spawn(fun() ->
    [re:run(<<"aaaaaaaa...">>, <<"a*b">>) || _ <- lists:seq(1, 100000)]
end),
Ticker = Spawn(fun Tick() -> io:format("."), timer:sleep(10), Tick() end),
%% Ticker must produce output continuously while Heavy runs.
```

**A3 (EEP-68 `json`)** — round-trip:

```erlang
Data = #{<<"key">> => 42, <<"arr">> => [1, 2, 3], <<"nested">> => #{<<"k">> => true}},
Encoded = json:encode(Data),
Data = json:decode(Encoded).
```

**B1 (digit separators)**: `N = 1_000_000, N == 1000000`.

**B2 (map comprehensions)**: `#{K => V*2 || K := V <- #{a => 1, b => 2}} == #{a => 2, b => 4}`.

**B3 (expression keys)**: `F = fun(K, M) -> #{K := V} = M, V end, F(a, #{a => 1}) == 1`.

**B4 (triple-quoted)**: a module-level string literal with internal quotes and leading indentation stripped correctly.

**B11 (`binary:match`)**: `binary:match(<<"hello world">>, <<"world">>) == {6, 5}`.

**Regression guardrails for every change:**
- `cargo test` across all crates (659+ unit tests)
- End-to-end Erlang test suite (220+ modules)
- Re-run the OneCached 9-benchmark baseline; no throughput regression vs the pre-change measurement
- ProcessOne's OneCached must still compile and pass all SET/GET/ADD/REPLACE/DELETE/INCR/DECR/flush_all commands
