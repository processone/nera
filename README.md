# nera — Erlang to native code

The goal is **Erlang that ships like Go**.

Nera compiles Erlang source into native executables that run without the BEAM
virtual machine. It provides its own runtime for Erlang processes and message
passing.

The compiler itself is a single executable: it embeds the matching runtime and
standard-library sources, so you can copy just `nera` for native builds.
You still need a C linker.

**Experimental.** Nera is not a drop-in replacement for Erlang/OTP and is not
ready for production. The first release, `v0.1.0`, is experimental;
Nera stays experimental until 1.0.

## Before you try it

Nera is tested on macOS arm64 and Linux x86-64. The macOS x86-64 and Linux arm64
archives are cross-built but have not been run. Windows does not work yet.

Cowboy, TLS, distributed Erlang and hot code upgrades are not supported. Other
OTP APIs are incomplete, and wrong results and hangs are still possible.
Read the [known limits](docs/KNOWN_LIMITS.md) and
[project compatibility](COMPATIBILITY.md) before choosing a workload.
A successful `nera check` does not establish that a program runs.

## Try it

For a prebuilt compiler, choose an archive for your platform from
[GitHub Releases](https://github.com/processone/nera/releases).
You still need a C linker (`cc`) to compile Erlang programs.
See the [installation guide](docs/USAGE.md#installation) for archive setup.

To build from source, install [Rust through rustup](https://rustup.rs/), Git
and a C linker: Xcode Command Line Tools on macOS, or GCC/Clang on Linux.
The checkout selects Rust 1.99.0.

Run this in a POSIX shell:

```sh
git clone https://github.com/processone/nera.git
cd nera
cargo build --release --locked -p nera-runtime
cargo build --release --locked -p nera
./target/release/nera build examples/hello.erl -e hello:start -o hello
./hello
```

Output:

```text
Hello, World!
```

Build the runtime first so the compiler can embed the matching library.
The `-e hello:start` option chooses `hello:start/0` as the entry point;
without it, Nera looks for `main:start/0`.

See the [command reference](docs/USAGE.md) for projects, dependencies,
debugging and cross-compilation.

## Examples

Start with the [Erlang developer walkthroughs](docs/ERLANG_WALKTHROUGHS.md):
a native program, linked processes, an OTP-style server and a JSON library.
Each example states what it demonstrates and what it does not prove.

For smaller examples, see [Hello World](examples/hello.erl),
[ping-pong](examples/ping_pong.erl) and the
[message-passing ring](examples/ring.erl).

## Editor tools

Nera provides diagnostics, formatting and navigation through its language
server and integrations for [VS Code](editors/vscode-nera/README.md),
[JetBrains](editors/jetbrains-nera/README.md) and
[Emacs](editors/emacs-nera/README.md).
You can use these tools while building and deploying with standard Erlang/OTP.
Follow the editor's setup guide to configure the Nera executable.

## Documentation and contributing

- [Language support](FEATURES.md): implementation inventory, not a compatibility guarantee.
- [Project status](docs/PROJECT_STATUS.md): recorded results and preview blockers, including historical baselines.
- [Contributing](CONTRIBUTING.md): builds, tests and bug reports.
- [Future direction](docs/XMPP_COMPONENT_ROADMAP.md): planned static contract checking and an XMPP component, not shipped features.
- [MCP server design](docs/MCP_SERVER.md): planned; there is no `nera mcp` command.

## License

[Apache License 2.0](LICENSE).
