# nera Feature Support

Status of Erlang language features in the current nera source tree. This does
not imply availability in an older release.

This is an implementation inventory, not a conformance guarantee. The
[2026-10-10 baseline and preview blockers](docs/PROJECT_STATUS.md) take precedence
where runtime or formatter behaviour contradicts a supported marker. See also
[external-project compatibility](COMPATIBILITY.md) for scoped validation results
and known differences from BEAM.

**Legend:** ✅ Supported | 🔶 Partial | ❌ Not yet implemented

## Data Types

| Feature                 | Status | Notes                            |
|-------------------------|--------|----------------------------------|
| Small integers (61-bit) | ✅      | Tagged immediate                 |
| Big integers            | ✅      | Arbitrary precision (num-bigint) |
| Atoms                   | ✅      | Interned in global atom table    |
| Floats (64-bit)         | ✅      | Heap-allocated boxed             |
| Lists (cons cells)      | ✅      | Head/tail, proper and improper   |
| Tuples                  | ✅      | Heap-allocated boxed             |
| Binaries                | ✅      | Byte-aligned, heap-allocated     |
| Maps                    | ✅      | Flat layout, O(n) lookup         |
| Strings                 | ✅      | As lists of Unicode codepoints, including non-ASCII literals and 1–3 digit octal escapes |
| Pids                    | ✅      | Tagged immediate                 |
| References              | ✅      | Unique per process               |
| Funs/closures           | ✅      | Heap-allocated with captured env |
| Records                 | ✅      | Desugared to tuples at IR level  |
| Char literals           | ✅      | $a, $\n, ordinary escapes ($\#), named escapes and 1–3 digit octal escapes ($\013) |
| Base-N integers         | ✅      | 16#FF, 2#1010, etc.              |
| Numeric digit separators | ✅     | Underscores between digits in decimal/base-N integers and float components; formatter preserves numeric lexemes in expressions and patterns |
| Ports                   | ❌      |                                  |
| Bitstrings (sub-byte)   | ✅      | Bit-level construction & match   |

## Expressions

| Feature                                     | Status | Notes                         |
|---------------------------------------------|--------|-------------------------------|
| Arithmetic (+, -, *, /)                     | ✅      | Integer and float; a zero divisor for `/` and a float result beyond the float range raise `badarith` |
| Integer division (div, rem)                 | ✅      |                               |
| Comparison (==, /=, =:=, =/=, <, >, =<, >=) | ✅      | Atoms order by name, as on BEAM; map ordering uses exact keys; `==` retains numeric map-value equality |
| Boolean operators (and, or, not)            | ✅      |                               |
| Short-circuit (andalso, orelse)             | ✅      | Conditional control flow; RHS evaluated only when needed |
| Bitwise (band, bor, bxor, bnot, bsl, bsr)   | ✅      |                               |
| List append/subtract (++, --)               | ✅      | Via FFI calls                 |
| Unary negation (-)                          | ✅      |                               |
| Match operator (=)                          | ✅      | In patterns and body          |
| Send operator (!)                           | ✅      | To PIDs and registered names  |
| Block expressions (begin...end)             | ✅      |                               |
| List comprehensions                         | ✅      | Generator patterns checked before filters/body; filters stop at the first `false`; keys/sizes read outer or earlier-generator inputs |
| Fun expressions (fun(...) -> ... end)       | ✅      | Closure capture includes head keys/sizes and nested callee expressions |
| Fun references (fun F/A, fun M:F/A)         | ✅      | Local and remote; imported functions require `fun M:F/A`, as on BEAM |
| Try/catch/after                             | 🔶      | throw/error/exit; bound pattern variables compare with `=:=`; resumed outer handlers regression-tested (P0-1 repaired) |
| Throw/exit/error                            | ✅      | Via ProcessContext status     |
| Binary comprehensions                       | ✅      | List + pattern-sized binary generators; bodies of any bit length; non-bitstring bodies raise badarg |
| Catch expression                            | 🔶      | Immediate and resumed catch cases regression-tested (P0-1 repaired) |
| Implicit string concatenation               | ✅      | Adjacent strings merged       |
| Record index (#rec.field)                   | ✅      | Returns 1-based field index   |
| Named funs (fun Name(...) -> ... end)       | ✅      | With self-recursion           |
| 3-part catch (Class:Reason:Stacktrace)      | ✅      | OTP 21+ stacktrace binding   |
| maybe expression (OTP 25+)                  | ✅      | `?=` cond match, else clauses |
| Map comprehensions (OTP 26+)                | ❌      |                               |
| Sigils / triple-quoted strings (OTP 27+)    | ❌      |                               |

## Control Flow

| Feature                | Status | Notes                              |
|------------------------|--------|------------------------------------|
| case...of...end        | ✅      | Full Maranget pattern compilation  |
| if...end               | ✅      | Guard sequences                    |
| receive...end          | ✅      | With resumable continuations       |
| receive...after...end  | ✅      | Timeout support                    |
| Function clauses       | ✅      | Multi-clause with pattern matching |
| Guards                 | ✅      | Guard expressions and sequences    |
| Tail call optimization | ✅      | True TCO via Cranelift return_call |

## Pattern Matching

| Feature                       | Status | Notes                         |
|-------------------------------|--------|-------------------------------|
| Variables                     | ✅      |                               |
| Wildcards (_)                 | ✅      |                               |
| Underscore variables (_Var)   | ✅      | Repeated vars in patterns     |
| Integer literals              | ✅      | Arbitrary precision; decimal and Base#Digits, positive/negative |
| Float literals                | ✅      |                               |
| Atom literals                 | ✅      |                               |
| String literals               | ✅      | Compiled to list patterns     |
| Tuple patterns                | ✅      | With arity discrimination     |
| List patterns ([H\|T])        | ✅      | Head/tail destructuring       |
| Nil pattern ([])              | ✅      |                               |
| Binary patterns (<<...>>)     | ✅      | Byte and bit-level segments   |
| Map patterns (#{K := V})      | ✅      | Key presence + binding        |
| Record patterns               | ✅      | Desugared to tuple patterns   |
| Alias patterns (X = Pat)      | ✅      |                               |
| Nested patterns               | ✅      | Arbitrary depth               |
| Maranget decision trees       | ✅      | Optimal compilation           |
| Binary /rest patterns         | ✅      | <<H:8, Rest/binary>>; remainder must be divisible by the effective unit (8 bits by default) |
| Bound binary segment values   | ✅      | <<X:8, R/binary>> with X bound compares with =:= |
| Guard expressions in patterns | ✅      |                               |
| Bit-level patterns (<<X:3>>)  | ✅      | Sub-byte extraction           |
| String prefix ("prefix" ++ R) | ✅      | Desugared to cons patterns    |

## Binary Operations

| Feature                         | Status | Notes                    |
|---------------------------------|--------|--------------------------|
| Binary construction (<<1,2,3>>) | ✅      | Every segment value and size is evaluated, left to right, before any segment is checked |
| Binary segment specifiers | ✅ | OTP-compatible signed UTF and ignored flags, repeated declarations and binary alias units; conflicting types, signs, endianness or units are compile errors with OTP's conflict message |
| Segment sizes (<<X:16>>)        | ✅      | Any bit count            |
| Big endian (default)            | ✅      |                          |
| Little endian (/little)         | ✅      |                          |
| Native endian (/native)         | ✅      |                          |
| Signed/unsigned                 | ✅      |                          |
| /integer type                   | ✅      |                          |
| /binary type (rest capture)     | ✅      |                          |
| /float type                     | ✅      | 64-bit IEEE 754          |
| /utf8, /utf16, /utf32           | ✅      | Construction + matching; `/native` is the machine's byte order |
| String segments in binary construction | ✅ | One segment per codepoint with the string's own size, unit, type and flags (`<<"é":16>>` is `<<0,233>>`); default 8-bit segments truncate each codepoint; `/utf8`, `/utf16` and `/utf32` encode it |
| String segments in binary patterns | ✅ | One segment per codepoint; default values above 255 never match (BEAM semantics); UTF-8/16/32 patterns compare decoded codepoints; a size or a non-UTF type (`<<"ab":16>>`, `<<X, "ab"/binary>>`) is a compile error, as in OTP |
| Bit-level sizes                 | ✅      | Sub-byte segments        |
| Binary comprehensions           | ✅      | List + pattern-sized binary generators; bodies of any bit length; non-bitstring bodies raise badarg |

## Built-in Functions (BIFs)

Unqualified names follow the committed **OTP 29.0.4** auto-import catalogue,
not the native implementation table. Recognizing a name does not implement it:
strict builds still reject unavailable targets, while the existing explicit
`defer_undefined` project option retains runtime dispatch.

Explicit `-import(Module, [Function/Arity])` selects that module. Local functions
otherwise override auto-imports, with OTP 29 clash warnings at body calls and
import declarations. Imported local fun references require `fun Module:Function/Arity`;
conflicting imports and import/local definitions remain errors.

`-compile(no_auto_import)` disables all implicit imports;
`-compile({no_auto_import, [length/1]})` disables selected signatures.
These options accumulate across attributes and work inside compile-option lists.
Selective lists ignore non-signature entries, as OTP does.
Suppressed calls and fun references must resolve locally or through an explicit
import. Unqualified guard calls cannot use shadowed or suppressed names, except
legal explicit reimports from `erlang`. Compiler-specific record test/info forms
currently report unsupported features, not native availability.

**Source compatibility:** the former Nera-only auto-imports now require prefixes:
`erlang:make_tuple/2,3`, `erlang:append_element/2`, `erlang:phash2/1,2`,
`erlang:get_stacktrace/0`, `erlang:monotonic_time/0,1`, `erlang:system_time/1`,
`erlang:hibernate/3`, `erlang:start_timer/3`, and `erlang:cancel_timer/1,2`.
Their existing qualified Nera support is unchanged; qualification does not restore
APIs removed from OTP itself, such as `get_stacktrace/0`.

Maintainers with OTP 29.0.4 installed can verify the catalogue with
`python3 scripts/update-otp-auto-imports.py --check`; end users need no OTP install.

### erlang module

| Function            | Status |
|---------------------|--------|
| abs/1               | ✅      |
| max/2               | ✅      |
| min/2               | ✅      |
| length/1            | ✅      |
| hd/1                | ✅      |
| tl/1                | ✅      |
| element/2           | ✅      |
| tuple_size/1        | ✅      |
| setelement/3        | ✅      |
| byte_size/1         | ✅      | Rounds partial final bytes up, including deferred pattern segments and guards; non-bitstrings raise badarg and fail guards; direct, apply, variable module/function and fun reference calls |
| bit_size/1          | ✅      | Non-bitstrings raise badarg; guards on a non-bitstring fail |
| map_get/2           | ✅      |
| is_map_key/2        | ✅      | Auto-imported; guards, negated guards and bodies; non-maps raise badmap |
| map_size/1          | ✅      | Non-maps raise badmap; guards on a non-map fail |
| is_integer/1        | ✅      |
| is_float/1          | ✅      |
| is_number/1         | ✅      |
| is_atom/1           | ✅      |
| is_list/1           | ✅      |
| is_tuple/1          | ✅      |
| is_pid/1            | ✅      |
| is_binary/1         | ✅      |
| is_map/1            | ✅      |
| is_function/1,2     | ✅      |
| is_bitstring/1      | ✅      |
| is_reference/1      | ✅      |
| is_boolean/1        | ✅      |
| atom_to_list/1      | ✅      |
| list_to_atom/1      | ✅      |
| list_to_existing_atom/1 | ✅  |
| binary_to_existing_atom/1,2 | ✅ | /1: UTF-8; /2: latin1, unicode or utf8; invalid or missing names raise badarg |
| integer_to_list/1,2 | ✅      | Base 2..36 for /2     |
| list_to_integer/1   | ✅      |                       |
| list_to_integer/2   | ✅      | Base 2..36; flat ASCII lists, optional sign, bignums; badarg like BEAM |
| float_to_list/1     | ✅      |
| list_to_tuple/1     | ✅      |
| tuple_to_list/1     | ✅      |
| self/0              | ✅      |
| spawn/1             | ✅      | Closures              |
| spawn/3             | ✅      | Module:Function/Arity |
| spawn_link/1        | ✅      | Closures              |
| spawn_link/3        | ✅      | Module:Function/Arity |
| spawn_monitor/1     | ✅      | Closures              |
| spawn_monitor/3     | ✅      | Module:Function/Arity |
| make_ref/0          | ✅      |                       |
| register/2          | ✅      |                       |
| whereis/1           | ✅      |                       |
| unregister/1        | ✅      |                       |
| registered/0        | ✅      |                       |
| link/1              | ✅      |                       |
| unlink/1            | ✅      |                       |
| monitor/2           | ✅      |                       |
| demonitor/1         | ✅      |                       |
| process_flag/2      | ✅      | trap_exit, fullsweep_after, max_heap_size (integer or default-policy size map); nondefault kill/logging/shared-binary policies remain unsupported |
| garbage_collect/0   | ✅      | Full collection after saving compiled roots; returns true on resumption. Other arities are not implemented. |
| hibernate/3         | ✅      | Shrink heap to zero, wake on message      |
| spawn_opt/2         | ✅      | Closure + options (link, monitor, heap)   |
| spawn_opt/4         | ✅      | M:F:A + options                           |
| put/2               | ✅      | Exact (`=:=`) keys, including numeric types within compounds; returns old value or `undefined` |
| get/1               | ✅      | Exact key lookup; returns value or `undefined` |
| erase/1             | ✅      | Exact key removal; returns old value or `undefined` |
| get/0               | ✅      | Enumerate process-local key/value pairs without removing entries; order unspecified |
| erlang:unique_integer/0 | ✅   | Unique across processes in one runtime; options arity not implemented |
| os:timestamp/0      | ✅      | Wall-clock `{MegaSeconds, Seconds, MicroSeconds}` |
| get_keys/0,1        | ✅      | All keys or keys whose values match exactly (`=:=`); order unspecified |
| error/1             | ✅      |                       |
| error/2             | ✅      | Raises error:Reason through direct, apply and fun calls; Args is not kept, so the top stacktrace frame shows the arity, not the arguments |
| error/3             | 🔶      | Raises error:Reason through direct, apply and fun calls; Args and Options are accepted but not kept in the stacktrace, including error_info metadata |
| exit/1,2            | ✅      |                       |
| throw/1             | ✅      |                       |
| apply/3             | ✅      | Via nera_rt_apply    |
| node/0,1            | 🔶     | Local node is `nonode@nohost`; /1 accepts local PIDs and references, otherwise badarg; guards and dynamic calls. Ports/distribution are not supported. |
| append_element/2    | ✅      |                       |
| integer_to_binary/1,2 | ✅    | Base 2..36 for /2     |
| binary_to_integer/1,2 | ✅      |                       |
| binary_to_float/1   | ✅      |                       |
| list_to_binary/1    | ✅      |                       |
| bitstring_to_list/1 | ✅      | Full bytes plus an optional short bitstring tail |
| crypto:hash/2       | ✅      | md5, sha, sha224, sha256, sha384, sha512 over iodata; other types badarg (plain, not OTP's `{badarg, Where, Text}`); direct, apply and fun calls |
| crypto:strong_rand_bytes/1 | ✅ | OS secure generator; badarg for negative or non-integer N |
| list_to_bitstring/1 | ✅      | Bytes and bitstrings, nested lists and bitstring tails, packed bit by bit; direct, apply and fun calls |
| os:getenv/1        | ✅      | Character-list names (including nested lists), Unicode environment values; no getenv/0 or mutation APIs |
| binary_to_list/1    | ✅      |                       |
| iolist_size/1       | ✅      | Nested lists, binary tails, badarg |
| iolist_to_binary/1  | ✅      |                       |
| universaltime/0     | ✅      | UTC date/time, whole seconds; direct and dynamic calls |
| system_time/1       | ✅      | second, millisecond   |
| monotonic_time/1    | ✅      | second, millisecond   |
| send_after/3        | ✅      | Pending delivery keeps the scheduler alive; no OS thread per pending timer |
| start_timer/3       | ✅      | Pending delivery keeps the scheduler alive; no OS thread per pending timer |
| cancel_timer/1,2    | ✅      | Cancels send_after/start_timer; async/info boolean options; already sent messages remain |
| binary_part/3       | ✅      |                       |
| process_info/2      | ✅      | Item atom or list; see notes below |
| is_process_alive/1  | ✅      |                       |
| halt/0,1            | ✅      | Immediate termination |
| make_tuple/2,3      | ✅      |                       |
| atom_to_binary/1,2  | ✅      | /1: UTF-8; /2: latin1, unicode or utf8; latin1 rejects characters above 255 |
| float_to_binary/1,2 | ✅     | /2: [short], {decimals,N}, {scientific,N} |
| binary_to_atom/1,2  | ✅      | /2: latin1, unicode or utf8; 255-character limit |
| round/1             | ✅      |                       |
| trunc/1             | ✅      |                       |
| float/1             | ✅      |                       |
| ceil/1              | ✅      | OTP 20+              |
| floor/1             | ✅      | OTP 20+              |
| phash2/1,2          | ✅      |                       |
| term_to_binary/1    | ✅      | ETF v131 encoding     |
| binary_to_term/1    | ✅      | ETF v131 decoding     |
| size/1              | ✅      | Deprecated but common |
| raise/3             | ✅      | Re-raise with trace   |
| process_info/1      | ✅      | All-info form         |

process_info items supported: `registered_name`, `message_queue_len`,
`messages`, `links`, `monitors`, `monitored_by`, `trap_exit`, `status`,
`reductions`, `heap_size`, `memory`, `dictionary`, `current_stacktrace`,
`group_leader` (stored leader, inherited on spawn), `priority` (`normal` or
`low`). Unknown items raise `badarg`; a dead pid returns `undefined`.

### io module

| Function | Status |
|----------|--------|
| format/1 | ✅      |
| format/2 | ✅      |
| fwrite/2 | ✅      |
| put_chars/1 | 🔶 | UTF-8 chardata to standard output; group-leader I/O capture is not implemented |

Supported format directives: `~w` (write term), `~p` (basic term rendering),
`~s` (Latin-1 string/iodata), `~ts` (Unicode chardata),
`~tw`/`~tp` (Unicode term modifiers), `~d` (legacy decimal extension),
`~b`/`~B` (integer with optional radix), `~x`/`~X` (integer with supplied
prefix), `~+`/`~#` (integer with radix prefix), `~f`/`~e`/`~g` (float),
`~c`/`~tc` (character), `~W`/`~P` (depth-limited `~w`/`~p`), `~n`
(newline), `~~` (tilde) and `~i` (ignore).

`io:format/2`, `io:fwrite/2` and `io_lib:format/2` share field parsing:
`~F.P.PadControl` accepts literal fields or `*` arguments, including negative
widths for left adjustment. Term/integer overflow prints stars; string fields
truncate/pad; floats use decimal precision and BEAM-style rounding (six digits
by default). For example, `~.3f` prints `0.019` for `0.019`, while `~5w ~w`
prints `    a b` for `[a,b]`. Malformed fields and missing/extra arguments raise
`badarg` before stdout output. `~p` fields consume their arguments but do not
implement pretty-printer line wrapping/indentation. `~W` and `~P` take a term
and a depth and replace what lies deeper with `...`, as OTP does: `[1,2|...]`
for `[1,2,3,4,5]` at depth 3, `...` at depth 0 and the whole term at a negative
depth. They differ only for maps, where `~W` follows `io_lib:write/2` and `~P`
the pretty printer.
`~w`, `~p`, `~W` and `~P` write atoms as OTP does: bare only when they start
with a lowercase letter and hold only letters, digits, `_` and `@`, and are
not reserved words; otherwise quoted, with escapes inside the quotes
(`'EXIT'`, `'hello world'`, `'maybe'`, `'it\'s'`). Characters above 255 print
as `\x{...}` unless `t` is given.
Unicode string directives decode each binary independently as UTF-8, falling
back to Latin-1 bytes for that entire binary if decoding fails. The `l` modifier
accepts printable lists as numeric lists, as basic `~p` and `~P` already do.
The `k` and `K` map-order modifiers are accepted, and `K`
consumes its order argument, but neither changes map output order yet.
The separate `error_logger` renderer is unchanged.

### io_lib module

| Function  | Status | Notes                              |
|-----------|--------|------------------------------------|
| format/2  | ✅      | Returns iolist, same directives    |
| fwrite/2  | ✅      | Alias of format/2; accepts composed format strings |

### error_logger module

| Function        | Status | Notes                         |
|-----------------|--------|-------------------------------|
| info_msg/1,2    | ✅      | Colored timestamped output    |
| error_msg/1,2   | ✅      | Red colored error output      |
| warning_msg/1,2 | ✅      | Yellow colored warning output |

### maps module

28 exported functions. Hot-path functions as Rust FFI BIFs, higher-order
functions as compiled Erlang in `erlang/stdlib/maps.erl`.

| Function             | Status | Notes         |
|----------------------|--------|---------------|
| new/0                | ✅      | BIF           |
| get/2,3              | ✅      | BIF           |
| put/3                | ✅      | BIF           |
| find/2               | ✅      | BIF           |
| is_key/2             | ✅      | BIF           |
| remove/2             | ✅      | BIF           |
| keys/1               | ✅      | BIF           |
| values/1             | ✅      | BIF           |
| to_list/1            | ✅      | BIF           |
| from_list/1          | ✅      | BIF           |
| from_keys/2          | ✅      | BIF           |
| size/1               | ✅      | BIF; non-maps raise badmap |
| merge/2              | ✅      | BIF           |
| update/3             | ✅      | BIF           |
| take/2               | ✅      | BIF           |
| iterator/1           | ✅      | Erlang stdlib; `none` or `{Key, Value, Next}` iterators, entries in `maps:to_list/1` order; non-maps raise badmap |
| next/1               | ✅      | Erlang stdlib; `none` at the end, badarg for non-iterators |
| map/2                | ✅      | Erlang stdlib |
| filter/2             | ✅      | Erlang stdlib |
| filtermap/2          | ✅      | Erlang stdlib |
| fold/3               | ✅      | Erlang stdlib |
| foreach/2            | ✅      | Erlang stdlib |
| with/2               | ✅      | Erlang stdlib |
| without/2            | ✅      | Erlang stdlib |
| update_with/3,4      | ✅      | Erlang stdlib |
| groups_from_list/2,3 | ✅      | Erlang stdlib |
| intersect/2          | ✅      | Erlang stdlib |
| intersect_with/3     | ✅      | Erlang stdlib |

### gen_tcp module

| Function              | Status | Notes                                        |
|-----------------------|--------|----------------------------------------------|
| listen/2              | ✅      | Supports `{packet, line}`, `binary` options  |
| accept/1              | ✅      | Inherits listener socket options             |
| recv/2,3              | ✅      | Blocking recv with optional timeout          |
| send/2                | ✅      | Binary, iolist (nested lists/binaries)       |
| close/1               | ✅      |                                              |
| connect/3             | ✅      |                                              |
| controlling_process/2 | ✅      |                                              |

### inet module

| Function    | Status | Notes                                        |
|-------------|--------|----------------------------------------------|
| setopts/2   | ✅      | `{active, once}`, `{active, false}`,         |
|             |        | `{packet, line}`, `{packet, raw}`            |
| port/1      | ✅      | Returns port number for a socket             |

Socket handles are plain integers (GC-safe). Blocking I/O operations are
dispatched to an async I/O thread pool so scheduler threads stay free for
CPU-bound process reductions. Active-mode sockets (`{active, once}`) deliver
`{tcp, Socket, Data}` messages to the controlling process.
On the mio reactor path, queued and in-flight registration commands keep the
scheduler alive until registration completes or fails. This includes active
TCP registration before any readiness event exists.

### file module

| Function       | Status | Notes                           |
|----------------|--------|---------------------------------|
| read_file/1    | ✅      | Returns `{ok, Binary}` or error |
| write_file/2   | ✅      | Writes binary data to file      |

### lists module

Hot-path functions as Rust FFI BIFs, higher-order
functions as compiled Erlang in `erlang/stdlib/lists.erl`.

| Function          | Status | Notes               |
|-------------------|--------|---------------------|
| reverse/1,2       | ✅      | BIF                 |
| member/2          | ✅      | BIF; exact (`=:=`) equality, including integer/float and signed-zero distinctions |
| keyfind/3         | ✅      | BIF; `badarg` unless the position is an integer from 1 to 2^59 - 1, as on 64-bit BEAM, and for a non-list or an improper tail reached before a match |
| keymember/3       | ✅      | BIF; same argument checks as keyfind/3 |
| keysearch/3       | ✅      | BIF; same argument checks as keyfind/3 |
| append/1          | ✅      | BIF                 |
| join/2            | ✅      | Erlang stdlib; preserves separator terms |
| suffix/2          | ✅      | Exact suffix equality, including nested integer/float distinctions |
| last/1            | ✅      | BIF                 |
| nth/2             | ✅      | BIF                 |
| sublist/2         | ✅      | Erlang stdlib; bounded prefix, truncates at end of list |
| map/2             | ✅      | Erlang stdlib       |
| filter/2          | ✅      | Erlang stdlib       |
| foldl/3           | ✅      | Erlang stdlib       |
| foldr/3           | ✅      | Erlang stdlib       |
| sort/1,2          | ✅      | Erlang stdlib       |
| foreach/2         | ✅      | Erlang stdlib       |
| flatten/1,2       | ✅      | Erlang stdlib       |
| zip/2             | ✅      | Erlang stdlib       |
| unzip/1           | ✅      | Erlang stdlib       |
| seq/2,3           | ✅      | Erlang stdlib       |
| any/2             | ✅      | Erlang stdlib       |
| all/2             | ✅      | Erlang stdlib       |
| takewhile/2       | ✅      | Erlang stdlib       |
| dropwhile/2       | ✅      | Erlang stdlib       |
| partition/2       | ✅      | Erlang stdlib       |
| duplicate/2       | ✅      | Erlang stdlib       |
| keytake/3         | ✅      | Erlang stdlib; first match, numeric equality, preserves order |
| keydelete/3       | ✅      | Erlang stdlib; first match, numeric (`==`) equality |
| keyreplace/4      | ✅      | Erlang stdlib; first match, numeric (`==`) equality; tuple replacement only (`function_clause`) |
| keystore/4        | ✅      | Erlang stdlib; numeric (`==`) equality, replaces first match or appends; tuple replacement only (`function_clause`) |
| keysort/2         | ✅      | Erlang stdlib       |
| usort/1,2         | ✅      | Erlang stdlib       |
| max/1             | ✅      | Erlang stdlib       |
| min/1             | ✅      | Erlang stdlib       |
| sum/1             | ✅      | Erlang stdlib       |
| subtract/2        | ✅      | Erlang stdlib       |
| delete/2          | ✅      | Erlang stdlib       |

### math module

| Function | Status | Notes |
| --- | --- | --- |
| sqrt/1 | ✅ | Rust runtime BIF; nonnegative numbers to float, preserves negative zero; direct/dynamic/remote-fun calls |
| exp/1 | ✅ | Rust runtime BIF; overflow raises `badarith` |
| log/1 | ✅ | Rust runtime BIF; zero and negative numbers raise `badarith` |
| pow/2 | ✅ | Rust runtime BIF; both arguments as floats; overflow, a complex result and zero to a negative power raise `badarith` |

Nonnumbers raise `badarg`. Numbers outside the float range, and results that
are not finite real numbers (such as a negative number's square root), raise
`badarith`. This does not imply other `math` APIs are supported.

### string module

| Function | Status | Notes |
|----------|--------|-------|
| trim/3 | ✅ | Unicode grapheme trimming, `leading`/`trailing`/`both`; binaries stay binary, trimmed list/mixed chardata may be flattened to codepoints. Uses Unicode 17 segmentation. Malformed UTF-8 is rejected eagerly, unlike some OTP fast paths. |
| lowercase/1 | ✅ | Full Unicode mapping per codepoint, no context rules (final sigma stays σ), as OTP. Unchanged binaries and flat codepoint lists are returned as given; other changed or nested chardata becomes a flat codepoint list. `{badarg, Rest}` for bad UTF-8 or non-chardata. |
| slice/3 | ✅ | Grapheme slice; binaries stay binary. Lists and mixed chardata are decoded eagerly and return flat codepoint lists (OTP decodes lazily and may return the original tail structure). Binary input mirrors OTP's lazy handling of invalid UTF-8. |

### filename module

| Function | Status | Notes |
|----------|--------|-------|
| extension/1 | ✅ | Project-owned Erlang stdlib; Unix semantics (`/` separator, no Windows backslash handling). Binary in gives binary; atoms and nested lists give a list. |
| flatten/1 | ✅ | Atoms and nested lists become one flat character list; a binary is returned unchanged, and binaries inside a list are kept. |


### binary module

| Function  | Status | Notes                        |
|-----------|--------|------------------------------|
| split/2   | ✅      | BIF; binary or nonempty list of nonempty binaries, earliest then longest match |
| split/3   | ✅      | BIF, same patterns; `global` and `trim_all` options (`trim` and `scope` are not supported yet) |
| part/3    | ✅      | BIF, sub-binary extraction   |
| at/2      | ✅      | Byte at position             |
| last/1    | ✅      | Final byte; rejects empty binaries and non-byte-aligned bitstrings |
| longest_common_prefix/1 | ✅ | Byte count for a nonempty proper list of binaries; validates all elements, including after an empty prefix |
| match/2   | ✅      | Binary or nonempty list of nonempty binaries; earliest, then longest match. Compiled patterns are not supported (`compile_pattern/1` is unavailable). |
| match/3   | ✅      | As match/2 with the `{scope, {Start, Length}}` option, negative lengths included; offsets relative to the whole subject; unknown options and out-of-range scopes raise badarg |
| copy/1,2  | ✅      | /2 repeats N times           |
| referenced_byte_size/1 | ✅ | Reports retained binary backing size; direct Thoas copy/sharing scenarios validated |

### rand module

OTP 29's `rand` module, ported to compiled Erlang in `erlang/stdlib/rand.erl`.
Every OTP algorithm is available (`exsss`, the default, `exro928ss`, `exrop`,
`exs1024s`, `exsp`, and the older `exs64`, `exsplus`, `exs1024`), and a seed
gives the same numbers, states and errors as on OTP 29.

| Function | Status | Notes |
|----------|--------|-------|
| seed/1,2, seed_s/1,2 | ✅ | Integer, `{A1, A2, A3}` and list seeds, `default`, and seeding from an exported or full state |
| export_seed/0, export_seed_s/1 | ✅ | |
| uniform/0,1, uniform_s/1,2 | ✅ | Integer ranges of any size, bignums included; other ranges raise function_clause |
| uniform_real/0, uniform_real_s/1 | ✅ | |
| bytes/1, bytes_s/2 | ✅ | |
| normal/0,2, normal_s/1,3 | ✅ | Slower than on BEAM: each draw rebuilds a 256-entry lookup table, because Nera has no constant literal storage |
| shuffle/1, shuffle_s/2 | ✅ | |
| jump/0,1 | ✅ | |
| mwc59 and splitmix64 utilities | ✅ | `mwc59/1`, `mwc59_value32/1`, `mwc59_value/1`, `mwc59_float/1`, `mwc59_seed/0,1`, `splitmix64_next/1`, `exsp_next/1`, `exsp_jump/1` |

Without a seed, the first draw in a process seeds the default algorithm from
the process, a unique integer and the time, as on OTP. The state lives in the
process dictionary under `rand_seed`.

### ets module

| Function             | Status | Notes                               |
|----------------------|--------|-------------------------------------|
| new/2                | ✅      | `set`, `ordered_set`, `bag`, `duplicate_bag`, `named_table`, `{keypos, N}`; returns the name for a `named_table`, otherwise an integer table identifier (BEAM returns a reference) |
| insert/2             | ✅      | Single or list of tuples            |
| lookup/2             | ✅      | Returns list of matching tuples     |
| delete/1             | ✅      | Delete table                        |
| delete/2             | ✅      | Delete by key                       |
| delete_all_objects/1 | ✅      |                                     |
| match_object/2       | ✅      | Simple pattern matching             |
| select_delete/2      | ✅      | Match spec for conditional delete   |
| tab2list/1           | ✅      |                                     |
| info/1,2             | ✅      | `size`, `name`, `type`, `keypos`    |
| update_counter/3     | ✅      | Atomic counter increment (set/ordered_set only) |
| update_element/3     | ✅      | Update tuple element by position (set/ordered_set only) |
| give_away/3          | ✅      | Transfer table ownership            |
| member/2             | ✅      |                                     |
| insert_new/2         | ✅      |                                     |
| first/1              | ✅      |                                     |
| next/2               | ✅      |                                     |
| last/1               | ✅      | Sorted order for ordered_set        |
| prev/2               | ✅      | Reverse iteration for ordered_set   |
| set                  | ✅      | Hash-based, one value per key       |
| ordered_set          | ✅      | BTreeMap with Erlang term ordering  |
| bag                  | ✅      | Multiple values per key, no exact duplicates |
| duplicate_bag        | ✅      | Multiple values per key, duplicates allowed  |

### timer module

| Function      | Status | Notes                      |
|---------------|--------|----------------------------|
| sleep/1       | ✅      | Erlang stdlib              |
| send_after/2,3| ✅      | Erlang stdlib              |
| apply_after/4 | ✅      | Erlang stdlib              |
| tc/1,2,3      | ✅      | Erlang stdlib              |
| seconds/1     | ✅      | Erlang stdlib              |
| minutes/1     | ✅      | Erlang stdlib              |
| hours/1       | ✅      | Erlang stdlib              |
| hms/3         | ✅      | Erlang stdlib              |

### base64 module

| Function | Status | Notes |
|----------|--------|-------|
| encode/1 | ✅ | Standard alphabet and padding; delegates to the Rust-backed encode/2 |
| decode/1 | ✅ | Standard alphabet, padding required; delegates to the Rust-backed decode/2 |
| encode/2 | ✅ | Erlang stdlib; binary or flat byte list; standard/urlsafe mode, optional padding |
| decode/2 | ✅ | Binary result; standard/urlsafe mode; optional padding; ignores space, tab, CR and LF |

MIME decoding and string-returning APIs remain unavailable. Regression coverage
follows OTP 29.0.5 for valid byte input and the recorded malformed binary cases;
behavior for out-of-range/non-byte lists is not claimed to match OTP internals.

### uri_string module

| Function | Status | Notes |
|----------|--------|-------|
| parse/1 | ✅ | URI references to maps; binary/list components, authority, IPv6, query and fragment |
| normalize/1 | ✅ | Case, percent-encoding, dot segments and HTTP/HTTPS/FTP default ports |
| recompose/1 | ✅ | Maps to URI strings; component quoting and UTF-8 percent-encoding |
| resolve/3 | ✅ | RFC 3986 relative references; `[]` or `[return_map]` |

Scoped differential coverage against OTP 29.0.5 is recorded in the Cowlib
validation notes. Other uri_string APIs are unavailable; this is not a claim
of exhaustive OTP compatibility for malformed input or every URI scheme.

### calendar module

| Function | Status | Notes |
|----------|--------|-------|
| day_of_the_week/1 | ✅ | Erlang stdlib; Monday=1 through Sunday=7 |
| datetime_to_gregorian_seconds/1 | ✅ | Proleptic Gregorian calendar, epoch year 0 |
| gregorian_seconds_to_datetime/1 | ✅ | Negative seconds/years and large integers; OTP 29 semantics |
| universal_time/0 | ✅ | Erlang wrapper for the current UTC date/time |
| valid_date/1 | ✅ | Gregorian leap rules; `function_clause` unless a tuple of three integers |
| system_time_to_universal_time/2 | ✅ | Units `second`, `millisecond`, `microsecond`, `nanosecond`, `native` or a positive integer; floors; `badarg` otherwise |
| universal_time_to_system_time/1 | ✅ | Seconds since 1970-01-01 UTC |

### nera module (compiler intrinsics)

| Function        | Status | Notes                    |
|-----------------|--------|--------------------------|
| memory_stats/0  | ✅      | Runtime memory reporting |

### persistent_term module (OTP 21+)

Read-optimized global key-value storage. Values are deep-copied into shared
storage; reads use `parking_lot::RwLock` with a fast path for immediate keys
(atoms, small integers).

| Function      | Status | Notes                                     |
|---------------|--------|-------------------------------------------|
| put/2         | ✅      | Store any term; deep-copied to global heap |
| get/1         | ✅      | Badarg if key not found                    |
| get/2         | ✅      | Returns default if key not found           |
| erase/1       | ✅      | Returns true                               |
| info/0        | ✅      | Returns #{count, memory}                   |

### counters module (OTP 22+)

Mutable integer counter arrays with lock-free atomic operations.
Raw pointer stored in reference term — zero-overhead access.

| Function      | Status | Notes                     |
|---------------|--------|---------------------------|
| new/2         | ✅      | Creates array, returns ref |
| get/2         | ✅      | 1-based index              |
| put/3         | ✅      |                            |
| add/3         | ✅      |                            |
| sub/3         | ✅      |                            |
| info/1        | ✅      | Returns #{size, memory}    |

### zlib module

Streaming deflate and inflate through `zlib:open/0` handles, implemented
natively with the pure-Rust `zlib-rs` (zlib-compatible output, no system zlib).
Covers the calls Cowlib's `cow_ws` and `cow_deflate` make. Streams belong to
the process that opened them until `set_controlling_process/2` hands them over;
other processes get `not_on_controlling_process`, as on OTP.

| Function                          | Status | Notes                                              |
|-----------------------------------|--------|----------------------------------------------------|
| open/0, close/1                   | ✅      | `close/1` frees the compression state              |
| deflateInit/2,6                   | ✅      | Window bits 8..15, -15..-8 (raw), 24..31 (gzip)    |
| deflate/2,3                       | ✅      | Flush: none, sync, full, finish                    |
| deflateReset/1, deflateEnd/1      | ✅      |                                                    |
| inflateInit/1,2                   | ✅      | Window bits 8..15 or -15..-8; gzip headers not yet |
| inflate/2, safeInflate/2          | ✅      | `safeInflate` stops after 16 KiB of output         |
| inflateReset/1, inflateEnd/1      | ✅      |                                                    |
| set_controlling_process/2         | ✅      |                                                    |
| compress/uncompress/gzip/zip, crc32, dictionaries | ❌ | Not implemented                          |

### atomics module (OTP 22+)

Atomic 64-bit integer arrays with CAS support. Same direct-pointer
optimization as counters — on par with BEAM performance.

| Function           | Status | Notes                              |
|--------------------|--------|------------------------------------|
| new/2              | ✅      | Options: [{signed, true\|false}]   |
| get/2              | ✅      | 1-based index                      |
| put/3              | ✅      |                                    |
| add/3              | ✅      |                                    |
| sub/3              | ✅      |                                    |
| add_get/3          | ✅      | Returns new value                  |
| sub_get/3          | ✅      | Returns new value                  |
| exchange/3         | ✅      | Returns old value                  |
| compare_exchange/4 | ✅      | ok on success, current val on fail |
| info/1             | ✅      | Returns #{size, max, min, memory}  |

### Not yet available stdlib modules

| Module      | Notes                                        |
|-------------|----------------------------------------------|
| string      | String manipulation functions                |
| math        | pi, sin, cos, etc. (sqrt, exp, log and pow are available) |
| os          | cmd, getenv, type, timestamp                 |
| re          | Regular expressions                          |
| crypto      | Cryptographic functions                      |
| proplists   | Property list operations                     |
| ordsets      | Ordered set operations                       |
| sets         | Set operations                               |
| gb_trees     | General balanced trees                       |
| gb_sets      | General balanced sets                        |
| queue        | Double-ended queue                           |
| array        | Functional arrays                            |
| dict / orddict | Dictionary operations                      |
| uri_string   | URI parsing and composition                  |

## OTP Behaviours

| Feature                       | Status | Notes                                  |
|-------------------------------|--------|----------------------------------------|
| gen_server                    | ✅      | start_link, call, cast, info, reply; `{error, Reason}` when init/1 fails, `ignore` when it ignores; stops with its parent when it traps exits |
| supervisor (one_for_one)      | ✅      | start_link, child specs, restart logic with retries; OTP start results: `{error, Reason}` when init/1 fails, `failed_to_start_child`, `ignore`; stops with its parent and stops its children in reverse start order by their shutdown setting |
| application                   | ✅      | start/stop, app_name → app_name_app    |
| proc_lib                      | ✅      | spawn, spawn_link, init_ack            |
| error_logger                  | ✅      | info/error/warning with colored output |
| gen_event                     | ❌      |                                        |
| gen_statem                    | ❌      |                                        |

The OTP modules are implemented as compiled Erlang in `erlang/stdlib/` and
automatically included with user code. They are sufficient to run real OTP
applications with supervision trees (validated with OneCached).

## Module System

| Feature                       | Status | Notes                                                 |
|-------------------------------|--------|-------------------------------------------------------|
| -module declaration           | ✅      |                                                       |
| -export declaration           | ✅      |                                                       |
| Local function calls          | ✅      |                                                       |
| Auto-imported BIFs            | ✅      | BIFs marked auto-imported in the BIF table; explicit imports take precedence |
| Remote calls (M:F(Args))      | ✅      | Static resolution via BIF table                       |
| Dynamic apply (M:F/A)         | ✅      | Via runtime function table lookup                     |
| -define / -ifdef preprocessor | ✅      | Object-like/function-like macros and arity overloads; argument grouping handles keyword blocks and distinguishes fun references |
| Macro argument text (`??Parameter`) | ✅ | Unexpanded argument tokens converted to a string; nested-header forwarding supported |
| -include                      | ✅      | Include search paths                                  |
| -include_lib                  | ✅      | Resolves via app registry (OTP + deps + project apps), including nested headers |
| -record declaration           | ✅      | Desugared to tuples at IR level                       |
| Multi-module compilation      | ✅      | Direct cross-module linking                           |
| -if constant conditions      | Partial | Integer-literal comparisons and boolean operators, including macro expansion; unsupported guards and `-elif` remain unsupported |
| ?OTP_RELEASE                 | ✅      | Major release from the selected external OTP installation's `releases/start_erl.data`; undefined when unavailable |
| BEAM parse-transform preparation | External build step | Explicit generic helper prepares external dependency sources; BEAM is required at build time only |
| ?MODULE / ?MODULE_STRING      | ✅      | Built-in preprocessor macros                          |
| ?LINE / ?FILE                 | ✅      | Built-in preprocessor macros                          |
| ?FUNCTION_NAME / ?FUNCTION_ARITY | ✅   | OTP 19+ macros                                        |
| -import declaration           | ✅      | Explicit module/function/arity resolution for ordinary and tail calls; see rules below |
| -on_load / -compile / -vsn    | 🔶     | Parsed but silently ignored                           |
| -type / -spec declarations    | 🔶     | Parsed but ignored                                    |
| -behaviour / -callback        | ✅      | Parsed; callback validation for OTP behaviours        |

An explicit `-import(Module, [Name/Arity])` makes that signature available
without a module qualifier. Duplicate imports and conflicts with local function
definitions are rejected. Imported functions from modules other than `erlang`
cannot be used as guard BIFs. Imports do not enable arbitrary `-compile` options
or general parse transforms. The explicit `eunit_autoexport` transform is
recognized for zero-arity `_test`/`_test_` functions with the default suffixes;
it does not execute arbitrary transforms or implement custom transform options.
Native OTP EUnit now runs both Cowlib HTTP suites
([validation](tests/integration/cowlib/README.md#native-otp-eunit--2026-10-14)).
Thoas native framework execution has not been rerun in this validation. The Thoas test module compiles, but direct execution through
an external adapter now passes all 36 assertion bodies
([validation details](COMPATIBILITY.md#thoas-native-assertion-corrections--2026-10-11)).

## Concurrency & Processes

| Feature                            | Status | Notes                          |
|------------------------------------|--------|--------------------------------|
| Process spawning (spawn/1,3)       | ✅      | Closures and M:F/A             |
| Message send (!)                   | ✅      | Deep-copy, to PIDs and names   |
| Message receive                    | ✅      | With pattern matching          |
| Receive with timeout (after)       | ✅      | Monotonic clock (NTP-safe)     |
| Process isolation (separate heaps) | ✅      |                                |
| Preemptive scheduling              | ✅      | 4000 reductions/slice          |
| Work-stealing scheduler            | ✅      | Multi-threaded                 |
| Back-edge yield checks             | ✅      | Prevents tight-loop starvation |
| self/0                             | ✅      |                                |
| Resume continuations               | ✅      | For receive-wait               |
| Process links                      | ✅      | Bi-directional                 |
| Process monitors                   | ✅      | With DOWN messages             |
| Registered processes               | ✅      | register/whereis/unregister    |
| Process dictionary                 | ✅      | put/get/erase/get_keys         |
| EXIT signals                       | ✅      | With trap_exit support         |
| Process hibernate                  | ✅      | Zero-heap suspend, MFA wake    |
| process_flag(fullsweep_after)      | ✅      | Control major GC frequency     |
| process_flag(max_heap_size)        | ✅      | Kill runaway processes         |
| spawn_opt/2,4                      | ✅      | link, monitor, heap options    |
| Async I/O pool                     | ✅      | Dedicated I/O thread pool      |
| Event-driven I/O (mio reactor)     | ✅      | epoll/kqueue for TCP sockets   |
| Supervision trees                  | ✅      | one_for_one strategy           |

## Memory Management

| Feature                       | Status | Notes                              |
|-------------------------------|--------|------------------------------------|
| Per-process heap (bumpalo)    | ✅      | Bump allocator                     |
| Generational GC               | ✅      | Young/old copying GC               |
| Adaptive GC threshold         | ✅      | 2× live data, min 256 KiB          |
| Idle heap compaction          | ✅      | Shrink on receive-wait             |
| fullsweep_after control       | ✅      | Configurable major GC frequency    |
| max_heap_size enforcement     | ✅      | Kill on exceed after GC            |
| Hibernate (zero heap)         | ✅      | Discard heap, wake via MFA         |
| Atom table limit              | ✅      | 1,048,576 atoms (matches BEAM)     |
| Deep copy for messages        | ✅      | With forwarding pointers           |
| Shared structure preservation | ✅      | In deep copy                       |
| Closure deep copy             | ✅      | Recursive env var copy in messages |
| Binary virtual heap           | ✅      | GC triggers on refc binary size    |
| ETS mmap region pool          | ✅      | Per-table mmap bump allocator      |
| Refc binaries (>64 bytes)     | ✅      | Arc-backed shared binaries         |
| Lock-free message passing     | ✅      | MPSC inbox via SegQueue            |

GC is triggered automatically after each process time-slice when the heap
exceeds its threshold. All roots are gathered (context, args, continuations,
mailbox, process dictionary) and the heap is compacted via copying collection.

Atoms are never garbage-collected (same as BEAM). The atom table is capped
at 1,048,576 entries; exceeding this panics with `system_limit`. Network
error reasons use pre-registered POSIX atoms (e.g. `eaddrinuse`, `econnreset`)
to avoid unbounded atom creation at runtime.

## Code Generation

| Feature                   | Status | Notes                            |
|---------------------------|--------|----------------------------------|
| Cranelift backend         | ✅      | AOT compilation to native binary |
| C-ABI FFI bridge          | ✅      | Runtime <-> compiled code        |
| True tail call opt (TCO)  | ✅      | All targets: local, cross-module, apply, closure |
| Pattern match compilation | ✅      | Maranget algorithm               |
| Reduction counting        | ✅      | For preemptive scheduling        |
| Multi-module linking      | ✅      | Direct cross-module calls        |
| Cross-compilation         | ✅      | Multi-arch via --target triple   |

## Compilation & Tooling

| Feature                    | Status | Notes                                     |
|----------------------------|--------|-------------------------------------------|
| --src directory scanning   | ✅      | Recursive .erl discovery                  |
| -I / --include paths       | ✅      | Repeatable include dirs                   |
| --deps-dir (OTP apps)      | ✅      | rebar3 dependency scanning                |
| nera init                  | ✅      | Initialize project in current directory   |
| nera deps get              | ✅      | Fetch hex.pm, git, and path dependencies  |
| nera deps add              | ✅      | Add hex dep to nera.toml and fetch        |
| .nera/deps/ auto-discovery | ✅      | Build picks up fetched deps automatically |
| --emit-ir                  | ✅      | Debug IR output                           |
| --emit-obj                 | ✅      | Object files only                         |
| -e entry point             | ✅      | module:function (default main:start)      |
| --target cross-compilation | ✅      | x86_64, aarch64, macOS/Linux/Windows      |
| --linker override          | ✅      | Custom toolchain support                  |
| nera test                  | ✅      | Built-in parallel test runner             |
| nera check                 | ✅      | Parse + IR + validate without codegen     |
| --no-stdlib                | ✅      | Disable stdlib auto-inclusion             |

### Explicit deferred remote calls and OTP EUnit

`[project] defer_undefined = true` in `nera.toml` permits unavailable remote
functions, external fun references and spawn targets to compile. They dispatch
at runtime and raise `error:undef` when invoked. The default remains strict;
undefined local functions and statically known private functions still fail
compilation. Recognized but unimplemented OTP auto-imports such as
`open_port/2`, `port_close/1`, `is_port/1` and `spawn_link/2` also follow this
rule. Recognizing a signature does not implement that primitive.

`nera test` supports a generic `[test]` section with `framework = "eunit"`,
an explicit `modules` list and optional test `src` directories. It builds an
isolated entry with project sources/includes/resolved dependencies and executes
selected modules cumulatively; failures and timeouts fail the command.
`--list` and module patterns are supported; `-j` remains legacy-runner only.
See [project test configuration](tests/README.md#project-eunit-configuration).

OTP EUnit is an **external project dependency**, not a Nera stdlib replacement.
Its source and the selected OTP `gb_trees` module stay outside this repository.
`module_info/1` is generated for `exports` and `module`; `exports` lists every
function of a module compiled with `export_all`. Other items and
`module_info/0` are not provided. `fun_info/2` supports `module`, `name`, `arity`,
and `fun_info_mfa/1` returns those three values. Anonymous names belong to the
Nera compiler and need not match BEAM's generated names.

Additional scoped APIs used by EUnit:

- `group_leader/0,2`: live local process leaders and inheritance on spawn.
- `process_flag(priority, normal | low)`: stores the priority and uses a
  reduced (one eighth) reduction quantum for low priority. This is weighted
  scheduling, not OTP's complete priority queue policy; `high`/`max` are absent.
- `statistics(wall_clock)`: elapsed and previous-call milliseconds only.
- `proplists:get_all_values/2`, `dict:new/0,size/1,find/2,store/3,erase/2,
  from_list/1,to_list/1`; `sets:new/0,from_list/1,to_list/1,size/1,is_element/2,
  add_element/2,del_element/2,union/2,fold/3`. Dict/set representations are opaque
  maps; the rest of their OTP APIs are not implemented.
- `unicode:characters_to_binary/1,2,3` and `characters_to_list/1,2`: chardata in
  `latin1`, `unicode`/`utf8`, `utf16`, `utf32` and `{utf16 | utf32, big | little}`,
  with `error` and `incomplete` results. Normalization and the other `unicode`
  APIs are not provided. In rare error results that split one character across
  nested binaries, the shape of the returned `Rest` can differ from OTP.

The Cowlib validation covers original functional assertions and controlled
EUnit success/failure, generator errors, exits, timeouts and process isolation.
It does not validate generic OTP/EUnit compatibility, captured test I/O,
remote nodes, peer/port helpers, or native PropEr generation/shrinking.

## Native PropEr (scoped)

With explicitly prepared external PropEr 1.5.0 and pinned OTP 27.3 `rand` and
`ordsets` dependencies, Nera runs real seeded generation, shrinking and
`proper:check/3` counterexample replay. Three seeds repeated cumulatively match
BEAM's complete integer/list/binary traces; minimized failures and adjacent
passing examples are checked. Cowlib's two original Base64url properties also
pass under native PropEr with seeded trials, false controls and an independent
encoding oracle. The original Cowlib date property also passes with native
generation, three date formats, exact field checks and false-control shrinking.
All 26 original Cowlib HTTP header properties also pass with seeded native
generation and false-control replays. Generic PropEr compatibility and the
remaining Cowlib modules are outside this bounded scope. BEAM remains a build-time
requirement for parse transforms; native execution uses no BEAM process.
See [scope and reproduction](tests/integration/proper/README.md).

## Compiler Diagnostics

| Feature                             | Status | Notes                               |
|-------------------------------------|--------|-------------------------------------|
| Line/column in errors               | ✅      | Via SourceMap                       |
| Source snippets in errors           | ✅      | With caret underlining              |
| Colored output (codespan-reporting) | ✅      | rustc-quality diagnostics           |
| Unused variable warnings            | ✅      | Respects `_`-prefix convention      |
| Unreachable clause warnings         | ✅      | Guardless catch-all detection       |
| Shadowed variable warnings          | ✅      | Nested scope detection              |
| Unused function warnings            | ✅      | Not exported and not called locally |
| Illegal map construction            | ✅      | `:=` in `#{}` flagged as error      |
| Exported variable warnings          | ✅      | Bound in all branches, used after   |
| Unused record warnings              | ✅      | Defined but never referenced        |
| Behaviour callback validation       | ✅      | Missing callbacks for OTP behaviours|
| Reserved keyword checks             | ✅      | All 27 Erlang reserved words        |
| Runtime error reporting             | ✅      | Process crash with class/reason     |
| Stack traces                        | ✅      | module:function/arity frames        |

## Formatter (nera-fmt)

Style choices, including intentional departures from `erlfmt`.

**Grouping repair (2026-10-10):** the reproduced unary-parentheses defect is
fixed, with deterministic regressions and BEAM comparisons. See P0-2 in project
status for the scope and evidence; this inventory is not a guarantee of full
Erlang formatter equivalence.

### Matches erlfmt

| Decision                                   | Notes                                  |
|--------------------------------------------|----------------------------------------|
| Hanging brackets for containers            | Tuples, lists, maps: `{`/`[`/`#{` and closing bracket on their own lines when the container breaks |
| One element per line on break              | "Consistent break" — no paragraph packing for data containers |
| One clause per line                        | `case`, `if`, `try`, function clauses never share a line |
| Short single-expr clause bodies flatten    | `ok -> 1;` stays on the arrow line when it fits under 100 cols |
| Multi-expr bodies always break after `->`  | Avoids half-flat/half-broken output    |
| 4-space indent, 100-col max width          | Defaults match erlfmt                  |

### Intentional departures from erlfmt

| Decision                                   | Rationale                              |
|--------------------------------------------|----------------------------------------|
| **Function call arguments use Fill (greedy packing)** | Call args are positional and semantically related to one signature, often short (atoms, vars, numbers). Fill packs them on as few lines as possible. erlfmt uses break-all everywhere; we think that explodes `gen_server:call(Name, Msg, Timeout)`-style calls unnecessarily the moment they tip past the width. **Rule: break-all for data containers, Fill for call arguments.** |

### Not yet supported

- `-spec` / `-type` / `-opaque` declarations — currently passed through as raw source
- `-define` bodies and macro call-site formatting
- "Last argument expansion" (funs or big containers as the last call arg staying inline with the call)
- Binary operator chains: pairwise recursion only — long `A andalso B andalso C` chains may indent unevenly

## Known Limitations

| Area                          | Notes                                                        |
|-------------------------------|--------------------------------------------------------------|
| Distribution / remote nodes   | No clustering; `node/0` returns stub atom                    |
| Ports                         | No port driver interface                                     |
| io input functions            | No `io:get_line/1`, `io:read/1`                              |

## Test Coverage

| Component                                             | Tests   |
|-------------------------------------------------------|---------|
| Syntax (lexer + parser + source_map + errors)         | 224     |
| IR (lowering + patterns + bif_table)                  | 245     |
| Codegen (code generation + linking)                   | 77      |
| Runtime (heap, GC, BIFs, terms, processes, scheduler) | 388     |
| CLI + dependency management                           | 246     |
| End-to-end (.erl compile + run)                       | 280     |
| **Total**                                             | **1459** |
