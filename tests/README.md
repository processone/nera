# End-to-End Test Suite for nera

Erlang end-to-end tests for the nera compiler. Some tests are adapted from the
[Erlang/OTP](https://github.com/erlang/otp) compiler and emulator test suites
(Apache 2.0 license); others are written specifically for nera.

For current results and known failures, see
[project status](../docs/PROJECT_STATUS.md). A successful process exit alone is
not proof that every assertion ran: the October baseline reproduces an early,
successful exit after a suspended exception. Use an explicit `%% EXPECT:`
completion marker for regressions where reaching the end is essential.

## Running

```bash
# Run all tests (parallel by default)
nera test

# Run specific tests (glob pattern)
nera test guard_*
nera test match_*

# List available tests
nera test --list

# Control parallelism and timeout
nera test -j 4 --timeout 30
```

The legacy shell script `./tests/run_tests.sh` is still available but `nera test`
is the recommended runner — it runs tests in parallel, has colored output, and
produces a failure summary.

## Project EUnit configuration

A project can select EUnit in `nera.toml`:

```toml
[project]
name = "my_app"
# Explicit compatibility mode for unavailable remote APIs in OTP dependencies.
defer_undefined = true

[deps]
eunit = { path = "/path/to/otp/lib/eunit-2.11" }

[test]
framework = "eunit"
modules = ["my_app_tests", "other_tests"]
src = ["test"]
```

Run `nera deps get` first, then `nera test --timeout 60`. EUnit source and its
required support modules must be available as external dependencies; for the
validated OTP version, this includes `gb_trees`. The Cowlib runner documents
that setup. Dependencies are not downloaded implicitly by `nera test`.

`modules` is a required, nonempty list of unquoted module identifiers. `src`
adds test source directories relative to the project root; when omitted, an
existing `tests/` directory is used. Production sources, include paths, OTP
root and resolved `.nera/deps` dependencies are inherited from the project.
Test modules should use `-include_lib("eunit/include/eunit.hrl")` or explicitly
request `eunit_autoexport`; the runner does not add a global `TEST` define.

`nera test --list` lists configured modules without compiling. Patterns select
modules to execute (`nera test 'my_app*'`); a pattern matching none is an error.
All selected modules execute cumulatively in one native EUnit program, using
EUnit's scheduling (`-j` applies to the legacy runner only). `--verbose` requests
EUnit's detailed output. `--timeout` bounds the whole suite's execution, not
compilation. The compilation directory is temporary and cleaned on completion;
tests execute from the project root so relative fixture paths still work.
Assertion failures, exceptions, timeouts and missing completion return a
nonzero status. `--no-warnings` suppresses warnings, while semantic errors
(including undefined local functions) remain compilation failures. Without
`[test]`, the existing `test/0` runner is unchanged.
The validated EUnit scope does not yet include captured test I/O or all OTP
helpers. Some EUnit failure details still render partially (`~P` and stacktrace
formatting); the failure count and command status remain checked. See
[Cowlib validation](integration/cowlib/README.md).

To run the project runner regressions with external OTP sources:

```sh
python3 tests/integration/eunit/project_test.py \
  --nera ./target/release/nera --eunit /path/to/otp/lib/eunit-2.11 \
  --gb-trees /path/to/otp/lib/stdlib-8.0.3/src/gb_trees.erl
```

## Convention

Each `.erl` file defines a module that exports `test/0`:

- `test/0` returns `ok` on success
- Failure is signaled by `badmatch` crash (pattern `Expected = actual()`)
- The `id(I) -> I` function prevents compile-time constant folding
- Tests print `ok` via `io:format("ok~n")` before returning

## Skip Markers

Tests that exercise known-broken features use a first-line comment:
```erlang
%% SKIP: description of the issue
```

These are automatically skipped by the runner and show up as SKIP in output.

## Test Inventory

### Arithmetic & Literals

| Test File              | Feature Category                       | Assertions |
|------------------------|----------------------------------------|------------|
| `arith_basic.erl`      | Integer arithmetic, bitwise ops        | ~20        |
| `arith_float.erl`      | Float arithmetic                       | ~10        |
| `float_arith_badarith.erl` | `/` with a zero divisor, signed zeros included, float overflow of `+`, `-`, `*` and `/`, and integers too large for a float raise `badarith`: inside `try` before a later send, in guards and in an uncaught process; literal operands; finite and underflowing results unchanged; a false list or binary comprehension filter stops the later filters; also run on BEAM | Table-driven |
| `base_n_integer.erl`   | Base-N integer literals (16#FF, 2#101) | ~8         |
| `number_separators.erl` | Decimal/base-N/big integer and float separators, macros, patterns and binaries | 11 |
| `char_literal.erl`     | Character literals ($a, $\n, etc.)     | ~6         |
| `bigint_arith.erl`     | Arbitrary precision integer arithmetic | ~10        |
| `bigint_bitwise.erl`   | Big integer bitwise operations         | ~8         |
| `bigint_factorial.erl` | Big integer factorial computation      | ~4         |
| `bigint_format.erl`    | Big integer formatting and conversion  | ~6         |

### BIFs

| Test File               | Feature Category                     | Assertions |
|-------------------------|--------------------------------------|------------|
| `bif_conversion.erl`    | atom_to_list, integer_to_list, etc.  | ~20        |
| `bif_list_ops.erl`      | hd, tl, length, element, etc.        | ~20        |
| `bif_type_tests.erl`    | is_integer, is_atom, etc.            | ~30        |
| `bif_binary_conv.erl`   | integer_to_binary, binary_to_integer | ~8         |
| `bif_conv_extended.erl` | integer_to_binary/2, integer_to_list/2, binary_to_float/1, binary_to_existing_atom/1, atom_to_binary/2, float_to_binary/1,2 | ~20 |
| `bif_binary_to_atom.erl` | binary_to_atom/1,2: encodings, sub-binaries, badarg, 255-character and large-input `system_limit`, dynamic calls | 50+ |
| `bif_atom_encoding.erl` | binary_to_existing_atom/1,2 and atom_to_binary/1,2: Latin-1 byte range, UTF-8 defaults, encoding validation, sub-binaries, bitstrings, length bounds and dynamic calls; also run on BEAM | Table-driven |
| `calendar_system_time.erl` | calendar:valid_date/1, system_time_to_universal_time/2 (units, flooring, badarg) and universal_time_to_system_time/1, direct, apply and fun reference calls; also run on BEAM | Table-driven |
| `bif_list_to_bitstring.erl` | list_to_bitstring/1: partial bytes, nested lists, bitstring tails, results above the inline size, pieces inside a larger binary; direct, erlang:-qualified, apply and fun reference calls, and badarg inputs through each; also run on BEAM | Table-driven |
| `bif_crypto_hash.erl` | crypto:hash/2 (md5, sha, sha2 family vectors, iodata, RFC 6455 handshake) and crypto:strong_rand_bytes/1: length, variability, apply and fun calls, badarg inputs; also run on BEAM | Table-driven |
| `string_lowercase.erl` | string:lowercase/1: binaries, lists, mixed chardata, full-mapping growth, no final-sigma rule, unchanged input identity, badarg/function_clause inputs, a checksum over every Unicode scalar value; also run on BEAM | Table-driven |
| `string_slice.erl` | string:slice/3: graphemes, binary/list/mixed input, infinity and zero lengths, large counts, lazy invalid UTF-8 in binaries, error inputs; also run on BEAM | Table-driven |
| `filename_extension.erl` | filename:extension/1: binaries, lists, atoms, nested names, leading dots, trailing slashes, error inputs; also run on BEAM | Table-driven |
| `zlib_streams.erl` | zlib streams as used by cowlib cow_ws and cow_deflate: raw deflate sync-flush with and without context takeover, deflateReset/inflateReset, safeInflate output limit, iodata and flush modes, OTP error reasons, dynamic calls; also run on BEAM | Round trips |
| `bif_process_alive.erl` | is_process_alive/1                   | ~4         |
| `bif_node.erl` | node/0,1: local live/dead PIDs and ordinary/monitor/resource references, badarg, guards, apply and fun calls; Ranch nodedown exit; also run on BEAM | Table-driven |
| `bif_system_time.erl`   | erlang:system_time/1                 | ~4         |

### Binary Operations

| Test File                      | Feature Category                           | Assertions |
|--------------------------------|--------------------------------------------|------------|
| `binary_construct.erl`         | Binary construction, sizes, endian         | ~15        |
| `binary_segment_specifiers.erl` | Signed UTF construction and patterns, ignored flags, repeated identical declarations and binary alias units; same fixture on BEAM | Table-driven |
| `binary_segment_type_conflict.erl` | Reject conflicting binary-utf8 construction types with OTP's conflict message | (compile error) |
| `binary_segment_pattern_conflict.erl` | Reject utf8-signed-binary patterns with OTP's conflict message | (compile error) |
| `binary_match.erl`             | Binary pattern matching                    | ~12        |
| `binary_pattern_size_overflow.erl` | Debug-safe constant size/unit pre-binding and UTF minimum sums: overflowing and at-limit sizes, case, heads, funs, receive, try/catch, binary/bitstring segments, huge prefixes before UTF-8/16/32 and ordinary extraction controls; also run on BEAM | 23 |
| `binary_string_construction.erl` | Codepoint string segments in construction: UTF-16/32 big, little and native byte order and surrogate pairs, literal and dynamic sizes, units, flags, floats, empty strings, binary-type badarg, arithmetic failures caught in try, guards, try and comprehensions; also run on BEAM | Table-driven |
| `binary_construction_order.erl` | Construction evaluates every segment value and size, value before size and left to right, before any segment check: badarith after earlier bad sizes, binary sources and `/binary` strings, an evaluation trace, comprehensions and same-function try; also run on BEAM | Table-driven |
| `binary_string_patterns.erl`   | Codepoint string segments in case, heads and matches; Latin-1, impossible default values above 255, UTF-8/16/32, following tails and shifted offsets; also run on BEAM | Table-driven |
| `binary_string_pattern_sized.erl` | Reject a sized string in a binary pattern, as OTP's typed_literal_string does | (compile error) |
| `binary_string_pattern_typed.erl` | Reject a non-UTF type on a string in a binary pattern, as OTP's typed_literal_string does | (compile error) |
| `binary_rest_case.erl`         | Deferred binary tails across case arms and joins | 8     |
| `binary_rest_if_try.erl`       | Deferred binary tails read in one `if` or `try ... of` branch: later branches, guards, handlers and `after` bodies, nested and repeated constructs, reads and comparisons after the expression; also run on BEAM | 32 |
| `binary_rest_guard_sequence.erl` | Guard sequences `G1; G2` whose first test raises before it reads a deferred binary tail: `if`, `case`, function head and `receive` guards, three-test sequences, the clause body and a read after the expression; also run on BEAM | 15 |
| `bitstring_size_badarg.erl` | byte_size/1, bit_size/1 and their erlang:-qualified forms: sizes of binaries, sub-binaries, bitstrings and a 100-byte binary, `badarg` for 20 terms that are not bitstrings, guards in heads, `case` and `if`, negated, `orelse` and comma guards, guard alternatives, an iolist walk shaped like Cowlib's `cow_iolists:split/2`; also run on BEAM | Table-driven |
| `bif_byte_size_dynamic.erl` | byte_size/1 through apply/2,3, variable module and function names and fun references (auto-imported, qualified, variable), compared with direct calls: binaries, sub-binaries, match tails, partial-byte bitstrings, 1000-byte binaries; `badarg` for 20 terms that are not bitstrings through each form; also run on BEAM | Table-driven |
| `binary_tail_byte_size.erl`    | Rounded-up deferred /bits, /bitstring and /binary-unit:N byte sizes in expressions/guards, shifted/recursive contexts and materialized controls; also run on BEAM | Table-driven |
| `binary_rest_unit.erl`         | Unsized /binary remainder units in case, heads, matches, shifted/recursive contexts, bound/wildcard tails, receive, maybe and list generators; also run on BEAM | Table-driven |
| `binary_generator.erl`         | Pattern-sized <= generators: widths, units, scaled float widths, bitstrings, UTF, nonfinite floats, mismatches, nesting, shadowing and cursor lifetime; same fixture on BEAM | 90+ |
| `bin_comp_bits.erl`            | Binary comprehensions whose bodies are not whole bytes: 1-, 3-, 4-, 7- and 12-bit pieces, filters, several generators, bitstring and nested-comprehension bodies, results above the inline size, bodies starting inside a byte and bodies sharing a larger binary; also run on BEAM | 22 |
| `bin_comp_badarg.erl`          | Binary comprehensions whose bodies are not bitstrings: integers, byte and binary lists, atoms, floats, tuples, generator values, a late bad value, filters and inner generators raise badarg at that iteration; side-effect traces, exceptions from the body, uncaught exits and valid bodies; also run on BEAM | 24 |
| `binary_generator_unsized.erl` | Reject unsized <= fields instead of looping | (compile error) |
| `binary_generator_unsized_default.erl` | Reject an unsized <= field in the default of a record no code creates | (compile error) |
| `binary_bound_segment.erl`     | Bound segment values (cow_multipart shape), repeated names; case, match, head, receive, maybe, list/binary generators, outer tails in try/catch; fun/generator shadowing | 88 |
| `maps_iterator.erl` | maps:iterator/1 and next/1: every entry once, none at the end, reuse, apply and fun calls, badmap and badarg; also run on BEAM | Table-driven |
| `binary_match_scope.erl` | binary:match/3 scope option: windows, negative lengths, pattern lists, sub-binaries, dynamic calls and badarg inputs; also run on BEAM | Table-driven |
| `bif_list_to_integer_base.erl` | list_to_integer/2: bases 2 to 36, signs, bignums, round trips and badarg inputs through each call form; also run on BEAM | Table-driven |
| `rand_uniform.erl` | rand:uniform/1: range and coverage for small, 2^62 and bignum bounds, dynamic calls, function_clause inputs; also run on BEAM | Table-driven |
| `rand_regression.erl` | Seeded rand against OTP 29 for every algorithm and seed form: uniform, uniform_real, bytes, normal, shuffle and jump draws, exported states, the process dictionary forms, mwc59 and splitmix64 utilities and error reasons; expectations generated by `scripts/generate-rand-regression.escript`; also run on BEAM | Table-driven |
| `math_exp_log_pow.erl` | math:exp/1, log/1 and pow/2: values, overflow and domain errors, badarg for nonnumbers, direct, remote fun and variable module calls; also run on BEAM | Table-driven |
| `float_integer_large.erl` | trunc/1, round/1, ceil/1 and floor/1 on floats beyond 2^63 return the exact integer, through direct, apply and remote fun calls; also run on BEAM | Table-driven |
| `binary_split.erl`             | binary:split/2,3 with [global]             | ~8         |
| `binary_split_patterns.erl`    | binary:split/2,3 with a list of patterns: one-element lists, Cowlib's server-sent events line and field splits, longest match at the same offset, non-overlapping global splits, trim_all, a 1,000-line stream and a view into it, apply and fun calls, badarg patterns; also run on BEAM | Table-driven |
| `binary_longest_common_prefix.erl` | Complete proper-list/binary validation, shared/shifted prefixes and dynamic calls | 17 |
| `binary_float.erl`             | Float binary segments (<<X:64/float>>)     | ~6         |
| `binary_refc.erl`              | Reference-counted large binaries           | ~6         |
| `binary_refc_advanced.erl`     | Advanced refc binary operations            | ~6         |
| `multi_clause_binary_rest.erl` | Multi-clause binary prefix patterns        | ~15        |
| `binary_bits.erl`              | Bit-level binary construction and matching | ~15        |
| `string_prefix_match.erl`      | String prefix binary matching              | ~6         |
| `binary_to_list_domain.erl` | Byte-aligned binary domain, partial heap/refc rejection and bitstring conversion distinction | 100+ |
| `etf_container_bounds.erl` | Impossible counts before reservation, minimal one-byte terms, empty/nested containers and list tails | 20+ |
| `unicode_list_scalar_domain.erl` | Full Unicode scalar domain before narrowing, atoms and string conversions | 20+ |
| `unicode_integer_scalar_domain.erl` | Strict malformed numeric lists; documented OTP 29 truncation difference | 7+ |
| `unicode_encodings.erl` | `unicode:characters_to_binary/2,3`, `characters_to_list/1,2` across latin1, UTF-8/16/32 | 70+ |
| `closure_binary_captures.erl` | Deferred bitstring captures, nested/named closures, GC, process and ETS transfer | 224+ |
| `binary_match_windows.erl` | Shifted/partial matching windows, cursor aliasing and process transfer | 224+ |
| `binary_integer_contracts.erl` | Integer/binary domains, endian widths and dynamic pattern bounds | 800+ |
| `binary_float_bits.erl`        | Float domains, bit windows, all binary16 patterns | 65,536+ |
| `binary_unicode_bits.erl`      | Unicode domains, bit offsets, malformed UTF | 600+       |
| `binary_utf.erl`               | UTF-8/16/32 construction and matching      | ~20        |

### Boolean & Comparison

| Test File            | Feature Category                | Assertions |
|----------------------|---------------------------------|------------|
| `bool_ops.erl`       | and/or/not, andalso/orelse      | ~20        |
| `comparison_ops.erl` | Equality, ordering, mixed types | ~30        |
| `map_comparison.erl` | Exact recursive map-key ordering, numeric map-value equality, insertion order, comparison operators, sibling key searches and ETS ordered-set identity/order; also run on BEAM | Table-driven |
| `atom_order.erl` | Atoms order by name whatever their creation order: operators, guards, lists:sort/1,2, usort/1, max/1 and min/1, run-time atoms, prefixes across eight bytes, Unicode names, tuples, lists, map keys and values, Cowlib's server-sent event keys, erlang:max/2 and min/2 between types, apply and fun calls, ordered_set iteration; also run on BEAM | Table-driven |
| `max_min_numbers.erl` | erlang:max/2 and min/2 return an argument unchanged, the first when equal: mixed integers and floats, signed zeros, exact comparison above 2^53, 2^60, 2^63 and beyond 64 bits, apply, fun and guard calls; also run on BEAM | 46 |

### Remote calls

| Test File | Feature Category | Assertions |
|-----------|------------------|------------|
| `parenthesized_remote_calls.erl` | Parenthesized module/function positions, computed targets, nested parentheses, static/dynamic controls, tail calls, returned funs, target/argument effects and first exceptions (including `try`); same fixture on BEAM | 27 |

### Control Flow

| Test File             | Feature Category              | Assertions |
|-----------------------|-------------------------------|------------|
| `control_flow.erl`    | Case, begin..end, nested flow | ~15        |
| `if_expr.erl`         | If expressions                | ~15        |
| `branch_export_reads.erl` | Names bound in `begin...end` or every `case`/`if`/`receive` branch, read afterwards: bodies, nested and scrutinee exports, call arguments, `try ... of`, fun-head and generator keys and sizes, construction sizes, guards, captures, later patterns and binary tails; funs in record field defaults; fun heads and generators still shadow, and a comprehension's first generator source keeps its bindings local; a binary tail read in a comprehension source or `maybe` block stays readable after it; a name local to an earlier fun does not stop a later read; a generator source or `catch` operand reads its own exports; a sibling clause, fun clause or receive timeout may reuse the name; also run on BEAM | 71 |
| `receive_timeout_bindings.erl` | Names bound in a `receive`'s `after` timeout expression, read after the `receive`: the timeout and message paths, a block timeout, binary tails, later patterns, guards, fun captures and fun-head map keys; also run on BEAM | 6 |
| `exception_basic.erl` | try/catch/throw/error/exit    | ~12        |
| `try_of.erl`          | try...of clause               | ~6         |
| `try_catch_bound_vars.erl` | Bound names in try-of and catch class/reason patterns: exact equality, guards, fresh names, binary tails and re-raise; also run on BEAM | Table-driven |
| `try_after_reraise.erl` | `try ... after` as the only handler in its function: the after body runs once when the body, an `of` clause or a catch clause raises, then the same class, reason and stacktrace reach the caller; a normal return keeps the body's value; an exception from the after body replaces the held one; an after body that waits, times out and collects garbage; an uncaught exit; also run on BEAM | 28 |
| `catch_expr.erl`      | Catch expression              | ~4         |
| `error_2.erl` | `error/2` and `erlang:error/2` raise `error:Reason` through the auto-imported and qualified names, a tail call, `apply/3`, a fun and a computed module, whatever `Args` holds; the code after the call does not run; `catch` gives `{'EXIT', {Reason, Stack}}`; also run on BEAM | 16 |
| `error_3.erl` | `error/3` and `erlang:error/3` raise `error:Reason` with `error_info` options through direct, tail, apply, fun and computed calls; arbitrary Args/Options, argument evaluation, no continuation after raising, cleanup and catch; also run on BEAM | Table-driven |
| `try_operation_short_circuit.erl` | An operator, unary op, map update, send, spawn, comprehension or `throw` that raises in a `try` body goes to the handler at once: no later send, put, receive or call runs and the first reason is kept, including `catch`, nested `try`, `of`, catch and `after` clauses; a raising guard there still just fails; outside `try`, a failed send or spawn or a `throw` stops the process at once and a map update evaluates every value first; also run on BEAM | Table-driven |

### Pattern Matching

| Test File                       | Feature Category                       | Assertions |
|---------------------------------|----------------------------------------|------------|
| `match_basic.erl`               | Literals, variables, tuples, lists     | ~20        |
| `match_case.erl`                | Case expressions with guards           | ~20        |
| `match_list.erl`                | Cons, exact, nested lists              | ~20        |
| `match_map.erl`                 | Map construction, pattern matching     | ~20        |
| `match_nested.erl`              | Deep nesting, complex patterns         | ~15        |
| `match_number.erl`              | Integer/float/negative matching        | ~15        |
| `match_tuple.erl`               | Arity discrimination, nesting          | ~15        |
| `match_alias.erl`               | Alias patterns (X = Pat)               | ~10        |
| `match_alias_chain.erl`         | Alias patterns in heads/guards/calls   | ~20        |
| `match_in_call.erl`             | LC generator aliases, call-arg patterns| ~12        |
| `match_same_var.erl`            | Repeated-var equality constraints      | ~15        |
| `match_rebind.erl`              | Pre-bound var match + badmatch behavior| ~10        |
| `match_chain_alias.erl`         | `A = B = Expr` chains, every var binds | ~10        |
| `match_call_alias.erl`          | Alias inside function-call argument    | ~8         |
| `match_nested_alias.erl`        | Alias nested inside tuple/list pattern | ~6         |
| `match_pinned_var.erl`          | Bound var equality, badmatch on differ | ~8         |
| `map_bound_value_vars.erl`      | Bound vars as map pattern values: repeated, heads, case, `=`, `?=`, receive; later keys/sizes in `=`/`?=`; bound binary tails as values and keys, and read in one branch; fun/generator shadowing | 75+ |
| `pattern_input_scope.erl` | Outer/earlier-generator map keys and binary sizes in fun/list/binary-comprehension patterns; shadowing, nested captures, captured guards, mismatch skipping, binary tails and earlier segments of the same binary (native/BEAM) | 45 |
| `generator_size_rebind.erl` | Earlier-generator sizes with later rebinding, exact dynamic/UTF binary ends, filters, later generators and shared pattern callers (native/BEAM) | 34 |
| `map_binary_keys.erl`           | Map ops with binary/tuple/float keys, semantic equality | ~35 |
| `map_update.erl`                | Map update syntax `M#{k => v, k := v}` | ~12        |
| `map_exact_update.erl`          | Map exact update `:=` operator         | ~8         |
| `map_get_bif.erl` | Auto-imported map_get/2, guards, errors, apply and remote fun | 10 |
| `map_key_bif.erl` | Auto-imported/qualified is_map_key/2; positive/negated guards, exact keys, badmap, apply and fun references; maps:is_key badmap; cow_ws guard; also run on BEAM | Table-driven |
| `map_size_badmap.erl` | map_size/1, erlang:map_size/1 and maps:size/1: sizes up to 100 keys, `{badmap, Term}` for 16 non-maps (binary tail included), apply/dynamic/fun calls, guards in heads, `case` and `if`, negated and `orelse` guards, guard alternatives; also run on BEAM | Table-driven |
| `undervar_convention.erl`       | `_Var` binding, repeated variables     | ~10        |
| `test_repeated_var_pattern.erl` | Repeated variables enforce equality    | ~6         |

### Guards

| Test File            | Feature Category                        | Assertions |
|----------------------|-----------------------------------------|------------|
| `guard_basic.erl`    | Type tests, comparisons in guards       | ~25        |
| `guard_bifs.erl`     | BIF calls in guards (abs, length, etc.) | ~20        |
| `guard_sequence.erl` | Guard conjunction/disjunction           | ~15        |
| `is_tuple_guard.erl` | is_tuple/is_float subtype check, thoas clause ordering | ~40 |

### Functions & Closures

| Test File                 | Feature Category                       | Assertions |
|---------------------------|----------------------------------------|------------|
| `fun_simple.erl`          | Basic anonymous fun usage              | ~6         |
| `fun_closure.erl`         | Closure capture and invocation         | ~8         |
| `fun_errors.erl`          | Fun error handling                     | ~6         |
| `fun_foldl.erl`           | Higher-order foldl                     | ~4         |
| `fun_higher_order.erl`    | map/filter with funs                   | ~8         |
| `fun_is_function.erl`     | is_function/1,2 type tests             | ~8         |
| `fun_multi_clause.erl`    | Multi-clause anonymous funs            | ~6         |
| `fun_nested.erl`          | Nested anonymous funs                  | ~6         |
| `fun_pattern_capture.erl` | Pattern matching in fun heads          | ~6         |
| `fun_pattern_complex.erl` | Complex fun patterns                   | ~8         |
| `fun_ref.erl`             | fun name/arity references              | ~6         |
| `fun_remote_ref.erl`      | fun M:F/A remote references            | ~6         |
| `fun_with_lc.erl`         | Funs combined with list comprehensions | ~4         |
| `closure_deep_copy.erl`   | Deep copy of closures in messages      | ~4         |
| `fun_suite.erl`           | Port of OTP compiler fun_SUITE         | ~40        |
| `tail_call_closure_wrapper.erl` | Tail-call trampoline through wrapper, chained, try/catch | ~30 |

### Lists & Strings

| Test File                | Feature Category                     | Assertions |
|--------------------------|--------------------------------------|------------|
| `list_append.erl`        | ++ and -- operators                  | ~15        |
| `lists_member_exact.erl` | lists:member/2 exact equality: integer/float and signed-zero distinctions, compound terms, separately constructed equals, empty/later matches and direct/apply/dynamic/fun calls; numeric key-search controls; also run on BEAM | Table-driven |
| `list_comprehension.erl` | Basic LC, filters, pattern gen       | ~8         |
| `lc_nested.erl`          | Nested list comprehension generators | ~8         |
| `string_as_list.erl`     | String operations, comparison        | ~12        |
| `string_codepoints.erl` | Non-ASCII expressions, full octal escape range, adjacency, list operations, patterns and Unicode format padding; also run on BEAM | 28 |

### Records

| Test File            | Feature Category                    | Assertions |
|----------------------|-------------------------------------|------------|
| `record_basic.erl`   | Record create/update/access/pattern | ~15        |
| `record_nested_patterns.erl` | Cowboy-style map/record nesting, aliases, mismatches, repeated bindings and string-prefix tails | 20+ |
| `include_record.erl` | -include with record definitions    | ~4         |

### Recursion & TCO

| Test File            | Feature Category                | Assertions |
|----------------------|---------------------------------|------------|
| `recursion.erl`      | Factorial, fibonacci, list ops  | ~20        |
| `tail_recursion.erl` | TCO stress test, state machines | ~8         |
| `tco_mutual.erl`     | Mutual tail call optimization   | ~4         |

### Preprocessor

| Test File                    | Feature Category                  | Assertions |
|------------------------------|-----------------------------------|------------|
| `preproc_define.erl`         | -define/-undef object-like macros | ~8         |
| `preproc_function_macro.erl` | Function-like macro expansion     | ~6         |
| `preproc_ifdef.erl`          | -ifdef/-ifndef/-else/-endif       | ~8         |
| `preproc_predefined.erl`     | ?MODULE, ?LINE, ?FILE macros      | ~6         |

### Process Management

| Test File                   | Feature Category                   | Assertions |
|-----------------------------|------------------------------------|------------|
| `self_send.erl`             | Self-send message passing          | ~4         |
| `selective_receive.erl`     | Selective receive (mailbox scan)   | ~6         |
| `receive_after.erl`         | Receive with timeout               | ~4         |
| `receive_timeout_domain.erl` | `after` values outside `infinity` and 0..4294967295: a matching queued message is received first, including selective receive and inside `try`; with no match, `error:timeout_value` at once, queued messages kept, no wait for a later sender, and as an uncaught exit; an error in the `after` expression or earlier in a `try` body comes first; valid boundaries; also run on BEAM | Table-driven |
| `receive_nil_message.erl`   | The message `[]` is received like any other: first, middle and last in the mailbox, `[]` patterns and guards, `after 0` and no `after`, selective receive skipping it, and a waiting process woken by it; later messages still arrive; also run on BEAM | ~21 |
| `process_dict.erl`          | put/get/erase/get_keys             | ~10        |
| `process_dict_exact_keys.erl` | put/get/erase exact numeric and compound keys, signed zeros, equal reconstructed keys, missing lookup/removal, replacement/erase returns, dictionary contents and direct/qualified/dynamic/fun calls; also run on BEAM | Table-driven |
| `process_dict_get_keys.erl` | get_keys/1 exact value filtering, auto-import/qualified/dynamic/fun calls, dictionary preservation, arbitrary keys, updates/erase and Ranch child-state lookup; also run on BEAM | Table-driven |
| `process_flag_basic.erl`    | process_flag(trap_exit, true)      | ~4         |
| `process_info.erl`          | process_info/1,2                   | ~6         |
| `reference.erl`             | make_ref, is_reference             | ~6         |
| `scheduler_yield.erl`       | Preemptive yield in tight loops    | ~4         |
| `spawn_link_basic.erl`      | spawn_link/1,3                     | ~6         |
| `spawn_monitor_basic.erl`   | spawn_monitor/1,3                  | ~6         |
| `link_basic.erl`            | Process links and EXIT signals     | ~8         |
| `link_normal.erl`           | Normal exit through links          | ~4         |
| `unlink_basic.erl`          | Process unlink                     | ~4         |
| `monitor_basic.erl`         | Process monitors and DOWN messages | ~8         |
| `monitor_noproc.erl`        | Monitor non-existent process       | ~4         |
| `demonitor_basic.erl`       | demonitor prevents DOWN delivery   | ~4         |
| `timer_many.erl`            | Multiple concurrent timers         | ~6         |
| `timer_cancel_on_msg.erl`   | Timer cancellation on message      | ~4         |
| `many_pending_timers.erl` | 50,000 pending start_timer/send_after timers in one program; short timers among them still fire in deadline order; every cancel returns the time left; also run on BEAM | ~4 |
| `exit_code_spawn_crash.erl`     | Exit code after spawn crash            | ~4         |
| `process_flag_fullsweep.erl`    | process_flag(fullsweep_after, N)       | ~4         |
| `garbage_collect_bif.erl` | Real garbage_collect/0 suspension, local/dictionary/mailbox roots and dynamic calls | ~9 |
| `gc_deep_nesting.erl` | An iolist, tuple, map and closure chain nested 200,000 levels deep survive requested and allocation-driven collections with every level intact; completion marker; also run on BEAM | ~10 |
| `copy_deep_nesting.erl` | An iolist, tuple, map and closure chain nested 200,000 levels deep sent to another process and back and captured in spawned funs; the iolist, tuple and map stored in a `set` ETS table and read back by lookup, `tab2list/1` and from another process; a 200,000-level iolist as a key found with an equal term; every level checked; completion marker; also run on BEAM | ~20 |
| `max_heap_size.erl` | Integer/map max_heap_size kills allocating workers; completion marker | 2 kills |
| `max_heap_size_map.erl` | Default-policy size maps, nominal error_logger=true, previous flags and errors | ~13 |
| `hibernate_basic.erl`           | erlang:hibernate/3 wake on message     | ~4         |
| `spawn_opt_basic.erl`           | spawn_opt/2 link, monitor, heap opts   | ~8         |

### Registered Processes

| Test File                 | Feature Category                       | Assertions |
|---------------------------|----------------------------------------|------------|
| `register_basic.erl`      | register/whereis/unregister lifecycle  | ~6         |
| `register_send.erl`       | Send messages to registered names      | ~4         |
| `register_process.erl`    | Register spawned workers, auto-cleanup | ~8         |
| `register_errors.erl`     | Registration error cases               | ~6         |
| `register_registered.erl` | registered/0 BIF                       | ~4         |
| `register_reuse.erl`      | Re-register after process death        | ~4         |
| `register_self_send.erl`  | Self-send via registered name          | ~4         |
| `register_freed_before_signal.erl` | A dead process's name is free when its `'EXIT'` or `'DOWN'` arrives: normal exit, own reason, trapped link, `exit(Pid, kill)` and link cascade, 500 rounds each; a supervisor restarts its named child 100 times; also run on BEAM | 7 |

### Cross-Module & Dynamic Dispatch

| Test File           | Feature Category                 | Assertions |
|---------------------|----------------------------------|------------|
| `xmod_basic.erl`    | Basic cross-module calls         | ~4         |
| `xmod_bidir.erl`    | Bidirectional cross-module calls | ~4         |
| `xmod_multi.erl`    | Multi-module linking             | ~4         |
| `dynamic_apply.erl` | apply/3, fun M:F/A               | ~8         |

### IO & Formatting

| Test File           | Feature Category                    | Assertions |
|---------------------|-------------------------------------|------------|
| `io_format.erl`     | io:format with ~w, ~p, ~s, ~n       | ~5         |
| `io_lib_format.erl` | io_lib:format/2 directives          | ~8         |
| `io_format_atom_quoting.erl` | Atoms quoted as OTP writes them under ~w, ~p, ~W and ~P: uppercase, empty and punctuated names, reserved words, escapes, Latin-1 letters, characters above 255 with and without ~t, nesting, field widths; io:format output matches; also run on BEAM | 36 |
| `io_concurrent.erl` | Concurrent io:format from processes | ~4         |
| `file_io.erl`       | file:read_file/1, file:write_file/2 | ~6         |

### OTP Behaviours

| Test File                | Feature Category                       | Assertions |
|--------------------------|----------------------------------------|------------|
| `gen_server_basic.erl`   | gen_server start_link/call/cast/info   | ~12        |
| `supervisor_basic.erl`   | Supervisor one_for_one restart         | ~8         |
| `gen_server_init_crash.erl` | gen_server:start_link/3,4, start/3 and supervisor:start_link/2,3 when init/1 raises, exits, throws, returns `{error, Reason}` or a bad value, when supervisor flags are invalid and when the name is taken: `{error, Reason}` for a caller trapping exits, no leftover `'EXIT'`/`'DOWN'`, name freed, a linked caller that does not trap exits exits with Reason; init/1 returning or throwing `ignore` gives `ignore`; also run on BEAM | 64 |
| `supervisor_child_start.erl` | supervisor:start_link/2,3 when a child's start returns `{error, Reason}`, a value that is not a start result, raises, exits, throws or is a gen_server whose init/1 crashes: `{error, {shutdown, {failed_to_start_child, Id, Reason}}}` with OTP's Reason, later children not started, earlier ones stopped, name freed, no leftover `'EXIT'`/`'DOWN'`, a linked caller that does not trap exits exits with it; a child returning or throwing `ignore` and a child supervisor returning `ignore` do not fail the start, and the children after them start; a failed restart tried again and counted against the restart limit, then `shutdown`; a restart returning `ignore` keeps the supervisor running; also run on BEAM | 48 |
| `supervisor_shutdown.erl` | How a one_for_one supervisor stops: when its parent exits (normally or not) or sends it `shutdown`, with the parent's reason, while another process's exit signal is ignored; before it exits, after its parent stops it, past its restart limit or when a child fails to start, it stops its running children one by one, the last started first, by their shutdown setting (`brutal_kill`, a timeout followed by a kill for a child that traps exits and ignores them, `infinity` for a child supervisor, which stops its own children), skipping a child that is not running and not waiting for one that already exited; an invalid child spec gives `{start_spec, Error}` before any child starts; a `gen_server` that traps exits runs `terminate/2`, which is optional, and stops with its parent's reason, while another process's exit signal, or its starter's when started without a link, reaches `handle_info/2`; helper module `_supervisor_shutdown_server.erl`; also run on BEAM | 76 |
| `sup_test_worker.erl`    | Supervised worker process              | ~4         |
| `application_start_ignore.erl` | application:start/1 when the application's start/2 returns the `ignore` of its top supervisor: `{error, {bad_return, {{Mod, start, [Type, Args]}, ignore}}}` as on BEAM (checked there with an `.app` file); helper module `_ignored_app.erl` | 1 |
| `error_logger_basic.erl` | error_logger info/error/warning output | ~6         |

### ETS

| Test File        | Feature Category                      | Assertions |
|------------------|---------------------------------------|------------|
| `ets_basic.erl`  | ETS new/insert/lookup/delete/tab2list | ~15        |
| `ets_keypos.erl` | ETS keypos and named tables           | ~8         |
| `ets_named_table_return.erl` | ets:new/2 with `named_table` returns the name (Cowboy's `?MODULE = ets:new(?MODULE, [named_table, ...])`): every table type, apply, variable module and fun calls; a table without `named_table` does not take the name, and creating or deleting one leaves a named table with the same atom reachable; the name is released by ets:delete/1; a table created in another process is reached by its name; also run on BEAM | Table-driven |

### Stdlib

| Test File          | Feature Category              | Assertions |
|--------------------|-------------------------------|------------|
| `lists_bifs.erl`   | lists module BIF functions    | ~12        |
| `lists_stdlib.erl` | lists module Erlang functions | ~15        |
| `lists_key_numeric.erl` | keydelete/3, keyreplace/4 and keystore/4 numeric equality, nested and map-valued keys, first-match/order/miss behavior, skipped non-tuples/short tuples, invalid positions and direct/apply/dynamic/fun calls; also run on BEAM | Table-driven |
| `lists_key_argument_errors.erl` | OTP 29 argument errors: keyreplace/4 and keystore/4 `function_clause` for non-tuple replacements; keyfind/3, keymember/3 and keysearch/3 `badarg` for positions below 1, non-integers and integers from 2^59, and for non-lists and improper tails reached before a match; largest valid position; hits before an improper tail; direct/apply/dynamic/fun calls; also run on BEAM | Table-driven |
| `maps_stdlib.erl`  | maps module functions         | ~12        |
| [`integration/thoas/`](integration/thoas/) | thoas JSON library encode/decode roundtrip (requires `nera deps get`) | ~30 |

### Type/Spec Attributes

| Test File             | Feature Category              | Assertions |
|-----------------------|-------------------------------|------------|
| `type_spec_attrs.erl` | -type/-spec attribute parsing | ~4         |

### Import resolution

`import_resolution.erl` and `import_resolution_global.erl` exercise selective/global
suppression, explicit imports and erlang guard reimports, local fun references,
throw overrides and optimized-name shadowing. Both carry explicit completion
markers and run on Nera and OTP 29.0.4. `scripts/test-import-resolution.py --nera
./target/release/nera` compares CLI semantic fixtures with OTP, including rejected
suppressed calls, illegal guards, clash warnings and unavailable native targets.
`python3 scripts/update-otp-auto-imports.py --check` independently compares the
committed catalogue with the pinned OTP abstract code.

### Compiler Diagnostics

| Test File                      | Feature Category                                       | Assertions      |
|--------------------------------|--------------------------------------------------------|-----------------|
| `pattern_expr_warnings.erl` | Map-key and binary-size reads, including earlier segments in clause/body-match patterns; real unused-value controls; shared with warning analysis and run on BEAM | Exact diagnostics and completion |
| `unresolved_call.erl`          | Expected compile error for missing module              | (compile error) |
| `record_undefined_pattern.erl` | Expected compile error for an undefined record pattern | (compile error) |
| `pattern_input_fun_key_unbound.erl` | Reject a map key supplied only by a sibling fun argument | (compile error) |
| `pattern_input_fun_size_unbound.erl` | Reject a binary size supplied only by a sibling fun argument | (compile error) |
| `pattern_input_list_unbound.erl` | Reject a list-generator key supplied only by its own pattern | (compile error) |
| `pattern_input_binary_unbound.erl` | Reject a binary-generator size supplied only by a later segment | (compile error) |
| `pattern_input_record_unbound.erl` | Reject a same-head map key in an unused record-default fun | (compile error) |

### "Programming Erlang" Adapted Tests

Tests adapted from Joe Armstrong's *Programming Erlang* (1st and 2nd editions),
covering sequential and concurrent patterns:

| Test File                       | Feature Category                     | Assertions |
|---------------------------------|--------------------------------------|------------|
| `jaerlang2_geometry.erl`        | Tuple pattern matching, arithmetic   | ~4         |
| `jaerlang2_fac.erl`             | Recursive factorial                  | ~4         |
| `jaerlang2_convert.erl`         | Temperature conversion               | ~4         |
| `jaerlang2_mylists.erl`         | Custom list functions                | ~6         |
| `jaerlang2_fib.erl`             | Fibonacci sequence                   | ~4         |
| `jaerlang2_qsort.erl`           | Quicksort with list comprehensions   | ~4         |
| `jaerlang2_bool.erl`            | Boolean logic patterns               | ~4         |
| `jaerlang2_guards.erl`          | Guard sequences and tests            | ~6         |
| `jaerlang2_counter_tuple.erl`   | Stateful counter with tuples         | ~4         |
| `jaerlang2_try.erl`             | Exception handling patterns          | ~6         |
| `jaerlang2_for.erl`             | For-loop via recursion               | ~4         |
| `jaerlang2_shop.erl`            | Shopping list operations             | ~6         |
| `jaerlang2_perimeter.erl`       | Perimeter calculation patterns       | ~4         |
| `jaerlang2_lists.erl`           | List utility patterns                | ~6         |
| `jaerlang2_odds_evens.erl`      | Partition lists into odds/evens      | ~4         |
| `jaerlang2_area_server.erl`     | Area server with message passing     | ~4         |
| `jaerlang2_counter_msg.erl`     | Counter with message passing         | ~4         |
| `jaerlang2_counter_timeout.erl` | Counter with receive timeout         | ~4         |
| `jaerlang2_every.erl`           | Periodic message patterns            | ~4         |
| `jaerlang2_stimer.erl`          | Simple timer process                 | ~4         |
| `jaerlang2_flush.erl`           | Mailbox flush patterns               | ~4         |
| `jaerlang2_priority.erl`        | Priority receive patterns            | ~4         |
| `jaerlang2_on_exit.erl`         | on_exit handler with links           | ~4         |
| `jaerlang2_pmap.erl`            | Parallel map using spawn             | ~4         |
| `jaerlang2_keep_alive.erl`      | Keep-alive process with restart      | ~4         |
| `jaerlang2_link_exit.erl`       | Link and exit signal handling        | ~4         |
| `jaerlang2_ring.erl`            | N-process message ring               | ~4         |
| `jaerlang_area_server.erl`      | Area server (1st edition)            | ~4         |
| `jaerlang_clock.erl`            | Clock process with ticks             | ~4         |
| `jaerlang_counter.erl`          | Counter process (1st edition)        | ~4         |
| `jaerlang_links.erl`            | Link/exit patterns (1st edition)     | ~4         |
| `jaerlang_name_server.erl`      | Named server pattern                 | ~4         |
| `jaerlang_server.erl`           | Generic server pattern (1st edition) | ~4         |
| `jaerlang_server2.erl`          | Enhanced server pattern              | ~4         |

### Helper Modules (not run directly)

| File                     | Used By          |
|--------------------------|------------------|
| `_xmod_math.erl`         | `xmod_basic.erl` |
| `_xmod_converter.erl`    | `xmod_bidir.erl` |
| `_xmod_string_utils.erl` | `xmod_multi.erl` |

**Total: ~1192+ assertions across 166 test modules (+ 3 helper modules)**

## Adding New Tests

1. Create `feature_name.erl` in this directory
2. Module name must match filename
3. Export `test/0` returning `ok`
4. Use `id(I) -> I` to prevent constant folding
5. Use pattern matching for assertions: `Expected = actual_call()`
6. Add `%% SKIP: reason` as first line if the feature is known-broken

### Base64 list decoding

`base64_default.erl` checks the arity-one defaults against BEAM: RFC vectors,
standard alphabet, padding, binary/list input, whitespace, errors and dynamic
calls. Both new arities retain the existing Rust-backed arity-two paths.

`base64_list_decode.erl` checks the public list path against the same expected
values/errors on BEAM, while `base64_native_decode.erl` also proves the internal
binary and list BIFs exist. `base64_list_fairness.erl` checks progress on large
list data, whitespace and errors. Run the following for the exhaustive direct
BEAM list corpus and strict single-scheduler checks:

```sh
python3 scripts/test-base64.py --nera /absolute/path/to/nera
python3 scripts/test-base64-scheduler.py --nera /absolute/path/to/nera
```

The list corpus is in `scripts/fixtures/base64_list_corpus.erl`. It supersedes
the frozen pre-native list error semantics without changing the historical
benchmark module. Fairness assertions are about progress, not a bound on GC or
allocator latency.
