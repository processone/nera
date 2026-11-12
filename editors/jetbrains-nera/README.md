# Nera Erlang — JetBrains Plugin

Language support for Erlang projects using the [nera](https://github.com/processone/nera) compiler.
Works with commercial JetBrains IDEs 2025.3 or later, such as IntelliJ IDEA Ultimate,
RustRover and CLion. The plugin uses the IDE's built-in LSP support and depends on the
commercial-IDE module (`com.intellij.modules.ultimate`), so IntelliJ IDEA Community is not supported.

## Features

- **Syntax highlighting** via bundled Erlang TextMate grammar
- **Comment toggling** (`Cmd+/` / `Ctrl+/`) with `%`
- **LSP diagnostics** from `nera lsp --transport stdio`

## Prerequisites

- A commercial JetBrains IDE, 2025.3 or later (build 253+)
- Java 21+ (for building)
- `nera` binary on your `PATH` (or set the `NERA_PATH` environment variable)

## Build

```bash
cd editors/jetbrains-nera
./gradlew buildPlugin
```

The distributable ZIP is generated in `build/distributions/`.

## Install

1. Get the plugin ZIP: download `jetbrains-nera-<version>.zip` from the
   [GitHub releases page](https://github.com/processone/nera/releases), or build it
   (see above) and use the ZIP from `build/distributions/`.
2. In your IDE go to **Settings → Plugins → ⚙️ → Install Plugin from Disk…**
3. Select the ZIP
4. Restart the IDE

## Development

```bash
./gradlew runIde
```

This launches a sandboxed IDE instance with the plugin loaded.

## Configuration

Go to **Settings → Languages & Frameworks → Nera** to set the path to the `nera` binary.

| Setting | Default | Description |
|---------|---------|-------------|
| Nera binary path | `nera` | Path to the nera executable (uses `NERA_PATH` env var as fallback) |

## License

This plugin is distributed under the MIT License.

The bundled Erlang grammar is from [erlang-ls/grammar](https://github.com/erlang-ls/grammar)
and is licensed under the Apache License 2.0. See [NOTICE](NOTICE) for details.
