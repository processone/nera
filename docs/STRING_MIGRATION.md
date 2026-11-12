# String Migration Plan: Lists to Binaries

## Problem

Erlang represents `"hello"` as `[104,101,108,108,111]` — a linked list of integers.
Each character costs 2 words (16 bytes on 64-bit). A 100-character string uses ~1600 bytes
plus GC pressure. Binaries (`<<"hello">>`) store the same in ~100 contiguous bytes.

Elixir solved this cleanly by making `"hello"` a UTF-8 binary from day 1, relegating
charlists to single-quote syntax. nera, as a new compiler with no legacy codebase to
protect, can follow a similar path — but gradually, to preserve .erl compatibility.

## Design

Three phases, each building on the previous.

### Phase 1: `-compile(binary_strings)` Pragma

A module-level opt-in that makes double-quoted string literals compile to UTF-8 binaries
instead of integer lists.

```erlang
-module(example).
-compile(binary_strings).

greet(Name) ->
    Msg = "Hello, " ++ Name ++ "!",  %% these are now binaries
    io:format("~s~n", [Msg]).
```

**Compiler changes:**

- Parser: recognize `binary_strings` in `-compile(...)` attributes.
- IR lowering: when the flag is set, emit `MakeBinary` for string literals instead of
  `MakeList`. Concatenation (`++`) on binary operands lowers to `BinaryConcat`.
- Also support a CLI flag (`--binary-strings`) to apply project-wide without per-module
  pragmas.

**Compatibility:** without the pragma, `"hello"` remains a list. Existing .erl files
compile unchanged.

### Phase 2: String Interpolation Syntax

A new sigil syntax that always produces a binary, regardless of any pragma:

```erlang
Name = <<"world">>,
Greeting = ~"Hello, ${Name}!",
```

This compiles to binary concatenation. The `~"..."` form is unambiguous — no risk of
confusion with legacy list strings.

**Compiler changes:**

- Lexer/parser: tokenize `~"..."` as a new literal form, parse `${Expr}` interpolations
  inside it.
- IR lowering: desugar into a chain of `BinaryConcat` / `term_to_binary_string` operations
  (the latter for non-binary interpolated expressions).

### Phase 3: Binary Strings by Default

Flip the default: `"hello"` compiles to a binary unless the module opts out with
`-compile(list_strings)`. The CLI flag `--list-strings` provides the same override
project-wide.

This phase should only ship once the stdlib and ecosystem are comfortable with
binary-first strings — likely after the `string` and `io_lib` stdlib modules are
implemented and tested against binary inputs.

## Stdlib Implications

- `io:format/2`: `~s` already needs to accept both binaries and lists (standard OTP
  behavior). Verify this works.
- `string` module: implement on top of binaries (UTF-8 aware), matching OTP 20+ design.
- `lists` module: functions like `lists:reverse/1`, `lists:flatten/1` naturally work on
  lists. Under `binary_strings`, users would use `string:reverse/1` instead. Document this
  transition clearly.
- `io_lib:format/2`: return iolists (mixed lists/binaries). No change needed.

## Operator Semantics Under `binary_strings`

| Operator | List mode (default)       | Binary mode (`binary_strings`)      |
|----------|---------------------------|-------------------------------------|
| `++`     | `lists:append/2`          | Binary concatenation when both args are binaries; list append otherwise |
| `"abc"`  | `[97,98,99]`              | `<<"abc"/utf8>>`                    |
| `~"..."` | *(Phase 2)* always binary | always binary                       |

## Migration Guide for Users

1. Start with `~"..."` interpolation (Phase 2) in new code — no pragma needed, no
   ambiguity.
2. Add `-compile(binary_strings)` to modules that are string-heavy and don't pass
   `"..."` literals to list-expecting functions.
3. Audit uses of `++` on strings — under binary mode, ensure both sides are binaries or
   use explicit conversion.
4. Replace `lists:reverse(Str)` with `string:reverse(Str)` in migrated modules.
5. Once all modules are migrated, the project can adopt `--binary-strings` globally.
