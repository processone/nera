# Vibe Coding Plan: Making Erlang AI-Friendly via the Compiler

## Problem Statement

Erlang's semantics (lightweight processes, pattern-matching message routing,
"let it crash" supervision) are arguably the best fit for network applications
of any language. But its syntax — particularly the comma/semicolon/period
punctuation rules — is hostile to the "vibe coding" workflow where an AI
generates code and a human skims rather than reads carefully.

The key insight: **most Erlang punctuation is redundant with the structure**.
Indentation, function names, and arity already tell you where clauses begin
and end. The punctuation merely confirms what is already obvious. A smart
compiler can reconstruct the intended structure from context.

## Goal

Make nera the best Erlang frontend for AI-assisted development by turning the
compiler from a gatekeeper into a collaborator.

## Features (in priority order)

### 1. Structured JSON Error Output (`--error-format=json`)

**Effort: Low | Impact: Huge**

Enable the AI-in-the-loop workflow: *generate → compile → read errors → fix → repeat*.

```json
{
  "file": "server.erl",
  "line": 42, "col": 18,
  "severity": "error",
  "code": "unexpected_comma_between_clauses",
  "message": "Use ';' to separate function clauses, ',' for expressions",
  "fix": { "replace": ",", "with": ";", "at": [42, 18] }
}
```

An AI agent applies the suggested fix without needing to understand Erlang
punctuation rules. The human never sees the error.

### 2. Contextual Error Messages with Fix Suggestions

**Effort: Medium | Impact: Huge**

Replace opaque parser-state errors with messages that describe intent:

```
-- BAD (traditional) --
server.erl:42: syntax error before: 'handle'

-- GOOD (contextual) --
server.erl:41: Function clause ended with '.' but line 42 continues
               with another clause of handle/2. Change '.' to ';'

  41 |     send(Response).
                          ^ change to ;
  42 | handle(post, Path) ->
```

Requires the parser to look ahead after a failure and consider alternative
interpretations rather than bailing out immediately.

### 3. Error Recovery (report multiple errors per compile)

**Effort: Medium | Impact: High**

Don't stop at the first error. After encountering a parse failure,
resynchronize (e.g., skip to the next function head or `-` attribute) and
keep parsing. One compile pass catches many issues, reducing round-trips
in the AI feedback loop.

### 4. Auto-Fix Mode (`--fix`)

**Effort: Medium | Impact: High**

The compiler fixes unambiguous punctuation errors itself:

```
$ nera server.erl --fix
  Fixed 3 issues:
    line 42: , → ; (between function clauses)
    line 67: ; → . (end of last clause)
    line 89: removed trailing comma in list
```

For the comma/semicolon/period problem, the correct token is almost always
deterministic from context:

- Between clauses of the same function → `;`
- After the last clause of a function → `.`
- Between expressions in a body → `,`

### 5. Tolerant Parsing Mode (`--tolerant`)

**Effort: High | Impact: Transformative**

Parse what was obviously intended rather than rejecting:

```erlang
handle(get, Path) ->
    Response = fetch(Path),
    send(Response).       %% wrong: should be ;
handle(post, Path, Body) ->
    store(Body),
    ok.
```

The tolerant parser sees `handle(post, ...)` after a `.` and recognizes it as
another clause of the same function. It emits a warning but compiles
successfully. This is analogous to Go's automatic semicolon insertion.

### 6. LSP Server for Real-Time Feedback

**Effort: High | Impact: High**

Provide diagnostics, fix suggestions, and auto-corrections live in the editor
as the AI (or human) types. Builds on features 1-3.

## Implementation Notes

- Features 1-3 are changes to the existing parser and error reporting in
  `nera-syntax`. No new crate needed.
- Feature 4 adds a rewriting pass that takes parser diagnostics and applies
  source-level patches. Could live in `nera-syntax` or a thin `nera-fix` layer.
- Feature 5 requires the parser to track "repaired" tokens and propagate
  confidence levels through later stages.
- Feature 6 is a separate binary (`nera-lsp`) wrapping the compiler frontend.

## Design Principle

> The compiler stops being a gatekeeper and becomes a collaborator. Traditional
> compilers say "you're wrong, figure it out." A vibe-coding compiler says
> "I know what you meant, here's what I did, and here's what you should know."
