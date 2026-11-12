# VS Code Reference Demo

This folder is a reference VS Code setup for editing Erlang code that targets
`nera`. It uses the **vscode-nera** extension, which provides:

- Erlang syntax highlighting (bundled TextMate grammar — no other extension needed)
- Live diagnostics via the `nera lsp` server
- Automatic error clearing when issues are fixed

## Prerequisites

Build the compiler from the repository root:

```bash
cargo build --release -p nera
```

Install the vscode-nera extension:

```bash
cd editors/vscode-nera
npm install
npm run compile
ln -s "$(pwd)" ~/.vscode/extensions/vscode-nera
```

Then open this folder in VS Code:

```bash
cd demo/editors/vscode
code .
```

## How It Works

The extension activates when it detects `nera.toml` in the workspace and starts
`nera lsp --transport stdio`. The workspace settings point `nera.path` to the
local build at `target/release/nera`.

- On workspace open: runs a full project check and publishes diagnostics.
- On file save: re-runs the check and updates diagnostics.
- Errors in `.hrl` files are reported with the correct file and location.

## Try The Workflow

1. Open `src/greeter.erl` — it should have syntax highlighting and no errors.
2. Introduce an error, for example change `message` to `mesage` — a diagnostic
   should appear in the Problems panel on save.
3. Fix the error — the diagnostic clears on save.

## Build / Run Tasks

The workspace also includes VS Code tasks for building and running the project
directly:

- `nera check` — fast front-end-only validation (default build task)
- `nera build` — compile the demo to a native executable
- `nera run` — build and run the demo

These tasks use a regex-based problem matcher and work independently of the LSP
extension. Run them via **Terminal → Run Task** or **Cmd+Shift+B** for the
default build task.

## Notes

The LSP server provides structured diagnostics and is the recommended approach.
The task-based problem matchers are a best-effort fallback against the
human-readable diagnostic output.
