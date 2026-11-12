# Developer Assistance Tools

Nera ships with a compiler and LSP server that provide a rich set of
developer-facing diagnostics, quick fixes, refactorings, and editor features.
This document catalogues everything that is currently supported.

---

## Diagnostics

All diagnostics are reported with source `"nera"` and carry a string code
that editors can match on.

### Parse / Syntax Errors

| Message | Severity |
|---------|----------|
| `unexpected token: expected {expected}, found {found}` | Error |
| `unexpected end of file` | Error |
| `invalid integer literal` | Error |
| `invalid float literal` | Error |
| `lexer error` | Error |
| `undefined macro: {name}` | Error |
| `unclosed conditional: -ifdef/-ifndef without -endif` | Error |
| `unexpected -{directive} without matching -ifdef/-ifndef` | Error |
| `cannot read include file: {path}` | Error |
| `macro recursion depth exceeded for {name}` | Error |
| `reserved keyword '{keyword}' cannot be used as {context}` | Error |
| `head mismatch: function '{name}' has arity {expected} ... but {found} here` | Error |

### Unused / Dead Code

| Code | Message | Severity | LSP Tag |
|------|---------|----------|---------|
| `unused_variable` | `variable '{name}' is unused` | Warning | Unnecessary |
| `unused_function` | `function '{name}/{arity}' is unused` | Warning | Unnecessary |
| `unused_record` | `record '{name}' is unused` | Warning | &mdash; |
| `unused_type` | `type '{name}' is unused` | Warning | &mdash; |
| `unreachable_clause` | `this clause in '{construct}' cannot be reached` | Warning | Unnecessary |

### Variable Scope and Binding

| Code | Message | Severity |
|------|---------|----------|
| (unbound) | `variable '{name}' is unbound` | Error |
| `shadowed_variable` | `variable '{name}' is shadowed by an outer binding` | Warning |
| `unsafe_variable` | `variable '{name}' unsafe in '{construct}'` | Error |
| `exported_var` | `variable '{name}' exported from '{construct}'` | Warning |

### Guards

| Code | Message | Severity |
|------|---------|----------|
| `illegal_guard_local_call` | `illegal call to '{name}/{arity}' in guard context` | Error |
| `illegal_guard_expr` | `illegal guard expression` / `illegal construct in guard` | Error |

### Pattern Matching

| Code | Message | Severity |
|------|---------|----------|
| `match_underscore_var` | `variable '{name}' should be prefixed with underscore to indicate intentional non-use` | Warning |

### Binary Segments

| Code | Message | Severity |
|------|---------|----------|
| `invalid_binary_segment` | `invalid binary segment: {details}` | Error |
| `conflicting_binary_specifiers` | `conflict in {type/sign/endianness/unit} specification for bit field: '{new}' and '{previous}'` | Error |

### Maps

| Code | Message | Severity |
|------|---------|----------|
| `illegal_map_construction` | `illegal map construction: found ':=' instead of '=>'` | Error |

### Function Definitions

| Code | Message | Severity |
|------|---------|----------|
| `duplicate_function` | `function '{name}/{arity}' already defined` | Error |
| `undefined_function` | `function '{name}/{arity}' is undefined` | Error |

### Records

| Code | Message | Severity |
|------|---------|----------|
| `redefine_record` | `record '{name}' already defined` | Error |
| `redefine_field` | `field '{name}' already defined in record '{record}'` | Error |
| `undefined_record_field` | `field '{name}' is undefined in record '{record}'` | Error |

### Module Structure

| Code | Message | Severity |
|------|---------|----------|
| `undefined_export` | `function '{name}/{arity}' listed in -export but not defined` | Error |
| `module_name_mismatch` | `module '{name}' does not match filename '{stem}'` | Warning |

### OTP Behaviours

| Code | Message | Severity |
|------|---------|----------|
| `missing_behaviour_callback` | `behaviour callback '{name}/{arity}' not implemented` | Warning |

### Recursion

| Code | Message | Severity |
|------|---------|----------|
| `non_tail_recursion` | `non-tail-recursive call to '{name}/{arity}'` | Warning |

Warns when a function calls itself recursively but not in tail position.
Such calls grow the stack on every iteration and are often unintentional.

### Deprecated Code

| Code | Message | Severity | LSP Tag |
|------|---------|----------|---------|
| `deprecated_function` | `function '{name}/{arity}' is deprecated` | Warning | Deprecated |

### Warning Suppression

Specific warnings can be suppressed per-function using a comment pragma:

```erlang
%% @nera:nowarn(non_tail_recursion)
fib(0) -> 1;
fib(1) -> 1;
fib(N) -> fib(N - 1) + fib(N - 2).
```

The pragma must appear in a comment line before the function definition.
Multiple pragmas can be stacked on separate lines. The code inside the
parentheses matches the diagnostic code (e.g. `unused_variable`,
`non_tail_recursion`, `shadowed_variable`).

---

## Quick Fixes

Quick fixes are code actions attached to a diagnostic. They appear in the
editor lightbulb menu and can usually be applied with a single shortcut.

| Diagnostic | Action | Description |
|------------|--------|-------------|
| `unused_variable` | **Rename to _{name}** | Prefixes the variable with `_` to suppress the warning. *(preferred)* |
| `unused_function` | **Add {name}/{arity} to -export list** | Inserts an `-export([{name}/{arity}]).` attribute after the `-module` declaration. *(preferred)* |
| `missing_behaviour_callback` | **Generate stub for {name}/{arity}** | Appends a callback function stub (returning `erlang:error(not_implemented)`) at the end of the module. *(preferred)* |
| `module_name_mismatch` | **Change -module({old}) to -module({stem})** | Rewrites the module declaration to match the filename. *(preferred)* |
| `module_name_mismatch` | **Rename file to {module}.erl** | Renames the file to match the declared module name. |

---

## Refactorings / Source Actions

Source actions are code actions that are not tied to a diagnostic. They
appear in the refactor or source sub-menus.

| Action | Description |
|--------|-------------|
| **Generate -spec for {name}/{arity}** | Infers parameter and return types from the function body and inserts a `-spec` attribute above the function. |

---

## Editor Features (LSP)

### Semantic Highlighting

Scope-aware token colouring with six token types and six modifiers.
Falls back to lexer-based tokens when the parser cache is stale.

**Token types:** Function, Variable, Namespace, Type, Property, Macro

**Modifiers:** Declaration, Definition, Unused, Exported, Deprecated,
DefaultLibrary

### Inlay Hints

Displays parameter names inline at function call sites, extracted from
the callee's clause patterns.

### Hover

Context-dependent hover information:

- **Functions** &mdash; clause signatures with parameter patterns
- **BIFs** &mdash; arity and auto-import status
- **Macros** &mdash; values of predefined macros (`?MODULE`, `?LINE`, etc.)
- **Record fields** &mdash; field definitions
- **Variables** &mdash; binding information

### Go to Definition

Navigates to the definition of the symbol under the cursor. Supports
functions (local and remote), variables, records, macros, include
directives, export entries, and type references.

### Document Highlight

Highlights all occurrences of the symbol under the cursor within the current
file. Distinguishes binding sites (Write) from use sites (Read):

- **Variables** &mdash; pattern positions (clause heads, `case`/`receive`/
  `try`/`fun` clause patterns, list-comprehension generators, `=` matches,
  `maybe ?=` bindings) are Write; expression and guard occurrences are Read.
- **Functions** &mdash; clause name spans are Write; local call sites, local
  fun refs, and remote refs to the current module within this file are Read.

### Find References

Finds all call sites and occurrences of a symbol across the project.
Works for functions (local and remote) and variables.

### Code Lenses

Shows reference counts above each function definition. Clicking the
lens triggers Find References.

### Document Symbols

Provides an outline view listing all functions (with arity and
export/local status) and records in the current file. Function clauses
are shown as children of their parent function. Recursive functions are
annotated with `recursive` in the detail field (e.g. `exported, recursive`).

### Workspace Symbols

Project-wide fuzzy search for functions (`module:name/arity`) and
records (`#name`), accessible via Cmd+T / Ctrl+T.

### Completions

Context-aware completion triggered by `:`, `#`, and `?`:

- Module names and remote functions (after `:`)
- Records and record fields (after `#`)
- Macros (after `?`)
- Local functions and BIFs (bare identifiers)
- In-scope variables
- Types (inside `-spec`/`-type`/`-callback` attributes)

### Signature Help

Shows parameter hints while typing inside a function call, triggered by
`(` and `,`. Displays clause patterns for project functions and arity
information for BIFs.

### Rename

Safe renaming of variables and functions.

- **Variables**: renames all occurrences within the enclosing clause.
  Validates that the new name starts with an uppercase letter or `_`.
- **Functions** (project-wide): rename a function from any of its
  occurrences — definition site, clause head, `-export` entry, `-spec` /
  `-callback` name, local call, remote call, or local / remote `fun` ref.
  Updates all clause heads, export entries, spec names, local call sites,
  local fun refs in the defining module, and all remote calls / remote fun
  refs in other project modules. Validates that the new name is a valid
  unquoted atom.

### Formatting

Full-document formatting powered by `nera-fmt`. Handles `.erl` source
files and term files (`.app`, `.app.src`). Indent size is configurable
via the editor.

### Breadcrumbs (JetBrains)

Shows the current scope context in the editor as a `module > function/arity
> clause` breadcrumb trail. Implemented as a JetBrains
`FileBreadcrumbsCollector` that pulls structure from the LSP
`textDocument/documentSymbol` response and caches it per document
modification stamp.

### Folding Ranges

Code folding for multi-line attributes (`-export`, `-type`, `-spec`,
etc.), function definitions, function clauses, record definitions, and
expression-level structures (`case`, lists, tuples, etc.).

---

## Roadmap: GoLand-Parity Developer Experience

The goal is to make Erlang development in JetBrains (and all LSP-compatible
editors) feel as polished as Go development in GoLand. GoLand's experience
rests on nine pillars: instant formatting, powerful refactoring, rich
navigation, deep code analysis, integrated testing, debugging, documentation,
project management, and profiling. This roadmap closes the gaps.

### P0 -- Table Stakes

Features that block daily developer workflow.

#### Include Management

Auto-add missing `-include`/`-include_lib`, auto-remove unused includes. The
Erlang equivalent of Go's `goimports`.

- **Layer**: LSP code action (`source.organizeImports`)
- **Size**: M

#### More Quick Fixes

Continue filling out the catalogue of diagnostic quick fixes. Implemented
so far: prefix unused variable with `_`, add function to `-export` list,
generate behaviour callback stub, fix module name / rename file. Still
wanted: fix arity hint.

- **Layer**: LSP code actions
- **Size**: M (remaining)

---

### P1 -- Professional Experience

Features that separate a toy editor from a professional development environment.

#### Call Hierarchy

Navigate the call graph -- who calls this function (incoming), and what does
this function call (outgoing). Building blocks exist in `references.rs`.

- **Layer**: LSP `callHierarchy/*` (3 handlers)
- **Size**: M

#### Extract Variable / Extract Function

Select an expression, extract it into a named variable or a new function.
Extract-function needs free-variable analysis to determine parameters.

- **Layer**: LSP code action (`refactor.extract`)
- **Size**: M (variable), L (function)

#### Inline Variable

Replace a variable binding with its value at all use sites within the clause.

- **Layer**: LSP code action (`refactor.inline`)
- **Size**: M

#### Navigate to Test

Jump from `foo.erl` to `foo_tests.erl` and back. Convention-based.

- **Layer**: JetBrains plugin (`GotoRelatedProvider`)
- **Size**: S

#### EDoc in Hover

Parse `@doc`, `@spec`, `@param` comments and render them as markdown in hover
popups. Requires preserving doc comments through the lexer (currently
discarded).

- **Layer**: LSP hover
- **Size**: M
- **Prerequisite**: Lexer change to preserve comments

#### Additional Inspections

Batch of 5-10 new warning passes: unused includes, dead code paths, boolean
simplification, redundant patterns, common anti-patterns.

- **Layer**: Compiler warning passes
- **Size**: M

#### Postfix Completion

Type `Expr.case` to wrap in a case expression, `List.hd` to wrap in
`hd(List)`, `Expr.spawn` for `spawn(fun() -> Expr end)`. Better implemented
as a native JetBrains `PostfixTemplateProvider`.

- **Layer**: JetBrains plugin or LSP snippets
- **Size**: M

#### Live Templates / Snippets

Predefined templates: gen_server skeleton, supervisor skeleton,
`-module/-export` boilerplate, EUnit test, Common Test suite.

- **Layer**: LSP snippets or JetBrains live template XML
- **Size**: S

#### Run Tests from Gutter Icons

Click a gutter icon next to a test function to run it; show pass/fail inline.

- **Layer**: JetBrains plugin (`RunLineMarkerContributor`)
- **Size**: L

#### Change Signature

Add, remove, or reorder function parameters with automatic call-site updates.

- **Layer**: LSP + JetBrains plugin
- **Size**: XL

#### Modernize Erlang Quick Fixes

A family of code actions that migrate legacy Erlang idioms to modern equivalents.
Each is offered as a refactor action on matching code patterns.

| Transform | Description |
|-----------|-------------|
| **Record → Map** | Convert a record definition and its usage sites (create, pattern, update, field access) to map syntax. |
| **List string → Binary string** | Convert `"string"` (list) usage to `<<"string">>` (binary) where safe. |
| **Introduce `maybe`** | Replace nested `case` chains with OTP 25+ `maybe ... else` expressions. |
| **`catch` → `try/catch`** | Convert old-style `catch Expr` to `try Expr catch ... end`. |
| **Replace deprecated BIFs** | Rewrite calls like `erlang:now()` to `erlang:monotonic_time()` / `erlang:system_time()`. |
| **`dict`/`gb_trees` → `maps`** | Migrate legacy `dict` and `gb_trees` calls to the `maps` module. |
| **Modernize `string` API** | Rewrite deprecated `string` calls (e.g. `string:tokens/2` → `string:lexemes/2`). |
| **Explicit fun references** | Convert inline `fun(X) -> foo(X) end` wrappers to `fun foo/1`. |

- **Layer**: LSP code actions (`refactor.rewrite`)
- **Size**: L (cumulative; each individual transform is S–M)

---

### P2 -- Premium Polish

Features that make the experience feel truly refined and complete.

#### Type-Aware Completion and Hover

Use inferred types to filter completions by expected type context; show
inferred types on hover. Requires expanding `HeuristicType` in
`type_infer.rs` with constraint propagation and unification.

- **Layer**: LSP + compiler type checker
- **Size**: XL

#### Code Coverage Visualization

Instrument codegen to track line-level execution; visualize coverage after
running tests with colored gutter markers.

- **Layer**: Codegen + JetBrains `CoverageEngine`
- **Size**: XL

#### Safe Delete

Check all usages before deleting a function. Warn if references exist.

- **Layer**: LSP code action
- **Size**: S

#### Move Function Between Modules

Move a function to a different module, updating all cross-module call sites
and export lists.

- **Layer**: LSP workspace edit
- **Size**: L

#### Dependency Tree Visualization

Visual tree of `nera.toml` dependencies in a tool window.

- **Layer**: JetBrains `ToolWindowFactory`
- **Size**: M

#### TODO/FIXME Tracking

Surface TODO, FIXME, HACK comments in the JetBrains TODO panel.

- **Layer**: JetBrains plugin (mostly built-in)
- **Size**: S

#### Spell Checking

Integrated spell checking in strings and comments.

- **Layer**: JetBrains `SpellcheckingStrategy`
- **Size**: S

#### EDoc Documentation Panel

Rendered EDoc documentation in a dedicated side panel, not just hover.

- **Layer**: JetBrains `DocumentationProvider`
- **Size**: M
- **Prerequisite**: EDoc in Hover (P1)

#### Generate Code

Generate test stubs for public functions, generate behaviour callback stubs
from the `-behaviour` attribute.

- **Layer**: LSP code action
- **Size**: M

#### Record Rename (project-wide)

Rename a record and all its references (create, pattern, update, access)
across the project.

- **Layer**: LSP `textDocument/rename`
- **Size**: M

#### External Documentation Links

Link from BIF/stdlib hover to erlang.org documentation.

- **Layer**: LSP hover (markdown links)
- **Size**: S

#### ~~Suppress Inspection Annotations~~ *(done)*

Implemented as `%% @nera:nowarn(warning_code)` comment pragmas. See the
**Warning Suppression** section above.

---

### P3 -- Advanced / Differentiating

Features that would put Nera ahead of what most Erlang tooling offers today.

#### Debugging via DAP

Debug Adapter Protocol support: breakpoints (line, conditional), step
over/into/out, variable inspection with deep term display, and -- uniquely
for Erlang -- per-process debugging (inspect individual process state,
mailbox, links/monitors).

This is the single most complex feature. Requires runtime breakpoint
injection via codegen, a DAP server binary, and stepping infrastructure.

- **Layer**: Runtime + DAP server + JetBrains plugin
- **Size**: XL

#### CPU/Memory Profiler

Profile nera programs, show flame graphs, identify allocation hotspots per
process.

- **Layer**: Runtime instrumentation + JetBrains plugin
- **Size**: XL

#### Remote Debugging

Attach to a running nera process on a remote machine.

- **Layer**: DAP extension
- **Size**: XL
- **Prerequisite**: Local debugging (P3)

#### Dataflow Analysis

Track data flow through the program to predict runtime errors (`badmatch`,
`badarg`, `function_clause`) before they happen. Erlang's dynamic typing
makes this harder than Go's static nilness analysis.

- **Layer**: Compiler analysis pass
- **Size**: XL
- **Prerequisite**: Type-Aware Completion (P2)

---

### Suggested Phasing

**Phase 1 -- Quick Wins (weeks 1-4):**
More Quick Fixes (batch of 5-6), TODO/FIXME Tracking, External Doc Links,
Suppress Inspection Annotations.

**Phase 2 -- Core Professional Features (weeks 5-12):**
Call Hierarchy, EDoc in Hover, Live Templates / Snippets, Include
Management, Record Rename.

**Phase 3 -- Refactoring Power (weeks 13-20):**
Extract Variable / Function, Inline Variable, Navigate to Test, Run Tests
from Gutter Icons, Generate Code.

**Phase 4 -- Type System and Advanced (weeks 21-32):**
Change Signature, Type-Aware Completion/Hover, Move Function Between
Modules, Breadcrumbs, Postfix Completion, Additional Inspections.

**Phase 5 -- Debugging and Profiling (weeks 33+):**
Debugging via DAP, Code Coverage Visualization, CPU/Memory Profiler.

---

### Key Observations

1. **22 of 32 features are pure LSP** -- implementing them in the Rust LSP
   server means VS Code and Emacs benefit simultaneously alongside JetBrains.

2. **The ref-finding infrastructure is the multiplier** -- `references.rs`
   already has everything needed for call hierarchy, safe delete, and
   record rename. These features share a foundation.

3. **JetBrains-specific work is minimal** -- only a handful of features need
   Kotlin plugin code: test navigation, gutter icons, coverage, postfix
   templates, TODO tracking, spell checking, dependency tree.

4. **Debugging (DAP) is the mountain** -- most complex single feature, but
   also the most impactful for production use. Worth architectural planning
   early even if delivered in a later phase.

5. **Erlang-specific opportunities** -- per-process debugging, mailbox
   inspection, and process-tree visualization are features that don't exist
   in Go tooling and could make the Erlang experience *better* than GoLand
   in concurrency-specific workflows.
