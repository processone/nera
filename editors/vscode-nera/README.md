# vscode-nera

VS Code extension for Erlang projects using the [nera](../../README.md) compiler. Provides:

- **Syntax highlighting** for `.erl`, `.hrl`, `.xrl`, `.yrl`, and `.escript` files (bundled TextMate grammar — no other Erlang extension required)
- **LSP diagnostics** via `nera lsp` (parse errors, validation warnings)
- **Erlang language configuration** (comment toggling, bracket matching, auto-closing pairs)

## Prerequisites

- [Node.js](https://nodejs.org/) (v16+)
- [npm](https://www.npmjs.com/)
- The `nera` binary on your `$PATH` (or configure `nera.path` in settings)

## Build

```sh
cd editors/vscode-nera
npm install
npm run compile
```

This compiles the TypeScript source into `out/`.

## Install locally

### Option A: Symlink (recommended for development)

Create a symlink in the VS Code extensions directory:

```sh
# macOS / Linux
ln -s "$(pwd)" ~/.vscode/extensions/vscode-nera

# Then reload VS Code (Cmd+Shift+P → "Developer: Reload Window")
```

Changes to the grammar or language configuration take effect on reload. TypeScript changes require re-running `npm run compile` first.

### Option B: Package as .vsix

Install the packaging tool and build a `.vsix`:

```sh
npm install -g @vscode/vsce
vsce package
```

This produces `vscode-nera-0.1.0.vsix`. Install it with:

```sh
code --install-extension vscode-nera-0.1.0.vsix
```

## Development

Watch mode recompiles TypeScript on save:

```sh
npm run watch
```

## Configuration

| Setting     | Default | Description              |
|-------------|---------|--------------------------|
| `nera.path` | `nera`  | Path to the nera binary  |

## License

MIT. The bundled Erlang TextMate grammar is from [erlang-ls/grammar](https://github.com/erlang-ls/grammar) under the Apache-2.0 license — see [NOTICE](./NOTICE).
