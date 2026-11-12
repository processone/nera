# nera LSP Server

The nera LSP server provides IDE support for Erlang files in VS Code, JetBrains,
and any LSP-compatible editor. It runs the same compiler frontend used by `nera build`,
so diagnostics, navigation, and completions are always consistent with compiler behavior.

## Supported Features

### Navigation

| Feature | Description |
|---|---|
| **Go to Definition** | Functions (local and remote), variables, records, macros, `-export` entries, `-include` directives |
| **Find References** | All call sites of a function (local + cross-module), variable occurrences within a clause |
| **Workspace Symbols** | Project-wide search for functions and records (Cmd+T / Ctrl+T) |
| **Document Symbols** | Outline view of functions and records in the current file, with exported/local annotations |

### Editing

| Feature | Description |
|---|---|
| **Completion** | Context-aware: `module:` triggers function completion, `#` triggers record/field completion, bare identifiers complete local functions, auto-imported BIFs, module names, and in-scope variables |
| **Signature Help** | Parameter hints when typing inside function calls, showing clause patterns for project functions and arity info for BIFs |
| **Hover** | Function clause signatures, BIF info with auto-import status, record field definitions, macro names |
| **Inlay Hints** | Parameter names displayed inline at call sites, extracted from function clause patterns |

### Diagnostics

| Feature | Description |
|---|---|
| **Diagnostics** | Real-time errors and warnings from the nera compiler (push on save, pull on demand). Deprecated function calls are tagged with `DiagnosticTag.Deprecated` for strikethrough rendering |
| **Code Actions** | Quick fix to rename unused variables (prefix with `_`) |
| **Code Lenses** | Reference count shown above each function definition |

### Syntax

| Feature | Description |
|---|---|
| **Semantic Tokens** | Scope-aware syntax highlighting for functions, variables, namespaces (module names), types, record fields, and macros with declaration/definition/unused/exported/deprecated modifiers. Pattern variables already bound in the enclosing scope (equality checks) are visually distinguished from fresh bindings. Functions marked with `-deprecated([fun/arity]).` get strikethrough rendering at both definitions and call sites |

#### Semantic Token Legend

**Token types:**

| Type | Used for |
|---|---|
| `function` | Function definitions, local calls, remote call targets |
| `variable` | Pattern bindings, variable uses, catch class variables |
| `namespace` | Module name in remote calls (`module:function`) |
| `type` | *(planned)* Type names in `-spec` and `-type` attributes |
| `property` | Record field names in definitions, patterns, creates, updates, and access |
| `macro` | Macro references (`?MACRO`) — scanned from pre-expansion source |

**Token modifiers:**

| Modifier | Used for |
|---|---|
| `declaration` | Function definitions |
| `definition` | Function definitions, fresh variable bindings in patterns (variables already bound in the enclosing scope are treated as equality checks and do not receive this modifier) |
| `unused` | `_`-prefixed variables (editors typically dim these) |
| `exported` | Exported function definitions (distinguishes public API from local helpers) |
| `deprecated` | Functions tagged with `-deprecated([fun/arity, ...])` — definitions and call sites get strikethrough rendering, compiler emits warnings |

## Comparison with Other Erlang LSPs

| Feature | nera | ELP (Meta) | erlang_ls (archived) |
|---|:---:|:---:|:---:|
| Go to Definition | Yes | Yes | Yes |
| Find References | Yes | Yes | Yes |
| Hover | Yes | Yes (richer with types) | Yes |
| Completion | Yes | Yes | Yes |
| Diagnostics | Yes (own type checker) | Yes (linters, Eqwalizer) | Yes (compiler, Dialyzer) |
| Code Actions | Yes (unused var rename) | Yes | Yes |
| Semantic Tokens | Yes (6 types, 5 modifiers) | Yes | No |
| Document Symbols | Yes | Yes | Yes |
| Workspace Symbols | Yes | Yes | Yes |
| Code Lenses | Yes (ref counts) | Yes | Yes |
| Signature Help | Yes | Yes | Yes (off by default) |
| Inlay Hints | Yes (param names) | Yes | Yes |
| Formatting | No | Yes | Yes |
| Rename | No | Partial (types/macros) | Yes |
| Folding Ranges | No | Yes | Yes |
| Call Hierarchy | No | Yes | Yes |

### nera's Differentiator

The LSP server shares its compiler frontend (lexer, preprocessor, parser, type checker,
BIF table, warning analysis) with `nera build`. This means:

- **Diagnostics are authoritative** -- the same checks that fail a build are shown in the editor.
- **No drift** between compiler and editor behavior.
- **New compiler features automatically improve the LSP** -- add a type check to the compiler and hover/diagnostics get it for free.
- **Single codebase to maintain** -- unlike ELP (tree-sitter + Eqwalizer) or erlang_ls (separate Erlang implementation).

## Architecture

The server is implemented in `crates/nera/src/lsp/` using the `tower-lsp` framework
over STDIO transport. The module is split into focused submodules: `mod.rs` (backend struct
and entry point), `handlers.rs` (LSP protocol handlers), `check.rs` (project checking),
`types.rs`, `util.rs`, `cursor.rs`, `references.rs`, `source_map.rs`, `diagnostics.rs`,
and `semantic_tokens.rs`. Key data structures:

- **`ProjectIndex`** -- cross-file symbol index mapping functions, records, and modules to file paths. Rebuilt on every full project check.
- **`ParsedModuleCache`** -- cached parsed AST + source for each project module. Enables cross-file operations (find references, completion, inlay hints).
- **`DocumentCache`** -- per-file parsed module with source maps for offset translation between original and preprocessed sources.
- **`raw_text`** -- latest source text per file, updated on every keystroke regardless of parse success. Used by completion and signature help to work correctly during mid-edit syntax errors.

### Fault-Tolerant Parsing

The parser always produces a partial AST, even when the source file contains
syntax errors. On a parse error within a function or attribute, the parser
synchronizes on the next `.` (form boundary) and resumes. All successfully
parsed forms become valid AST nodes; only the erroneous forms are missing.

This means that if 1 out of 20 functions has a syntax error, the other 19
still have full semantic tokens, hover, goto-def, and index entries. The
`DocumentCache` and `ParsedModuleCache` always contain the latest partial AST,
so IDE features degrade gracefully rather than disappearing entirely during
editing.

### Request Flow

1. `did_open` / `did_change` -- caches raw text, parses + preprocesses, stores partial AST (always succeeds — errors are reported as diagnostics, valid forms are cached)
2. `did_save` -- triggers full project check (all files), rebuilds ProjectIndex and project module cache
3. Feature requests (hover, completion, etc.) -- read from caches, never block on parsing

## Roadmap

### Recently Completed

- **Fault-tolerant parsing** -- the parser always produces a partial AST alongside any errors. Valid forms are preserved even when other forms have syntax errors, so semantic tokens, hover, goto-def, and indexing continue working on the valid portions of the file during editing.
- **Lexer-based fallback semantic tokens** -- when the parsed AST cache is stale, lexer-based tokens provide syntax highlighting until the next successful parse.

### Short Term

- **Formatting** -- integrate with erlfmt or implement built-in formatter
- **Rename** -- safe rename of variables within a clause, functions across the project

### Medium Term

- **Call Hierarchy** -- incoming/outgoing call trees
- **Folding Ranges** -- code folding for functions, case expressions, records
- **Document Highlight** -- highlight all occurrences of the symbol under cursor
- **Clause-level recovery** -- within a function, recover at `;` boundaries so a broken clause doesn't lose sibling clauses

### Long Term

- **Type-aware hover and completion** -- leverage nera's type checker to show inferred types on hover and filter completion by expected type
- **Expression-level recovery** -- recover within expressions at `,`, `)`, `]`, `}`, `end`, `->` boundaries for mid-expression completions

## Editor Setup

### VS Code

The nera VS Code extension is in `editors/vscode-nera/`. It starts the LSP server
automatically when an `.erl` file is opened.

### JetBrains (IntelliJ / RustRover)

Configure an external LSP server pointing to the `nera lsp` binary. The nera LSP
communicates over STDIO.

### Other Editors

Any LSP-compatible editor can use nera by running `nera lsp` as a language server
over STDIO.
