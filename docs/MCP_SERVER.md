# nera MCP Server — Design Document

## Motivation

Large language models generate better code when they have access to
project-specific semantic information. Today, AI assistants working on
Erlang projects must rely on raw file reads and generic text search to
understand a codebase. This leaves significant value on the table: the
nera compiler already knows the project's module graph, exported functions,
types, record shapes, and diagnostics. An MCP (Model Context Protocol)
server can expose that knowledge directly to any MCP-capable AI tool —
Claude Code, Cursor, VS Code Copilot, and others — enabling dramatically
better Erlang code generation, review, and refactoring.

### Why not just use the LSP?

LSP and MCP serve different consumers with different interaction patterns:

| Dimension        | LSP                          | MCP                               |
|------------------|------------------------------|------------------------------------|
| Consumer         | Human via IDE                | AI assistant                       |
| Interaction      | Real-time, keystroke-driven  | Query/action-driven, batch-friendly |
| Transport        | Persistent bidirectional     | Request/response (stdio or SSE)    |
| Granularity      | Single cursor position       | Whole-project or multi-file        |
| Output format    | IDE-specific structures      | JSON optimized for LLM context     |

An AI assistant asking "what functions does this module export and what are
their type signatures?" needs a single bulk response, not a series of
hover requests at manufactured cursor positions.

### Why not a generic IDE MCP (JetBrains MCP, etc.)?

A generic IDE MCP can provide open files and cursor context, but it cannot
expose nera's compiler internals: the project symbol index, type inference,
pattern match analysis, cross-module dependency graph, or Erlang-specific
diagnostics. A nera-native MCP provides **Erlang-semantic intelligence**
that no generic adapter can match, and it works with any MCP client
regardless of editor.

The two approaches are complementary: the IDE MCP provides editor state
(open files, selections, recent edits), while the nera MCP provides
compiler-grade project understanding.

## Architecture

### Transport and Invocation

```
nera mcp [--transport stdio|sse] [--port 8080]
```

The MCP server is a new subcommand alongside `nera lsp`, `nera check`, and
`nera build`. It shares the same compiler frontend crates (`nera-syntax`,
`nera-ir`, `nera-fmt`) and reuses the core analysis infrastructure that
powers the LSP.

**STDIO** (default): The AI tool spawns `nera mcp` as a child process and
communicates over stdin/stdout using JSON-RPC, identical to how LSP clients
launch `nera lsp`. This is the primary mode for Claude Code and similar
CLI-based AI tools.

**SSE** (optional): For web-based AI tools or remote setups, an HTTP
server on a configurable port serves the MCP protocol over Server-Sent
Events.

### Shared Analysis Core

The MCP server reuses the same analysis infrastructure as the LSP:

```
┌─────────────────────────────────────────────────┐
│                   nera binary                    │
│                                                  │
│  ┌──────────┐  ┌──────────┐  ┌───────────────┐  │
│  │ nera lsp │  │ nera mcp │  │ nera check/   │  │
│  │          │  │          │  │ build/fmt      │  │
│  └────┬─────┘  └────┬─────┘  └──────┬────────┘  │
│       │              │               │           │
│       └──────────────┴───────────────┘           │
│                      │                           │
│          ┌───────────▼────────────┐              │
│          │   Shared Analysis      │              │
│          │                        │              │
│          │  • ProjectIndex        │              │
│          │  • ParsedModuleCache   │              │
│          │  • Type inference      │              │
│          │  • Reference finder    │              │
│          │  • Diagnostics engine  │              │
│          │  • Formatter           │              │
│          └───────────┬────────────┘              │
│                      │                           │
│       ┌──────────────┼──────────────┐            │
│       ▼              ▼              ▼            │
│  nera-syntax     nera-ir       nera-fmt          │
│  (lex/parse)     (lower/check) (format)          │
└─────────────────────────────────────────────────┘
```

Key difference from the LSP: the MCP server does **not** maintain a
persistent document cache or listen for file-change notifications. Instead,
it rebuilds the project index on demand (with hash-based caching for
incremental performance) in response to each tool call. This stateless
design keeps the server simple and avoids stale-state bugs — AI
interactions are inherently request/response, not real-time streaming.

For repeated queries within a session, the parse cache (keyed by source
hash) ensures unchanged modules are never re-parsed.

## MCP Tools

### Phase 1 — Core (MVP)

These tools provide the highest value for AI code generation and review.
They map directly onto existing analysis infrastructure.

#### `nera/check`

Run the nera compiler frontend on the project and return structured
diagnostics.

```json
// Request
{
  "tool": "nera/check",
  "arguments": {
    "files": ["src/my_module.erl"],   // optional: limit to specific files
    "severity": "warning"              // optional: minimum severity filter
  }
}

// Response
{
  "diagnostics": [
    {
      "file": "src/my_module.erl",
      "line": 42,
      "column": 5,
      "end_line": 42,
      "end_column": 18,
      "severity": "error",
      "code": "undefined_function",
      "message": "function foo:bar/2 is undefined",
      "source": "nera"
    }
  ],
  "checked_files": 12,
  "cache_hits": 10
}
```

**Implementation**: Calls `check_for_lsp()` (or a shared variant) and
serializes the resulting diagnostics as JSON. The existing hash-based parse
cache makes repeated checks fast.

#### `nera/project-symbols`

Return the project symbol index: all modules, their exported functions
(with arities), records, and types.

```json
// Request
{
  "tool": "nera/project-symbols",
  "arguments": {
    "module": "gen_server_impl",    // optional: filter to one module
    "kind": "function"              // optional: function | record | type | module
  }
}

// Response
{
  "modules": [
    {
      "name": "gen_server_impl",
      "file": "src/gen_server_impl.erl",
      "exports": [
        { "name": "start_link", "arity": 1 },
        { "name": "init", "arity": 1 },
        { "name": "handle_call", "arity": 3 },
        { "name": "handle_cast", "arity": 2 }
      ],
      "records": [
        {
          "name": "state",
          "fields": ["socket", "buffer", "config"]
        }
      ],
      "types": []
    }
  ]
}
```

**Implementation**: Builds the `ProjectIndex` and `ParsedModuleCache`,
then serializes. Exports are already tracked; record fields and types
come from the parsed AST.

#### `nera/function-info`

Get detailed information about a specific function: clauses, inferred
spec, call sites, and documentation comments.

```json
// Request
{
  "tool": "nera/function-info",
  "arguments": {
    "module": "my_server",
    "function": "handle_call",
    "arity": 3
  }
}

// Response
{
  "module": "my_server",
  "function": "handle_call",
  "arity": 3,
  "exported": true,
  "deprecated": false,
  "spec": "-spec handle_call(term(), {pid(), term()}, #state{}) -> {reply, term(), #state{}}.",
  "clauses": [
    {
      "line": 45,
      "patterns": ["{get, Key}", "From", "State"],
      "guards": [],
      "body_summary": "maps:get/2 lookup, replies with value"
    },
    {
      "line": 52,
      "patterns": ["{put, Key, Value}", "From", "State"],
      "guards": [],
      "body_summary": "maps:put/3, replies ok"
    }
  ],
  "references": [
    { "file": "src/my_client.erl", "line": 23, "context": "gen_server:call(Pid, {get, Key})" }
  ],
  "source": "... full function source code ..."
}
```

**Implementation**: Combines `ProjectIndex` lookup, `infer_function_spec()`
from `type_infer.rs`, `find_remote_call_spans()` from `references.rs`,
and AST traversal for clause details.

#### `nera/references`

Find all call sites of a function across the project.

```json
// Request
{
  "tool": "nera/references",
  "arguments": {
    "module": "my_server",
    "function": "start_link",
    "arity": 1
  }
}

// Response
{
  "function": "my_server:start_link/1",
  "references": [
    {
      "file": "src/my_sup.erl",
      "line": 18,
      "column": 12,
      "context": "my_server:start_link(Config)"
    },
    {
      "file": "test/my_server_tests.erl",
      "line": 7,
      "column": 15,
      "context": "{ok, Pid} = my_server:start_link(test_config())"
    }
  ],
  "count": 2
}
```

**Implementation**: Reuses `find_remote_call_spans()` and
`find_local_call_spans()` from the existing LSP references module,
iterating over all `ParsedModuleCache` entries.

#### `nera/format`

Format Erlang source code and return the result.

```json
// Request
{
  "tool": "nera/format",
  "arguments": {
    "code": "-module(messy).\nfoo(X)->   case X of\n1 -> ok;\n_ ->error\nend.",
    "file": "src/messy.erl"    // optional: format a file in place
  }
}

// Response
{
  "formatted": "-module(messy).\n\nfoo(X) ->\n    case X of\n        1 -> ok;\n        _ -> error\n    end.\n"
}
```

**Implementation**: Calls `nera-fmt` on the input. Accepts either inline
code (for AI to format generated snippets before returning them) or a file
path.

### Phase 2 — Enhanced Context

These tools provide richer project understanding for more sophisticated
AI interactions.

#### `nera/module-graph`

Return the dependency graph between modules (who calls whom, include
relationships).

```json
// Request
{
  "tool": "nera/module-graph",
  "arguments": {
    "module": "my_server",      // optional: subgraph rooted here
    "depth": 2                   // optional: traversal depth
  }
}

// Response
{
  "nodes": ["my_server", "my_sup", "my_client", "gen_server"],
  "edges": [
    { "from": "my_sup", "to": "my_server", "kind": "call", "functions": ["start_link/1"] },
    { "from": "my_client", "to": "my_server", "kind": "call", "functions": ["start_link/1", "get/2"] },
    { "from": "my_server", "to": "gen_server", "kind": "behaviour", "callbacks": ["init/1", "handle_call/3"] }
  ]
}
```

**Implementation**: Walk all parsed modules collecting remote calls
and `-behaviour` attributes. The `ProjectIndex` already maps modules
to files; this adds edge information.

#### `nera/type-info`

Return inferred type information for a function or expression.

```json
// Request
{
  "tool": "nera/type-info",
  "arguments": {
    "module": "my_server",
    "function": "handle_call",
    "arity": 3
  }
}

// Response
{
  "spec": "-spec handle_call(term(), {pid(), term()}, #state{}) -> {reply, term(), #state{}} | {noreply, #state{}}.",
  "param_types": ["term()", "{pid(), term()}", "#state{}"],
  "return_type": "{reply, term(), #state{}} | {noreply, #state{}}",
  "source": "inferred"
}
```

**Implementation**: Extends `infer_function_spec()` from `type_infer.rs`
with richer output.

#### `nera/stdlib-docs`

Serve documentation for Erlang standard library modules and BIFs so the
AI does not hallucinate function signatures.

```json
// Request
{
  "tool": "nera/stdlib-docs",
  "arguments": {
    "module": "lists",
    "function": "foldl",         // optional
    "arity": 3                   // optional
  }
}

// Response
{
  "module": "lists",
  "function": "foldl",
  "arity": 3,
  "spec": "-spec foldl(Fun, Acc0, List) -> Acc1 when Fun :: fun((Elem :: T, AccIn) -> AccOut), Acc0 :: term(), Acc1 :: term(), AccIn :: term(), AccOut :: term(), List :: [T], T :: term().",
  "description": "Calls Fun(Elem, AccIn) on successive elements A of List, starting with AccIn == Acc0. Fun/2 must return a new accumulator, which is passed to the next call. The function returns the final value of the accumulator. Acc0 is returned if the list is empty.",
  "examples": ["lists:foldl(fun(X, Sum) -> X + Sum end, 0, [1,2,3,4,5])."]
}
```

**Implementation**: Bundles documentation extracted from the nera stdlib
modules (which are already embedded in the binary). Could also be served
as MCP resources rather than a tool.

### Phase 3 — AI Workflow Tools

These tools support end-to-end AI-assisted development workflows.

#### `nera/generate-spec`

Generate `-spec` attributes for functions that lack them.

```json
// Request
{
  "tool": "nera/generate-spec",
  "arguments": {
    "module": "my_server",
    "function": "handle_call",    // optional: all functions if omitted
    "arity": 3
  }
}

// Response
{
  "specs": [
    {
      "function": "handle_call/3",
      "line": 45,
      "spec": "-spec handle_call({get, term()} | {put, term(), term()}, {pid(), term()}, #state{}) -> {reply, term(), #state{}}."
    }
  ]
}
```

**Implementation**: Already exists as a code action in the LSP. Extract
and expose via MCP.

#### `nera/suggest-tests`

Analyze a module and suggest test cases based on function clauses,
pattern matches, and guard conditions.

```json
// Request
{
  "tool": "nera/suggest-tests",
  "arguments": {
    "module": "my_server"
  }
}

// Response
{
  "suggestions": [
    {
      "function": "handle_call/3",
      "cases": [
        "Call with {get, Key} when key exists in state",
        "Call with {get, Key} when key is missing",
        "Call with {put, Key, Value} — verify state is updated",
        "Call with unexpected message — verify error handling"
      ]
    }
  ]
}
```

**Implementation**: Walk the AST clause patterns and guards to enumerate
the cases the function handles, then generate descriptions. The AI can
then write the actual test implementations.

## MCP Resources

In addition to tools (which perform computation), the MCP server exposes
**resources** that provide static context:

| Resource URI               | Description                                      |
|----------------------------|--------------------------------------------------|
| `nera://project/modules`   | List of all modules with file paths               |
| `nera://project/config`    | Contents of `nera.toml` (dependencies, settings)  |
| `nera://stdlib/{module}`   | Source code of a stdlib module                     |
| `nera://module/{name}`     | Full source of a project module                    |

Resources are useful for AI tools that want to populate their context
window with project information before starting a conversation.

## MCP Prompts

The server can provide pre-built prompts that guide the AI toward common
Erlang development tasks:

| Prompt                    | Description                                       |
|---------------------------|---------------------------------------------------|
| `review-module`           | Review a module for correctness, OTP compliance, and style |
| `explain-supervision`     | Explain the supervision tree of the project        |
| `migrate-function`        | Help migrate a function to a different API          |

## Implementation Plan

### Step 1: MCP Transport Layer

- Add `nera mcp` subcommand to `main.rs`
- Implement STDIO JSON-RPC transport (similar to the existing LSP transport)
- Use the `rmcp` Rust crate for MCP protocol handling, or implement the
  thin JSON-RPC layer directly (the protocol is simpler than LSP)
- Register tool definitions (name, description, input schema)

### Step 2: Shared Analysis Extraction

- Extract the analysis core from `lsp/check.rs` into a shared module
  that both LSP and MCP can call
- The key function is `check_for_lsp()` which returns
  `(diagnostics, ProjectIndex, Vec<ParsedModuleCache>)` — rename to
  `check_project()` and make it usable outside the LSP context
- Similarly extract `infer_function_spec()` and reference-finding
  functions from their LSP-specific wrappers

### Step 3: Phase 1 Tools

- Implement `nera/check`, `nera/project-symbols`, `nera/function-info`,
  `nera/references`, `nera/format`
- Each tool: parse arguments, call shared analysis, serialize response
- Add integration tests that invoke the MCP server over STDIO

### Step 4: Phase 2 and 3 Tools

- Implement remaining tools incrementally
- Add MCP resources and prompts
- Add SSE transport option

### Step 5: Editor Integration

- Update `CLAUDE.md` or `.mcp.json` in nera projects to auto-configure
  the MCP server for Claude Code
- Document MCP setup for other AI tools

## Dependencies

**New crate dependencies (estimated):**

- `rmcp` or equivalent — MCP protocol handling (JSON-RPC, tool
  registration, schema generation). Alternatively, since the protocol is
  simple, implement directly with `serde_json` + `tokio::io`.
- No other new dependencies expected — `serde`, `tokio`, `serde_json`
  are already in the dependency tree.

**No new internal crates needed.** The MCP server lives in `crates/nera/`
alongside the LSP, sharing the same analysis modules.

## File Layout

```
crates/nera/src/
  main.rs               # Add `Mcp` variant to CLI enum
  mcp/
    mod.rs              # MCP server entry point and transport
    tools.rs            # Tool handler dispatch
    check.rs            # nera/check tool
    symbols.rs          # nera/project-symbols tool
    function_info.rs    # nera/function-info tool
    references.rs       # nera/references tool
    format.rs           # nera/format tool
    resources.rs        # MCP resource handlers
    types.rs            # MCP-specific request/response types
  analysis/
    mod.rs              # Shared analysis (extracted from lsp/check.rs)
    project_index.rs    # ProjectIndex building
    type_infer.rs       # Type inference (extracted from lsp/type_infer.rs)
    references.rs       # Reference finding (extracted from lsp/references.rs)
  lsp/
    ...                 # Existing LSP code, now calling into analysis/
```

## Success Metrics

- AI-generated Erlang code that uses nera MCP produces fewer compiler
  errors on first attempt compared to code generated from raw file reads
- AI can correctly identify all modules, exports, and record shapes in a
  project without reading every source file
- `nera/check` round-trip time under 500ms for a 50-module project
  (leveraging parse cache)
- The MCP server works with Claude Code, Cursor, and any other
  MCP-compatible AI tool without editor-specific code
