# Nera Erlang — JetBrains Plugin

Language support for Erlang projects using the [nera](https://github.com/processone/nera) compiler.
Works with IntelliJ IDEA (Community & Ultimate), CLion, and other JetBrains 2025.1+ IDEs.

## Features

- **Syntax highlighting** via bundled Erlang TextMate grammar
- **Comment toggling** (`Cmd+/` / `Ctrl+/`) with `%`
- **LSP diagnostics** from `nera lsp --transport stdio`

## Prerequisites

- JetBrains IDE 2025.1 or later
- Java 21+ (for building)
- [LSP4IJ](https://plugins.jetbrains.com/plugin/23257-lsp4ij) plugin installed
- `nera` binary on your `PATH` (or set the `NERA_PATH` environment variable)

## Build

```bash
cd editors/jetbrains-nera
./gradlew buildPlugin
```

The distributable ZIP is generated in `build/distributions/`.

## Install

1. In your IDE go to **Settings → Plugins → ⚙️ → Install Plugin from Disk…**
2. Select the ZIP from `build/distributions/`
3. Restart the IDE

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
