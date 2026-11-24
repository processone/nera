# Using Nera

This guide covers installation and everyday commands. Start with the
[README](../README.md) for an overview and first run.
Nera is experimental, not a drop-in replacement for Erlang/OTP, and not ready
for production. Wrong results and hangs remain possible. Read the
[known limits](KNOWN_LIMITS.md) before choosing a workload.

## Installation

The Nera compiler is a single executable with its host runtime and
standard-library sources embedded. You can copy just `nera` to another location
for native builds, without installing separate Nera libraries.
Cross-compilation still needs a runtime built for the target, as described
[below](#debugging-and-cross-compilation).

You need a C linker (`cc` on `PATH`) even with a prebuilt compiler: Xcode
Command Line Tools on macOS, or GCC/Clang on Linux. Nera is tested on macOS
arm64 and Linux x86-64. The macOS x86-64 and Linux arm64 archives are cross-built
but have not been run. Windows does not work yet and has no release archive.

### Release archives

Download the archive matching your system from
[GitHub Releases](https://github.com/processone/nera/releases).
For the experimental `v0.1.0` release, the compiler archives are:

| Platform | Archive |
| --- | --- |
| macOS arm64 | `nera-0.1.0-aarch64-apple-darwin.tar.gz` |
| Linux x86-64 | `nera-0.1.0-x86_64-unknown-linux-gnu.tar.gz` |
| macOS x86-64 | `nera-0.1.0-x86_64-apple-darwin.tar.gz` |
| Linux arm64 | `nera-0.1.0-aarch64-unknown-linux-gnu.tar.gz` |

In the download directory, extract your archive and add its `bin/` to `PATH`.
For macOS arm64:

```sh
tar xzf nera-0.1.0-aarch64-apple-darwin.tar.gz
export PATH="$(pwd)/nera-0.1.0-aarch64-apple-darwin/bin:$PATH"
nera --help
```

Use the corresponding archive and directory names for other platforms.
The export lasts for the current shell; add an export using the installation's
absolute path to your shell profile if you want it in later sessions.
The first release remains experimental until 1.0, regardless of its GitHub
release label.

### Build from source

Install Git, the linker and [Rust through rustup](https://rustup.rs/).
The checkout selects Rust 1.100.0; `--locked` uses the committed dependency lock.
Run these commands from a POSIX shell:

```sh
git clone https://github.com/processone/nera.git
cd nera
cargo build --release --locked -p nera-runtime
cargo build --release --locked -p nera
```

Build the runtime first so the compiler embeds the matching library.
The outputs are `target/release/nera` and `target/release/libnera_runtime.a`.
Use `./target/release/nera` in place of `nera` below until you install it.
For a debug build, omit `--release` from both commands and use `target/debug/`.

### Local installation

After building the matching runtime and compiler, the local installer builds
and copies the compiler to a prefix:

```sh
./scripts/install-local.sh
export PATH="$HOME/.local/bin:$PATH"
```

It installs only `~/.local/bin/nera`, not a runtime or standard-library tree.
Use `--prefix /your/prefix` for a different destination or `--debug` for a
debug build. Re-run the installer after rebuilding to update your installation.

## Compile and run

In a source checkout, compile the included Hello World example:

```sh
nera build examples/hello.erl -e hello:start -o hello
./hello
```

The expected output is:

```text
Hello, World!
```

With an archive installation and no checkout, save this as `hello.erl`:

```erlang
-module(hello).
-export([start/0]).
start() -> io:format("Hello, World!~n").
```

Then run `nera build hello.erl -e hello:start -o hello` followed by `./hello`.
Require normal exit as well as the expected output when validating a program.

`-e hello:start` selects `hello:start/0`; the default is `main:start/0`.
For several modules, pass each source file:

```sh
nera build module_a.erl module_b.erl -e module_a:start -o myapp
```

Explicit-file builds include the named files, built-in standard library and
any specified dependency directories; they do not discover all nearby sources.
Use these options to choose inputs or inspect a build:

| Option | Purpose |
| --- | --- |
| `--src DIR` | Recursively discover `.erl` files; repeat for more directories. |
| `-I DIR` | Add an include search path; repeat as needed. |
| `--deps-dir DIR` | Scan subdirectories as OTP applications. |
| `-o NAME` | Name the executable; default `a.out`. |
| `-e MODULE:FUNCTION` | Select the zero-argument entry point. |
| `--no-stdlib` | Disable automatic standard-library inclusion. |
| `-v` | Show compilation phases. |

Run `nera build --help` for the full option list. User modules shadow
standard-library modules with the same name.

## Projects and dependencies

Create a project with `nera new myapp`, then `cd myapp`.
To initialize an existing directory instead, run `nera init` there.
A minimal `nera.toml` declares the entry point and executable name:

```toml
[project]
name = "myapp"
entry = "myapp:start"
output = "myapp"
```

Run `nera build` from the project root to discover project sources and
`.nera/deps/`. Project builds also include `tests/`.
To choose directories explicitly:

```sh
nera build --src src/ -I include/ -e myapp:start -o myapp
```

A `tests/` directory beside the first `--src` directory is included too.
Keep this in mind when a build unexpectedly includes a test module.

Dependencies can come from Hex, Git or local paths. Add entries under `[deps]`
in the manifest; these illustrate syntax, not native compatibility guarantees:

```toml
[deps]
jsx = "3.1.0"
ranch = { git = "https://github.com/ninenines/ranch.git", tag = "1.8.0" }
utils = { path = "../shared_utils" }
```

Run `nera deps get` to fetch declared dependencies into `.nera/deps/`.
`nera deps add jsx 3.1.0` adds and fetches a pinned Hex package;
omitting the version requests the latest version.
Fetching or compiling a library does not establish that it runs correctly.
Check [project compatibility](../COMPATIBILITY.md) for recorded results.

For existing rebar3 projects, let rebar3 fetch dependencies, then point Nera
at its application directories:

```sh
rebar3 get-deps
nera build --src src/ -I include/ --deps-dir _build/default/lib \
  -e myapp:start -o myapp
```

Nera discovers each dependency's `src/` files and `include/` directory.
Explicit files, repeated `--src` directories and `--deps-dir` can be combined.

## Processes and examples

The [ping-pong example](../examples/ping_pong.erl) spawns a process, sends
messages with `!`, and waits for replies with `receive`. From a checkout:

```sh
nera build examples/ping_pong.erl -e ping_pong:start -o ping_pong
NERA_SCHEDULERS=1 ./ping_pong
```

It prints the exchanges and finishes with `Ping done`.
By default the runtime starts one scheduler worker per CPU core, capped at 255.
Set `NERA_SCHEDULERS` when launching the executable to choose 1–255 workers;
invalid values exit with status 2.

Follow the [Erlang developer walkthroughs](ERLANG_WALKTHROUGHS.md) for linked
processes, an OTP-style server and a JSON library. Each walkthrough explains
its limits; these examples do not prove general OTP compatibility.

## Checking and editor tools

`nera check` parses, preprocesses, lowers and validates sources without code
generation or linking. It reports errors and warnings, including unresolved
calls and undefined variables:

```sh
nera check examples/hello.erl
nera check
nera check --src src/ -I include/ --deps-dir _build/default/lib
```

The second form checks a project from its root. Editors should invoke checking
from the workspace root, preferably on the whole project, so cross-module and
include diagnostics have the right context. Use `--no-warnings` to suppress
warnings. A successful check does not establish native runtime compatibility.

The language server supports diagnostics, formatting and navigation.
Follow setup instructions for [VS Code](../editors/vscode-nera/README.md),
[JetBrains](../editors/jetbrains-nera/README.md) or
[Emacs](../editors/emacs-nera/README.md), including executable configuration.
You can use these tools while building and deploying with standard Erlang/OTP.
Release plugin assets are `vscode-nera-<version>.vsix` and
`jetbrains-nera-<version>.zip`; the editor guides explain installation.

## Debugging and cross-compilation

From a source checkout, inspect intermediate output before investigating linking:

```sh
nera build examples/hello.erl -e hello:start --emit-ir
nera build examples/hello.erl -e hello:start --emit-obj
nera build examples/hello.erl -e hello:start -o hello -v
```

`--emit-ir` prints the intermediate representation and stops;
`--emit-obj` writes object files without linking.

Cross-compilation needs a runtime for the target plus a suitable cross-linker
and target system libraries. For Linux x86-64 from another host, install the
Rust target and cross-toolchain first, then run from the checkout:

```sh
cargo build --release --locked -p nera-runtime --target x86_64-unknown-linux-gnu
nera build examples/hello.erl -e hello:start \
  --target x86_64-unknown-linux-gnu --emit-obj
nera build examples/hello.erl -e hello:start \
  --target x86_64-unknown-linux-gnu --linker x86_64-linux-gnu-gcc -o hello-linux
```

The target runtime is under `target/<triple>/release/`.
`--linker` overrides the linker command; otherwise Nera uses `$CC`, then `cc`.
A successful cross-build is not target execution. Run and check the output on
the target before claiming it works; platform execution limits are listed above.

## Packaging

From a source checkout, `./scripts/dist.sh` builds a relocatable host archive;
`--target <triple>` selects a cross-build and `--version <version>` sets its name.
The distribution contains:

```text
nera-<version>-<target>/
  bin/nera
  lib/libnera_runtime.a
  lib/nera/erlang/stdlib/*.erl
```

Move the whole tree, or copy only `bin/nera` for native builds. Avoid mixing
Nera versions: runtime libraries found beside the executable, in `../lib/`,
or through working-directory lookups take precedence over embedded copies.
Standard-library sources found through installed, workspace or working-directory
lookups also override embedded sources. Use a clean location and working
directory to rely on the compiler's built-in copies.

Extract and add `bin/` to `PATH` as in [Installation](#installation).
Generated executables need no BEAM installation, but native output does not
mean fully static operating-system libraries. Linking and deployment still
depend on the target's system libraries and ABI.

## Further documentation

- [Language support](../FEATURES.md): canonical implementation inventory, not a compatibility guarantee.
- [Compatibility](../COMPATIBILITY.md) and [known limits](KNOWN_LIMITS.md): scoped evidence and unsupported behavior, including Cowboy, TLS, distribution and hot upgrades.
- [Contributing](../CONTRIBUTING.md): builds, tests and bug reports; [project status](PROJECT_STATUS.md) retains historical baselines.
- [Testing plan](TESTING_PLAN.md): fuzzing and property-based testing guidance.
- [Compiler architecture](ARCHITECTURE.md): compiler and runtime design.
- [XMPP component roadmap](XMPP_COMPONENT_ROADMAP.md): planned static contract checking and component work.
- [MCP server design](MCP_SERVER.md): planned integration; there is no `nera mcp` command.
