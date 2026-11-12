# Nera walkthroughs for Erlang developers

Start with Erlang source, compile it to a native executable, then explore
processes, an OTP-style server, a real JSON library, and editor tooling.
You do not need `erl`, `erlc`, or `rebar3` for these native examples.

This guide was verified on **2026-10-12**, on macOS arm64, against `origin/main`
at `4f5a266e291d5b4c3dc56cfc060663000bb59d9c`, using Rust 1.99.0 and Apple
Clang 21.0.0. The sources are in [examples/erlang-guide/](examples/erlang-guide/).
The outputs below are program stdout; Cargo, compiler and linker progress
messages are separate and can vary by machine. These small walkthroughs do
not establish compatibility of an entire OTP application or a full test-suite pass.
Check [project compatibility](../COMPATIBILITY.md) and
[project status](PROJECT_STATUS.md) before porting a larger system.

## 1. Build and install into a temporary workspace

Start in the root of a Nera source checkout. Install Rust through
[rustup](https://rustup.rs/) if necessary; `rust-toolchain.toml` selects the
pinned toolchain. You also need Git and a C linker (`cc`), provided by Xcode
Command Line Tools on macOS or GCC/Clang on Linux. Python 3 is needed only
for the LSP probe in section 6. These commands use a POSIX shell.

1. Save the checkout path and confirm the tools are available:

   ```sh
   repo=$(pwd -P)
   rustc --version
   cargo --version
   cc --version
   ```

2. Build the runtime first, then the compiler and workspace:

   ```sh
   cargo build --release --locked -p nera-runtime
   cargo build --release --locked
   ```

   This order makes the matching runtime archive available when the compiler's
   build script embeds it. The compiler also embeds its bundled Erlang stdlib
   sources. Keep the root `Cargo.lock`; `--locked` prevents dependency resolution
   from changing it.

3. Create an isolated workspace and install there:

   ```sh
   work=$(mktemp -d /tmp/nera-guide.XXXXXX)
   mkdir -p "$work/tmp"
   ./scripts/install-local.sh --prefix "$work/install"
   nera="$work/install/bin/nera"
   examples="$repo/docs/examples/erlang-guide"
   export TMPDIR="$work/tmp"
   cd "$work"
   "$nera" --help
   ```

   The local installer rebuilds Nera and copies the binary to the chosen
   prefix. This walkthrough compiles from outside the checkout with a fresh
   temporary cache and no separately installed runtime archive or stdlib.
   Use this same shell for the remaining sections so the variables stay defined.
   Outputs and intermediate objects go in the temporary workspace.
   Keep it until you finish; a temporary prefix is not a permanent installation.

## 2. Compile your first module

1. Read [guide_hello.erl](examples/erlang-guide/guide_hello.erl):

   ```erlang
   -module(guide_hello).
   -export([start/0]).

   start() ->
       Numbers = [1, 2, 3, 4],
       Squares = [N * N || N <- Numbers],
       io:format("Hello from native Erlang!~n"),
       io:format("Squares: ~p~n", [Squares]).
   ```

2. Check it without generating native code:

   ```sh
   "$nera" check "$examples/guide_hello.erl"
   ```

   A successful front-end check is useful feedback, but does not prove that
   a program links or runs. Follow it with a build and execution.

3. Compile and run:

   ```sh
   "$nera" build "$examples/guide_hello.erl" -e guide_hello:start -o hello
   ./hello
   ```

   Output:

   ```text
   Hello from native Erlang!
   Squares: [1,4,9,16]
   ```

   `-e guide_hello:start` chooses `guide_hello:start/0`; the default entry is
   `main:start/0`. `-o hello` names the executable. Lists, comprehensions,
   pattern matching, module exports, and `io:format/2` retain their familiar
   Erlang syntax. The resulting executable runs without starting BEAM.

## 3. Exchange messages with a linked worker

1. Open [guide_links.erl](examples/erlang-guide/guide_links.erl). The parent
   traps exits before creating a linked process:

   ```erlang
   process_flag(trap_exit, true),
   Parent = self(),
   Worker = spawn_link(fun() -> worker(Parent) end),
   Ref = make_ref(),
   Worker ! {square, Ref, 12}
   ```

   The worker receives the request and replies to the captured parent PID
   with `{result, Ref, 144}`. The reference identifies this particular request;
   the parent's selective receive waits for that reference.

2. Compile and execute the complete source:

   ```sh
   "$nera" build "$examples/guide_links.erl" -e guide_links:start -o links
   ./links
   ```

   Output:

   ```text
   Worker replied: 144
   Linked worker exited: finished
   ```

3. Follow the shutdown path in the source. After receiving and asserting the
   result, the parent sends `stop`. The worker deliberately calls
   `exit(finished)`. Because the parent traps exits, the link delivers
   `{'EXIT', Worker, finished}` as a message, which the parent explicitly
   consumes. Only the parent prints, keeping the output deterministic.
   Both waits have a five-second failure bound; a missing reply or exit
   raises an error rather than silently reporting success.

   This demonstrates a local process link. It does not demonstrate distributed
   Erlang, supervisor restart policies, or equivalence of every timeout case.

## 4. Use the bundled `gen_server` subset

Nera supplies its own [gen_server.erl](../erlang/stdlib/gen_server.erl),
automatically compiled with user code. This is a subset implementation,
not the complete OTP module. In particular, its source explicitly excludes
the `sys` protocol, code upgrades, `format_status`, and hibernation; timeout
values returned by callbacks are not implemented as OTP callback timers.

1. Read [guide_counter.erl](examples/erlang-guide/guide_counter.erl). It starts
   a linked server with integer state, then calls `increment` twice:

   ```erlang
   handle_call(increment, _From, Count) -> {reply, Count + 1, Count + 1};
   handle_call(get, _From, Count) -> {reply, Count, Count};
   handle_call(stop, _From, Count) -> {stop, normal, ok, Count}.

   handle_cast(reset, _Count) -> {noreply, 0}.
   ```

   Each reply returns the new count and keeps it as state. The client asserts
   replies `1` and `2`, casts `reset`, then calls `get` and asserts `0`.
   The reset and get messages come from the same sender to the same server.

2. Build and run:

   ```sh
   "$nera" build "$examples/guide_counter.erl" -e guide_counter:start -o counter
   ./counter
   ```

   Output:

   ```text
   Counter: 1, 2, then reset to 0
   Counter stopped normally
   ```

3. Inspect `stop`: the callback replies `ok` and returns a stop tuple. The
   client then waits for the linked server's normal exit. The source includes
   `code_change/3` for a familiar callback shape, but this walkthrough does
   not exercise it and Nera's bundled server does not call it.

## 5. Compile a pinned external library: Thoas

[COMPATIBILITY.md](../COMPATIBILITY.md) records scoped passes for **Thoas
1.2.1**. This example uses its unmodified library sources for JSON decode,
map access, encode, and a decode roundtrip. It does not run EUnit or ExUnit.
Git/network access is needed to fetch the library; no BEAM toolchain is needed.

1. Fetch the exact revision used by the compatibility register:

   ```sh
   git clone --quiet https://github.com/lpil/thoas.git "$work/thoas"
   git -C "$work/thoas" checkout --quiet 7cdcec7e52b80d2869900893b2acf47199d84190
   git -C "$work/thoas" rev-parse HEAD
   ```

   The last command must print:

   ```text
   7cdcec7e52b80d2869900893b2acf47199d84190
   ```

2. Read [guide_json.erl](examples/erlang-guide/guide_json.erl). It decodes a
   fixed JSON object, asserts all three fields, encodes the resulting map,
   and asserts that decoding it yields the original term. It avoids relying
   on a particular JSON object-key output order.

3. Pass all library modules and the entry module to the compiler:

   ```sh
   "$nera" build "$work/thoas/src/"*.erl "$examples/guide_json.erl" \
       -e guide_json:start -o json
   ./json
   ```

   Output:

   ```text
   Thoas decoded Erlang, native=true, numbers=[1,2,3]
   JSON roundtrip: ok
   ```

   The verified build also emitted four non-tail-recursion warnings from
   upstream `thoas_encode.erl`: `non_recursive_array_loop/1`,
   `non_recursive_object_loop/2`, `list_loop/2`, and `map_naive_loop/2`.
   They were warnings, not build failures; the unmodified source compiled
   and this roundtrip completed successfully.

   Nera links the supplied modules into one executable. A dependency's source
   being available does not establish compatibility: keep versions pinned,
   compile, and exercise the actual APIs you need. The
   [Thoas integration notes](../tests/integration/thoas/README.md) describe
   broader validation and its limits. Poolboy and the other candidates in the
   compatibility register are not interchangeable with this verified example.

## 6. Try the LSP, then connect your editor

The current CLI implements `lsp --transport stdio`. An LSP client starts that
process and exchanges framed JSON-RPC messages over stdin/stdout; running a
server in a terminal alone does not produce interactive editor help.

1. Run the included small client against the hello source:

   ```sh
   python3 "$examples/lsp_probe.py" "$nera" "$examples/guide_hello.erl"
   ```

   Output:

   ```text
   LSP initialized: document symbols supported
   Document symbols: start/0
   LSP shutdown: ok
   ```

   The [probe source](examples/erlang-guide/lsp_probe.py) launches the server,
   initializes it, opens the file, waits for a diagnostics notification,
   requests the actual document symbols, then shuts down cleanly. This verifies
   the server protocol and outline response, not an editor's rendering or every
   advertised capability.

2. Choose the repository's integration for your editor:
   [VS Code](../editors/vscode-nera/README.md),
   [JetBrains](../editors/jetbrains-nera/README.md), or
   [Emacs](../editors/emacs-nera/README.md). Follow that integration's build and
   installation instructions. Editor installation/UI was not exercised for
   this guide.

3. Set its Nera executable path to the absolute installed binary path printed
   by the installer. VS Code uses the `nera.path` setting; JetBrains has
   **Settings → Languages & Frameworks → Nera** and requires LSP4IJ; Emacs
   configures the executable in its LSP client. A temporary install must remain
   on disk for the editor to use it.

4. Open an Erlang project root and an `.erl` file. The integrations launch
   Nera over stdio. For these examples, the hello function should appear in
   the editor's outline; for a real project, keep its source/include/dependency
   layout available to the checker. Use native compilation and execution to
   validate runtime behavior separately. See [LSP documentation](LSP.md) for
   the protocol and feature descriptions.

### MCP availability in this revision

The current binary's help contains no `mcp` subcommand, and its CLI source
defines none. [MCP_SERVER.md](MCP_SERVER.md) is a design document, not an
executable setup guide for this revision. Do not configure an assistant to
launch that proposed server yet. An assistant can use the verified `check`
and `build` commands above through its shell tools; successful checking still
needs execution and review. Consult [project status](PROJECT_STATUS.md) for
readiness, and verify a future implementation before adopting MCP instructions.

## 7. What changes when you leave BEAM?

- **Build and entry:** Nera compiles `.erl` source to host native code and
  links a runtime. Choose an exported zero-argument entry function; supply
  the modules needed by the program at build time. These examples do not
  produce `.beam` files, start `erl`, or use an OTP release boot script.
- **Runtime and stdlib:** the deployed program contains Nera's runtime and
  selected compiled modules. Its bundled OTP-shaped modules provide subsets;
  familiar API names and `-behaviour` attributes do not promise full OTP
  behavior. Read the implementation and test the paths your application uses.
- **Local concurrency:** the examples retain `spawn_link`, `!`, selective
  `receive`, references, and trapped exits. They establish these specific
  local paths; do not infer every BEAM scheduling, timeout, or failure detail.
- **Deployment:** a native executable does not require a BEAM installation,
  but is built for a particular OS/architecture and may depend on host system
  libraries. This guide verifies macOS arm64 execution only; it makes no
  Linux, Windows, cross-compilation, or fully static-linking claim.
- **Upgrades and distribution:** Nera is not a drop-in replacement for BEAM's
  hot code loading or distributed Erlang. Port a bounded workload first,
  following [COMPATIBILITY.md](../COMPATIBILITY.md) and the dated
  [project status](PROJECT_STATUS.md), rather than treating roadmap entries
  as shipped features.

All four native programs above were checked for their exact stdout and a zero
exit status using a freshly built compiler/runtime. The installation and LSP
probe were also run. No compiler, runtime, or editor implementation was changed
to make these examples work.
