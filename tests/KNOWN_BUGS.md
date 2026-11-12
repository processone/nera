# Known Bugs & Parser/Syntax Limitations

Historical issues discovered during test development; the entries below are
resolved or could not be reproduced. This is not the active open-bug inventory.
See [project status](../docs/PROJECT_STATUS.md) for the 2026-10-10 failures,
confirmed preview blockers and small reproductions.

## Parser Bugs

### ~~1. `throw` keyword as atom in expression context~~ — FIXED
**Symptom**: `{throw, Reason}` fails to parse with "expected '(', found ','". The `throw` keyword token (`KwThrow`) in `parse_primary()` unconditionally expected `throw(Expr)`.
**Fix**: `parse_primary()` now checks for `(` after `throw` — if absent, treats bare `throw` as the atom `'throw'`.
**Scope**: Only `throw` was affected — `error` and `exit` are regular atom tokens, not keywords.
**Test**: `catch_tuple_body.erl`

### ~~2. `:=` in map patterns (match context)~~ — FIXED
**Symptom**: `#{key := Value} = M` fails with "expected `=>`, found `:=`".
**Fix**: `parse_map_field_expr()` now accepts both `=>` and `:=`. The parser builds a `MapField` with an `op` field; `expr_to_pattern()` conversion handles both operators.
**Test**: `map_pattern_match.erl`

## Runtime Bugs

### ~~3. `exit/2` kill signal not propagating through link chains~~ — FIXED
**Symptom**: `exit(Pid, kill)` kills the target process, but linked processes are not killed.
**Fix**: `process_exit_signals()` in `process.rs` properly processes EXIT signals after `drain_inbox` in the worker loop.
**Test**: `link_cascade.erl`

### ~~4. Selective receive with bound variable in tuple pattern~~ — FIXED
**Symptom**: `receive {Pid, Result} -> ...` where `Pid` is already bound does not correctly filter by PID — messages are returned in arrival order instead of matching by the bound variable.
**Root cause**: Pattern compiler only checked `seen_vars` (within-clause duplicates) for equality, not outer-scope bindings. Variables bound in outer scope were treated as fresh bindings.
**Fix**: Added `outer_bindings: HashMap<AtomId, VarId>` parameter to `compile_match`. Callers collect current scope bindings before pattern compilation. The pattern compiler checks `outer_bindings` in addition to `seen_vars` and emits equality checks using the stored VarId.
**Files**: `pattern.rs` (outer_bindings threading), `expr.rs` (match/case/receive collect bindings)
**Test**: `receive_bound_var.erl`

### ~~5. `is_function/2` not implemented~~ — FIXED
**Symptom**: `is_function(F, 2)` returns `[]` (empty list) instead of `true`/`false`.
**Fix**: Added `nera_rt_is_function2` FFI, BIF table entry, and apply dispatch.
**Test**: `is_function2.erl`

### ~~6. `binary_part/3` returns nil~~ — FIXED
**Symptom**: `binary_part(Bin, Pos, Len)` returns `[]` (nil) instead of the expected sub-binary.
**Fix**: `binary_part/3` was registered as `binary:part/3` but not as `erlang:binary_part/3` with `auto_import: true`. Added BIF table entry and apply dispatch.
**Test**: `binary_part.erl`

### ~~7. Unused variable warnings for bound variables in patterns~~ — FIXED
**Symptom**: Variables already bound in outer scope used in receive/case patterns trigger "unused variable" warnings.
```erlang
Pid = spawn(...),
receive {Pid, Result} -> Result end.  %% Warning: variable 'Pid' is unused
```
**Fix**: `collect_expr_refs` in `ast_walkers.rs` was not collecting variable references from clause patterns in case/receive/try/fun/maybe constructs. Added `collect_pattern_bindings` calls for clause patterns so outer-scope variables used for equality matching are counted as references.

### ~~8. Process exit reason not preserved for `error/1`~~ — CANNOT REPRODUCE
**Symptom**: When a process calls `error(boom)` and a monitor receives `{'DOWN', Ref, process, Pid, Reason}`, `Reason` may be `normal` instead of `boom`.
**Investigation**: Exhaustive testing of `error/1`, `exit/1`, `throw/1`, normal exit, closures-in-closures, error-after-receive (resume path), and 50 rapid concurrent errors — all propagate the correct exit reason. Code analysis confirms three exit paths in the scheduler all correctly capture and propagate exit reasons. May have been fixed as a side effect of the lock-free mailbox or exit signal processing work.
**Test**: `monitor_exit_reason.erl`

### ~~9. Exception from nested catch-clause body escapes outer try~~ — FIXED
**Symptom**: When an inner try's catch clause body raises an exception, it was propagating out of the whole function instead of being caught by the outer try. Example:
```erlang
try
    try throw(first) catch throw:first -> error(second) end
catch
    error:second -> caught  %% never reached
end.
```
**Root cause**: Codegen's `is_try_protected` flag was a local heuristic — it only checked for a `CheckException` instruction *later in the same block*. An inner catch-clause body lives in a block whose `CheckException` (the outer try's) is in a different block reachable only via CFG successors.
**Fix**: Compute `blocks_reach_check` — the set of blocks from which an exception-handling construct (`CheckException`, `GetExceptionClass`, or `GetExceptionReason`) is forward-reachable through the CFG. A call is try-protected if its own block has a later `CheckException` *or* any CFG successor reaches one.
**File**: `crates/nera-codegen/src/func_translator/mod.rs`
**Test**: `catch_in_catch.erl` (`inner_raises_outer_catches`)

### ~~10. Catch-clause re-raise loses original exception class~~ — FIXED
**Symptom**: When no catch clause matches, the caught exception was re-raised as `throw`, regardless of its original class. `try error(nope) catch error:other -> X end` would escape as `throw:nope` instead of `error:nope`.
**Root cause**: The re-raise path emitted `RaiseException(..., ExceptionClass::Throw, ...)` with a hardcoded class, discarding the original class and stacktrace.
**Fix**: Re-raise by calling `erlang:raise/3` with the already-captured `(class, reason, stacktrace)` so the original class is preserved.
**File**: `crates/nera-ir/src/lower/exception.rs`
**Test**: `catch_eclectic.erl` (`rethrow_unmatched`)

### ~~11. Catch-clause guards silently skipped for simple variable patterns~~ — FIXED
**Symptom**: `catch error:V when is_integer(V), V < 10 -> small` would always match (guard not evaluated) when the pattern was a bare variable.
**Root cause**: `is_simple_pattern` optimization branched straight to the clause body on class match, bypassing the match compiler that would have evaluated the guard.
**Fix**: `is_simple_pattern` now also requires `clause.guard.is_none()`.
**File**: `crates/nera-ir/src/lower/exception.rs`
**Test**: `catch_eclectic.erl` (`classify`)

### ~~12. `andalso` / `orelse` evaluated both sides~~ — FIXED
**Symptom**: `false andalso crashing_fn()` evaluated `crashing_fn()` and crashed. `true orelse crashing_fn()` same.
**Root cause**: Both operators were lowered as plain `Instruction::BinOp`, which evaluated both operands eagerly before the codegen's `select` picked a value. No short-circuit control flow was emitted.
**Fix**: In `lower_expr`, `Andalso`/`Orelse` now emit an explicit `Branch` on the LHS, evaluating the RHS only in the needed branch.
**File**: `crates/nera-ir/src/lower/expr.rs`
**Test**: `bool_ops.erl`, `catch_andalso_orelse.erl`

### ~~13. `throw` as bare atom in patterns rejected~~ — FIXED
**Symptom**: `case X of throw -> ... end` and `f(throw) -> ...` failed with a parse error. `throw` is a keyword token (for `throw(Expr)` expressions) but is *not* a reserved word in Erlang — it is a valid atom.
**Fix**: Added `Token::KwThrow` to `is_pattern_start` and to `parse_pattern_primary`'s atom cases.
**File**: `crates/nera-syntax/src/parser.rs`
**Test**: `catch_tuple_body.erl`
