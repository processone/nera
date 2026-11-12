# emacs-nera

Emacs integration for Erlang projects using the [nera](../../README.md) compiler. Uses the built-in `erlang-mode` with Nera's LSP server for diagnostics, navigation, and more.

## Prerequisites

- Emacs 26+ (for `lsp-mode`) or Emacs 29+ (for built-in `eglot`)
- `erlang-mode` (ships with Erlang/OTP, or available via MELPA)
- The `nera` binary on your `$PATH`

## Setup with eglot (recommended, built-in since Emacs 29)

Add to your `init.el`:

```elisp
(require 'eglot)

(add-to-list 'eglot-server-programs
             '(erlang-mode . ("nera" "lsp")))

(add-hook 'erlang-mode-hook #'eglot-ensure)
```

This starts `nera lsp` automatically when you open any `.erl` or `.hrl` file.

## Setup with lsp-mode

Install `lsp-mode` from MELPA, then add to your `init.el`:

```elisp
(require 'lsp-mode)

(lsp-register-client
 (make-lsp-client
  :new-connection (lsp-stdio-connection '("nera" "lsp"))
  :major-modes '(erlang-mode)
  :server-id 'nera-lsp
  :priority 1))

(add-hook 'erlang-mode-hook #'lsp)
```

The `:priority 1` ensures Nera is selected over `erlang_ls` if both are registered.

## Features

The Nera LSP server provides the following capabilities:

| Feature              | eglot | lsp-mode |
|----------------------|-------|----------|
| Diagnostics          | yes   | yes      |
| Go to definition     | yes   | yes      |
| Find references      | yes   | yes      |
| Hover info           | yes   | yes      |
| Completion           | yes   | yes      |
| Signature help       | yes   | yes      |
| Code actions         | yes   | yes      |
| Semantic tokens      | yes   | yes      |
| Document symbols     | yes   | yes      |
| Workspace symbols    | yes   | yes      |
| Inlay hints          | yes   | yes      |
| Code lenses          | yes   | yes      |
| Folding ranges       | yes   | yes      |

## Custom nera binary path

If `nera` is not on your `$PATH`, specify the full path:

**eglot:**

```elisp
(add-to-list 'eglot-server-programs
             '(erlang-mode . ("/path/to/nera" "lsp")))
```

**lsp-mode:**

```elisp
(lsp-register-client
 (make-lsp-client
  :new-connection (lsp-stdio-connection '("/path/to/nera" "lsp"))
  :major-modes '(erlang-mode)
  :server-id 'nera-lsp
  :priority 1))
```

## Troubleshooting

### LSP server not starting

1. Verify `nera` is accessible: `M-x shell-command RET nera --version RET`
2. Check the LSP log buffer:
   - eglot: `*EGLOT (...) events*`
   - lsp-mode: `*lsp-log*`

### Conflicts with erlang_ls

If you also have `erlang_ls` configured, either remove it or ensure Nera has higher priority (`:priority 1` in lsp-mode, or list it first in `eglot-server-programs`).

## License

MIT
