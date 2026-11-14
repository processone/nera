# nera — Erlang-to-Native Compiler

The goal is **Erlang that ships like Go**.

nera compiles Erlang source files to standalone native binaries — no BEAM VM
required.

**Experimental software (2026-11-12).** `v0.1.0` is Nera's first experimental
release. It is developed and tested on macOS arm64 and Linux x86-64; Windows
does not work yet, and Cowboy, TLS, distributed Erlang and hot upgrades do not work.
Read the [known limits](docs/KNOWN_LIMITS.md) before relying on it, and see the
[current project status](docs/PROJECT_STATUS.md) for the evidence. It stays
experimental until 1.0. Later suffixed tags, such as `vX.Y.Z-experimental`, are
published as GitHub prereleases.

See [project compatibility](COMPATIBILITY.md) for tested libraries and
applications, known blockers, untested candidates, and the scope of each result.

New to Nera? Follow the [Erlang developer walkthroughs](docs/ERLANG_WALKTHROUGHS.md)
for runnable native, concurrency, OTP-style server, library, and LSP examples.

For the planned combination of Erlang source reuse, compile-time contract
checking and native deployment, see the
[Nera positioning and XMPP component roadmap](docs/XMPP_COMPONENT_ROADMAP.md).
It defines the first demonstrator and its validation gates; static contract
enforcement and the component remain planned capabilities.

## Why nera?

- **Single binary deployment** — `nera build` produces one standalone
  executable. No VM install, no release tooling, no `erl` on the target
  machine.
- **Fast startup** — native code starts immediately. Sub-200 ms wall-clock
  for most programs (vs ~1 s BEAM startup overhead).
- **Low memory footprint** — 5–30 MB RSS typical, vs 56–63 MB for equivalent
  BEAM workloads.
- **Erlang concurrency primitives** — lightweight processes, message
  passing, selective receive, links/monitors, per-process generational GC,
  work-stealing scheduler. Compatibility gaps in the supported subset are
  listed in the [known limits](docs/KNOWN_LIMITS.md); Nera is not a drop-in BEAM replacement.
- **No bytecode** — nera is an ahead-of-time (AOT) compiler. Erlang source →
  Cranelift IR → native machine code. There is no interpreter, no bytecode,
  no JIT. The output is a regular native executable.
- **AI tooling (planned)** — an **MCP server** that would expose project
  symbols, cross-module references and structured compiler diagnostics to
  coding assistants is designed but not built yet; there is no `nera mcp`
  command in this release.

## Using nera with the BEAM compiler

You don't have to choose between nera and BEAM — you can use both. Even if you
compile and deploy with the standard Erlang/OTP toolchain, nera's LSP server and
editor plugins (VS Code, JetBrains) give you live diagnostics as you type:
instant feedback on syntax errors, unbound variables, unused functions,
unreachable clauses, and more — without waiting for a full `rebar3 compile`
cycle.

Install the nera editor plugin, point your project at `nera check`, and get a
faster feedback loop on your existing Erlang codebase. nera understands standard
OTP project layouts and rebar3 dependency directories out of the box, so setup is
minimal.

### MCP Server — planned

An MCP (Model Context Protocol) server for AI coding assistants is planned but
not built yet: this release has no `nera mcp` command. It would give assistants
direct access to the compiler's project understanding, as the LSP does for human
developers in editors. The design proposes these tools:

- **`nera/check`** — structured diagnostics with error codes, locations, and
  severity, so the AI can fix issues precisely instead of guessing from text output.
- **`nera/project-symbols`** — the full module graph, exported functions, records,
  and types, without reading every source file.
- **`nera/function-info`** — inferred specs, clause patterns, and cross-module
  call sites for any function.
- **`nera/references`** — all callers of a function across the project.
- **`nera/format`** — format generated code before returning it to the user.

The MCP interface would make Nera's project analysis available independently of
an editor. It would complement existing Erlang tooling; it is not a claim of
exclusive semantic-analysis capabilities.

See [`docs/MCP_SERVER.md`](docs/MCP_SERVER.md) for the full design document.

## How it works

```
  .erl source → [nera compiler] → standalone native binary
                  │
                  ├─ Parse (nera-syntax)
                  ├─ Lower to IR (nera-ir)
                  ├─ Generate native code (Cranelift)
                  └─ Link with runtime (libnera_runtime.a)
```

## nera vs BEAM vs Go

|                       | nera                         | BEAM (OTP 28)                  | Go                                   |
|-----------------------|------------------------------|--------------------------------|--------------------------------------|
| Compilation           | AOT to native                | Bytecode + JIT (OTP 24+)       | AOT to native                        |
| Deployment            | Single static binary         | Requires OTP install + release | Single static binary                 |
| Startup time          | < 200 ms                     | ~1 s                           | < 100 ms                             |
| Memory (typical)      | 5–30 MB                      | 56–63 MB                       | 10–30 MB                             |
| Concurrency model     | Erlang processes             | Erlang processes               | Goroutines                           |
| Preemptive scheduling | Yes (reduction-based)        | Yes (reduction-based)          | Yes (cooperative + async preemption) |
| Per-process GC        | Yes                          | Yes                            | No (global GC with STW pauses)       |
| Hot code upgrade      | No                           | Yes                            | No                                   |
| Pattern matching      | Full Erlang                  | Full Erlang                    | No                                   |
| Message passing       | Copy semantics               | Copy semantics                 | Channel semantics                    |
| Fault tolerance       | Links, monitors, supervisors | Links, monitors, supervisors   | Manual (recover/restart)             |

**Where each shines** — nera targets Erlang's concurrency and fault-tolerance
model with Go-like deployment simplicity. BEAM still wins for hot code
upgrades, the massive OTP ecosystem, and distributed Erlang. Go still wins for
raw compute throughput and mature tooling.

## Benchmark highlights

Historical measurements, not revalidated in the October 2026 baseline.
Measured on Apple M4 Pro, single-node, release builds. Internal timing
(`erlang:monotonic_time/1`) excludes startup.

| Benchmark               | nera    | BEAM    | nera RSS | BEAM RSS |
|-------------------------|---------|---------|----------|----------|
| binary_send (100K×10KB) | 25 ms   | 15 ms   | 27 MB    | 63 MB    |
| tcp_ping_pong (50K×1KB) | 1844 ms | 1631 ms | 7 MB     | 56 MB    |
| tcp_throughput (100MB)  | 46 ms   | 36 ms   | 30 MB    | 57 MB    |

Full benchmark suite in `benchmarks/`.

## Building the Compiler

### Prerequisites

- **Rust 1.99.0** — pinned in `rust-toolchain.toml` and CI. Install via
  [rustup](https://rustup.rs/); rustup selects this version inside the checkout.
  Build with `--locked` to use the versioned root `Cargo.lock`.
- **A C linker** — `cc` must be available on `PATH` (Xcode Command Line Tools on
  macOS, `gcc` or `clang` on Linux)

### Build

```bash
# Debug build (faster compilation)
cargo build

# Release build (optimized compiler + runtime)
cargo build --release
```

This produces two artifacts:

| Artifact               | Path                                       |
|------------------------|--------------------------------------------|
| Compiler binary        | `target/{debug,release}/nera`              |
| Runtime static library | `target/{debug,release}/libnera_runtime.a` |

Both are needed — the compiler invokes the system linker to combine generated
object files with `libnera_runtime.a` into a final executable.

### Run the Tests

```bash
# Workspace tests; continue after a failed target to obtain a full baseline
cargo test --workspace --no-fail-fast

# End-to-end tests (see docs/PROJECT_STATUS.md for current counts and failures)
cargo build --release
./target/release/nera test
```

### Fuzzing

The parser and lexer have [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) targets to catch panics and crashes on arbitrary inputs. Requires the nightly toolchain:

```bash
# Install cargo-fuzz (once)
cargo install cargo-fuzz

# Run the parser fuzzer
cd crates/nera-syntax
cargo +nightly fuzz run fuzz_parse -- -max_len=4096

# Run the lexer fuzzer
cargo +nightly fuzz run fuzz_lex -- -max_len=4096

# Run the formatter fuzzer (idempotency oracle)
cd crates/nera-fmt
cargo +nightly fuzz run fuzz_format -- -max_len=4096
```

Crash artifacts are written to `fuzz/artifacts/`. See `docs/TESTING_PLAN.md` for the full fuzzing and property-based testing roadmap.

## Compiling and Running Erlang Programs

### Quick Start

```bash
# Build the compiler (release recommended)
cargo build --release

# Compile an Erlang program
./target/release/nera examples/hello.erl -o hello

# Run it
./hello
# => Hello, World!
```

### Compiler Usage

```
nera [OPTIONS] [FILES]...

Arguments:
  [FILES]...               Input .erl files (optional if --src or --deps-dir is used)

Options:
      --src <DIR>          Source directory — recursively discover .erl files (repeatable)
  -I, --include <DIR>      Include search path for -include() directives (repeatable)
      --deps-dir <DIR>     Dependency directory — scan subdirs as OTP apps (repeatable)
  -o, --output <OUTPUT>    Output binary name [default: a.out]
  -e, --entry <ENTRY>      Entry point as module:function [default: main:start]
      --target <TRIPLE>    Target triple for cross-compilation (e.g. x86_64-unknown-linux-gnu)
      --linker <LINKER>    Override the linker command (default: $CC, then "cc")
      --emit-ir            Print the IR and exit (do not compile)
      --emit-obj           Emit .o files only (do not link)
      --no-stdlib          Do not include the built-in stdlib modules
  -v, --verbose            Verbose output
  -h, --help               Print help
```

### Entry Point

By default the compiler looks for `main:start/0` as the program entry point.
For a module named something other than `main`, pass `-e module:function`:

```bash
./target/release/nera examples/factorial.erl -o factorial -e factorial:start
./factorial
# => 10! = 3628800
```

### Multi-Module Programs

Pass all source files on the command line. The compiler will compile each module
to a separate object file and link them together:

```bash
./target/release/nera module_a.erl module_b.erl -o myapp -e module_a:start
```

Only the named files are compiled, with the stdlib and any `--deps-dir`
dependencies, whatever folder you run the command from.

### Compiling Erlang Projects

nera understands the standard Erlang project directory layout. Use `--src` to
auto-discover `.erl` files in a directory and `-I` to add include search paths:

```bash
nera build --src src/ -I include/ -o myapp -e myapp:start
```

A `tests/` folder beside the first `--src` directory is compiled too, and so is
a project's `tests/` in a project build (`nera build` with no file arguments).

### Dependency Management

nera has built-in support for fetching dependencies from hex.pm, git
repositories, and local paths.

#### Creating a Project

```bash
# Create a new project in a new directory
nera new myapp

# Or initialize in an existing directory
cd myproject
nera init
```

#### Declaring Dependencies

Add a `[deps]` section to your `nera.toml`:

```toml
[project]
name = "myapp"
entry = "myapp:start"
output = "myapp"

[deps]
jsx = "3.1.0"                                                    # hex.pm
ranch = { git = "https://github.com/ninenines/ranch.git", tag = "1.8.0" }
mylib = { git = "https://github.com/user/repo.git", branch = "main" }
utils = { path = "../shared_utils" }
```

#### Fetching Dependencies

```bash
# Fetch all declared dependencies
nera deps get

# Add a hex.pm package and fetch it
nera deps add jsx 3.1.0

# Add the latest version of a package
nera deps add cowlib
```

Dependencies are fetched into `.nera/deps/`. When you run `nera build`, the
compiler auto-discovers this directory and includes all dependency modules
automatically — no `--deps-dir` flag needed.

#### Compiling with rebar3 Dependencies

If your project uses [rebar3](https://rebar3.org/) for dependency management,
let rebar3 fetch the dependencies, then point nera at the result:

```bash
# 1. Fetch dependencies (rebar3 downloads them into _build/)
rebar3 get-deps

# 2. Compile everything with nera
nera build --src src/ -I include/ \
      --deps-dir _build/default/lib \
      -o myapp -e myapp:start
```

The `--deps-dir` flag scans each subdirectory in the given path as a standard
OTP application: it discovers `.erl` files under `<dep>/src/` and adds
`<dep>/include/` to the include search path automatically.

You can combine all three source modes freely:

```bash
nera build \
    extra_module.erl \            # explicit file
    --src src/ \                  # project source directory
    --src lib/mylib/src/ \        # additional source directory
    -I include/ \                 # project includes
    --deps-dir _build/default/lib \  # rebar3 deps
    -o myapp -e myapp:start
```

### Concurrency Examples

Programs that use `spawn`, message passing (`!`), and `receive` work
out of the box. The runtime starts a work-stealing scheduler with one thread
per CPU core (up to 255 workers). Set `NERA_SCHEDULERS` when launching a
compiled executable to choose a count from 1 to 255, for example
`NERA_SCHEDULERS=1 ./ping_pong`. Invalid values exit with status 2. This is also
useful for reproducible scheduler-fairness tests.

```bash
# Two processes exchanging ping/pong messages
./target/release/nera examples/ping_pong.erl -o ping_pong -e ping_pong:start
./ping_pong

# 100 processes passing messages around a ring
./target/release/nera examples/ring.erl -o ring -e ring:start
./ring
# => Ring complete
```

### Checking Without Compiling

`nera check` runs the full front-end pipeline (parse, preprocess, IR lowering,
validation) without code generation or linking. It catches syntax errors,
undefined variables, unresolved calls, and emits warnings — much faster than a
full build.

```bash
# Check a single file
nera check src/myapp.erl

# Check an entire project (discovers files from nera.toml / src/)
nera check

# Check with include paths and dependencies
nera check --src src/ -I include/ --deps-dir _build/default/lib

# Suppress warnings
nera check --no-warnings
```

This is useful as a "run on save" editor task for fast feedback without waiting
for codegen and linking. Editors should invoke `nera check` from the workspace
root, preferably on the whole project (`nera check` or `nera check --src ...`)
rather than only the current file, so cross-module and `-include` diagnostics
remain accurate. See `demo/editors/vscode/` for a reference VS Code setup.

### Debugging

```bash
# Dump the intermediate representation
./target/release/nera examples/hello.erl --emit-ir -e hello:start

# Produce object files without linking
./target/release/nera examples/hello.erl --emit-obj -e hello:start

# Verbose mode shows each compilation phase
./target/release/nera examples/hello.erl -o hello -e hello:start -v
```

### Cross-Compilation

nera can generate code for a different target architecture than the host. Use
`--target <triple>` to specify the target:

```bash
# Cross-compile for Linux x86_64 from macOS arm64
# Step 1: Build the runtime for the target
cargo build -p nera-runtime --target x86_64-unknown-linux-gnu --release

# Step 2: Cross-compile an Erlang program (emit object files)
./target/release/nera build hello.erl --target x86_64-unknown-linux-gnu --emit-obj

# Step 3: Link with a cross-linker
./target/release/nera build hello.erl --target x86_64-unknown-linux-gnu \
    --linker x86_64-linux-gnu-gcc -o hello
```

Supported targets (all 64-bit):

| Target triple               | OS      | Arch   |
|-----------------------------|---------|--------|
| `aarch64-apple-darwin`      | macOS   | arm64  |
| `x86_64-apple-darwin`       | macOS   | x86_64 |
| `x86_64-unknown-linux-gnu`  | Linux   | x86_64 |
| `aarch64-unknown-linux-gnu` | Linux   | arm64  |

Windows (`x86_64-pc-windows-msvc`) does not work yet: the compiler does not
link there, and releases include no Windows archive.

Use `--linker` to override the linker command for custom toolchains (e.g.,
`zig cc`, Android NDK, iOS SDK).

The distribution script also supports cross-compilation:

```bash
./scripts/dist.sh --target x86_64-unknown-linux-gnu --version 0.2.0
```

## Examples

| File                         | Description                                          |
|------------------------------|------------------------------------------------------|
| `examples/hello.erl`         | Hello World — basic `io:format`                      |
| `examples/factorial.erl`     | Recursive factorial with pattern matching and guards |
| `examples/fibonacci.erl`     | Fibonacci sequence                                   |
| `examples/ping_pong.erl`     | Two processes exchanging messages                    |
| `examples/ring.erl`          | 100 processes in a message-passing ring              |
| `examples/echo.erl`          | Echo server with request/reply pattern               |
| `examples/echo_server.erl`   | TCP echo server with gen_tcp                         |
| `examples/counter.erl`       | Stateful counter process with increment/get/stop     |
| `examples/parallel_map.erl`  | Parallel map using spawn and message passing         |
| `examples/list_comp.erl`     | List comprehension examples                          |
| `examples/list_ops.erl`      | List operations and pattern matching                 |
| `examples/math_utils.erl`    | Math utility functions                               |
| `examples/multi_module.erl`  | Multi-module compilation                             |
| `examples/exception.erl`     | Try/catch/after exception handling                   |
| `examples/bif_test.erl`      | Built-in function demonstrations                     |
| `examples/simple_list.erl`   | Basic list operations                                |

## Supported Erlang Features

- **Data types**: atoms, integers (arbitrary precision), floats, strings (as
  lists), tuples, lists, maps, binaries (byte-aligned, bit-level,
  bitstring, and UTF-8/16/32 segments), PIDs,
  references, closures/funs
- **Pattern matching** with nested patterns, guards, and multiple clauses
- **Control flow**: `case`, `if`, `receive...after`, `begin...end`, `try/catch/after`
- **Concurrency**: `spawn/1,3`, `spawn_link/1,3`, `spawn_monitor/1,3`,
  `spawn_opt/2,4` (with link, monitor, heap options),
  message send (`!`), selective `receive` with timeouts, process links
  (`link/1`, `unlink/1`) with EXIT signal propagation, monitors (`monitor/2`,
  `demonitor/1`) with DOWN messages, `process_flag(trap_exit, true/false)`,
  `process_flag(fullsweep_after, N)`, `process_flag(max_heap_size, Words)`,
  `erlang:hibernate/3`, registered processes (`register/2`, `whereis/1`),
  generational per-process garbage collection, work-stealing scheduler with
  timer-based timeout management
- **OTP behaviours**: `gen_server`, `supervisor` (one_for_one), `application`,
  `proc_lib` — enough to run real OTP applications with supervision trees
- **ETS tables**: `ets:new/2`, `ets:insert/2`, `ets:lookup/2`,
  `ets:delete/2`, `ets:match_object/2`, `ets:select_delete/2`, and more —
  shared term storage with named tables and `keypos` support
- **Functions**: multi-clause functions, tail call optimization, anonymous
  functions (`fun`), closures with free variable capture, local and remote
  function references (`fun F/A`, `fun M:F/A`)
- **Explicit imports**: `-import(Module, [Name/Arity])`, including ordinary and
  tail calls and precedence over automatically imported BIFs. Imported fun
  references use the qualified `fun Module:Name/Arity` form.
- **Records**: creation, update, field access, index, pattern matching
  (desugared to tuples)
- **List comprehensions** with generators and filters
- **Preprocessor**: `-define`/`-undef` (object-like and function-like macros),
  macro overloads by arity, `?MACRO` expansion, `??Parameter` argument text,
  predefined macros (`?MODULE`, `?LINE`, `?FILE`),
  `-ifdef`/`-ifndef`/`-else`/`-endif` conditional compilation, `-include` and
  nested `-include_lib` resolution
- **Process dictionary**: `put/2`, `get/1`, `erase/1`, `get_keys/0`
- **BIFs**: `io:format/1,2`, `io_lib:format/2`, `file:read_file/1`,
  `file:write_file/2`, `binary:split/2,3`, `binary:referenced_byte_size/1`,
  `bit_size/1`, type-check BIFs (`is_atom`,
  `is_integer`, `is_reference`, ...), conversion BIFs (`atom_to_list`,
  `list_to_integer`, `integer_to_binary`, `binary_to_integer`, ...),
  list/tuple/map operations, `erlang:system_time/1`, `erlang:monotonic_time/1`,
  `erlang:send_after/3`, `erlang:halt/0,1`,
  `error_logger:info_msg/1,2`, `error_logger:error_msg/1,2`
- **OTP 21+ storage**: `persistent_term` (read-optimized global key-value store),
  `counters` (lock-free atomic counter arrays), `atomics` (atomic integers with
  CAS) — on par with BEAM performance for counters/atomics
- **Networking**: `gen_tcp:listen/2`, `gen_tcp:accept/1`, `gen_tcp:send/2`,
  `gen_tcp:recv/2,3`, `gen_tcp:close/1`, `inet:setopts/2` — with `{active, once}`
  mode, `{packet, line}`, and iolist support
- **Compiler diagnostics**: colored error messages with source snippets
  (via `codespan-reporting`), warnings for unused variables, unreachable
  clauses, shadowed variables, and unused functions
- **Built-in test runner**: `nera test` discovers and runs test modules
  in parallel with colored output and pattern filtering

### Real-World Validation

nera successfully compiles and runs **[OneCached](https://github.com/processone/onecached)**,
an open-source memcached server by ProcessOne. All memcached protocol
commands work correctly: SET, GET, ADD, REPLACE, DELETE, INCR, DECR, and
flush_all — using gen_server, supervisor, ETS, gen_tcp, and binary pattern
matching.

See [CHANGELOG.md](CHANGELOG.md) for detailed milestone history.

## Standard Library

nera embeds the standard library sources directly in the compiler binary — no
external files needed. Stdlib modules are automatically compiled alongside user
code. If stdlib `.erl` files are found on the filesystem (e.g. in the installed
`lib/nera/erlang/stdlib/` directory), they take priority over the embedded
versions, allowing local overrides for development and debugging.

Currently included:

| Module        | Description                                                         |
|---------------|---------------------------------------------------------------------|
| `lists`       | 42 exported functions. Hot-path functions (`reverse`, `member`,     |
|               | `keyfind`, `nth`, ...) are native BIFs; higher-order functions      |
|               | (`map`, `filter`, `foldl`, `sort`, ...) are compiled from Erlang.   |
| `maps`        | 28 exported functions. Hot-path functions (`get`, `put`, `remove`,  |
|               | `keys`, `values`, ...) are native BIFs; higher-order functions      |
|               | (`map`, `filter`, `fold`, `foreach`, ...) are compiled from Erlang. |
| `gen_server`  | OTP generic server behaviour. `start_link`, `call`, `cast`,         |
|               | `reply`, callback dispatch (`init`, `handle_call`, `handle_cast`,   |
|               | `handle_info`, `terminate`, `code_change`).                         |
| `supervisor`  | OTP supervisor behaviour. `start_link`, `start_child`,              |
|               | `one_for_one` restart strategy with max restart intensity tracking. |
| `application` | Minimal OTP application controller. `start/1,2`, `stop/1`,          |
|               | `get_env/2,3`. Maps `app_name` to `app_name_app:start/2`.           |
| `proc_lib`    | OTP process library. `spawn/1,3`, `spawn_link/1,3`,                 |
|               | `init_ack/1,2` for supervised process startup.                      |

Additionally, these OTP 21+ modules are implemented as native BIFs:

| Module          | Description                                                        |
|-----------------|--------------------------------------------------------------------|
| `persistent_term` | Read-optimized global storage. `put/2`, `get/1,2`, `erase/1`,  |
|                 | `info/0`. Deep-copied terms with `parking_lot::RwLock`.            |
| `counters`      | Lock-free atomic counter arrays. `new/2`, `get/2`, `put/3`,       |
|                 | `add/3`, `sub/3`, `info/1`. Direct-pointer access, BEAM-speed.    |
| `atomics`       | Atomic 64-bit integers with CAS. `new/2`, `get/2`, `put/3`,       |
|                 | `add/3`, `sub/3`, `add_get/3`, `sub_get/3`, `exchange/3`,         |
|                 | `compare_exchange/4`, `info/1`. Direct-pointer access, BEAM-speed. |

User modules shadow stdlib modules of the same name. Use `--no-stdlib` to
disable automatic inclusion.

## Installation

### Release archives

Each release on the [GitHub releases page](https://github.com/processone/nera/releases)
has one archive per platform:

| Archive                                           | Platform                              |
|---------------------------------------------------|---------------------------------------|
| `nera-<version>-aarch64-apple-darwin.tar.gz`      | macOS arm64 (tested)                  |
| `nera-<version>-x86_64-unknown-linux-gnu.tar.gz`  | Linux x86-64 (tested)                 |
| `nera-<version>-x86_64-apple-darwin.tar.gz`       | macOS x86-64 (cross-built, never run) |
| `nera-<version>-aarch64-unknown-linux-gnu.tar.gz` | Linux arm64 (cross-built, never run)  |

There is no Windows archive: Nera does not work on Windows yet. Check the
[known limits](docs/KNOWN_LIMITS.md) first, then extract the archive and add its
`bin/` directory to your `PATH`:

```bash
tar xzf nera-0.1.0-aarch64-apple-darwin.tar.gz
export PATH="$(pwd)/nera-0.1.0-aarch64-apple-darwin/bin:$PATH"
```

`v0.1.0` is a full GitHub release that stays experimental until 1.0. Later tags
with a suffix, such as `v0.2.0-experimental`, are published as prereleases.

### Editor plugins from a release

Each release also includes the editor plugins:

- **VS Code** — `vscode-nera-<version>.vsix`. Install it with
  `code --install-extension vscode-nera-<version>.vsix`. See
  [`editors/vscode-nera`](editors/vscode-nera/README.md).
- **JetBrains** — `jetbrains-nera-<version>.zip`. Install it from
  **Settings → Plugins → ⚙ → Install Plugin from Disk…**. It needs a commercial
  JetBrains IDE (such as IntelliJ IDEA Ultimate, RustRover or CLion), version
  2025.3 or later. See [`editors/jetbrains-nera`](editors/jetbrains-nera/README.md).

Both plugins need the `nera` binary on your `PATH`, or its path in the plugin
settings.

### Local install

Use `scripts/install-local.sh` to build and install nera to a local prefix
(default: `~/.local`):

```bash
./scripts/install-local.sh
```

This installs:

```
~/.local/
  bin/nera                       # compiler binary (stripped)
  lib/libnera_runtime.a          # runtime static library
  lib/nera/erlang/stdlib/*.erl   # standard library sources
```

After the first install, add `~/.local/bin` to your `PATH` (if not already
there) by adding this line to your `~/.zshrc` or `~/.bashrc`:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

To update after making changes, just re-run the script:

```bash
./scripts/install-local.sh
```

Options:

```
--prefix <dir>   Install to a custom prefix (default: ~/.local)
--debug          Use debug build instead of release
```

### Building from source

After `cargo build --release`, two artifacts are required to compile Erlang
programs:

| Artifact               | Path                               |
|------------------------|------------------------------------|
| Compiler binary        | `target/release/nera`              |
| Runtime static library | `target/release/libnera_runtime.a` |
| Standard library       | `erlang/stdlib/*.erl`              |

The compiler auto-discovers the runtime library and stdlib relative to its own
location (workspace layout) or relative to the current working directory.

### Distributable tarball

Use the `scripts/dist.sh` script to produce a relocatable tarball:

```bash
./scripts/dist.sh                     # build for current host
./scripts/dist.sh --version 0.2.0     # override version
```

This creates a tarball with the following layout:

```
nera-<version>-<target>/
  bin/nera                       # compiler binary (stripped)
  lib/libnera_runtime.a          # runtime static library
  lib/nera/erlang/stdlib/*.erl   # standard library sources
```

To install, extract and add the `bin/` directory to your `PATH`:

```bash
tar xzf nera-0.1.0-aarch64-apple-darwin.tar.gz
export PATH="$(pwd)/nera-0.1.0-aarch64-apple-darwin/bin:$PATH"
nera hello.erl -o hello -e hello:start
```

The compiler auto-discovers the runtime and stdlib relative to the binary
location (`../lib/`), so no environment variables or configuration is needed.

## Project Structure

```
crates/
  nera-syntax/    Lexer (logos) + preprocessor + recursive-descent parser + AST
  nera-ir/        AST-to-IR lowering, pattern match compilation (Maranget)
  nera-codegen/   Cranelift code generation + system linker integration
  nera-runtime/   Lightweight runtime: tagged terms, scheduler, GC, BIFs
  nera-fmt/       Code formatter
  nera/           CLI driver
  nera-syntax/fuzz/  Fuzz targets (parser, lexer) — see docs/TESTING_PLAN.md
erlang/
  stdlib/          Standard library modules (compiled with user code)
scripts/
  install-local.sh Install to ~/.local (or custom prefix)
  dist.sh          Build distributable tarball
```

## License

See [LICENSE](LICENSE) for details.
