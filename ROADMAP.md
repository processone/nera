# nera Roadmap

This document tracks planned improvements and future work for the nera compiler,
organized by priority and area.

**Restart note (2026-10-10):** this is a historical feature roadmap, not the
public-preview gate. Use [PROJECT_STATUS.md](docs/PROJECT_STATUS.md) for the
verified baseline, concrete blockers and next work. Completed checkboxes here
do not establish current release readiness.

**XMPP demonstrator direction (2026-10-14):** the
[positioning and component roadmap](docs/XMPP_COMPONENT_ROADMAP.md) defines the
first combined proof of Erlang source reuse, enforced static contracts and
native deployment. Its P0–P5 milestones have explicit acceptance criteria and
remain planned; the existing public-preview gates still apply.

---

## Error Reporting

The compiler now has source locations, snippets and runtime crash reporting.
This section preserves the diagnostic implementation roadmap. Project status
tracks remaining gaps and the repaired silent exit after a resumed exception.

### Current state

| Phase       | What exists                                                          | Location info                          |
|-------------|----------------------------------------------------------------------|----------------------------------------|
| Lexer       | `LexerError { position }`                                            | file:line:col via SourceMap            |
| Parser      | `UnexpectedToken`, `UnexpectedEof`, `InvalidInteger`, `InvalidFloat` | file:line:col via SourceMap + snippets |
| IR lowering | `UndefinedVariable(String, Span)`, `Unsupported(String, Span)`       | file:line:col via SourceMap + snippets |
| Codegen     | `CompileError::Codegen(String)`, `Module(String)`                    | None                                   |
| Linker      | `LinkFailed`, `LinkerNotFound` + captures stderr                     | N/A                                    |
| Runtime     | Prints `Error in process <0.N.0>: {class, reason}` to stderr         | Process PID + exception class/reason   |

Spans are tracked on every AST node (`Expr`, `Pattern`, `Function`, etc.) and
propagate to IR lowering errors via `LowerError` variants with `Option<Span>`.

### P0 — Critical for usability

- [x] **Line/column conversion**: `SourceMap` in `nera-syntax/src/source_map.rs` builds
      a line index from source text and converts byte offsets to `line:column` pairs.
      All parse errors now use `file:line:col` format.

- [x] **Source code context in errors**: Parse errors show the offending source line
      with carets (`^`) pointing at the error span. The `Diagnostic` type in
      `nera-syntax/src/error.rs` renders formatted diagnostics. Example:
      ```
      error[hello.erl:7:5]: unexpected token: expected '.', found atom ('io')
       7 |     io:format("World~n").
              ^^
      1 error emitted
      ```

- [x] **Runtime error reporting**: Scheduler prints `Error in process <0.N.0>:
      {class, reason}` to stderr on crash. All FFI functions raise proper exceptions
      (`badarith`, `badarg`, `badmap`, `badkey`) on invalid arguments — no silent
      nil/false/0 returns. Exception propagation through
      `emit_call_site_status_check` with try-protected block support. Process
      exits with code 1. Badmatch validation on `=` operator raises
      `{badmatch, Value}`.

- [x] **Propagate spans through IR**: `LowerError` variants carry `Option<Span>`.
      CLI renders them as `Diagnostic` with source snippets via `SourceMap`.
      Compile-time errors show file:line:col with caret pointing at the offending
      span.

### P1 — Important for developer experience

- [x] **Runtime stack traces**: Call-stack tracking now records
      `module:function/arity` frames per process via codegen-inserted
      push/pop hooks (`nera_rt_stack_push` / `nera_rt_stack_pop`).
      Exception-raising FFI captures a snapshot trace, and scheduler crash
      reports print it after `{class, reason}`.

- [x] **Reserved keywords check**: All 27 Erlang reserved words (`after`, `and`,
      `andalso`, `band`, `begin`, `bnot`, `bor`, `bsl`, `bsr`, `bxor`, `case`,
      `catch`, `cond`, `div`, `end`, `fun`, `if`, `let`, `not`, `of`, `or`,
      `orelse`, `receive`, `rem`, `try`, `when`, `xor`) are now rejected when
      used as unquoted atom names in function definitions, export lists, and
      expressions. Added `KwCond`, `KwLet`, `KwXor` keyword tokens,
      `ParseError::ReservedKeyword` with a helpful error message suggesting the
      quoted form, and `is_reserved_keyword()` guard. Non-reserved nera keywords
      (`module`, `export`, `import`) remain usable as atom names.

- [x] **Better parser error messages**: Added contextual `hint` field to
      `ParseError::UnexpectedToken` and `Diagnostic`, rendered as a note line in
      CLI output (via `codespan_reporting`'s `.with_notes()`) and appended to LSP
      messages. Covers four common Erlang mistakes:
      - Missing comma between function arguments (`expect_rparen_after_args()`)
      - Missing `->` after clause head (`expect_or(Arrow, ...)`)
      - Missing `end` for case/if/receive/try/begin/maybe/fun (`expect_or(KwEnd, ...)`)
      - Unexpected token at module level (hint: "each function or attribute must end with '.'")

- [x] **Structured codegen errors**: Replace `CompileError::Codegen(String)` with
      typed variants (`IsaSetup`, `FunctionCodegen`, `WrapperCodegen`,
      `MainCodegen`, `ObjectEmission`) that carry function name, arity, source
      line, and IR block ID. `TranslateError::Codegen` now propagates the
      current `BlockId` from the translator state.

- [x] **Warning system**: Compiler warnings printed to stderr, never block
      compilation. Implemented in `nera-ir/src/warnings.rs` via `analyze_module()`:
      - Unused variables (respects `_`-prefix convention)
      - Unreachable clauses (guardless catch-all detection)
      - Shadowed variables in nested scopes (fun clauses, list comprehensions)
      - Unused functions (not exported and not called locally)
      - [ ] `receive` without `after` timeout (potential deadlock hint)
      - [ ] Self-deadlock patterns (`self() ! Msg` followed by blocking receive)
      See also: [Pony learnings](docs/PONY_LEARNINGS.md) for the rationale behind
      compile-time safety hints inspired by Pony's static guarantees.

- [ ] **More IR lowering error variants**: The current 2 variants (`UndefinedVariable`,
      `Unsupported`) are not enough. Add errors for:
      - ~~Arity mismatch in function clauses~~ — **DONE** (parser `HeadMismatch`)
      - Invalid guard expressions
      - Non-exhaustive pattern matches (if detectable)
      - ~~Invalid binary segment specifications~~ — **DONE**

### P2 — Polish

- [x] **Colored output**: Adopted `codespan-reporting` for rustc-quality error
      display with colored output, source snippets, underlined spans, notes, and
      help text. Auto-detects tty for color support.

- [x] **Error count summary**: After compilation, print `N error(s), M warning(s)`
      if there were any.

- [ ] **Multi-file error context**: When an error involves multiple files (e.g.,
      cross-module call to undefined function), show locations from both files.

- [ ] **JSON error output**: `--error-format=json` for IDE/editor integration
      (LSP-compatible diagnostics).

---

## Compiler Diagnostics — OTP Parity

The OTP Erlang compiler (`erlc` / `erl_lint`) emits ~175 distinct diagnostics.
Nera currently checks for ~15.  This section tracks closing the gap, prioritised
by how likely the missing check is to hide real bugs.

### P0 — Errors that hide runtime crashes

- [x] **Unsafe variables**: Variables bound in some branches of
      `case`/`if`/`receive`/`try` but not all, then used after the expression.
      OTP emits `{unsafe_var, V, {What, Where}}`.  Nera emits
      `unsafe_variable` warnings explaining which construct makes the variable
      unsafe.

- [x] **Illegal guard expressions**: Guards must be side-effect-free
      (only BIFs, type tests, comparisons, arithmetic, boolean ops).  OTP emits
      `{illegal_guard_local_call, {F,A}}` and `illegal_guard_expr`.  Nera emits
      `illegal_guard` errors for non-guard-safe calls and constructs in `when`
      clauses.

- [x] **Export list validation**: `-export([foo/2])` referencing an undefined
      function should error.  OTP emits `{undefined_function, {F,A}}`.  Nera emits
      `undefined_export` errors.

- [x] **Duplicate function definitions**: Defining the same `f/N` twice should
      error.  OTP emits `{redefine_function, {F,A}}`.  Nera emits
      `duplicate_function` errors.

- [x] **Binary segment validation**: Unsized binary not at end, conflicting
      type/size/unit specs, invalid UTF segments, illegal bit sizes.
      OTP emits ~10 distinct binary-related errors. **Partial**: validates
      float size (16/32/64), signedness (integer-only), endianness (not on binary),
      size/unit on UTF types, unsized binary not at end.

- [x] **Module name vs filename mismatch**: `-module(foo)` in file `bar.erl`
      warns.  OTP emits `{module_name, Mod, Filename}`. Nera emits a warning
      (code: `module_name_mismatch`) with a hint suggesting the fix. Works in
      both CLI compilation and LSP.

### P1 — Important warnings for code quality

- [x] **Behaviour callback validation**: Modules declaring
      `-behaviour(gen_server)` should implement all required callbacks.
      OTP emits `{undefined_behaviour_func, {Func,Arity}, Behaviour}`.  Nera
      emits `missing_behaviour_callback` warnings for `gen_server`, `gen_statem`,
      `supervisor`, `application`, and `gen_event`. Supports both `-behaviour`
      and `-behavior` spellings. Unknown behaviours are silently skipped.

- [x] **Record redefinition**: Defining the same record name twice should error.
      OTP emits `{redefine_record, T}`.  Duplicate fields within a record
      should also error (`{redefine_field, T, F}`).

- [x] **Singleton/underscore variable matching**: `_V` already bound then
      used in a pattern match is often a bug.  OTP emits
      `{match_underscore_var, V}`.

- [x] **Exported variables**: Variables that "leak" out of all branches of
      `case`/`if`/`receive` — bound in every branch but fragile to use
      outside.  OTP emits `{exported_var, V, {What, Where}}` (opt-in).

- [x] **Map construction validation**: Using `:=` in map construction instead
      of `=>` should error.  OTP emits `illegal_map_construction`.

- [ ] **Stacktrace variable constraints**: Stacktrace variable in
      `catch Class:Reason:Stk` must not be previously bound or used in a guard.

- [ ] **Too many arguments**: Functions with >255 arguments should error.

### P2 — Nice-to-have polish

- [x] **Unused records**: Records defined but never used.
      OTP emits `{unused_record, T}`.

- [ ] **Unused types**: Types defined but never referenced.
      OTP emits `{unused_type, {T,A}}`.

- [ ] **Type/spec validation**: Undefined types, redefining built-in types,
      singleton type variables, specs for non-existent functions.

- [ ] **Missing type specs** (opt-in): Warn about exported functions without
      `-spec` annotations.

- [x] **Attributes after functions**: Module attributes appearing after
      function definitions.  OTP emits `{attribute, A}`.

- [ ] **Import validation**: Unused imports, import clashes with BIFs.

- [ ] **Optimization-pass warnings**: Expressions that always fail at runtime,
      patterns that can never match, pure function results that are ignored.
      (Requires Core Erlang–level analysis after IR lowering.)

---

## Compiler Safety — OTP Parity

The OTP BEAM VM provides strong runtime safety guarantees that native
compilation can violate. This section tracks gaps where valid-looking code could
compile but behave differently from OTP, or where missing checks allow unsafe
states that the BEAM would prevent.

### P0 — Can cause undefined behavior or silent wrong results

- [ ] **Closure type validation (`badfun`)**: `CallTarget::Closure(var_id)`
      assumes the variable holds a valid closure. If a non-fun value is called,
      Nera produces undefined behavior (likely memory corruption or segfault).
      OTP raises a clean `{badfun, Value}` exception. Fix: emit a runtime type
      check before closure calls — verify the term is a `BOXED_CLOSURE` and that
      the arity matches the argument count.

- [x] **Stack overflow protection for non-tail recursion**: `nera_rt_stack_push`
      checks the call-stack depth against `STACK_DEPTH_LIMIT` (10 000) and raises
      `{error, system_limit}` when exceeded, preventing segfaults from deep
      non-tail recursion.  Tail calls are unaffected (they reuse the frame).

- [ ] **IR safety verifier (beam_validator equivalent)**: OTP runs
      `beam_validator` as a final pass to verify that generated bytecode cannot
      crash the VM — it checks register liveness, type state at each instruction,
      and stack frame correctness. Nera has no equivalent IR-level verifier;
      a bug in lowering could produce IR that Cranelift compiles but that
      corrupts the process context or memory at runtime. Fix: add a verification
      pass on the IR that checks variable def-before-use, type consistency of
      operations, and block terminator validity before handing off to Cranelift.

### P1 — Compile-time warnings that prevent runtime surprises

- [ ] **Pattern exhaustiveness checking**: OTP warns when `case`/function clause
      patterns don't cover all possible inputs. Nera detects unreachable clauses
      (dead code after catch-all) but not the inverse — missing clauses. If no
      clause matches at runtime, the fail block raises `case_clause` or
      `function_clause`, but the developer gets no compile-time hint. Fix:
      implement exhaustiveness analysis (at minimum for literal atoms, tuples of
      known arity, and `true`/`false` in `if` expressions).

- [ ] **Guard BIF arity cross-validation**: The guard whitelist checks function
      names but may not validate arity against the BIF table. Calling
      `is_atom(A, B)` in a guard might not be caught at compile time.

### P2 — Correctness and compatibility

- [ ] **Binary bounds validation at compile time**: OTP's compiler validates
      binary segment sizes where possible (e.g., rejecting statically impossible
      sizes). Nera defers all bounds checking to runtime FFI functions.

- [ ] **`module_info/0,1` auto-generation**: OTP auto-generates `module_info/0`
      and `module_info/1` for every module. Code calling
      `Module:module_info()` (including OTP internals, observer, debugger) will
      get `undef` if these are not synthesized.

- [ ] **Spawn arity validation**: `spawn(Mod, Fun, Args)` validates that the
      (module, function) pair exists but cannot check arity at compile time since
      the args list length is dynamic. Consider a warning when the args list is a
      literal and the arity can be statically determined.

---

## Refactoring (Complete)

Post-Phase-2 consolidation — behavior-preserving, all tests pass throughout.

- [x] **FFI deduplication** (`ffi.rs`): `extract_numeric` helper (10 sites),
      pre-interned atom constants for `bool_to_term`/`raise_badarith`/`raise_badarg`,
      `binary_data`/`binary_data_mut` helpers (4 sites).
- [x] **IR lowering consolidation** (`lower.rs`): `patch_block_terminator` method
      (5 sites), extracted 5 sub-methods from `lower_expr` (631 → ~60 lines):
      `lower_expr_match`, `lower_expr_case`, `lower_expr_if`, `lower_expr_receive`,
      `lower_expr_binary`.
- [x] **Codegen FFI call helpers** (`func_translator.rs`):
      `emit_ffi_call`/`emit_ffi_call_void` with `declare_runtime_func_void` for
      void FFI. Replaced ~30 repetitive call sites.
- [x] **Dead code cleanup** (`lower.rs`): Removed unused `current_func` field.

---

## Code Generation Optimizations

### Direct linking for cross-module calls (Option B)

Currently, all cross-module calls go through `nera_rt_apply` (runtime function
table lookup). When modules are compiled together, the compiler could resolve
cross-module calls at link time via mangled symbol names, making them as fast as
local calls.

- [x] At codegen time, check if the target module is in `all_modules`
- [x] If yes, emit a direct call to the mangled symbol (declared as `Linkage::Import`)
- [x] Fall back to `nera_rt_apply` only for truly dynamic calls

### Other optimizations

- [x] **Inlining of small BIF calls**: `hd/1`, `tl/1`, small-int arithmetic
      (`+`, `-`, `*`, `div`, `rem`), comparisons (`<`, `>`, `=<`, `>=`, `=:=`,
      `=/=`, `==`, `/=`), bitwise ops (`band`, `bor`, `bxor`, `bsl`, `bsr`),
      `MakeCons`, and all type-check BIFs (`is_atom`, `is_list`, `is_pid`,
      `is_integer`, `is_binary`, `is_map`, `is_function`, `is_reference`,
      `is_number`, `is_boolean`, `is_bitstring`, `is_tuple`) inlined as
      Cranelift IR. Binary pattern ops (`BsMatchStart2`, `BsStartMatch`,
      `BinarySize`, `BsGetIntegerInline`) also emit inline Cranelift IR.
      Fast path avoids FFI call overhead; slow path falls back to FFI.

- [x] **Constant folding in IR**: Fold `1 + 2` to `3` at IR level.
      Integer arithmetic (`+`, `-`, `*`, `div`, `rem`) with overflow check,
      float division (`/`), bitwise ops (`band`, `bor`, `bxor`, `bsl`, `bsr`,
      `bnot`), all comparisons, unary `-` and `not`. Division by zero and
      integer overflow are preserved as runtime ops. Results chain-propagate.

- [x] **Dead code elimination**: Remove unreachable blocks after constant folding.
      Phase 1 simplifies constant branches (`Branch` on known true/false → `Jump`)
      and switches (`Switch` on known literal → `Jump` to matching case or default).
      Phase 2 removes blocks unreachable from the entry via BFS.

- [ ] **Register allocation hints**: Guide Cranelift's register allocator for
      hot variables (e.g., loop accumulators).

---

## Known Bugs

- ~~**Nested LC generators**: `[{X,Y} || X <- L1, Y <- L2]` — "undefined variable: Y"~~ — **FIXED**
- ~~**`_Var` convention**: `_X` treated as true wildcard, not as unused-but-bindable variable~~ — **FIXED**
- ~~**Map update syntax**: `M#{k := v}` not parsed (use `maps:put/3` workaround)~~ — **FIXED**
- ~~**Selective receive**: `receive` popped first message before pattern matching; non-matching messages were lost~~ — **FIXED**: three-BIF protocol (`receive_peek`/`receive_next`/`receive_accept`) scans mailbox without removing non-matching messages
- ~~**Exit code semantics**: `any_crash` flag set for `error()`/`throw()` in ANY process, causing exit code 1 even when the main process succeeds~~ — **FIXED**: only the initial (main) process determines the exit code via `initial_pid` tracking
- ~~**Multi-clause binary rest pattern**: In multi-clause functions like `f(<<"get ", R/binary>>) -> ...; f(<<"set ", R/binary>>) -> ...`, the variable `R` from one clause leaked to another due to shared variable scopes in the pattern compiler~~ — **FIXED**: push/pop scope per clause in `compile_column` isolates variable bindings
- ~~**String escape sequences**: `"\r\n"` was compiled as 4 literal characters instead of 2 bytes (CR+LF)~~ — **FIXED**: lexer regex updated for backslash escapes, `unescape_erlang_string()` added to parser
- ~~**iolist handling in gen_tcp:send**: Lists of binaries like `[<<"STORED">>, <<"\r\n">>]` produced null bytes~~ — **FIXED**: recursive `flatten_iolist` handles nested lists, binaries, and integers

---

## Language Features

### High priority (needed for real programs)

- [x] **Anonymous functions (funs)**: `fun(X) -> X + 1 end`, multi-clause funs,
      closures with free variable capture, local function references (`fun F/A`),
      and remote function references (`fun M:F/A`). Implemented via lambda
      lifting with heap-allocated BOXED_CLOSURE terms. Remote fun refs use a
      null fn_ptr sentinel and dispatch through `nera_rt_apply` at call time,
      with BIF fallback for `erlang:*` functions.

- [x] **Try/catch/after**: Exception handling with throw/error/exit classes.

- [x] **List comprehensions**: `[X * 2 || X <- List, X > 3]`. Basic support
      works; nested generators (`[{X,Y} || X <- L1, Y <- L2]`) fixed.

- [x] **Begin...end blocks**: Sequence of expressions returning the last value.

- [x] **Records**: Syntactic sugar over tuples. Parsed into dedicated AST nodes,
      desugared to `MakeTuple`/`TupleElement` during IR lowering. Supports
      creation, update, field access, record index, and pattern matching.
      Unknown attributes (`-type`, `-spec`, etc.) are now silently skipped.

### Medium priority

- [x] **Process links and monitors**: `link/1`, `unlink/1`, `spawn_link/1,3`,
      `spawn_monitor/1,3`, `monitor/2`, `demonitor/1`, `process_flag/2`
      (trap_exit). EXIT signal propagation from linked processes, DOWN message
      delivery from monitors. `spawn_monitor` atomically spawns and sets up a
      monitor, returning `{Pid, Ref}`. Extract-then-propagate pattern avoids
      deadlocks. Cascading kills for non-trapping links.

- [x] **Registered processes**: `register/2`, `whereis/1`, `unregister/1`,
      `registered/0`. Global name registry mapping atoms to PIDs, with
      `Name ! Msg` support via send_message atom resolution. Auto-cleanup
      on process death. Self-registration uses `ctx.otp_ptr` directly to
      avoid Mutex deadlock (same pattern as self-send).

- [x] **Reference type**: `make_ref/0`, `is_reference/1`. Heap-allocated
      BOXED_REFERENCE with unique ID. Full pipeline: term representation,
      GC copy, message deep-copy, term ordering, comparison, formatting.

- [x] **Process dictionary**: `put/2`, `get/1`, `erase/1`, `get_keys/0`.
      Per-process key-value store using `Vec<(ErlTerm, ErlTerm)>` with linear
      scan. Dictionary pointer wired through `ProcessContext` (same deadlock
      avoidance pattern as `heap_ptr`/`mailbox_ptr`). Fully GC-rooted with
      gather/scatter in `collect_garbage`.

- [x] **Type/spec declarations**: `-type`, `-spec`, `-opaque`, `-callback`,
      `-behaviour`, `-export_type`, and other unknown attributes are silently
      skipped by the parser. The skip logic is nesting-aware (tracks `()[]{}<<>>`)
      and uses span adjacency to distinguish the form-terminating `.` from `..`
      (integer ranges) and `...` (fun types). Real code with type annotations
      compiles without errors.

- [x] **Conversion BIFs**: `atom_to_list/1`, `list_to_atom/1`, `integer_to_list/1`,
      `list_to_integer/1`, `float_to_list/1`, `tuple_to_list/1`, `list_to_tuple/1`,
      `setelement/3`.

- [x] **Lists module**: Hot-path functions (`reverse`, `member`, `keyfind`,
      `keymember`, `keysearch`, `append/1`, `last`, `nth`) as Rust FFI BIFs;
      higher-order functions (`map`, `filter`, `foldl`, `foldr`, `sort`, etc.)
      as compiled Erlang in `erlang/stdlib/lists.erl`. 42 exported functions.

- [x] **Maps module**: Hot-path functions (`new/0`, `get/2,3`, `find/2`,
      `put/3`, `remove/2`, `is_key/2`, `keys/1`, `values/1`, `to_list/1`,
      `from_list/1`, `from_keys/2`, `size/1`, `merge/2`, `update/3`,
      `take/2`) as Rust FFI BIFs; higher-order functions (`map/2`,
      `filter/2`, `filtermap/2`, `fold/3`, `foreach/2`, `with/2`,
      `without/2`, `update_with/3,4`, `groups_from_list/2,3`,
      `intersect/2`, `intersect_with/3`) as compiled Erlang in
      `erlang/stdlib/maps.erl`. 28 exported functions total.

- [x] **OTP behaviours**: `gen_server`, `supervisor` (one_for_one strategy),
      `application`, `proc_lib`, and `error_logger` — implemented as compiled
      Erlang in `erlang/stdlib/`. Enough to run real OTP applications with
      supervision trees. `gen_server` supports `start_link`, `call`, `cast`,
      `reply` with full callback dispatch. `supervisor` supports `start_link`,
      `start_child`, max restart intensity tracking. Validated by compiling and
      running ProcessOne's OneCached memcached server.

- [x] **ETS tables**: `ets:new/2`, `ets:insert/2`, `ets:lookup/2`,
      `ets:delete/1,2`, `ets:delete_all_objects/1`, `ets:match_object/2`,
      `ets:select_delete/2`, `ets:tab2list/1`, `ets:info/1,2`, `ets:first/1`,
      `ets:next/2`. Named tables with `{keypos, N}` support.

- [x] **OTP 21+ storage primitives**: `persistent_term` (read-optimized global
      key-value store — `put/2`, `get/1,2`, `erase/1`, `info/0`), `counters`
      (lock-free atomic counter arrays — `new/2`, `get/2`, `put/3`, `add/3`,
      `sub/3`, `info/1`), and `atomics` (atomic 64-bit integers with CAS —
      `new/2`, `get/2`, `put/3`, `add/3`, `sub/3`, `add_get/3`, `sub_get/3`,
      `exchange/3`, `compare_exchange/4`, `info/1`). `persistent_term` reuses
      ETS deep-copy infrastructure; `counters`/`atomics` use direct pointer in
      reference term for zero-overhead access (on par with BEAM).

- [x] **Additional BIFs**: `integer_to_binary/1`, `binary_to_integer/1`,
      `list_to_binary/1`, `binary_to_list/1`, `iolist_to_binary/1`,
      `binary:split/2,3` (with `[global]`), `binary:part/3`,
      `erlang:system_time/1`, `erlang:monotonic_time/1`,
      `erlang:send_after/3`, `erlang:process_info/1,2`,
      `erlang:is_process_alive/1`, `io_lib:format/2`,
      `error_logger:info_msg/1,2`, `error_logger:error_msg/1,2`.

### Lower priority

- [x] **Preprocessor**: Token-level preprocessor operating between lexing and
      parsing. Supports `-define`/`-undef` (object-like and function-like macros),
      `?MACRO` expansion with argument substitution, predefined macros (`?MODULE`,
      `?LINE`, `?FILE`), conditional compilation (`-ifdef`/`-ifndef`/`-else`/`-endif`
      with nesting), and `-include("file.hrl")` with recursive preprocessing.
      Macro expansion has a depth limit to catch infinite recursion. The pipeline
      is now: source → lex → preprocess → parse → IR → codegen → link.

- [x] **Big integers**: Arbitrary-precision integers via `num-bigint`. Arithmetic
      overflow transparently promotes small ints (61-bit) to heap-allocated
      BOXED_BIGINT. All arithmetic (`+`, `-`, `*`, `div`, `rem`, negation) and
      bitwise (`band`, `bor`, `bxor`, `bsl`, `bsr`, `bnot`) operations handle
      bigints. Integer literals exceeding the small int range are allocated at
      runtime. `is_integer/1`, `is_number/1`, `=:=`, `integer_to_list/1`,
      `list_to_integer/1`, `abs/1`, and `io:format` all support bigints.

- [x] **Bit-level binaries**: Sub-byte binary segments (`<<X:3>>`). All IR
      instructions, codegen, and runtime operate at bit granularity with
      byte-aligned fast paths. Includes `bit_size/1` BIF and unit specifier
      support.

- [x] **UTF-8/16/32 binary segments**: Construction (`<<C/utf8>>`) and pattern
      matching (`<<C/utf8, Rest/binary>>`). Variable-width encoding with
      worst-case allocation and resize. All three encodings with endianness
      for UTF-16/32.

- [x] **Binary/list comprehensions**: `<< <<X>> || X <- List >>`.

- [ ] **Hot code loading**: Not a goal for v1 but worth considering for the
      long-term architecture.

---

## Type-Driven Analysis

The parser already accepts `-type`, `-spec`, `-opaque`, `-callback`, and
`-export_type` declarations (silently skipping them). The next step is to
actually parse their contents into an internal type representation and use it
for both compile-time diagnostics and code generation optimizations.

### P0 — Type representation and spec parsing

- [ ] **Internal type AST**: Define a `Type` enum in `nera-syntax` covering
      Erlang's type language: `integer()`, `float()`, `atom()`, `binary()`,
      `pid()`, `reference()`, `fun()`, `tuple()`, `list()`, `map()`, union
      (`T1 | T2`), `none()`, `any()`, `term()`, literal atoms/integers,
      ranges (`1..255`), record types, remote types (`module:type()`), and
      user-defined parametric types (`my_type(T)`).

- [ ] **Parse `-type` and `-spec` bodies**: Instead of skipping, parse the
      type expressions into the `Type` AST. Attach `-spec` signatures to their
      corresponding functions in the module AST. Build a per-module type
      environment mapping type names to their definitions.

- [ ] **`-spec` to function association**: Validate that every `-spec`
      matches an exported or local function by name and arity. Warn on
      orphaned specs (spec for a function that doesn't exist) and on
      arity mismatches.

### P1 — Compile-time diagnostics

- [ ] **Spec arity/clause mismatch warnings**: Warn when a `-spec` declares
      N argument types but the function has a different arity, or when the
      number of spec clauses doesn't match the function clause count (where
      applicable).

- [ ] **Obvious type violations**: Without full type inference, catch
      clear-cut violations at the call site:
      - Passing an atom literal where `-spec` expects `integer()`
      - Returning a value whose type visibly contradicts the spec's return type
      - Calling a function with the wrong number of arguments (already caught,
        but spec information can improve the error message)

- [ ] **Guard-informed narrowing**: Use guard expressions (`is_integer(X)`,
      `is_atom(X)`, `is_list(X)`, etc.) to narrow types within clause bodies.
      Warn when a guard is always true or always false given the spec
      constraints (dead clause detection).

- [ ] **Pattern exhaustiveness with types**: When a function has a `-spec`
      restricting the input to a union of known types (e.g.,
      `-spec f(ok | error) -> ...`), check that the function clauses cover
      all variants. Warn on missing clauses.

### P2 — Type-guided code generation optimizations

- [ ] **Unboxed arithmetic**: When `-spec` guarantees a value is `integer()`
      or `float()`, skip the tag-check branches in arithmetic operations.
      Emit direct integer/float instructions instead of the generic
      dispatch through `nera_rt_add` / `nera_rt_mul` etc.

- [ ] **Specialized comparisons**: When both operands are known to be the
      same type (e.g., two integers), emit a direct integer comparison
      instead of the generic `nera_rt_compare_terms` which handles
      cross-type ordering.

- [ ] **Tuple field access without bounds check**: When `-spec` specifies a
      fixed-size tuple (e.g., `{ok, integer()}`), `element(2, T)` can skip
      the runtime bounds check.

- [ ] **Map field access without key check**: When `-spec` declares a map
      type with required keys (e.g., `#{name := binary()}`), map field
      access can skip the key-not-found check and directly index into the
      map structure.

- [ ] **Call devirtualization**: When `-spec` constrains a fun argument to a
      specific signature (e.g., `fun((integer()) -> integer())`), and the
      call site passes a known fun reference, inline or directly call the
      target instead of going through `nera_rt_call_closure`.

- [ ] **Dead branch elimination**: When type information proves a `case` or
      `if` branch is unreachable (e.g., the `error` branch when the spec
      guarantees `ok`), eliminate it from the generated code.

### P3 — Integrated type inference (faster than Dialyzer)

Dialyzer is slow because it is a standalone whole-program tool: it builds a
monolithic PLT of all dependencies, runs global constraint solving across
every reachable function, and must re-analyze large chunks when anything
changes. Most Erlang developers skip it entirely because of this cost.

nera can do better by making type analysis an integral part of the compiler
pipeline rather than a separate tool:

- **No PLT**: nera already compiles all modules together in one invocation.
  Type summaries are computed as a natural by-product of compilation, not
  stored in an external database that must be rebuilt.
- **Per-function, not whole-program**: Infer types forward through the call
  graph within a module during IR lowering. No fixpoint iteration over the
  entire program — each function is analyzed once, in topological order.
- **Incremental**: Cache per-module type summaries (function signatures +
  inferred return types) to disk. On recompilation, only re-analyze modules
  whose source or dependencies changed. Cross-module callers just read the
  cached summary.
- **Optimistic, not exhaustive**: Focus on the common profitable cases
  (arithmetic on known integers, tuple access on known shapes, guard-narrowed
  types) rather than trying to model every Erlang type corner case. Unsound
  in theory but correct in practice for the optimizations we emit — if the
  type info is wrong, we simply fall back to the generic path at runtime.

- [ ] **Local type propagation**: Infer types from literals, guards, and
      pattern matches within a function body. Use this for the same
      optimizations as P2 without requiring `-spec` annotations. This runs
      during IR lowering (one pass, no fixpoint) and annotates each VarId
      with a `TypeInfo` enum.

- [ ] **Cross-function inference**: After lowering all functions in a module,
      propagate return types through the intra-module call graph in
      topological order. When `f/1` always returns an integer (all clauses
      return integer literals or arithmetic), callers of `f/1` benefit
      automatically. O(functions) not O(program).

- [ ] **Cross-module type summaries**: Emit a compact `.nera_types` sidecar
      file per module containing exported function type signatures (inferred
      or from `-spec`). When compiling a caller module, read the callee's
      summary instead of re-analyzing it. Invalidate on source hash change.

- [ ] **Success typing (lightweight)**: Warn when a call can be proven to
      *definitely* fail (e.g., passing an atom to a function whose every
      clause pattern-matches on integers). Same principle as Dialyzer's
      success typing, but scoped to the current compilation unit and
      running in the same pass as IR lowering — no separate tool invocation.

---

## Alternative Input Formats

Currently nera only accepts `.erl` source files. Supporting additional input
formats would unlock the OTP ecosystem without requiring full OTP compatibility
in the preprocessor (e.g., parse transforms, `-include_lib` with OTP paths).

### Core Erlang (`.core`) — High priority

Core Erlang is the intermediate functional IR emitted by `erlc +to_core`. It is
a small, stable, well-documented language with a handful of constructs (case,
let, letrec, apply, call, receive, etc.) that maps naturally to nera's IR.

**Why it matters**: Any valid Erlang module — including those using macros,
includes, parse transforms, and all OTP features — can be compiled to `.core`
by the standard Erlang compiler. This gives nera access to the entire OTP
ecosystem without reimplementing the preprocessor, macro system, or parse
transforms.

Pipeline: `.erl → erlc +to_core → .core → nera Core Erlang frontend → IR → Cranelift → native`

- [ ] **Core Erlang parser**: Parse the text-based Core Erlang format into an
      AST. The grammar is small (~15 node types) and well-specified in the
      [Core Erlang spec](https://www.it.uu.se/research/group/hipe/cerl/).
- [ ] **Core Erlang → IR lowering**: Translate Core Erlang AST to nera IR.
      Core Erlang's `case` maps to Branch, `let` to variable bindings, `letrec`
      to function definitions, `apply` to local calls, `call` to remote calls.
- [ ] **CLI integration**: `nera file.core -o output` auto-detects Core Erlang
      input by extension and routes through the Core Erlang frontend.
- [ ] **`--from-core` pipeline**: `nera --from-core file.erl` that shells out to
      `erlc +to_core` first, then compiles the resulting `.core` file. One-step
      compilation from Erlang source through the OTP frontend.
- [ ] **Gleam-to-native compilation**: Use the Core Erlang input path to compile
      Gleam programs to native executables. Target pipeline:
      `.gleam → Gleam Erlang output → erlc +to_core → .core → nera → native`.
      Validate an end-to-end Gleam example, identify required runtime and
      standard-library support, and document supported dependencies and limits.

### BEAM Bytecode (`.beam`) — Lower priority

BEAM files contain the compiled bytecode, atom table, literal table, export
table, and other metadata produced by `erlc`. This is a more complex input
format than Core Erlang but could be useful for compiling pre-built OTP
releases or `.beam`-only distributions.

**Tradeoffs**: The BEAM instruction set has 150+ opcodes that evolve across OTP
releases. Pattern matching has already been compiled down to jump tables and
select sequences, losing high-level structure. Core Erlang is generally the
better input format, but BEAM support would handle cases where only `.beam`
files are available.

- [ ] **BEAM file parser**: Parse the IFF-based BEAM file format (chunks: Atom,
      Code, StrT, ImpT, ExpT, FunT, LitT, etc.).
- [ ] **BEAM bytecode → IR lowering**: Translate BEAM register-based instructions
      to nera's SSA-based IR. Requires reconstructing control flow from jump
      targets, mapping x/y registers to SSA values, and handling BEAM's
      allocation instructions.
- [ ] **CLI integration**: `nera file.beam -o output` auto-detects BEAM input
      by extension.
- [ ] **OTP version compatibility**: Document and test which OTP versions'
      BEAM format is supported (instruction set changes across major releases).

---

## Runtime Improvements

- [x] **Execution budget enforcement (yield)**: self-tail-call loops now check
      reductions and yield via continuation propagation (same mechanism as
      `receive_wait`). This prevents tight loops from monopolizing a worker.
      Covered by scheduler unit tests and `tests/scheduler_yield.erl`.

- [x] **GC tuning**: Two-generation (young/old) copying GC. Young gen uses
      multi-chunk `Vec<u64>`-backed bump allocator. Minor GC promotes live data
      to old gen; major GC copies everything. Adaptive threshold (2× live data,
      min 256 KiB). Per-process configurable limits via `process_flag/2`
      (`fullsweep_after`, `max_heap_size`). No write barrier needed (Erlang
      immutability). Result: tcp_ping_pong -15%, RSS -56%.

- [x] **Shared binaries**: Large binaries (>64 bytes) use `Arc<Vec<u8>>`
      reference counting instead of deep copy, like BEAM's refc binaries.
      Binaries are immutable in Erlang, making them natural candidates for
      cross-process sharing (equivalent to Pony's `val` reference capability).
      Auto-promotion at 64-byte threshold in `nera_rt_alloc_binary`.

- [x] **Binary virtual heap for GC triggering**: When a process receives refc
      binaries, the 3-word heap descriptors and `Arc` clones accumulate in the
      receiver's heap and `refc_binaries: Vec<Arc<Vec<u8>>>`. GC only triggers
      based on the small heap descriptor size, not the large off-heap binary
      data they reference. BEAM solves this with a "binary virtual heap" — each
      refc binary's byte-size is added to a virtual counter that contributes to
      the GC trigger threshold, causing collection earlier and preventing
      unbounded accumulation. Without this, the send benchmark shows ~34ms
      overhead and ~9MB extra RSS from 100K descriptor accumulation (2.4 MB
      heap + 1.6 MB Arc Vec) and slow GC sweeps via `retain()`. Not a
      correctness issue — the code is correct and benchmarks fairly measure the
      same path as BEAM — but a meaningful GC optimization gap.
      Files: `crates/nera-runtime/src/heap.rs` (GC threshold,
      `refc_binaries` management), `crates/nera-runtime/src/message.rs`
      (descriptor copy during send).

- [x] **ETS tables**: `ets:new/2`, `ets:insert/2`, `ets:lookup/2`,
      `ets:delete/1,2`, `ets:delete_all_objects/1`, `ets:match_object/2`,
      `ets:select_delete/2`, `ets:tab2list/1`, `ets:info/1,2`, `ets:first/1`,
      `ets:next/2`. Supports `set` type, `named_table`, and `{keypos, N}`.
      Implemented as a global `RwLock<HashMap>` registry with per-table
      `RwLock<Vec<ErlTerm>>`. Two end-to-end tests: `ets_basic`, `ets_keypos`.

- [x] **Timer wheel**: Min-heap timer (`BinaryHeap<Reverse<TimerEntry>>`) with
      dedicated timer thread and condvar-based worker notifications. Processes
      entering `receive...after` register a deadline; the timer thread sleeps
      until the next deadline and wakes processes precisely. `send_message`
      cancels timers on early message arrival. Workers sleep on a Condvar
      instead of busy-polling. Lazy cancellation via `pid_to_timer` HashMap
      avoids heap mutation on cancel. Timer cleanup at all 4 process exit
      points + `propagate_exit_signals`. `should_terminate` checks
      `has_pending_timers()` to avoid premature shutdown.
      Implementation: `crates/nera-runtime/src/timer.rs`.

- [ ] **Scheduler improvements**:
      - Work-stealing: idle threads steal from busy threads' run queues
        (inspired by Pony's scheduler design)
      - [x] Condvar-based worker sleep: workers sleep on a Condvar instead of
        busy-polling with `park_timeout(100us)`, woken by timer thread or
        message delivery
      - [x] Back-edge yield checks: `YieldCheck` at CFG back-edges prevents
        tight-loop starvation
      - [x] Register-based reduction counter: Cranelift `Variable` avoids
        reloading from `ProcessContext` on every check
      - Backpressure: track mailbox depth per process, throttle senders when
        a consumer is overloaded (prevents fast-producer/slow-consumer OOM)
      - Bind schedulers to CPU cores (affinity)
      - Dirty schedulers for long-running operations
      - Scheduler load balancing metrics

- [ ] **Message passing optimizations** (inspired by Pony's zero-copy messaging;
      see [docs/PONY_LEARNINGS.md](docs/PONY_LEARNINGS.md)):
      - [x] **Lock-free MPSC mailbox**: `SegQueue<MessageFragment>` for send
        path, direct-to-heap send when target lock available.
      - [x] **FastMailbox ring buffer**: `#[repr(C)]` power-of-2 ring with
        inline receive ops in Cranelift IR.
      - [x] **SimpleTuple/InlineTuple fragments**: Zero-alloc for small tuples
        of immediates and refc binaries.
      - **Move semantics (dead-after-send)**: When IR liveness analysis shows a
        variable is never used after `!`, transfer heap ownership instead of
        deep-copying. Equivalent to Pony's `iso` capability without type annotations.
      - **Immutable term skip**: Compound terms made entirely of immediates
        (atoms, small integers) don't need their tuple shell copied. Tag them
        as fully-immutable at compile time.
      - **Compile-time constant sharing**: Terms built entirely from literals
        can live in a read-only `.rodata` segment, shared by all processes with
        no refcount and no copy.

- [x] **Shared I/O pool**: Fixed-size pool of OS threads (`min(4, num_cpus)`)
      for blocking I/O (file operations, `gen_tcp` accept/recv/send). FFI
      functions submit work closures and set `STATUS_IO_WAIT = 4`. The codegen
      call-site bailout automatically suspends the process. On I/O completion,
      `wake_process_with_result()` converts the result to an ErlTerm on the
      process heap, stores it in `ctx.result`, and re-enqueues the process.
      Implementation: `crates/nera-runtime/src/io_pool.rs`.

- [ ] **Regular expression support**: Implement the `re` module with at least
      `re:run/2,3`, `re:replace/3,4`, `re:split/2,3`, and `re:compile/1,2`.
      Use the Rust `regex` crate as the backend engine. Map Erlang PCRE-style
      options (`caseless`, `multiline`, `dotall`, `global`, etc.) to equivalent
      Rust regex flags. Return match results as `{match, Captured}` /
      `nomatch` tuples matching OTP conventions. Compiled regex terms can be
      stored as opaque boxed terms (similar to references).

---

## Runtime Observability

Compile-time-gated metrics for debugging, tuning, and profiling. When disabled
(the default), zero overhead — all instrumentation is removed via
`#[cfg(feature = "metrics")]`. When enabled, atomic counters and lightweight
bookkeeping provide insight into runtime behavior.

### P0 — Infrastructure

- [ ] **`metrics` Cargo feature flag** on `nera-runtime`: conditional compilation
      gate for all instrumentation code. Disabled by default.

- [ ] **`--metrics` CLI flag**: when passed to `nera`, compile with the `metrics`
      feature enabled. On program exit, dump a summary to stderr.

- [ ] **Per-process `Metrics` struct**: spawned alongside each process context.
      Tracks:
      - Message send/receive counts
      - Mailbox high-water mark
      - GC runs and bytes reclaimed
      - Heap high-water mark
      - Reduction count (already partially exists)

- [ ] **Global scheduler metrics**: one `SchedulerMetrics` struct (atomic counters)
      shared across threads:
      - Total spawns, exits
      - Context switches
      - Run queue length snapshots
      - Scheduler utilization (busy vs idle time)

### P1 — Memory & allocation metrics

- [ ] **Heap accounting**: total bytes allocated / freed across all processes,
      peak resident set.

- [ ] **Binary allocation tracking**: count and total size of heap binaries and
      (future) refc binaries.

- [ ] **Atom table stats**: atom count, total string bytes.

### P2 — Runtime introspection BIFs

- [ ] **`erlang:statistics/1`**: expose key metrics at runtime (`wall_clock`,
      `reductions`, `garbage_collection`, `run_queue`).

- [ ] **`erlang:process_info/2`**: per-process metrics (`message_queue_len`,
      `heap_size`, `reductions`, `total_heap_size`).

- [ ] **FFI/BIF call counters**: per-function atomic call counts, queryable
      via a `nera:bif_stats/0` BIF.

### P3 — Reporting & export

- [ ] **Structured output**: `--metrics-format=json` for machine-readable stats
      on exit.

- [ ] **Periodic sampling**: optional background thread that snapshots metrics
      every N ms and writes to a file or stdout.

- [ ] **Integration with Profiler**: feed metrics into the planned Profiler
      tool (see Tooling section) for per-function timing and flamegraph
      generation.

---

## Tooling

- [ ] **`nera fmt` — Source code formatter**: Canonical formatter for Erlang
      source files, similar to `gofmt` / `rustfmt`. Erlang has historically
      relied on editor conventions (Emacs erlang-mode indentation) rather than
      a standard tool. `nera fmt` would leverage the existing `nera-syntax`
      parser to parse, then pretty-print the AST back to a canonical form.

      **P0 — Core formatter**:
      - [ ] Pretty-printer module in `nera-syntax` that walks the AST and
            emits formatted source text. Handles indentation, line breaking,
            alignment of clause arrows (`->`), and comma/semicolon placement.
      - [ ] `nera fmt <files...>` CLI subcommand: format files in-place
            (with `--check` mode that exits non-zero if files would change,
            for CI).
      - [ ] Preserve comments: attach comments to AST nodes during parsing
            (currently discarded by the lexer) so the formatter can re-emit
            them in the correct positions.

      **P1 — Polish**:
      - [ ] Configurable style options (indent width, max line length) via
            `.nera-fmt.toml` or CLI flags. Sensible defaults matching
            traditional Erlang conventions (4-space indent, 80-column lines).
      - [ ] `--diff` mode: show a unified diff of what would change instead
            of modifying files.
      - [ ] Handle preprocessor directives (`-define`, `-ifdef`, `-include`)
            gracefully — format around them without breaking macro expansions.
      - [ ] stdin/stdout mode (`nera fmt -`) for editor integration.

- [ ] **REPL / shell**: Interactive Erlang shell using JIT compilation.

- [ ] **Debugger**: Step-through debugging with breakpoints. Requires source
      maps from compiled code back to Erlang source.

- [ ] **Profiler**: Per-function execution time and call counts. Leverage
      reduction counting infrastructure.

- [ ] **Crash dumps**: When a process crashes, optionally write a crash dump
      file with process state, mailbox contents, and stack trace.

- [ ] **`--emit-asm`**: Dump the generated assembly for a function (useful for
      performance analysis).

### Language Server Protocol (LSP)

Existing Erlang LSPs (erlang_ls, ELP) target OTP/BEAM and have no knowledge of
nera's supported subset or its specific diagnostics. A nera-native LSP would
provide accurate feedback for code targeting the nera compiler.

**P0 — `nera check` (prerequisite)**:

- [x] **`nera check <files...>` CLI command**: Parse + IR-lower without codegen
      or linking. Report all diagnostics (parse errors, undefined variables,
      unsupported features) with source locations and exit non-zero on errors.
      This is trivially wirable into any editor as a "run on save" task and
      provides 80% of LSP value at minimal cost.

- [x] **Fault-tolerant parsing**: `parse_module()` now always returns a
      partial AST alongside any errors, instead of discarding all valid forms
      on the first error. The parser synchronizes on `.` (form boundaries) and
      continues parsing — if 1 out of 20 functions has a syntax error, the
      other 19 still produce valid AST nodes. The LSP uses the partial AST
      for semantic tokens, hover, goto-def, and indexing even during editing.
      CLI compilation still blocks on parse errors.

**P1 — LSP server** (implemented in `crates/nera/src/lsp/`):

- [x] **Basic LSP server**: Implemented using `tower-lsp` over STDIO.
      `textDocument/didOpen`, `textDocument/didChange`, and `textDocument/didSave`
      run the parse + IR-lower pipeline and publish diagnostics.

- [x] **Go-to-definition**: Functions (local and remote), variables, records,
      macros, `-export` entries, `-include` directives.

- [x] **Hover information**: Function clause signatures, BIF info with
      auto-import status, record field definitions, macro names.

- [x] **Completion**: Context-aware: `module:` triggers function completion,
      `#` triggers record/field completion, bare identifiers complete local
      functions, auto-imported BIFs, module names, and in-scope variables.

- [x] **Workspace diagnostics**: Full project check on save, cross-module
      error detection (undefined remote calls, arity mismatches).

- [x] **Code actions**: Quick fix to rename unused variables (prefix with `_`).

**P2 — Rich editor features**:

- [ ] **Formatting** — integrate with erlfmt or implement built-in formatter.

- [ ] **Rename** — safe rename of variables within a clause, functions across
      the project.

- [ ] **Call Hierarchy** — incoming/outgoing call trees.

- [ ] **Folding Ranges** — code folding for functions, case expressions, records.

- [ ] **Document Highlight** — highlight all occurrences of the symbol under cursor.

---

## Test Framework

Erlang's standard test tools (EUnit, Common Test) are notoriously difficult to
set up — they require rebar3 plugins, OTP application structure, and extensive
boilerplate. nera can do better with a built-in test runner that works
out of the box, leveraging the `test/0 → ok` convention already used in the
`tests/` directory.

### P0 — `nera test` subcommand

Replace `tests/run_tests.sh` with a native test runner built into the compiler.

- [x] **`nera test` CLI command**: Discover all `.erl` files in the `tests/`
      directory exporting `test/0`, compile them, run each test function, and
      report pass/fail. A test passes if `test/0` returns `ok`; any exception
      (badmatch, error, throw) is a failure. Implemented in
      `crates/nera/src/test_runner.rs`.

- [x] **Pass/fail reporting**: Print each module's result as it completes
      (`PASS module_name` / `FAIL module_name`). Print a summary at the end:
      `N passed, M failed (K compile), S skipped / T total`. Failure details
      (compile errors, runtime output) listed at the end.

- [x] **`%% SKIP` support**: Honor the existing `%% SKIP: reason` convention
      on the first line — skip the module and report the reason.

- [x] **Exit code**: Exit 0 if all tests pass, 1 if any test fails. Compatible
      with CI pipelines.

- [x] **Parallel test execution**: Run test modules concurrently using OS
      threads. Default to number of CPUs; configurable with `-j N`.

- [x] **Pattern filtering**: `nera test guard_* arith_*` runs only tests
      matching glob patterns.

- [x] **Timing**: Report wall-clock time per test. Per-test timeout with
      `--timeout` flag (default 10s).

- [x] **Colored output**: Green for pass, red for fail, yellow for skip.
      Auto-detect tty via `IsTerminal`.

- [ ] **Stack traces on failure**: Print the stack trace captured by the
      exception, so the user can see where the failure occurred.

### P1 — Test discovery and filtering

- [ ] **Per-function test discovery**: In addition to `test/0`, discover all
      exported functions matching `test_*/0` in each module and run them
      individually. Report results per function, not just per module.

- [ ] **`--color=always|never|auto` flag**: Explicit color control beyond
      auto-detection.

### P2 — Advanced features

- [ ] **Setup/teardown**: Support optional `setup/0` and `teardown/0` exports
      per module. `setup/0` runs before all tests in the module; `teardown/0`
      runs after (even on failure).

- [ ] **Assert macros**: Provide a `testing.hrl` header with assertion macros:
      `?assertEqual(Expected, Actual)`, `?assertMatch(Pattern, Expr)`,
      `?assertError(Pattern, Expr)`, `?assertThrow(Pattern, Expr)`.
      These produce clear failure messages with expected vs actual values.

- [ ] **Test isolation**: Each `test_*/0` function runs in a fresh process
      to prevent state leakage between tests (process dictionary, registered
      names, mailbox contents).

- [ ] **`--repeat N`**: Run the test suite N times to catch flaky tests.
      Report any test that fails intermittently.

- [ ] **JUnit XML output**: `--output=junit` for CI integration (Jenkins,
      GitHub Actions, etc.).

---

## Target Applications

Showcase programs to validate nera against real Erlang workloads. Ordered by
complexity; each tier builds on the features proven by the previous one.

### Tier 1 — Write from scratch, no missing features (~30-150 LOC)

These fit nera today (pattern matching, recursion, gen_tcp, spawn/3, maps,
binaries, try/catch, closures/funs, records). They need zero stdlib.

- [x] **Echo server** (~30 LOC) — accept TCP connections, echo back data, one
      process per client. Available as `examples/echo_server.erl`.
- [x] **Ring benchmark** (~50 LOC) — spawn N processes in a ring, pass a message
      M times. Available as `examples/ring.erl`.
- [ ] **Binary trees benchmark** (~60 LOC) — allocate/deallocate binary trees,
      count nodes. Stress-tests GC and allocation. From the Computer Language
      Benchmarks Game.
- [ ] **Sorting algorithms** (~40 LOC) — quicksort, mergesort on lists. Pure
      list recursion + pattern matching.
- [x] **Key-value store** — OneCached memcached server by ProcessOne (~400 LOC).
      Full memcached text protocol: SET, GET, ADD, REPLACE, DELETE, INCR, DECR,
      flush_all. Uses gen_server, supervisor, ETS, gen_tcp, binary pattern
      matching. All protocol commands verified correct.
- [ ] **Chat server** (~180 LOC) — multi-client TCP chat with `NICK`, `SAY`,
      `QUIT` commands. Lobby process holds `[{Nick, Pid}]`, broadcasts via
      message passing.

### Tier 2 — Needs minor adaptation (~200-400 LOC)

These are now mostly feasible with the current feature set (file I/O,
`binary:split`, gen_tcp, ETS, OTP behaviours all available).

- [ ] **TCP proxy / load balancer** (~100 LOC) — accept on port A, relay to
      backend on port B. Shuttle data both directions with receive loops.
- [ ] **HTTP/1.0 static file server** (~250 LOC) — parse `GET /path HTTP/1.0`,
      serve files, return headers. Needs file I/O BIF.
      Ref: [vinoski/sws.erl](https://gist.github.com/vinoski/4996859)
- [ ] **Line-protocol log collector** (~150 LOC) — accept syslog-style messages
      over TCP, parse priority/timestamp/hostname, aggregate stats in a map.
- [ ] **Finger protocol server** (RFC 742, ~60 LOC) — accept TCP on port 79,
      receive username, return info. Trivial protocol, good RFC-compliance demo.

### Tier 3 — Ambitious showcase (~300-500 LOC)

These produce genuinely impressive demos but require more effort.

- [ ] **Mini Redis (RESP protocol)** (~350 LOC) — implement the Redis wire
      protocol (RESP). Support GET, SET, DEL, PING, KEYS. Binary pattern
      matching is a perfect fit for parsing RESP frames. Compatible with
      `redis-cli`.
- [ ] **IRC bot** (~250 LOC) — connect to an IRC server, join a channel, respond
      to commands. IRC is a simple `PRIVMSG #channel :text\r\n` text protocol.
      Ref: [erlbot](https://github.com/michaelmelanson/erlbot)
- [ ] **Mini IRC server** (~500 LOC) — multi-room chat with channels, nicknames,
      and the IRC wire protocol. Combines gen_tcp + maps + multiple processes.
      Ref: [docs/MINI_IRC_SERVER.md](docs/MINI_IRC_SERVER.md)
- [ ] **JSON parser** (~300 LOC) — recursive-descent parser from binary input to
      Erlang terms (maps, lists, atoms, integers, floats). No closures needed.
      Ref: [thoas](https://github.com/lpil/thoas) (for spec, not code — it uses funs)

---

## Self-Contained Compiler

Currently the compiler requires two external artifacts at compile time: the
stdlib `.erl` files (discovered via `find_stdlib_dir()`) and `libnera_runtime.a`
(discovered via `Linker::find_runtime_lib()`). Embedding these directly into the
compiler binary would make `nera` a single-file distribution — no tarball layout,
no path discovery, just one binary that can compile and link Erlang programs.

### P0 — Embed stdlib `.erl` sources — DONE

- [x] **`include_bytes!` / `include_str!` for stdlib**: Use Rust's `include_str!`
      macro to embed each stdlib `.erl` file (e.g. `erlang/stdlib/lists.erl`) as a
      string constant in the compiler binary at build time.

- [x] **Virtual file system for stdlib**: Replace `find_stdlib_dir()` + filesystem
      reads with an in-memory lookup (e.g. `HashMap<&str, &str>` mapping module
      names to source text). The parser already accepts `&str` source — no
      filesystem I/O needed for stdlib.

- [x] **`build.rs` auto-discovery**: Write a Cargo build script that scans
      `erlang/stdlib/*.erl` and generates the embedded source table, so new stdlib
      modules are picked up automatically without manual edits.

- [x] **Fallback to filesystem**: Keep `find_stdlib_dir()` as a fallback so users
      can override built-in stdlib modules with local files (useful for development
      and debugging). Precedence: user files > filesystem stdlib > embedded stdlib.

### P1 — Embed `libnera_runtime.a`

- [x] **Embed runtime archive**: `crates/nera/build.rs` compresses and embeds
      the runtime when it exists at compiler build time. The compiler extracts
      it to a temporary cache for linking. The 2026-10-10 repair makes
      `scripts/dist.sh` build the runtime first; installed-archive tests exercise
      both external and embedded runtime lookup. See
      [repair evidence](docs/baselines/2026-10-10-repairs/README.md).

- [ ] **Conditional embedding**: Gate runtime embedding behind a Cargo feature
      flag (e.g. `--features embed-runtime`) since it increases compiler size.
      This remains a proposal: current embedding is automatic when the runtime
      file is present; external libraries take precedence at link time.

- [ ] **Temp file cleanup**: Ensure the temporary runtime archive is cleaned up
      after linking, even on error paths.

### P2 — Truly single-binary distribution

- [ ] **Unified build**: When both stdlib and runtime are embedded, `nera` becomes
      a single binary that only needs the system C linker (`cc`) to produce
      executables. Document this as the recommended distribution method.

- [ ] **`--extract-runtime` flag**: Allow extracting the embedded runtime archive
      to disk for debugging or custom linker workflows.

---

## Platform Support

- [x] macOS arm64 (Apple Silicon) — native
- [x] macOS x86_64 — cross-compiled from arm64, CI-tested
- [x] Linux x86_64 — cross-compiled, CI-tested
- [x] Linux arm64 — cross-compiled, CI-tested
- [ ] Windows x86_64 — calling convention support exists, needs testing

Cross-platform support mainly requires testing the linker integration and
ensuring Cranelift's target support works correctly on each platform.

---

## Cross-Compilation

Only 64-bit architectures are targeted. The term representation (`ErlTerm` as
tagged `u64`) assumes 64-bit pointers throughout the runtime, IR, and codegen;
supporting 32-bit targets would require a major rework and is not planned.

### P0 — CLI and codegen target selection

- [x] **`--target <triple>` CLI flag**: parse a target triple string (e.g.,
      `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`) and propagate it to
      the codegen layer. Default remains `Triple::host()`.

- [x] **Cranelift ISA lookup by triple**: replace `cranelift_native::builder()`
      (host-only) with `cranelift_codegen::isa::lookup(triple)` when a
      `--target` is specified. Cranelift already supports x86_64, aarch64,
      riscv64, and s390x backends. Object format (ELF / Mach-O / COFF) is
      handled automatically by `ObjectModule` based on the triple.

- [x] **Calling convention selection**: select `CallConv::WindowsFastcall`
      for Windows targets instead of the hardcoded `CallConv::SystemV`.
      `CallConv::Tail` (used for Erlang functions) is target-independent and
      handled by Cranelift per-platform.

### P1 — Linker and runtime library

- [x] **Runtime triple-based linker selection**: replace `#[cfg(target_os)]`
      compile-time checks with runtime decisions based on the target triple's
      `operating_system` and `architecture`. Select the appropriate cross
      compiler (e.g., `aarch64-linux-gnu-gcc`, `x86_64-w64-mingw32-gcc`) and
      platform libraries (`-lpthread`, `-lm`, etc.) at link time.

- [x] **`--linker <path>` CLI flag**: allow overriding the linker command for
      custom toolchains (e.g., `zig cc --target=...`, Android NDK, iOS SDK).

- [x] **Dead-strip flags per target**: pass `-Wl,-dead_strip` (macOS),
      `-Wl,--gc-sections` (Linux/MinGW), or rely on `/OPT:REF` default
      (MSVC) based on the target triple rather than the build host.

- [x] **Cross-compiled runtime static libraries**: build `libnera_runtime.a`
      for each supported target via `cargo build --target <triple>`. Establish
      a lookup convention (e.g., `libnera_runtime-<triple>.a` or a
      per-target directory) so the linker can find the correct archive.
      The runtime is pure Rust with no C dependencies, so cross-building
      via Cargo is straightforward.

### P2 — Target matrix and CI

Supported cross-compilation pairs (all 64-bit):

| Target triple                    | OS      | Arch    | Notes                  |
|----------------------------------|---------|---------|------------------------|
| `x86_64-apple-darwin`            | macOS   | x86_64  | Rosetta host or cross  |
| `aarch64-apple-darwin`           | macOS   | arm64   | Native on Apple Silicon|
| `x86_64-unknown-linux-gnu`       | Linux   | x86_64  |                        |
| `aarch64-unknown-linux-gnu`      | Linux   | arm64   |                        |
| `x86_64-pc-windows-msvc`         | Windows | x86_64  | Needs CallConv fix     |
| `aarch64-unknown-linux-musl`     | Linux   | arm64   | Static musl linking    |
| `x86_64-unknown-linux-musl`      | Linux   | x86_64  | Static musl linking    |

- [x] **CI cross-compilation smoke tests**: compile and link a minimal Erlang
      program for each target triple on every release. Verify the binary is
      a valid executable for the target (e.g., `file` output, ELF/Mach-O
      headers). GitHub Actions workflow: `.github/workflows/ci.yml`.

- [ ] **Pre-built runtime archives**: publish `libnera_runtime.a` for each
      supported target triple alongside compiler releases.

---

## Mobile Support (iOS / Android)

Depends on cross-compilation infrastructure above (P0 + P1). Only 64-bit
targets are supported.

- [ ] **Cross-compile runtime static library** for:
      `aarch64-apple-ios`, `aarch64-apple-ios-sim`, `aarch64-linux-android`,
      and `x86_64-linux-android`.
- [ ] **iOS linker path**: support `clang` + SDK/sysroot selection via
      `--linker` and produce app-linkable artifacts.
- [ ] **Android linker path**: support NDK toolchains and API-level flags.
- [ ] **CI smoke tests**: compile and link minimal Erlang programs for each
      mobile target on every release.
