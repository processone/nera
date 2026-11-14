# Error Messages and Diagnostics: Learning from Rust and Elm

> *"A compiler error is a conversation. The compiler knows what went wrong.
> The question is whether it can explain it in a way that respects the
> developer's time and intelligence."*

## The Problem

Erlang's error messages are one of the language's biggest adoption barriers.
At every level — compile-time, runtime, and logging — the developer experience
assumes deep OTP knowledge that newcomers don't have.

**Compile-time** (`erlc`):
```
my_module.erl:42: variable 'X' is unbound
my_module.erl:55: function foo/2 undefined
```
One line, no source context, no suggestion.

**Runtime crash report** (the wall of nested tuples):
```
=ERROR REPORT==== 24-Feb-2026::10:15:33 ===
** Generic server my_server terminating
** Last message in was {request,<<"data">>}
** When Server state == {state,undefined,0,[]}
** Reason for termination ==
** {badmatch,{error,timeout},
    [{my_module,handle_call,3,[{file,"my_module.erl"},{line,42}]},
     {gen_server,try_handle_call,4,[{file,"gen_server.erl"},{line,721}]},
     {gen_server,handle_msg,6,[{file,"gen_server.erl"},{line,750}]},
     {proc_lib,init_p_do_apply,3,[{file,"proc_lib.erl"},{line,226}]}]}
```
Understanding this requires knowing that `{badmatch,...}` means a pattern
match failed, that the list is a stack trace, that the tuples inside are
`{Module, Function, Arity, Location}`, and that the first two frames are your
code while the rest is OTP internals. A developer coming from Python, Go, or
Rust would be completely lost.

**Dialyzer** (the type checker people avoid running):
```
my_module.erl:42: The pattern {ok, _} can never match the type {'error','timeout'}
```
Technically correct, practically unhelpful. No source snippet, no suggestion.

nera controls the entire pipeline — parser, IR, codegen, and runtime. This
is a rare advantage that lets us fix error reporting at every stage.

---

## Design Principles

### From Rust: Precision and Structure

Rust's error messages are the industry gold standard. The key ideas:

**1. Show the code.** Every error includes the source snippet with line
numbers and carets pointing at the exact span. The developer never has to
open a file to understand the error.

**2. Explain the conflict.** When two things don't match (expected vs. actual
type, pattern vs. value), show both with connecting labels.

**3. Suggest the fix.** Don't just say what's wrong — show what to write
instead, as a diff-like suggestion.

**4. Progressive disclosure.** Short message first. Details in notes below.
Deep explanation available on demand via error codes.

**5. Consistent visual structure.** Every error follows the same format:
severity, code, message, source snippet, notes, suggestions. Muscle memory
develops quickly.

### From Elm: Empathy and Education

Elm pushes beyond Rust into developer empathy:

**1. Conversational tone.** "I was expecting X but found Y" instead of
"expected X, got Y." The compiler feels like a colleague, not a judge.

**2. Teach through errors.** Include explanations of *why* something is
wrong, not just *what* is wrong. A developer who reads an Elm error learns
something about the language.

**3. Category headers.** Every error starts with a clear human-readable
category (`TYPE MISMATCH`, `NAMING ERROR`, `MISSING PATTERNS`) so you know
what kind of problem you're dealing with before reading the details.

**4. Don't flood.** Show a small number of errors. Fix the most important
one first; cascading errors usually resolve.

### Our additions: Runtime context

Neither Rust nor Elm faces the problem of runtime crash reports in a
concurrent system. Erlang does, and this is where nera can innovate most:

**1. Format runtime errors like compile-time errors.** Same visual structure,
same source snippets (when available), same suggestion style.

**2. Filter OTP noise.** Stack traces should highlight user code and collapse
framework internals, not mix them equally.

**3. Show the value that caused the crash.** A badmatch error should show
the actual value that didn't match, formatted readably.

**4. Process context.** In a concurrent system, knowing *which* process
crashed and *what it was doing* matters as much as the error itself.

---

## Compile-Time Errors

### Current State

nera already has a solid foundation:

- `Span` on every AST node (byte offsets, `crates/nera-syntax/src/span.rs`)
- `SourceMap` with line/column lookup (`crates/nera-syntax/src/source_map.rs`)
- `Diagnostic` struct with `.render()` (`crates/nera-syntax/src/error.rs`)
- Source snippets with caret underlines
- `ParseError` enum with 10+ variants, each carrying a span

Current output:
```
error[hello.erl:4:12]: unexpected token: expected ',', found ')'
 4 |     io:format("Hello"))
                              ^
1 error emitted
```

This is good. The path forward is refinement, not rebuilding.

### Proposed Improvements

#### Category headers (Elm-style)

Replace the `error[file:line:col]` prefix with a clear category header,
keeping the location on the right:

```
-- SYNTAX ERROR --------------------------------- src/main.erl:4:12

Unexpected `)` — I was expecting a `,` to separate function arguments.

 4 |     io:format("Hello"))
                             ^

Hint: Remove the extra `)`, or add another argument before it.
```

```
-- UNDEFINED FUNCTION --------------------------- src/main.erl:15:5

I cannot find a function called `lisst:map/2`.

15 |     lisst:map(fun double/1, Items),
         ^^^^^^^^^

Did you mean `lists:map/2`?
```

```
-- UNDEFINED VARIABLE --------------------------- src/main.erl:8:12

The variable `Reuslt` is not defined in this scope.

 8 |     Total = Reuslt + Tax,
                 ^^^^^^

Did you mean `Result`? It was defined at line 6:

 6 |     Result = calculate(Price),
         ^^^^^^
```

#### Multi-span errors

When the problem involves two locations, show both:

```
-- ARITY MISMATCH ------------------------------- src/main.erl

The function `process/1` has clauses with different arities:

 10 | process(start) -> ok;
      ^^^^^^^^^^^^^^       this clause has 1 argument

 11 | process(stop, Force) -> ok.
      ^^^^^^^^^^^^^^^^^^^^    this clause has 2 arguments

All clauses of a function must have the same number of arguments.
```

#### Pattern exhaustiveness warnings

```
-- MISSING PATTERNS ----------------------------- src/main.erl:20:5

This `case` expression does not handle all possible values:

 20 |     case fetch(Url) of
 21 |         {ok, Data} -> process(Data);
 22 |         {error, not_found} -> default()
     |     end
     |     ^^^

Values not handled:
  - {error, timeout}
  - {error, _}   (any other error)

Hint: Add a catch-all clause:
    {error, Reason} -> handle_error(Reason)
```

#### Error count and ordering

Show errors one at a time during interactive use. In batch mode, cap at 10
errors and show a count:

```
... and 7 more errors.

Fix the errors above and recompile. Later errors are often caused by
earlier ones.
```

### Implementation

#### Files to modify

- `crates/nera-syntax/src/error.rs` — Extend `Diagnostic` with:
  - `category: &'static str` field ("SYNTAX ERROR", "UNDEFINED FUNCTION", etc.)
  - `secondary_spans: Vec<(Span, String)>` for multi-span errors
  - `suggestions: Vec<Suggestion>` where `Suggestion` has a span and replacement text
  - `notes: Vec<String>` for hints and explanations
  - Updated `.render()` to produce the new format

- `crates/nera-syntax/src/error.rs` — Add `Suggestion` struct:
  ```rust
  pub struct Suggestion {
      pub message: String,
      pub span: Option<Span>,        // Where to apply the fix
      pub replacement: Option<String>, // What to replace with
  }
  ```

- `crates/nera-ir/src/lower.rs` — Expand `LowerError` enum:
  - `ArityMismatch { name, arities, spans }`
  - `InvalidGuard { expr, span }`
  - `UnreachableClause { span }`
  - `NonExhaustiveMatch { missing_patterns, span }`
  - `UndefinedFunction { module, name, arity, span, did_you_mean: Option<String> }`

- `crates/nera/src/main.rs` — Levenshtein distance function for "did you
  mean?" suggestions. Check available functions/variables when an undefined
  error occurs.

#### Color support

Add ANSI color output gated on terminal detection:

- Red for error headers and carets
- Yellow for warning headers
- Cyan for note/hint headers
- Bold for error messages
- Dim for line numbers and border characters

Use the `supports-color` crate for terminal detection, or simple
`isatty()` check on stderr. Add `--color=always|never|auto` CLI flag.

```
crates/nera-syntax/src/style.rs  (new file)

pub enum Style { Error, Warning, Note, Hint, LineNumber, Code }
pub fn styled(text: &str, style: Style, color_enabled: bool) -> String
```

---

## Runtime Errors

This is where nera can make the biggest difference. The BEAM's runtime
errors are Erlang terms displayed raw — tuples and atoms meant for machine
processing, shown directly to humans. nera can do much better.

### Current State

Runtime crash output (`crates/nera-runtime/src/scheduler.rs`, lines 784-803):

```
Error in process <0.42.0>: {error, badarg}
Stacktrace (most recent call first):
  lists:member/2
  main:start/0
```

Information available:
- Process PID
- Exception class and reason (as ErlTerm)
- Call stack: module:function/arity (atom IDs via atom table)

Information missing:
- Source file path and line numbers
- The actual value that caused the crash
- Local variable values
- Argument values passed to the failing function
- Registered process name (if any)

### The Gap: Source Locations at Runtime

The critical missing piece is source location metadata in generated code.

**Current pipeline:**
```
Source (.erl)     →  AST (has Span)  →  IR (no Span)  →  native code (no Span)
     ✓                   ✓                  ✗                    ✗
```

Spans exist in the AST but are not propagated to the IR or codegen. At
runtime, stack frames only contain `(module, function, arity)` — no file
path, no line number.

**Proposed pipeline:**
```
Source (.erl)  →  AST (Span)  →  IR (Span)  →  codegen  →  native + source map
     ✓              ✓              ✓              ✓              ✓
```

### Proposed: Source Map for Runtime

#### Design

Generate a compile-time source map that maps native code addresses to source
locations. This is the same approach used by debuggers (DWARF info) and
JavaScript source maps, adapted for nera.

**Source map structure** (embedded in the binary or as a sidecar file):

```rust
// crates/nera-codegen/src/source_map.rs (new)
pub struct RuntimeSourceMap {
    /// Map from function (module, name, arity) to source file path
    functions: HashMap<(AtomId, AtomId, u32), FunctionSourceInfo>,
}

pub struct FunctionSourceInfo {
    pub file: String,
    pub line: u32,          // Function definition line
    pub source: String,     // The source text of the function (for snippets)
}
```

For the initial implementation, we don't need instruction-level mapping.
Function-level granularity (which function, which file, which line) is
enough to produce dramatically better error messages.

#### Propagating spans through IR

Add an optional span to IR instructions and blocks:

```rust
// In crates/nera-ir/src/ir.rs
pub struct Block {
    pub id: BlockId,
    pub instructions: Vec<Instruction>,
    pub terminator: Terminator,
    pub span: Option<Span>,  // NEW: source location for this block
}
```

The span doesn't need to be on every instruction. Block-level granularity
is sufficient: each block corresponds roughly to a clause or expression,
which is enough to point at the right source line in error messages.

#### Enriched stack frames

Extend `StackFrame` to include source location:

```rust
pub struct StackFrame {
    pub module: u32,
    pub function: u32,
    pub arity: u32,
    pub file_index: u16,    // NEW: index into file table
    pub line: u32,          // NEW: source line number
}
```

The file table is a compact array of file paths, indexed by `file_index`,
stored alongside the atom table at startup. The cost is 6 extra bytes per
stack frame — negligible.

### Proposed: Formatted Runtime Errors

#### badmatch

**Instead of:**
```
Error in process <0.5.0>: {error, {badmatch, {error, timeout}}}
Stacktrace (most recent call first):
  main:handle_request/1
  main:loop/1
```

**Show:**
```
-- BADMATCH ------------------------------------- <0.5.0>

The right side of `=` does not match the pattern on the left:

 42 |     {ok, Data} = fetch(Url),
          ^^^^^^^^^^   ----------
          pattern       value: {error, timeout}

The function `fetch/1` returned `{error, timeout}` but the pattern
expects a tuple starting with `ok`.

Hint: Use a `case` expression to handle both outcomes:

    case fetch(Url) of
        {ok, Data} -> process(Data);
        {error, Reason} -> handle_error(Reason)
    end

Stack:
  main:handle_request/1  src/main.erl:42
  main:loop/1            src/main.erl:28
```

#### function_clause (no matching clause)

**Instead of:**
```
Error in process <0.5.0>: {error, function_clause}
Stacktrace (most recent call first):
  main:process/1
  main:start/0
```

**Show:**
```
-- NO MATCHING CLAUSE --------------------------- <0.5.0>

No clause in `process/1` matches the arguments:

  Called with: process(pause)

The function has these clauses:

 15 | process(start) -> ...;
 16 | process(stop)  -> ...;

The value `pause` does not match `start` or `stop`.

Hint: Add a clause to handle `pause`, or add a catch-all:

    process(_Other) -> {error, unknown_command}

Stack:
  main:process/1  src/main.erl:15
  main:start/0    src/main.erl:8
```

#### case_clause (no matching case branch)

**Instead of:**
```
Error in process <0.5.0>: {error, {case_clause, 42}}
```

**Show:**
```
-- NO MATCHING CASE ----------------------------- <0.5.0>

No branch in this `case` expression matches the value `42`:

 30 |     case Type of
 31 |         text -> handle_text(Body);
 32 |         html -> handle_html(Body)
 33 |     end

The value `42` is an integer, but all branches match atoms.

Hint: Add a branch for unexpected values:

    _ -> {error, {unknown_type, Type}}

Stack:
  main:dispatch/2  src/main.erl:30
  main:handle/1    src/main.erl:22
```

#### badarg (bad argument to BIF)

**Instead of:**
```
Error in process <0.5.0>: {error, badarg}
Stacktrace (most recent call first):
  erlang:list_to_integer/1
  main:parse_id/1
```

**Show:**
```
-- BAD ARGUMENT --------------------------------- <0.5.0>

The function `list_to_integer/1` was called with an invalid argument:

 87 |     Id = list_to_integer(Input),
                                ^^^^^
              value: <<"not_a_number">>

`list_to_integer/1` expects a list of digit characters (e.g., "123"),
but received a binary.

Hint: If `Input` is a binary, use `binary_to_integer/1` instead:

    Id = binary_to_integer(Input)

Stack:
  erlang:list_to_integer/1  (BIF)
  main:parse_id/1           src/main.erl:87
```

#### badarith (arithmetic error)

**Instead of:**
```
Error in process <0.5.0>: {error, badarith}
```

**Show:**
```
-- ARITHMETIC ERROR ----------------------------- <0.5.0>

An arithmetic operation failed:

 23 |     Total = Price + Tax,
                  ^^^^^   ^^^
                  ok      10

Cannot add `ok` (atom) and `10` (integer).

The variable `Price` contains the atom `ok`, which is not a number.
This likely means an earlier function returned `ok` instead of a
numeric value.

Stack:
  main:calculate/2  src/main.erl:23
```

### Stack Trace Formatting

#### Filter framework internals

In OTP, stack traces mix user code with gen_server/supervisor internals.
nera should visually separate them:

```
Stack:
  main:handle_request/1  src/main.erl:42    ← your code
  main:loop/1            src/main.erl:28    ← your code
  --- gen_server internals ---
  gen_server:handle_msg/6
  proc_lib:init_p_do_apply/3
```

Or collapse them entirely by default:

```
Stack:
  main:handle_request/1  src/main.erl:42
  main:loop/1            src/main.erl:28
  (2 gen_server frames hidden — use --full-stack to show)
```

#### Registered name in header

When a process has a registered name, show it:

```
-- BADMATCH ------------------------------------- <0.42.0> (session_manager)
```

This immediately tells the developer which component crashed, without
needing to trace PIDs.

### Implementation

#### Phase 1: Function-level source map (high leverage, modest effort)

1. During codegen, build a `HashMap<(AtomId, AtomId, u32), FunctionSourceInfo>`
   mapping each compiled function to its source file and line number. The
   information is already available — function AST nodes have spans, and
   `SourceMap` converts spans to line numbers.

2. Embed the source map in the binary as a static data section (or write
   to a `.nera_srcmap` sidecar file).

3. At runtime, the crash reporter looks up the failing function in the
   source map and includes the file:line in the stack trace.

**Files:**
- `crates/nera-codegen/src/source_map.rs` (new) — Source map data structure
- `crates/nera-codegen/src/compiler.rs` — Build source map during compilation
- `crates/nera-runtime/src/scheduler.rs` — Use source map in crash reporter
- `crates/nera/src/main.rs` — Pass source map to runtime initialization

**Result:** Stack traces go from `main:start/0` to
`main:start/0  src/main.erl:12`. Immediate improvement, modest effort.

#### Phase 2: Formatted crash reports (high leverage, medium effort)

1. Extend the crash reporter in `scheduler.rs` to recognize common error
   patterns (`badmatch`, `function_clause`, `case_clause`, `badarg`,
   `badarith`, `if_clause`) and format them with category headers,
   explanations, and hints.

2. When the source map is available and the source file is accessible,
   include source snippets in the crash report (read the source line from
   the file at crash time, or embed function source text in the source map).

3. Use the same `Diagnostic`-style rendering as compile-time errors —
   consistent visual language across the entire developer experience.

**Files:**
- `crates/nera-runtime/src/crash_report.rs` (new) — Formatted crash reports
- `crates/nera-runtime/src/scheduler.rs` — Call new crash reporter
- `crates/nera-runtime/src/ffi.rs` — Capture argument values in BIF errors

**Result:** The examples shown above become reality. The biggest single
improvement to developer experience.

#### Phase 3: Enriched stack frames (medium leverage, medium effort)

1. Add `file_index` and `line` fields to `StackFrame`.
2. Modify `nera_rt_stack_push` to accept file and line parameters.
3. In codegen, pass the source location when inserting stack push calls.
4. The source map becomes unnecessary for stack traces (info is inline),
   but still useful for source snippets.

**Files:**
- `crates/nera-runtime/src/process.rs` — Extend `StackFrame`
- `crates/nera-runtime/src/ffi.rs` — Extend `nera_rt_stack_push`
- `crates/nera-codegen/src/func_translator.rs` — Pass location to stack push

**Result:** Every stack frame has file:line, even without the source map.
Enables future features like debugger breakpoints.

#### Phase 4: Color support

1. Add terminal detection (`isatty` on stderr).
2. Style module with ANSI escape codes.
3. `--color=always|never|auto` CLI flag.
4. Apply to both compile-time and runtime errors.

**Files:**
- `crates/nera-syntax/src/style.rs` (new) — Color/styling utilities
- `crates/nera-syntax/src/error.rs` — Use styles in Diagnostic::render()
- `crates/nera-runtime/src/crash_report.rs` — Use styles in crash reports
- `crates/nera/src/main.rs` — CLI flag and terminal detection

---

## Logging

### The Erlang Logging Problem

OTP's logger (since OTP 21) is a significant improvement over `error_logger`,
but the default output is still Erlang-term-formatted:

```
=NOTICE REPORT==== 24-Feb-2026::10:15:33.042 ===
    supervisor: {local,my_sup}
    started: [{pid,<0.189.0>},{id,worker_1},{mfargs,...}]
```

This is fine for Erlang developers using `observer` or `recon`. It's
alienating for anyone coming from structured logging in other ecosystems
(Python's `logging`, Go's `slog`, Rust's `tracing`).

### Proposed: Human-First Logging

nera should provide logging that looks like modern infrastructure logs
out of the box, while being machine-parseable when needed.

#### Default format (human-readable)

```
10:15:33.042 [info]  Listening on 0.0.0.0:5222
10:15:34.128 [info]  <0.42.0> (session_handler) started
10:15:35.891 [warn]  <0.42.0> (session_handler) mailbox depth: 1542
10:15:36.003 [error] <0.42.0> (session_handler) crashed: badmatch
                     at src/session.erl:87 in session:parse_id/1
                     value: {error, timeout}
```

Principles:
- One line per event for quick scanning
- Timestamp, level, process identity (PID + registered name)
- Source location for errors and warnings
- Multi-line detail indented under the first line

#### Structured format (`--log-format=json`)

```json
{"ts":"2026-02-24T10:15:36.003Z","level":"error","pid":"<0.42.0>",
 "name":"session_handler","msg":"crashed: badmatch",
 "file":"src/session.erl","line":87,"fn":"session:parse_id/1",
 "value":"{error, timeout}"}
```

Same information, machine-parseable. Compatible with `jq`, Datadog,
Elasticsearch, Grafana Loki, and every modern log aggregation system.

#### Log levels

Match the standard set that every ecosystem uses:

| Level     | Use                             |
|-----------|---------------------------------|
| `debug`   | Verbose development tracing     |
| `info`    | Normal operational events       |
| `notice`  | Significant but normal events   |
| `warning` | Unexpected but recoverable      |
| `error`   | Process crash, failed operation |

Default: `info` and above. Configurable via `--log-level` or environment
variable `NERA_LOG_LEVEL`.

#### Process lifecycle logging

nera can automatically log process lifecycle events without user code:

```
10:15:33.042 [debug] <0.42.0> spawned by <0.5.0>  (session:init/1)
10:15:33.043 [debug] <0.42.0> registered as session_handler
10:15:36.003 [error] <0.42.0> (session_handler) crashed: badmatch
10:15:36.004 [debug] <0.42.0> EXIT signal sent to <0.5.0> (linked)
10:15:36.005 [info]  <0.43.0> spawned by <0.5.0>  (session:init/1)  [restart]
```

This gives visibility into the supervision tree behavior without any
instrumentation code. Gated behind `debug` level so it's zero-cost in
production.

### Implementation

#### Phase 1: Basic logging API

```erlang
%% User API
logger:info("Connection accepted from ~s", [Addr]).
logger:warning("Queue depth ~p exceeds threshold", [Depth]).
logger:error("Failed to parse: ~p", [Reason]).
```

Implemented as BIFs that format the message and write to stderr. No
gen_event, no handler configuration — just direct output.

**Files:**
- `crates/nera-runtime/src/logger.rs` (new) — Log formatting and output
- `crates/nera-runtime/src/ffi.rs` — `nera_rt_log` FFI function
- `crates/nera-ir/src/bif_table.rs` — Register logger BIFs

#### Phase 2: Automatic process lifecycle logging

Add optional log calls at process spawn, register, crash, and exit points
in the scheduler. Gated behind a log level check (single atomic read —
zero cost when disabled).

**Files:**
- `crates/nera-runtime/src/scheduler.rs` — Log at spawn/exit/crash points
- `crates/nera-runtime/src/ffi.rs` — Log at register/unregister points

#### Phase 3: JSON output and log routing

- `--log-format=text|json` CLI flag
- `NERA_LOG_LEVEL` environment variable
- Optional file output (`--log-file=path`)

---

## Comparison: Before and After

### Compile-time: undefined function

**Before:**
```
error[main.erl:15:5]: unsupported expression: remote call lisst:map/2
15 |     lisst:map(fun double/1, Items),
         ^
1 error emitted
```

**After:**
```
-- UNDEFINED FUNCTION --------------------------- src/main.erl:15:5

I cannot find a function called `lisst:map/2`.

 15 |     lisst:map(fun double/1, Items),
          ^^^^^^^^^

Did you mean `lists:map/2`? (from the stdlib)

1 error emitted
```

### Runtime: badmatch

**Before:**
```
Error in process <0.5.0>: {error, {badmatch, {error, timeout}}}
Stacktrace (most recent call first):
  main:handle_request/1
  main:loop/1
```

**After:**
```
-- BADMATCH ------------------------------------- <0.5.0>

The right side of `=` does not match the pattern:

 42 |     {ok, Data} = fetch(Url),
          ^^^^^^^^^^   ----------
          pattern       value: {error, timeout}

Hint: Use `case` to handle both outcomes:

    case fetch(Url) of
        {ok, Data} -> ...;
        {error, Reason} -> ...
    end

Stack:
  main:handle_request/1  src/main.erl:42
  main:loop/1            src/main.erl:28
```

### Runtime: process crash in supervision tree

**Before:**
```
Error in process <0.42.0>: {error, {badarg, [{erlang,list_to_integer,1,...}]}}
```

**After:**
```
10:15:36.003 [error] <0.42.0> (session_handler) crashed

-- BAD ARGUMENT --------------------------------- <0.42.0> (session_handler)

`list_to_integer/1` received an invalid argument:

 87 |     Id = list_to_integer(Input),
                                ^^^^^
              value: <<"not_a_number">>

Expected a string of digits, got a binary. Use `binary_to_integer/1`
for binary input.

Stack:
  session:parse_id/1       src/session.erl:87
  session:handle_call/3    src/session.erl:54
  (2 gen_server frames hidden)

10:15:36.005 [info]  <0.43.0> restarted session_handler (1/5 restarts)
```

---

## Priority and Effort

| Phase | What | Impact | Effort |
|-------|------|--------|--------|
| **C1** | Category headers + "did you mean?" | High | Small |
| **C2** | Multi-span errors + suggestions | High | Medium |
| **C3** | Color support | Medium | Small |
| **C4** | Pattern exhaustiveness warnings | Medium | Large |
| **R1** | Function-level source map | High | Medium |
| **R2** | Formatted crash reports | Very high | Medium |
| **R3** | Enriched stack frames (file:line) | High | Medium |
| **L1** | Basic logging API | Medium | Small |
| **L2** | Automatic lifecycle logging | Medium | Small |
| **L3** | JSON output + log routing | Medium | Small |

Recommended order: **R1 → R2 → C1 → C3 → R3 → C2 → L1 → L2 → L3 → C4**

Start with runtime errors (R1, R2) because that's where the biggest pain
is and the biggest differentiation opportunity. Then improve compile-time
(C1, C3) for the first-use experience. Layer in the rest progressively.

---

## The Strategic Value

Error messages are not a feature — they're the product's personality. Every
developer forms their opinion of a language in the first hour, and most of
that hour is spent reading error messages.

Erlang's error messages say: "You should already know this. Figure it out."

nera's error messages should say: "Here's what went wrong, here's why, and
here's how to fix it."

This is not a polish item for later. It's a core differentiator from day one.
When someone compares nera to the BEAM, the first thing they'll notice isn't
performance or binary size — it's whether the compiler helps them or
frustrates them. Rust and Elm proved that great error messages are a
competitive advantage. nera should make them a priority.
