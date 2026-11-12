# Linter Warnings Roadmap

Nera already has a solid warning system with ~22 warning types in `crates/nera-ir/src/warnings/`. This document tracks
additional warnings inspired by Erlang's `erl_lint`, Elvis, and Hank linter tools.

## Current Coverage

- Unused: variables, functions, records, types
- Undefined: functions, exports, record fields
- Unreachable clauses, shadowed variables, unsafe/unbound variables
- Illegal guards, binary segment validation, map construction
- Deprecated function calls, behaviour callbacks
- Duplicate functions/records, non-tail recursion
- Module name mismatch, exported variables
- Cross-module record usage tracking
- Pragma suppression (`@nera:nowarn`)

## Tier 1 — High Value (catch real bugs)

| Warning                  | Source   | Status | Description                                                                                                                                | 
|--------------------------|----------|--------|--------------------------------------------------------------------------------------------------------------------------------------------| 
| Format string validation | erl_lint | TODO   | Validate `io:format/2,3` format strings against argument count/types. Catches `io:format("~p ~p", [X])` (arity mismatch). Very common bug. |
| Missing spec on exports  | erl_lint | TODO   | Warn when exported functions lack a `-spec`. Improves documentation and enables better tooling.                                            |
| Unused imports           | erl_lint | TODO   | Detect `-import(Mod, [F/A])` where `F/A` is never called. Dead imports add confusion.                                                      |
| Unused macro definitions | Hank     | Done   | Detect `-define(M, ...)` macros that are never referenced. Similar to unused records.                                                      |
| Unused include files     | Hank     | TODO   | Detect `-include("file.hrl")` where no macro/record/type from that file is used.                                                           |
| Conflicting behaviours   | erl_lint | TODO   | Warn when implementing multiple behaviours with conflicting callback signatures.                                                           |

## Tier 2 — Medium Value (code quality)

| Warning                    | Source   | Status | Description                                                                           |
|----------------------------|----------|--------|---------------------------------------------------------------------------------------|
| Single-clause case         | Elvis    | TODO   | `case X of P -> E end` — should usually be a match or handled in function head.       |
| Redundant guard conditions | erl_lint | TODO   | Guards like `when is_list(X), is_list(X)` — duplicate conditions.                     |
| Unused record fields       | Hank     | TODO   | Record fields defined but never read or written anywhere in the project.              |
| No-effect expressions      | erl_lint | TODO   | Expressions whose result is discarded (e.g., `X + 1` as a statement with no binding). |
| Atom/keyword shadowing     | erl_lint | TODO   | Using atoms that are or will become reserved keywords.                                |

## Tier 3 — Nice to Have

| Warning                   | Source | Status | Description                                                      |
|---------------------------|--------|--------|------------------------------------------------------------------|
| Max function length       | Elvis  | TODO   | Warn when a function body exceeds N clauses or lines.            |
| Deep nesting              | Elvis  | TODO   | Warn when nesting depth (case/if/try/receive) exceeds threshold. |
| God module                | Elvis  | TODO   | Warn when module exceeds N exported functions.                   |
| Debug calls in production | Elvis  | TODO   | Detect `io:format` / `ct:pal` left in non-test code.             |
| Boolean comparison        | Elvis  | TODO   | `X == true` instead of just `X`, or `X /= false`.                |

## Out of Scope

- **Formatting / whitespace** — handled by `nera-fmt`
- **Code duplication detection** — better as standalone tool
