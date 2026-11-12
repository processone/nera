# Changelog

All notable milestones for the nera project are documented in this file.

## erlang:max/2 and min/2 return an argument unchanged

- `erlang:max/2` and `min/2` now return one of their two arguments
  unchanged, the first one when the two compare equal, as on BEAM:
  `max(2, 1.5)` is `2` and `max(1, 1.0)` is `1`. Previously a mixed integer
  and float pair turned the integer into a new float, so these returned `2.0`
  and `1.0`, and `min(1, 1.0)` returned `1.0`.
- An integer and a float now compare exactly, as with `<`:
  `max(9007199254740993, 9007199254740992.0)` returns the integer, which is
  larger. Previously the integer was rounded to a float first, so an integer
  above 2^53 could compare equal to a smaller float.
- The `max_min_numbers` fixture runs natively and on BEAM.

## Cowlib 2.22.0 cookie and date upstream suites run natively

- Cowlib 2.22.0's original `cow_cookie` and `cow_date` EUnit suites pass
  through `nera test` with OTP's EUnit, 157/157 and 27/27 as on BEAM, with no
  skipped or changed assertions. The Linux workflow runs both.
- `filename:flatten/1` is exported, as in OTP. OTP's EUnit server calls it, so
  native EUnit projects failed to build with "function `filename:flatten/1` is
  not exported".
- `~W` and `~P` print the term cut at the given depth, as OTP does, instead of
  the literal text `~W` or `~P`. EUnit's reporter passed that text to
  `io:fwrite/2` again and failed with `badarg` when it reported a cancelled
  test group.
- A process in `receive ... after` could wait forever. A sender cancelled the
  receiver's timer after queueing it, when the receiver could already be in a
  later timed receive, and a late registration could replace a newer timer.
  Senders now leave the timer to the worker that runs the process, and the
  timer is registered before the wait becomes visible.
- A program could exit with status 0 while a process that had just exited
  still had to deliver its EXIT or DOWN signal. The idle check now waits for
  exit cleanup to finish.
- With these three scheduler fixes, the native `cow_cookie` run no longer
  hangs or stops early: about 1 run in 30 did before, and 1,200 consecutive
  runs pass after. Runtime unit tests replay each ordering.
- The `filename_flatten` and `io_format_depth` fixtures run natively and on
  BEAM. The Cowlib harness takes `--pin 2.22.0` beside the default 2.20.0 pin.
- Cowlib's date property still cannot run natively: `rand:uniform/1` is a
  built-in that `rand:seed/2` does not control.

## ets:new/2 returns the name of a named table

- `ets:new(Name, Options)` with `named_table` now returns `Name`, as on
  BEAM. Previously it returned an integer, so the common
  `?MODULE = ets:new(?MODULE, [named_table, ...])` match failed with
  `{badmatch, 1}`; Cowboy's `cowboy_clock:init/1` stopped there.
- Only a table created with `named_table` is reachable by its name. A table
  created without it no longer takes the name, so creating or deleting one
  leaves a named table with the same atom in place, and a named table may
  now be created after an unnamed one with the same atom.
- A lookup by table name no longer holds the name lock while it waits for
  the table lock. Process-exit cleanup takes the two locks in the other
  order, so the two could block each other and every later ETS call.
- When two processes create the same named table at once, only one now
  succeeds; the other gets `badarg`, as on BEAM. Previously both could
  succeed.
- The `ets_named_table_return` fixture runs natively and on BEAM; runtime
  unit tests cover the name's release on owner exit, the lock order and
  concurrent creation.

## Supervisor child start results and ignore

- `supervisor:start_link/2,3` now fails with
  `{error, {shutdown, {failed_to_start_child, Id, Reason}}}` when a child does
  not start, as in OTP, instead of logging the error and starting without
  that child. The children after it are not started. A registered name is
  released first, and a linked caller that does not trap exits exits with
  that reason.
- A child's start result is read as OTP's supervisor reads it: `{ok, Pid}`
  or `{ok, Pid, Info}` with a pid starts the child, `ignore` keeps it as not
  running (a temporary child is dropped), `{error, Reason}` gives Reason and
  any other value gives that value. A thrown value is the start function's
  return value; an exit gives `{'EXIT', Reason}` and an error
  `{'EXIT', {Reason, Stacktrace}}`. Before, `ignore` failed the supervisor
  with `case_clause`, an exit failed it with the exit reason, an error with
  `{Reason, Stacktrace}` and a throw with `{{nocatch, Value}, Stacktrace}`.
- A restart that fails is tried again once the messages already waiting are
  handled, and each try counts against the restart limit; past the limit the
  supervisor exits with `shutdown`, as in OTP. Before, the child was left
  stopped. A restart that returns `ignore` now keeps the child as not
  running; before, it crashed the supervisor.
- When init/1 of a gen_server or a supervisor returns or throws `ignore`,
  `gen_server:start/3,4`, `gen_server:start_link/3,4` and
  `supervisor:start_link/2,3` return `ignore`, as in OTP, instead of
  `{error, ignore}`. The process exits normally, so a linked caller that does
  not trap exits gets `ignore` too.
- `application:start/1,2` returns
  `{error, {bad_return, {{Mod, start, [Type, []]}, Value}}}` when the
  application's `start/2` returns a value it does not accept, `ignore`
  included, as in OTP, instead of crashing with `case_clause`.
- The new `supervisor_child_start` fixture and the extended
  `gen_server_init_crash` fixture run natively and on BEAM. The new
  `application_start_ignore` fixture runs natively.

## Garbage collection of deeply nested terms

- A process holding a list nested hundreds of thousands of levels deep in
  head position, such as an iolist built by prepending (`[Acc, Chunk]`),
  now survives garbage collection, as on BEAM. Previously the collector
  copied list heads recursively on the native stack, and the program died
  with a bus error once a collection copied about 13,000 levels.
- Tuples, maps and closure environments nested the same way survive too.
  Long lists kept working, since the collector already followed list tails
  in a loop.
- The collector now keeps the slots it still has to copy on a heap-allocated
  work stack, so nesting depth costs memory instead of native stack. Objects
  are copied in the same order as before, and list cells with immediate heads
  are still copied in one loop, so full collections of a 1,000,000-element
  integer list stay within a few percent of their previous time.
- The `gc_deep_nesting` fixture collects a 200,000-level iolist, tuple, map
  and closure chain, on request and from allocation, natively and on BEAM.
  Storing such terms in ETS, comparing them with `<` or `==`, sending them
  to another process, capturing them in a spawned fun, and
  `term_to_binary/1`, `erlang:phash2/1` or `iolist_to_binary/1` on them can
  still crash; [known limits](docs/KNOWN_LIMITS.md) gives the depths.

## gen_server and supervisor start when init/1 fails

- `gen_server:start_link/3,4`, `gen_server:start/3,4` and
  `supervisor:start_link/2,3` no longer wait forever when init/1 raises,
  exits or returns a value they do not accept. The caller monitors the new
  process from the moment it is spawned, as OTP's `proc_lib` does, and gets
  `{error, Reason}` when it traps exits or did not link. A linked caller that
  does not trap exits exits with Reason, as on BEAM.
- Reason is BEAM's: `{Error, Stacktrace}` for an error, the exit reason for an
  exit, `{bad_return_value, Value}` for a gen_server and
  `{bad_return, {Mod, init, Value}}` for a supervisor. A thrown value is
  init/1's return value, as in OTP. The failure is also written to the error
  log, except an exit or `{stop, Reason}` whose reason is `normal`,
  `shutdown` or `{shutdown, _}`.
- When a gen_server's init/1 returns `{stop, Reason}`, the new process now
  exits with Reason, as in OTP, so a linked caller that does not trap exits
  exits too. When it returns `{error, Reason}`, the caller gets
  `{error, Reason}` and the process exits normally, as on OTP 26 and later.
  A supervisor's init/1 returning `{stop, Reason}` gives
  `{bad_return, {Mod, init, {stop, Reason}}}` and throwing it gives
  `{error, Reason}`, as in OTP. A throw from a child's start function fails
  the supervisor's start instead of being read as init/1's return value.
- A name that is already registered gives
  `{error, {already_started, Pid}}`, as in OTP, instead of
  `{error, badarg}`. init/1 is not run.
- A registered name is released before the caller gets the answer, and the
  caller keeps no `'EXIT'` or `'DOWN'` message from the failed start, as on
  OTP 26 and later. This also holds when a supervisor's flags are invalid or
  a child crashes while the supervisor starts.
- After `demonitor/1,2`, no `'DOWN'` message for that monitor arrives, as on
  BEAM. Before, a process that exited at the same moment could still deliver
  one.
- Previously a supervisor, which traps exits, hung while starting a child
  whose init/1 crashed. It now gets the child's `{error, Reason}`, logs it and
  goes on with its other children; OTP instead fails the supervisor's start
  with `{shutdown, {failed_to_start_child, Id, Reason}}`.
- The `gen_server_init_crash` fixture runs natively and on BEAM.

## Unsafe variables rebound by funs and comprehensions

- A name bound in only some branches of a `case`, `if`, `receive` or `try`
  is no longer reported as unsafe when a later fun head, named fun or
  comprehension generator binds it again, as in `[K || K <- L]`,
  `<< <<K>> || <<K>> <= B >>`, `fun(K) -> K end` or
  `fun K(0) -> 0; K(N) -> K(N - 1) end`. erlc binds those names afresh and
  compiles such code; `nera check` and `nera build` now accept it too.
- Reads that still reach the enclosing name keep the
  `variable 'K' unsafe in '...'` error, as in erlc: a fun body, guard or
  other clause whose head does not rebind it, a map key or binary size in a
  fun head, a comprehension body or filter where no generator rebinds it,
  and a generator source, as in `[K || K <- K]`.

## Atoms ordered by name

- Atoms now compare by their names, as on BEAM, whatever order they were
  created in: `zebra < apple` is false and `lists:sort([zebra, apple, mango])`
  returns `[apple, mango, zebra]`. Previously atoms compared by their atom
  table index, which follows creation order, so results depended on which
  atoms a program mentioned first.
- This applies wherever terms are ordered: `<`, `>`, `=<`, `>=` and guards,
  `lists:sort/1,2`, `usort/1`, `max/1` and `min/1`, tuples,
  lists and maps that contain atoms, and `ordered_set` ETS tables with atom
  keys. A sorted key list of Cowlib's server-sent event map now matches BEAM.
- Names compare as UTF-8 bytes, a name before every longer name that starts
  with it, which is BEAM's order for Unicode atoms too.
- `erlang:max/2` and `min/2` follow the term order for every pair that is not
  two numbers, and return the first argument when the terms are equal.
  Previously they compared raw term words, so `max(zebra, apple)` and
  `max(1, foo)` could return the wrong term.
- Atom equality is still one integer comparison. Atom names are now read
  without a lock, and an order key made from each name's first eight bytes
  decides most comparisons.
- The `atom_order` fixture runs natively and on BEAM.

## Receiving the message []

- A process now receives the message `[]` like any other message, and the
  messages queued after it are still received, as on BEAM. Previously the
  mailbox scan used `[]` to mean "no more messages", so `receive X -> X end`
  timed out on a `[]` message and on every message behind it.
- The scan now ends with a value that is never a term, and compiled
  `receive` checks for it with one inline comparison instead of comparing
  each message with `[]`.
- The `receive_nil_message` fixture covers `[]` first, in the middle and last
  in the mailbox, `[]` patterns and guards, `after 0` and no `after`,
  selective receive skipping it, and a waiting process woken by it, natively
  and on BEAM.

## Cowlib missing functions: maps iterators, binary:match/3, list_to_integer/2, rand:uniform/1

- `maps:iterator/1` and `maps:next/1` iterate every entry of a map and end
  with `none`; a non-map raises `{badmap, Term}` and a non-iterator `badarg`.
- `binary:match/3` accepts the `{scope, {Start, Length}}` option, including
  negative lengths; offsets in the result are relative to the whole subject.
  Unknown options and scopes outside the subject raise `badarg`.
- `list_to_integer/2` parses flat character lists in bases 2 to 36, with an
  optional sign and bignum results, and raises `badarg` on BEAM's inputs.
- `rand:uniform/1` returns an integer in 1..N for any positive integer N,
  bignums included, and raises `function_clause` for other arguments.
- Together these removed five of the 23 errors `nera check` reported on
  Cowlib 2.22.0. The `maps_iterator`, `binary_match_scope`,
  `bif_list_to_integer_base` and `rand_uniform` fixtures run natively and on
  BEAM.

## byte_size/1 through apply and fun references

- `byte_size/1` reached through `apply/2,3`, a variable module or function
  name (`M:byte_size(B)`, `erlang:F(B)`) or a fun reference
  (`fun byte_size/1`, `fun erlang:byte_size/1`, `fun M:F/1`) now returns the
  byte count, rounding a partial final byte up, as on BEAM. Previously these
  calls returned the bit count, so `apply(erlang, byte_size, [<<"abc">>])`
  was 24.
- These calls raise `badarg` for a term that is not a bitstring, as direct
  calls do.
- The `bif_byte_size_dynamic` fixture compares every call form with a direct
  call for binaries, sub-binaries and partial-byte bitstrings, and checks
  `badarg` for 20 terms that are not bitstrings, natively and on BEAM.

## Bound names in clause patterns

- A clause pattern naming an already-bound variable is a comparison, as in
  erlc: `T = id(0), case id(0) of T -> matched; _ -> other end` no longer
  warns that the `_` clause cannot be reached. This covers case, receive,
  `try ... of` and catch clauses, and names bound by a parameter, a block,
  a receive timeout, a case scrutinee, a generator or a fun's enclosing
  scope.
- Such a name is no longer reported unused when a block, a receive timeout
  or another expression bound it; a later `T = ...` match compares too.
- A catch clause catches everything only when its class is `_` or a fresh
  variable, so `catch error:_ -> a; _:_ -> b` and `catch X -> X; _:_ -> b`
  no longer warn that the `_:_` clause cannot be reached.
- A clause after a fresh variable, after a fun head that shadows an outer
  name, or after a catch-all of the same exception class, is still reported
  unreachable.

## Badarg from byte_size/1 and bit_size/1

- `byte_size/1` and `bit_size/1`, and their `erlang:`-qualified forms, now
  raise `badarg` for a term that is not a bitstring, as on BEAM.
  Previously they returned 0, so a list, atom, tuple, number or map looked
  like an empty binary.
- A guard that calls `byte_size/1` or `bit_size/1` on such a term now fails,
  so the next clause or guard alternative is tried. Previously
  `when byte_size(X) =< N` accepted any list, which made Cowlib 2.22.0's
  `cow_iolists:split/2` keep a whole iolist as one piece.
- Known gap: a comprehension filter such as `[B || B <- L, byte_size(B) > 0]`
  now raises `badarg` for an element that is not a bitstring. On BEAM such a
  filter is a guard and skips the element. Native filters that call
  `map_size/1`, `length/1` or `tuple_size/1` already raise the same way.
- The `bitstring_size_badarg` fixture runs these sizes, errors and guards
  in function heads, `case` and `if`, natively and on BEAM.

## binary:split with a list of patterns

- `binary:split/2,3` now accept a nonempty list of nonempty binaries as the
  pattern, as on BEAM. Each split is at the earliest match, taking the
  longest pattern at that offset, and `global` splits never overlap:
  `binary:split(<<"a\r\nb">>, [<<"\r">>, <<"\r\n">>])` returns
  `[<<"a">>, <<"b">>]`. Previously any list, even `[<<2>>]`, raised
  `badarg`, so Cowlib's `cow_sse:parse/2` failed on `<<"data: hi\n\n">>`.
- `binary:split/2,3` and `binary:match/2` read their patterns with the same
  rules. An empty list, an empty binary, an improper list, a non-binary
  element or a bitstring with a partial last byte raises `badarg`. A single
  bitstring pattern such as `<<1:3>>` now raises `badarg` in
  `binary:split/2,3` too, instead of being rounded up to a whole byte.
- The `binary_split_patterns` fixture runs Cowlib's line and field splits
  natively and on BEAM.

## zlib streams

- The `zlib` calls Cowlib's `cow_ws` and `cow_deflate` make now build and run
  natively: `open/0`, `close/1`, `deflateInit/2,6`, `deflate/2,3`,
  `deflateReset/1`, `deflateEnd/1`, `inflateInit/1,2`, `inflate/2`,
  `safeInflate/2`, `inflateReset/1`, `inflateEnd/1` and
  `set_controlling_process/2`. Previously every call failed with
  `module zlib is not available`, so Cowlib did not build.
- Streams are typed references backed by the pure-Rust `zlib-rs` crate, a new
  dependency of `nera-runtime`. They return OTP's error reasons
  (`not_initialized`, `already_initialized`, `data_error`, `stream_error`,
  `bad_windowbits` and the other `bad_*` reasons) and enforce the controlling
  process.
- The `zlib_streams` fixture runs the Cowlib patterns, context takeover and
  no_takeover resets, the `safeInflate` output limit, error reasons and
  dynamic calls, natively and on BEAM.
- Not yet covered: gzip inflate, preset dictionaries, `compress/1`, `gzip/1`
  and the other one-shot helpers. A closed stream keeps its small handle until
  the node exits; `close/1` frees the compression state.
## calendar:valid_date/1 and system-time conversions

- Add `calendar:valid_date/1`, `calendar:system_time_to_universal_time/2` and
  `calendar:universal_time_to_system_time/1`, which Cowlib's `cow_date` and
  `cow_cookie` call. Results and errors match OTP 29, including flooring of
  negative system times and `function_clause` for malformed dates.
- The `calendar_system_time` fixture runs natively and on BEAM.

## crypto:hash/2 and crypto:strong_rand_bytes/1

- `crypto:hash/2` now compiles and returns OTP 29's digest of any iodata for
  `md5`, `sha`, `sha224`, `sha256`, `sha384` and `sha512`, and
  `crypto:strong_rand_bytes/1` returns that many bytes from the operating
  system's secure generator. Previously `nera check` and `nera build` rejected
  them as unresolved functions, which blocked Cowlib's `cow_ws` and
  `cow_http_hd`; calls through `apply` raised `undef`.
- Unsupported digest types, data that is not iodata and a negative or
  non-integer byte count raise `badarg`. OTP 29 raises
  `{badarg, {File, Line}, Text}` for an unknown digest type, which native
  code reports as plain `badarg`. Other `crypto` functions and digest types
  are not available yet.
- The `bif_crypto_hash` fixture runs published digest vectors and these calls

## string:lowercase/1, string:slice/3 and filename:extension/1

- Add `string:lowercase/1` and `string:slice/3` as native BIFs and
  `filename:extension/1` as a project-owned stdlib function. All three
  previously failed to resolve, which is among the gaps keeping Cowlib 2.22.0
  from building.
- `lowercase/1` maps every Unicode scalar value like OTP 29 (checked over the
  whole range against BEAM); `slice/3` counts graphemes and follows OTP's
  handling of invalid UTF-8 in binaries; `extension/1` follows OTP's Unix rules.
- Known differences: `slice/3` decodes lists eagerly and returns flat codepoint
  lists; `extension/1` has no Windows backslash handling.
- New `string_lowercase`, `string_slice` and `filename_extension` fixtures run
  natively and on BEAM.

## unicode:characters_to_binary/2,3 and characters_to_list/1,2

- `unicode:characters_to_binary/2,3` and `unicode:characters_to_list/1,2` now
  compile and run natively, as on OTP 29. They accept `latin1`, `unicode`,
  `utf8`, `utf16`, `utf32` and `{utf16 | utf32, big | little}` input and output
  encodings: `unicode:characters_to_binary(<<233>>, latin1, utf8)` returns
  `<<195,169>>` and `unicode:characters_to_list(<<0,97>>, utf16)` returns `"a"`.
  Previously these calls were rejected at build time as unknown functions, which
  stopped Cowlib from building.
- Unconvertible and truncated input returns `{error, Converted, Rest}` or
  `{incomplete, Converted, Rest}`; unknown encodings and non-chardata raise
  `badarg`.
- The `unicode_encodings` fixture runs these calls natively and on BEAM.


## list_to_bitstring/1

- `list_to_bitstring/1` now compiles and returns BEAM's bitstring, joining
  bytes and bitstrings nested in lists bit by bit:
  `list_to_bitstring([<<1:3>>, <<2:5>>])` returns `<<34>>` and
  `list_to_bitstring([1 | <<5:3>>])` returns `<<1, 5:3>>`. Previously calls
  naming it directly failed to compile as an unresolved function, and calls
  through `apply` or variable module and function names raised `undef`.
- Direct, `erlang:`-qualified, `apply` and fun reference calls all use the
  runtime join that binary comprehensions already use, and raise `badarg`
  on the same inputs as BEAM.
- The `bif_list_to_bitstring` fixture runs these calls natively and on BEAM.

## Binary comprehensions with partial-byte bodies

- A binary comprehension whose body yields bitstrings that are not whole
  bytes now packs them bit by bit, as on BEAM:
  `<< <<X:4>> || <<X>> <= <<1,2>> >>` returns `<<18>>` and
  `<< <<B:1>> || B <- [1,0,1] >>` returns `<<5:3>>`. Previously both raised
  `badarg`, because the comprehension joined its pieces with
  `iolist_to_binary/1`, which accepts whole bytes only.
- Results above the inline binary size, and bodies taken from inside a
  larger binary, keep every bit.
- The `bin_comp_bits` fixture runs these comprehensions natively and on
  BEAM.


## Badmap from map_size/1 and maps:size/1

- `map_size/1`, `erlang:map_size/1` and `maps:size/1` now raise
  `{badmap, Term}` for a non-map, as on BEAM, including through `apply`,
  dynamic calls and `fun M:F/A` references. Previously they returned 0, so
  an atom, list, tuple or binary looked like an empty map.
- A guard that calls `map_size/1` on a non-map now fails, so the next clause
  or guard alternative is tried. Previously `when map_size(M) =:= 0`
  accepted any non-map as an empty map.
- The `map_size_badmap` fixture runs these sizes, errors and guards in
  function heads, `case` and `if`, natively and on BEAM.

## Bindings from receive timeouts

- A name bound in a `receive`'s `after` timeout expression can now be read
  after the `receive`, as erlc allows and BEAM runs. Previously
  `receive never -> ok after (T = id(0)) -> ok end, T` failed to compile
  with "variable 'T' is unbound"; it now returns 0. The name is bound
  whether a message clause or the `after` body ends the `receive`. Later
  patterns, guards, fun captures and fun-head map keys read it too, and a
  binary tail matched in the timeout can be read after the `receive`.
- As in erlc, the message clauses and the `after` body still cannot read a
  name the timeout binds. A later read is rejected unless every branch
  binds that name again on every path, or none binds it at all.
- The `receive_timeout_bindings` fixture runs these reads natively and on
  BEAM.


## Bindings exported from blocks and branches

- A variable bound inside `begin ... end`, or in every branch of a `case`,
  `if` or `receive` (its `after` branch included), can now be read after
  the expression, as on BEAM, also inside a fun in a record field default.
  Previously `case M of 1 -> Y = a; _ -> Y = b end, Y` failed to compile
  with "undefined variable", and so did reads in later bodies, binary
  constructions and fun captures.
- Map keys and binary sizes in fun heads and comprehension generators now
  use such a variable's value. Previously they compiled without an error
  but used no value: after `case W of true -> N = 16; _ -> N = 8 end`,
  `[V || <<V:N>> <= <<1, 2, 3, 4>>]` returned `[1,2,3,4]` instead of
  `[258,772]`, and `fun(<<V:N>>) -> V end` raised `function_clause`.
- Guards and later patterns now compare against the exported value.
  Previously a guard such as `when B > L` did not see it, and a later
  `{Y}` pattern or `Y = X` match bound a fresh `Y` instead of comparing.
  A binary tail bound in every branch, such as `Rest` in
  `<<1, Rest/binary>>`, is exported the same way.
- A name bound in a comprehension's first generator source, such as `Y` in
  `[X || X <- (Y = L)]`, now stays local to it, as on BEAM. Previously a
  later `Y = 2` compared against `L` and raised `badmatch`.
- A binary tail read inside a `maybe` block without `?=`, such as `Rest`
  from `<<_, Rest/binary>>` in `maybe id(Rest), ok end, Rest`, can still be
  read after the block. Previously the later read returned an undefined
  value.
- A variable bound in only some branches, or inside `try`, `maybe` or
  `catch`, is still rejected as unsafe.
- The `branch_export_reads` fixture runs these reads natively and on BEAM.

## Binary tails in guard sequences

- In a guard sequence `G1; G2`, when `G1` raises before it reads a binary
  tail bound by an earlier pattern (such as `K` in `<<_, K/binary>>`), `G2`
  now reads that tail as BEAM does. Previously `G2` compared an undefined
  value, so `if element(1, M) =:= K; K =:= <<"k">> -> b; true -> c end`
  returned `c` for a map `M`. This applied to `if`, `case`, function head,
  `receive`, `try` and `maybe ... else` guards.
- One regression fixture covers `if`, `case`, function head and `receive`
  guards, with the clause body and a read after the expression, on native
  Nera and on BEAM.

## On-time receive timeouts

- A native `receive ... after N` (and so `timer:sleep/1`) now resumes about
  1 ms after N on macOS, at normal, utility and background priority, in the
  main process or a spawned one. macOS postponed the timer thread's timed wait
  when the program ran at utility or background priority (timer coalescing):
  a 37 ms timeout resumed about 67 ms late at utility priority and 93 ms late
  at background priority, and five 10 ms waits in a row took about 200 ms and
  475 ms. At normal priority it resumed 3–5 ms late, and the five waits took
  60 ms. Receive deadlines now use a kqueue timer that macOS fires on time;
  the timer thread's idle wait stays coalescable.
- A timer registered while the timer thread was computing its next sleep could
  lose its wake-up, then fire only at the earlier deadline or after the 100 ms
  idle wait. Wake-ups now stay pending until the timer thread waits.
- `scripts/test-native-timer.py` measures the delay from a program's output
  just before a `receive after` to the timeout's output, in the main process, a
  spawned process and a loop of waits, at each priority on macOS.
- `erlang:send_after/3` and `erlang:start_timer/3` still use an ordinary timed
  wait and can be late at background priority on macOS. Linux and Windows
  timing was not measured.

## Improper lists in lists key searches

- `lists:keyfind/3`, `lists:keymember/3` and `lists:keysearch/3` now raise
  `badarg` when the list argument is not a list, or when the search reaches
  an improper tail before finding the key, as on BEAM. Previously they
  returned `false`, so a wrong argument looked like a missing key. A match
  found before an improper tail is still returned, because BEAM checks the
  tail only when the search reaches it.
- The key argument-error fixture covers non-list values, improper tails
  after short and 100,000-element walks, atom, integer, float and tuple keys,
  and hits before the tail, through direct, `apply`, dynamic and
  `fun M:F/A` calls, on native Nera and on BEAM.

## Argument errors in lists key functions

- `lists:keyreplace/4` and `lists:keystore/4` now raise `function_clause`
  when the replacement is not a tuple, as on BEAM. Previously both put it in
  place of a matching tuple, and `keystore/4` appended it when no key matched.
- `lists:keyfind/3`, `lists:keymember/3` and `lists:keysearch/3` now raise
  `badarg` for a position BEAM rejects: below 1, not an integer, or an
  integer of 2^59 or more (a bignum on 64-bit BEAM). Previously they
  returned `false`, so a bad position looked like a missing key.
- One regression fixture checks every error through direct, `apply`,
  dynamic and `fun M:F/A` calls, on native Nera and on BEAM.

## Explicit build inputs

- `nera build` and `nera check` with `.erl` files named on the command line and
  no `--src` now compile only those files, plus the stdlib and dependencies,
  whatever folder they run from. Previously the current folder's `tests/` was
  added too, so building `demo/serverless/handler.erl` from the repository root
  also compiled every fixture under `./tests` and failed on their unresolved
  calls. Builds given only `--deps-dir` follow the same rule.
- Project builds (`nera build`, `nera run` without file arguments) and `--src`
  builds still compile the project's `tests/`, as does the language server.
- A CLI regression runs the built compiler on an explicit file from a folder
  whose `tests/` cannot compile, and from an unrelated folder.

## Unused-variable warnings in pattern expressions

- Variables used only in map-pattern keys or binary sizes count as used.
  Unused-variable analysis now also includes sizes reading earlier segments
  of the same binary, in clause patterns and body matches (including chains).
  Truly unused map values, binary values and arguments still produce warnings.
- A shared diagnostic/native regression covers the HTTP stream lookup and
  prefix/tag extraction shapes, nested patterns, function and fun heads,
  case, receive and try/catch. The same fixture is checked against BEAM.

## Numeric equality in tuple-key updates

- `lists:keydelete/3`, `lists:keyreplace/4` and `lists:keystore/4` now match
  keys with `==`, as on BEAM. Integer and float keys with the same value
  match, including inside compound keys. Only the first matching tuple changes;
  `keystore/4` appends only when no key matches.
- These searches now skip non-tuples and tuples shorter than the requested
  position, rather than raising `badarg` for short tuples. All three public
  entries reject nonpositive or noninteger positions with `function_clause`.
- Shared map comparison now preserves exact map-key identity and canonical
  ordering, including nested integer/float and signed-zero distinctions.
  Corresponding map values still compare numerically with `==`. The correction
  also applies to sibling key searches, comparison operators and ETS ordered sets.
- The same native and BEAM regressions cover numeric equality, map-valued keys,
  preserved list order, misses, invalid positions and direct, apply, dynamic
  and remote-fun calls, plus shared comparison and ETS ordering.

## Negative binary-size formatting (2026-10-27)

- The formatter keeps required parentheses around negative binary sizes such as
  `<<_:(-1)/unit:256>>`. Previously it printed `<<_:-1/unit:256>>`, which the
  parser rejects, because the parsed negative number was treated as a bare
  primary literal. Binary construction and pattern sizes share the correction;
  ordinary positive sizes and segment values keep their existing formatting.

## Oversized binary patterns in debug builds (2026-10-26)

- Debug builds now compile enormous constant binary-pattern sizes, including
  `<<_:1152921504606846975/unit:256>>`, without an integer overflow panic.
  Pre-binding checks the size/unit product against the pattern compiler's
  nonnegative, below-2^60 range and uses an impossible-size placeholder for
  rejected sizes instead of wrapping in release builds. Matching still
  rejects these clauses; ordinary size/unit extraction is unchanged.
  Pre-binding and matching now share this checked-size policy and limit.
- Minimum-size sums also saturate when an enormous fixed prefix precedes
  a UTF-8, UTF-16 or UTF-32 segment, preventing a second debug overflow.
  Impossible clauses still reject, and ordinary UTF decoding is unchanged.
- A native regression covers case, function and fun heads, receive, try-of
  and catch patterns, with binary/bitstring segments and normal controls.

## Unbound fun and generator pattern inputs (2026-10-26)

- Map keys and binary sizes in fun heads and generators now report an
  unbound-variable compile error when their only binding comes from the same
  head, as erlc does. For example, `fun(K, #{K := V}) -> V end` and
  `[V || {N, <<V:N>>} <- Pairs]` are rejected instead of silently reading
  a nil key or using a default 8-bit size.
- Outer inputs, earlier-generator inputs, named fun self references and sizes
  reading earlier segments of their own binary remain accepted. A binding in
  a sibling argument, map value or separate binary cannot supply an input.
- Both list and binary comprehensions check both generator variants. These
  semantic errors remain errors with `--no-warnings`; sibling tuple elements
  or call arguments do not supply inputs.
- The AST diagnostic pass now recognizes `begin` bindings and common bindings
  from every `case`, `if` and `receive` branch. Direct ordinary body reads of
  those exports still fail during IR lowering with an undefined-variable
  error. When an export supplies a fun-head or generator map key or binary
  size instead, the build succeeds silently and the read falls back to a nil
  key or an 8-bit size. That silent fallback already exists on main. NERA-127
  tracks both export-lowering limitations. Case-scrutinee bindings are
  available inside its clauses and do compile correctly.
- Record defaults now reject any unbound variable, not only fun-head or
  generator inputs. An unbound default such as `-record(r, {a = X})` is a
  compile error even if the record is unused or its default is overridden.

## Binary segment specifier compatibility (2026-10-26)

- Binary segments now accept signed UTF types such as `<<X/utf8-signed>>`,
  as OTP 29 does. Irrelevant signedness and byte-order flags on float and
  binary segments are accepted too; they do not change the encoded value.
- Conflicting explicit types, signs, byte orders and units are compile errors
  with OTP's conflict message, instead of silently keeping the last value.
  `<<X/binary-utf8>>` reports "conflict in type specification for bit field:
  'utf8' and 'binary'". Repeated identical declarations remain valid.
- Binary aliases follow OTP's merging rules: `bytes` declares unit 8, while
  `bits` and `bitstring` declare unit 1. Plain `binary` supplies only a default
  unit, so `binary-bits` and `bits-binary` both use unit 1, but `bytes-bits`
  conflicts. String-pattern checks still take precedence as in OTP.
- The formatter preserves written specifier lists, including conflicting
  lists and the `bits`, `bitstring` and `bytes` aliases. Formatting cannot
  remove a conflict before compilation or insert an alias's implicit unit.
- Native regressions cover accepted lists in construction and matching, plus
  conflicting types in both positions. A formatter regression covers written
  lists in expressions and patterns.

## Typed string literals in binary patterns (2026-10-26)

- A string literal in a binary pattern with a size, or with a type list that
  names no UTF type, is now a compile error with OTP's message "a literal
  string in a binary pattern must not have a type or a size". Before,
  `<<"ab":16, R/binary>>` compiled and matched an 8-bit `"ab"`, and
  `<<X, "ab"/binary>>` matched any non-empty binary.
- As in OTP 29, explicit defaults such as `"ab"/integer` or `"ab"/big` are
  refused too, and the refused segment gets no other segment error. Default,
  `/utf8`, `/utf16` and `/utf32` string patterns, and strings in binary
  construction, are unchanged.
- The check covers every pattern position, including generators and record
  defaults, and module lowering refuses these patterns where lint does not
  run. Two native compile-error regressions cover both examples.

## Parenthesized remote-call targets (2026-10-26)

- Remote calls now accept parenthesized module and function expressions,
  including `m:(F)()`, `(M):f()` and `(module_name()):(function_name())(Args)`,
  as OTP does. A trailing call still applies the returned fun.
- Computed remote calls evaluate the module, function and then arguments,
  left to right, as OTP does. Fun calls also evaluate the callee before their
  arguments. An earlier target error keeps its reason and skips later effects.
- The formatter keeps required parentheses around computed remote targets,
  but prints argument-less module macro targets bare: `?MODULE:f()` and
  `?SERVER:call(Request)`. It also removes explicit module-target parentheses
  around these macros; multi-token definitions must supply their own grouping.
  Function macro targets keep parentheses, as in `m:(?F)(X)`, so formatting
  cannot select a parameterized `?F(X)` macro instead.
- A shared native/BEAM regression exercises both positions, computed targets,
  nested parentheses, zero- and multiple-argument calls, returned funs, effects
  and exception order, including same-function `try` handlers.

## Binary construction evaluation order (2026-10-26)

- Binary construction now evaluates every segment value and size, left to
  right and value before size, before it checks any segment, as BEAM does.
  `<<X:Neg, (1 div Z)>>` with a negative `Neg` and `Z = 0` raises `badarith`
  instead of the `badarg` of the earlier size, with or without `try`. This
  covers every segment check: integer, float and binary sizes, unsized
  binary sources and `/binary` strings.
- An empty string's dynamic size is also checked after every expression ran,
  as OTP's evaluator does; the OTP 29 compiler checks it where it appears.
- A shared native/BEAM regression compares 28 readings with OTP 29.

## Binary string construction widths and encodings (2026-10-26)

- A string in binary construction is now one segment per codepoint, each with
  the string's own size, unit, type and flags, as OTP documents:
  `<<"é":16>>` gives `<<0,233>>`, `<<"λ"/utf16>>` gives `<<3,187>>` and
  `<<"λ"/utf32>>` gives `<<0,0,3,187>>`. Endianness, surrogate pairs, sub-byte
  sizes, units and float types apply to every codepoint. Previously every
  string except `/utf8` was written as truncated 8-bit bytes.
- A dynamic size is evaluated once and checked like any segment size, even
  for an empty string: `<<"":N>>` raises `badarg` for a negative or
  non-integer `N`. A `/binary` or `/bits` string raises `badarg`, as on BEAM.
- Inside `try`, a segment size or value that fails, such as `1 div 0`, now
  keeps its own reason (`badarith`) instead of being reported as `badarg` by
  the segment check that followed it.
- Default 8-bit strings and `/utf8` strings are unchanged.
- `/utf16-native` and `/utf32-native` segments now use the machine's byte
  order when building and matching, as integer segments already did. They
  were big-endian before, so on little-endian machines such as arm64 and
  x86-64 they disagreed with BEAM.
- A shared native/BEAM regression compares 75 readings with OTP 29, in
  constant and runtime-built binaries, guards, `try` and comprehensions. It
  leaves out literal size 8 with a binary type or another unit, where OTP 29's
  compiler takes a byte shortcut that its own evaluator does not:
  `<<"ab":8/binary>>` gives `<<"ab">>` compiled but `badarg` evaluated.

## Binary-pattern string codepoints (2026-10-23)

- Binary-pattern strings now expand to Unicode codepoints instead of UTF-8
  bytes, both when checking a match and when binding the following segments.
  `<<"é">>` matches `<<233>>`, and `<<"λ"/utf8>>` matches `<<206,187>>`,
  in case expressions, function heads and match expressions, as on BEAM.
- Default 8-bit patterns containing characters above 255, including `\400`
  and `\777`, never match on OTP 29; they do not truncate like binary
  construction or match the characters' UTF-8 bytes.
- A shared native/BEAM regression covers Latin-1, octal boundaries, UTF-8,
  UTF-16/32 endianness, malformed/truncated input, following tails and shifted
  bit offsets, with ASCII/empty-string controls and an explicit completion marker.

## Private embedded runtime cache on Unix (2026-10-23)

- Standalone Unix compilers now extract embedded runtimes into an owner-only
  per-user cache directory. A cache hit requires a regular, owner-only archive
  owned by the effective user; shared-temp archives and symlinks are not reused.
  This prevents another local account from planting or modifying the archive
  used by a fallback build. Extraction refuses temporary-folder ancestry that
  other accounts can replace; normal system-owned sticky `/tmp` is supported.
  If the temporary root is unsafe or unavailable, the compiler instead creates
  a private `$HOME/.nera-runtime` cache. The account home must belong to the
  effective user and must not be writable by group or other users; its ancestors
  must also prevent rename-swaps. Linux namespace overflow ownership is tolerated
  for ancestors only, without relaxing their write/sticky permission checks.
- If another account preclaims the per-user directory name, or an unsafe entry
  obstructs the archive name, extraction uses a fresh private directory instead.
  The compiler creates disposable storage under the selected temp or home root,
  removes it after linking (including linker failures), and leaves the
  obstructing entry untouched. Content keys, concurrent atomic
  publication, and installed/workspace library precedence are unchanged.
- The home cache keeps its archives rather than pruning them: normally about
  30 MB per compiler build, with identical embedded runtimes sharing one archive.
  When no Nera build is running, users can manually delete the archives or
  `$HOME/.nera-runtime` itself; the next fallback build recreates what it needs.
- Windows retains account-temporary-directory caching. Symlink archive entries
  are treated as obstructions rather than followed.

## Exact list membership (2026-10-22)

- `lists:member/2` now uses exact (`=:=`) equality, as on BEAM. It no longer
  matches integers against numerically equal floats, including inside tuples,
  lists and maps. Positive and negative zero are distinct, as on OTP 27+.
- A shared native/BEAM regression covers mismatches, equal separately constructed
  values, empty lists, later exact matches and direct, dynamic, apply and fun
  calls. The key-search family keeps its numeric (`==`) equality.

## Exact process dictionary keys (2026-10-22)

- `put/2`, `get/1` and `erase/1` now use exact (`=:=`) key equality, as on
  BEAM. Integer and float keys such as `1` and `1.0` remain separate, including
  within tuples, lists and maps. Positive and negative zero keys are distinct
  as on OTP 27+; updating or erasing one key leaves the other unchanged.
- A shared native/BEAM regression covers missing keys, replacement and erasure
  return values, dictionary contents, equal separately constructed keys and
  direct, qualified, dynamic and fun calls.

## Process dictionary value filtering (2026-10-22)

- `get_keys/1` is now auto-imported and available as `erlang:get_keys/1`,
  including dynamic calls and fun references. It returns all process dictionary
  keys whose stored values are exactly equal (`=:=`) to the argument, without
  changing the dictionary. Result order is unspecified; `get_keys/0` is unchanged.
- Exact float comparison now distinguishes positive and negative zero, including
  inside compound values, as on OTP 27+. This also corrects the shared exact
  comparison used by `=:=`, `=/=`, pattern matching, map-key lookup and list
  subtraction. Float-literal patterns such as `f(0.0)` no longer match `-0.0`;
  numeric `==` is unchanged.
- A native/BEAM regression covers Ranch's active-child lookup, multiple matches,
  empty/missing matches, nested integer/float and signed-zero distinctions,
  compound values, binaries, arbitrary keys, state updates, erasure and isolation.

## Exact ends for dynamic binary patterns (2026-10-22)

- Binary patterns with dynamic segment sizes or variable-width UTF now reject
  surplus bits unless an explicit rest segment consumes them. The final size
  check is shared by heads, case, match, maybe, receive and generators.
- A generator such as `<<B:N/binary, N:8>> <- L2`, after `<<N:8>> <- L1`,
  uses the earlier `N` for its size and binds the new `N`, as on OTP 29.
  The input scopes already handled this; the missing end check incorrectly
  accepted longer candidates as well.
- A shared native/BEAM regression covers the requested results, list and binary
  comprehensions, filters, later generators, repeated names and exact ends
  across the shared pattern compiler's callers.


## OTP import resolution (2026-10-22)

- Align unqualified names with a reproducibly derived OTP 29.0.4 catalogue,
  separate from native availability. Missing implementations remain rejected
  in strict builds; deferred mode still requires explicit configuration.
- Honor global and selective `no_auto_import`, including compile-option lists,
  across calls, guards, fun references and optimized self/spawn/binary-size paths.
  Unqualified `throw` now follows the same resolution policy. Compiler-generated
  module metadata explicitly qualifies its error call so suppression and local
  overrides cannot alter it.
- Warn about local/import auto-import clashes as OTP 29 does. Reject shadowed
  or suppressed guard calls, retaining legal explicit `erlang` reimports.
  Report unsupported compiler-specific record forms explicitly.
- **Breaking source change:** the 13 former Nera-only auto-import signatures
  now require module prefixes. See `FEATURES.md` for the complete list.
  Qualified support is unchanged; the append-element regression now uses
  `erlang:append_element/2` without removing its behavioral assertion.

## Local node BIFs (2026-10-22)

- `node/1` is now auto-imported and available as `erlang:node/1`, including
  guards, dynamic calls and fun references. Local PIDs and references return
  `nonode@nohost`; other supported term types raise `badarg` in bodies and
  fail guards, including negated guards.
- `node/0` now has a runtime implementation returning the same local node,
  including in `node(Pid) =:= node()` guards. Ranch's `node(SupPid)` call
  in its `nodedown` exit can now compile. Ports and distributed identifiers
  remain unsupported.
- A shared native/BEAM regression covers live/dead PIDs, ordinary, monitor and
  atomics references, invalid arguments, direct/dynamic/fun calls and guards.
  
## Deferred tail byte sizes (2026-10-22)

- `byte_size/1` now rounds a deferred binary-pattern segment's bit count up to
  whole bytes, as on BEAM, instead of truncating a partial final byte.
  For example, `f(<<_:8,R/bits>>) -> byte_size(R)` returns `1` for `<<1,1:2>>`.
- The shared expression/guard shortcut covers `/bits`, `/bitstring` and
  `/binary-unit:N` tails, plus sized bitstring segments pre-bound by case,
  receive and try patterns. Empty and whole-byte segments keep their sizes;
  `bit_size/1` and rest-segment unit matching are unchanged.
- A shared native/BEAM regression covers byte boundaries, non-byte units,
  guards, pattern contexts, shifted/recursive tails and materialized values.

## Binary comprehension generators (2026-10-21)

- `<=` binary generators now consume a complete pattern per iteration in
  both list and binary comprehensions. Literal sizes, sizes bound earlier
  inside the comprehension, units, multiple segments, partial-byte sources
  and UTF encodings no longer become byte-list iteration.
- Literal and repeated-variable mismatches skip the consumed prefix. An
  incomplete final pattern ends iteration; non-bitstring sources raise
  `{bad_generator, Source}`, as on BEAM.
- Iteration advances an exclusive match cursor instead of copying unaligned
  tails, and binds innermost values without extracting each prefix twice.
  UTF readers account for a reused cursor's intra-byte offset in both pattern
  compilation and body extraction.
- Nonfinite float fields skip a complete element instead of ending iteration.
  Unsized binary/bitstring fields in `<=` patterns now fail at compile time,
  and generator patterns receive the existing segment-specifier checks. The
  lint reports each unsized field once, as OTP does, and walks every
  expression and pattern position, including callee expressions, guards,
  pattern sizes, map keys, `++` patterns and record defaults; module
  lowering refuses both unsized-field errors before any function is
  lowered, so embedded modules and error-discarding paths cannot bypass
  them.
- Float segment sizes are validated after scaling by the unit, so
  `<<X:1/float-unit:64>>` is accepted everywhere, as on BEAM.
- A shared native/BEAM regression covers sized patterns, nested mixed
  generators, filters, shadowing, mismatches and incomplete tails.

## Inputs to fun and comprehension patterns (2026-10-21)

- Map keys and binary sizes in fun heads and comprehension generator patterns
  now capture outer variables even when those variables appear only in the
  pattern. Later generators can read variables from earlier generators.
- Key reads keep the input binding when a pattern value shadows the same name,
  as in `K = k, [V || #{a := K, K := V} <- Maps]`. Binary sizes likewise read
  the input scope, except that earlier segments of the same binary are visible.
- Fun guards now read captured variables before selecting a clause, so
  `fun(X) when X > Limit -> ... end` can match using the enclosing `Limit`.
- List and binary comprehensions share capture analysis, and captured deferred
  binary tails are materialized before entering the helper function.
- A native/BEAM regression covers outer inputs, earlier generators, shadowing,
  multiple and nested funs, binary comprehensions, mismatch skipping and tails.

## Map-key BIF (2026-10-21)

- `is_map_key/2` is now auto-imported and available as `erlang:is_map_key/2`,
  including guards, negated guards, function bodies and dynamic calls.
- Both forms share `maps:is_key/2`'s exact key lookup. Non-map arguments now
  raise `{badmap, Map}` instead of returning `false`; in guards the exception
  makes the guard fail, including under `not`.
- A shared native/BEAM regression covers present/missing keys, exact key
  equality, invalid maps, guard recovery and the `cow_ws` negotiation guard.
  
## Binary rest segment units (2026-10-21)

- An unsized `/binary` pattern segment now matches only when its remaining
  bit count is divisible by eight, as in BEAM. Partial-byte tails reach the
  next clause, or raise `badmatch` in a match expression, instead of binding
  a bitstring. This applies even when the tail is unused or already bound.
- The check uses the remainder, not the whole input or its physical alignment:
  `<<_:3, Rest/binary>>` can still capture a whole-byte tail. Explicit `unit:N`
  requires a multiple of N bits; `/bits` and `/bitstring` keep their default
  one-bit unit.
- Native/BEAM regression coverage includes case clauses, function heads,
  match expressions, shifted and recursive tails, custom units, guards,
  receive, maybe and list generators.
  
## Bound variables in try-of and catch patterns (2026-10-21)

- `try ... of` and `catch Class:Pattern` now compare already-bound pattern
  variables with exact equality (`=:=`), as in BEAM, instead of accepting
  mismatches or overwriting outer variables. This includes bare names,
  compound patterns, binary segments and deferred binary tails.
- Catch class and reason patterns share one matching scope, so a bound class
  name or a name repeated between class and reason also constrains the match.
  Fresh names still bind, guards still run, and mismatches try the next clause
  or re-raise the original exception when no catch clause matches.
- Catch clauses use bindings from before the protected body, not body-only
  registers that an exception can leave unset. Try-of clauses still see
  variables bound by the body.
- A native/BEAM regression covers all catch dispatch shapes, integer/float
  distinction, nested patterns and preservation of outer binary values.

## Unicode string expressions (2026-10-21)

- String expressions now produce lists of Unicode codepoints, as on BEAM,
  rather than UTF-8 bytes. For example, `"λ"` is `[955]`, and `"\351"`
  is `[233]`. ASCII and empty strings keep their existing values.
- This also fixes literal Unicode padding in format strings. Octal string
  escapes from `\400` through `\777` now retain their codepoints instead of
  disappearing during parsing. List-string patterns already use codepoints;
  binary-string encoding rules are unchanged.
- A native/BEAM regression covers literal and escaped characters, adjacent
  strings, supplementary and combining characters, list operations, matching
  and formatted output.

## Atom conversion encodings (2026-10-21)

- `binary_to_existing_atom/2` and `atom_to_binary/2` now accept only `latin1`,
  `utf8` or `unicode`; other encodings raise `badarg` instead of being ignored.
- Latin-1 input maps each byte to one character, and Latin-1 output uses one
  byte per character, raising `badarg` for characters above 255. UTF-8 and
  `unicode` use UTF-8 bytes; the `/1` forms keep their UTF-8 defaults.
- Existing-atom conversion now rejects partial bitstrings instead of rounding
  them up. It shares `binary_to_atom`'s bounded decoder, but overlong names
  raise `badarg`, not `system_limit`, and missing names never create atoms.
- A native/BEAM regression covers all Latin-1 bytes, encoding validation,
  Unicode, sub-binaries, length boundaries and dynamic calls.

## Embedded runtime cache isolation (2026-10-21)

- Standalone compilers now cache the extracted runtime by the SHA-256 of
  their embedded compressed archive, rather than compiler version and build
  date. Compilers built on the same day with different runtimes no longer
  silently link whichever runtime was extracted first.
- Concurrent extractions use unique temporary files and publish only a
  complete archive with an atomic rename. If another extraction publishes
  first, reuse its archive even when replacing it fails (for example, while
  another Windows linker has it open). Installed and workspace runtime
  lookup still take priority; existing version/date cache files are ignored.

## Prompt native shutdown (2026-10-21)

- Native programs no longer start another reactor poll after receiving shutdown.
  When all processes have finished, the scheduler stops and wakes its workers
  without the idle termination-confirmation waits. Timer and file-worker shutdown
  notifications are synchronized with their wait locks to avoid missed wakeups.
- Existing timeout durations and liveness checks for live processes, messages,
  timers and I/O remain in place. A native regression checks full output, normal
  exit and median last-output-to-exit latency for five workloads with one worker
  and the default worker count; it does not promise a maximum exit latency.

## Bound variables in map pattern values (2026-10-20)

- A map pattern value that is an already-bound variable is now compared with
  `=:=`, as in BEAM. `#{a := X, b := X}` no longer matches
  `#{a => 1, b => 2}`. `f(X, #{k := X})`, `f(#{k := X}, X)` and
  `{X, #{k := X}}` fall through to the next clause when the values differ.
  `case`, `receive`, `maybe ... ?=` and `=` on `#{k := X}` with `X` bound
  before now fail instead of rebinding `X`.
- Previously the compiler copied the map value over the bound variable, so the
  match succeeded. Later code, including a function's remaining clauses and
  the code after a `case`, then read the map value instead of the original.
- In `=` and `maybe ... ?=` patterns, a variable bound before the pattern now
  keeps its binding while the pattern matches and after it fails. A later key
  or binary size in the same pattern reads it, as in
  `#{a := K, K := V} = M`, `{N, <<X:N/binary, _/binary>>} = T` and
  `{K, #{K := V}} = T`; such patterns previously failed to match or ignored
  the bound value. After a failed `=` inside `try`, the variable no longer
  reads as an undefined value in the handler or after the `try`.
- A bound binary tail kept as a deferred sub-binary, such as `K` in
  `f(<<_, K/binary>>, M)`, can now be compared as a map value and read as a
  map key, segment size or guard of the same match. This works in `case`,
  `receive`, `=`, `maybe ... ?=` and `maybe ... else`. Reading it there no
  longer uses up its definition, so other clauses, the other branch of a
  `maybe` or `receive`, and the code after the match still read the tail.
  These patterns previously crashed the compiler, raised `badkey` or
  ignored the bound value.
- Reading a bound binary tail in one branch no longer changes it for the
  other branches or the code after them. This covers a `receive` after body,
  an `if` clause or guard, a `try` body, `of` or `catch` clause, `catch Expr`
  and the right side of `andalso`/`orelse`. A later comparison or read of the
  tail previously used a value defined only in that branch: it returned a
  wrong result or an undefined value, or crashed the compiler.

## Bound variables in binary patterns (2026-10-20)

- A binary segment whose value is an already-bound variable now constrains
  the match with exact equality (`=:=`), as in OTP. This covers integer,
  float, UTF and sized or rest binary segments, and names repeated in one
  binary or across a tuple. Previously the match ignored the bound value and
  overwrote the variable: `<<X:8>>` with `X = 2` matched `<<1>>`.
- Fresh variables after a bound segment now bind in `case`, `receive`,
  `maybe`, match expressions and function heads. Previously
  `<<Boundary:Size/binary, "--", Rest/bits>>` with `Boundary` bound
  failed with `undefined variable: 'Rest'`.
- Comprehension generators: a binary pattern may reuse a name an earlier
  generator bound; the new binding shadows it, as in OTP. A later
  generator's segment size may name an earlier binary generator's variable;
  previously such a generator matched nothing.
- The Cowboy dependency check now passes Cowlib's `cow_multipart` and stops
  at `cow_ws` on `is_map_key/2`; see
  [COMPATIBILITY.md](COMPATIBILITY.md#cowboy-analysis--2026-10-20).

## Serverless demo status (2026-10-20)

- `docs/SERVERLESS_ERLANG.md` now states that `demo/serverless/` is a local
  demo and that no AWS Lambda runtime is qualified. It withdraws the
  unreproduced 26 ms / 2.2 MB / 1.4 MB figures and the concurrency and
  "2–4x cheaper" claims, and lists AWS's requirements and the steps that
  would qualify a Lambda function. The fictional DynamoDB application and
  the cross-language comparison tables are removed.
- `demo/serverless/build.sh` builds and runs the demo locally by default.
  `--package` writes `lambda.zip` only for a 64-bit Linux ELF executable and
  reports the architecture read from it; it previously packaged a macOS
  executable and suggested deploying it as `x86_64`. The unimplemented
  `--docker` option is removed. For testing,
  `NERA_SERVERLESS_PREBUILT_HANDLER` packages an existing executable
  instead of building one; Linux execution remains unverified.
- `build.sh` and `bench_cold_start.sh` now work from the repository root.
  `build.sh` builds the compiler with `--locked`, so it no longer rewrites
  `Cargo.lock`. The benchmark reports local process time and peak RSS
  without Lambda cost estimates.

## Undefined record patterns (2026-10-20)

- A pattern naming a record with no definition, such as `f(#nope{}) -> ok.`,
  is now an `undefined record 'nope'` error at that pattern in `nera check`,
  `nera build` and the language server. It previously panicked the compiler
  with `Pattern::Record` (exit 101), or reported an unbound field variable.
  This covers function and fun heads, nested patterns, `=` matches, `case`,
  `receive`, `try`, `catch`, `maybe` and comprehension generators.
- Record expressions with no definition now report the same
  `undefined record 'nope'` message instead of
  `unsupported expression: undefined record 'nope'`.

## binary_to_atom/2 (2026-10-20)

- Add the auto-imported `erlang:binary_to_atom/2` for direct, qualified,
  `apply` and fun-reference calls. `latin1` maps each byte to one character;
  `utf8` and `unicode` require valid UTF-8. Other encodings, non-binaries and
  bitstrings raise `badarg`; names over 255 characters raise `system_limit`.
  Like OTP, input over 255 Latin-1 bytes or 1,020 UTF-8 bytes raises
  `system_limit` before it is decoded or validated, so large binaries are
  not copied.
- `binary_to_atom/1` now shares this conversion as `binary_to_atom(B, utf8)`.
  It previously accepted bitstrings by rounding their last byte up and
  allowed names over 255 characters.
- The Cowboy dependency check now passes `cowboy_router` and stops at a
  separate Cowlib binary-pattern error; see
  [COMPATIBILITY.md](COMPATIBILITY.md#cowboy-analysis--2026-10-20).

## Nested record patterns and Cowboy analysis (2026-10-20)

- Desugar record patterns inside map values and string-prefix tails before
  pattern compilation. Cowboy 2.20.0's HTTP/2 stream-map patterns no longer
  trigger the `Pattern::Record` compiler panic.
- Cover tagged-tuple matching, aliases, omitted fields, repeated bindings and
  mismatches in native/BEAM regressions, plus structural IR verification.
- Keep Cowboy execution and full OTP compatibility outside this analysis fix;
  see the pinned analysis reproduction in [COMPATIBILITY.md](COMPATIBILITY.md).

## Locked release builds in paths containing spaces (2026-10-20)

- Pin the TiKV allocator wrappers and sys crate together to the upstream
  jemalloc 5.4.0 update, which includes its whitespace-safe build fix.
  Fresh `cargo build --release --locked` builds no longer need a space-free
  external target directory. No allocator is disabled or vendored.
- Export the prefixed allocator configuration with its C pointer ABI and
  retain that symbol when linking native programs. The old unprefixed
  configuration was not read by the allocator.
- Default to `tcache_max:4096,narenas:2` on non-Windows targets, adding
  `retain:false` only on macOS. Linux keeps jemalloc's retain default to avoid
  persistent VM map holes on 64-bit systems. Do not export a competing strong
  configuration symbol on Windows. Leave dirty/muzzy decay at jemalloc's
  defaults (10000/0 ms), without enabling background threads.
  The measured macOS profile reduces retained/cache/arena overhead, but two
  automatic arenas can increase contention and it is not a universal speed win.
  Users can override the profile at startup with `_RJEM_MALLOC_CONF`.
- Add fresh spaced-path build and native effective-option regressions,
  including the user override and rejected-option startup check.
- Validate locally on macOS. Complete Linux x86_64 and aarch64-linux-gnu
  compiler/distribution builds and execution remain unverified; cross-building
  runtime libraries alone is not native validation. GitHub CI remains paused.

## I/O field width and precision (2026-10-19)

- Share directive parsing between `io:format/2`, `io:fwrite/2` and
  `io_lib:format/2`, consuming static and `*` width, precision and padding
  fields before their values so later arguments remain aligned.
- Apply term/integer field padding and overflow, string truncation and
  adjustment, and BEAM-style `~f`, `~e` and `~g` decimal precision/rounding.
  EUnit's `[done in ~.3f s]` timing line now renders the duration.
- Preserve integer radix and padding support, including supplied/radix-prefixed
  integer forms. Accept mixed text/list/map-order modifiers and consume their
  arguments; map-order modifiers do not change output order yet.
- Match equal width/precision string adjustment and per-binary UTF-8/Latin-1
  fallback. Avoid debug overflow when `~g` selects scientific notation for
  large magnitudes. Cover Unicode fields, character/newline repetition, dynamic
  calls and invalid arguments against OTP 29.0.4. Pretty-printer line wrapping
  and depth controls remain unsupported.

## Cancellable message timers and Cowlib HTTP/2 machine validation (2026-10-19)

- Extend the external EUnit runner to `cow_http2_machine`, preserving its
  mailbox helper and full production dependency closure. Accept EUnit's real
  single-test summary without weakening duplicate/completion checks.
- Replace the `cancel_timer/1` stub and add `cancel_timer/2` with boolean
  async/info options. Serialize cancellation with delivery, release cancelled
  pending work immediately, and preserve already delivered messages.
- Cover direct/dynamic calls, remote funs, cross-process references,
  cancellation/delivery ownership and callback-failure cleanup with regressions.
- Record the complete original machine suite passing 1/1 on BEAM and native
  Nera, with no upstream PropEr properties at the pin and no normalization.

## Cowlib HPACK native validation (2026-10-19)

- Extend the external native EUnit runner to `cow_hpack`, retaining its functional
  helpers and resolving both HPACK headers from the pinned external source.
- Add a native PropEr scenario for the original raw/Huffman string properties,
  with reproducible generation, shrinking, seeded replays and exact typed traces.
- Accept digit separators in numeric literals and preserve numeric source
  spelling in formatted expressions and patterns, including floats and bigints.
- Expose the auto-imported `map_get/2` alias, including guard, dynamic-call and
  remote-fun behavior.
- Implement `garbage_collect/0` through suspension and a scheduler-safe full
  collection before resumption, and add `binary:longest_common_prefix/1` with
  complete input validation.
- Accept default-policy `max_heap_size` maps, return the previous normalized
  flags and verify both integer/map limits kill an allocating worker. Reject
  unsupported nondefault policies explicitly rather than ignoring them.
- Keep unrelated APIs and subsequent Cowlib modules outside this scope.

## Binary list domain and ETF allocation bounds (2026-10-17)

- Reject partial bitstrings in `binary_to_list/1` before converting padded
  bytes; preserve `bitstring_to_list/1` and byte-aligned heap/refc conversions.
- Reject impossible ETF list, tuple and map counts before reserving memory,
  using remaining input bytes as a conservative lower bound. Preserve empty
  containers, one-byte encoded terms, improper list tails and trailing data.

## Checked Unicode lists and direct binary window materialization (2026-10-17)

- Validate the full integer before Unicode scalar conversion, rejecting large
  values that previously aliased characters after truncation. Shared string and
  numeric-list conversions use this strict input domain; malformed numeric lists
  can differ from OTP 29's truncating behavior.
- Materialize shifted/partial windows directly into their final heap storage or
  owned refcounted buffer, preserving exact bits and zero padding. Retain the
  aligned sharing path and validate logical ranges before allocation.

## Binary closure captures and explicit BIF safety contracts (2026-10-17)

- Materialize deferred binary/bitstring windows once before capturing them in
  closures, rebind later uses, and isolate matching metadata between nested
  functions. Preserve ordinary cursor reuse outside escaping captures.
- Mark graph-reading Rust BIFs and raw-term formatting/conversion helpers unsafe,
  with explicit input layout, owner lifetime, immutability and GC/reentry
  requirements. Keep immediate-only operations safe; the C ABI is unchanged.

## Active TCP registration liveness (2026-10-16)

- Keep reactor registrations visible to scheduler termination detection from
  command enqueue through processing and publication of live poll registration.
  Release ownership on rejected/discarded commands and disconnected channels.
- Add three deterministic regressions for queued/in-flight active registration,
  successful handoff, re-registration, failure cleanup, cloned handles and
  shutdown with undrained commands. Preserve the existing wake coalescing path.

## Validated matching windows and allocation-free shifted copies (2026-10-16)

- Encapsulate matching bounds and their traced binary in a `repr(C)` descriptor;
  derive runtime and generated-code offsets from this layout. Check materialized
  ranges against the matching window before allocation and preserve exact bits
  when a window crosses heaps or processes, including fast refcounted sends.
- Transfer a mutable matching cursor only to an exclusive tail-call argument;
  materialize shared windows before forwarding, including nested argument uses.
- Copy shifted binary segments directly a destination byte at a time, preserving
  neighboring bits and overlap semantics without a temporary allocation. Keep
  the aligned bulk-copy path.

## Checked integer/binary segments and full integer decoding (2026-10-15)

- Reject invalid integer and binary segment values with `badarg`, including
  zero-width segments, instead of silently writing or retaining zero bytes.
  Check exact logical ranges and propagate caught and uncaught construction errors.
- Decode integer segments wider than 64 bits with bigint promotion, correct
  partial little/native-endian groups, and support zero-width signed integers.
- Validate dynamic pattern sizes and unit scaling before raw access, fail invalid
  sizes without raising, and check remaining bits after dynamic segments.
- Retain direct aligned 8/16/32/64-bit loads and inline valid size/source checks.

## Cowlib HTTP/1 and transfer-encoding upstream validation (2026-10-15)

- Extend the external EUnit runner to `cow_http1` and `cow_http_te`, preserving
  original recursive fragmentation helpers and the production dependency closure.
- Record 39/39 and 10/10 upstream assertions passing on BEAM and native Nera
  from merged main, with reproducible inventories and source/binary fingerprints.
- Fix premature scheduler shutdown while `send_after/3` / `start_timer/3`
  callbacks are pending. Register their work until message delivery and wake
  dispatch; keep receive-timer cancellation independent. Add minimal native/
  BEAM regressions and a Rust callback-lifetime test. EUnit timeout, failure,
  isolation and completion controls now pass; clarify the existing
  `cancel_timer/1` stub limitation in the feature table.
- Preserve timed-receive liveness while registering or dispatching an expired
  receive timer, using the published process wait reason until its wake is
  published. Add deterministic phase regressions, single-wake accounting and
  untimed-deadlock controls without timing-dependent sleeps.


## Checked floating segments and bit-offset access (2026-10-15)

- Reject nonnumeric float values and invalid segment widths instead of silently
  writing zero bytes. Validate dynamic widths before allocation and propagate
  construction errors through lexical handlers and uncaught process exits.
- Read and write IEEE 16/32/64-bit floating segments at arbitrary bit offsets
  through bounded borrowed windows, preserving neighboring bits and the aligned
  byte path. Validate logical lengths before any access.
- Support binary16 with direct f64 rounding, signed zero and subnormals; reject
  nonfinite encodings during matching, including wildcard float segments.
- Keep valid constant width checks at compile time and valid dynamic width/unit
  checks inline; malformed reads fail without allocating a replacement float.

## Unicode scalar validation and bounded bit access (2026-10-15)

- Validate the complete integer as a Unicode scalar before narrowing; reject
  oversized integers rather than encoding their truncated low 32 bits.
- Read and write UTF-8/16/32 at arbitrary bit offsets using borrowed, bounded
  windows. Preserve neighboring bits and reject truncated input or insufficient
  logical destination space before access, retaining direct aligned byte copies.
- Route UTF construction errors through lexical try/catch handlers.
- Cover scalar boundaries, offsets 0..7, big/little endian encodings, malformed
  sequences, refc-backed tails and release-mode range checks.

## Cowlib HTTP header properties with native PropEr (2026-10-15)

- Validate all 26 original header properties with native PropEr against BEAM:
  600 passing trials each per engine, three seeds repeated cumulatively, and
  identical input/verdict/shrink traces. Preserve original generators,
  constraints and predicates; no third-party code is vendored.
- Add `math:sqrt/1`, the missing primitive used by PropEr's nonnegative-integer
  generator, with BEAM-compatible type/domain errors, integer-to-float handling,
  signed zero, dynamic-call and remote-fun regression coverage.
- Verify 156 false quickchecks and 312 retained-counterexample replays per
  engine. Bound shrinking to 100 steps without claiming global minimality.
  Add reproducible integration evidence and strict harness tests.

## Checked binary segment sizes and list operator contracts (2026-10-15)

- Validate binary segment sizes before unit scaling, summing or allocation;
  reject noninteger/negative sizes with badarg and unrepresentable capacities
  with system_limit, including whole-byte descriptor rounding.
- Reject invalid segment units before narrowing their representation. Keep
  valid constant-size checks at compile time and dynamic small-size checks inline.
- Validate list spines for ++ and --, preserving arbitrary right tails for ++.
- Use exact structural equality for -- while retaining the tight immediate-value
  scan. Avoid allocating a comparison work buffer for flat immediate tuples.
- Prefilter short flat tuple subtraction candidates with cached fingerprints;
  confirm every hash match with exact equality and retain the immediate path.
- Propagate construction/operator failures before later expressions, including
  lexical try/catch, and add native/domain regression coverage.
  
## Cowlib date with native PropEr (2026-10-15)

- Validate the original date property and its three-format generator through
  native PropEr: 6,000 passing trials per engine, three seeds repeated twice,
  matching complete input/parsed-date traces and an independent format oracle.
- Verify six deliberately false wrappers reduce to the minimum date components
  and 12 seeded replays distinguish them from the original passing property.
- Share external Cowlib module preparation with the Base64url runner, add date
  harness regressions and record reproducible scope/coverage evidence. Revalidate
  native Base64url properties after sharing preparation. No third-party source
  is vendored and no compiler/runtime/stdlib change is required.

## Cowlib Base64url with native PropEr (2026-10-15)

- Validate both original upstream Base64url properties through real native
  PropEr from main, with independent generation on BEAM/Nera, three seeds
  repeated cumulatively, and 6,000 passing trials per property per engine.
- Compare full input/encoding traces and an independent Base64url oracle;
  verify 12 false controls shrink to the empty binary and 24 seeded replays
  distinguish the false wrappers from the original passing properties.
- Extend the shared integration runner with external scenarios, preserve the
  original property generators/predicates and RFC 7515 assertion, add harness
  regressions and record reproducible evidence. No third-party code is vendored
  and no compiler/runtime/stdlib changes are required.

## Native PropEr generation and shrinking (2026-10-15)

- Exercise real PropEr generators, shrinking and counterexample replay in Nera,
  with three reproducible seeds repeated cumulatively and full traces compared
  against BEAM. Cover bounded integers, lists and binaries, failing properties,
  known reduced examples and adjacent passing cases.
- Provide the original pinned OTP `ordsets` module as an external dependency;
  preserve the explicit generic BEAM preparation step. No third-party source
  is vendored and the native executable runs without BEAM.
- Add `lists:sublist/2` to Nera's Erlang stdlib with direct/dynamic/remote-fun,
  prefix-boundary, improper-list, invalid-argument and long-list regressions
  compared with BEAM. Add strict integration protocol/oracle tests and evidence.
- Keep native Cowlib properties, full PropEr coverage and newer OTP `rand`
  zipped generators outside this milestone.


## Checked receive timeouts and helper safety contracts (2026-10-15)

- Distinguish infinity from finite receive deadlines and validate the Erlang
  timeout domain before mailbox scanning, raising timeout_value for invalid data.
- Propagate timeout failures through generated code and try/catch without
  consuming queued messages or entering receive-wait on an exception.
- Clear a receive deadline when its message is accepted, including generated
  mailbox fast paths. Keep the existing zero-sentinel context representation.
- Declare remaining TCP option/formatting/trace graph readers unsafe with
  explicit live-graph and GC contracts, without changing their runtime behavior.
- Add timeout regressions and preserve receive/polling and TCP timing evidence.

## Validated I/O data and checked process options (2026-10-15)

- Share checked iodata conversion between file writes and TCP sends. Reject
  out-of-range bytes and malformed lists before submitting any output, and
  support nested lists and binary tails consistently.
- Preserve the binary copy path and the direct loop for flat byte lists.
- Parse spawn options into Result, reject improper lists, and validate numeric
  conversions before process creation. Check the u32 fullsweep_after domain in
  both spawn options and process_flag instead of silently truncating values.
- Add regressions and preserve native I/O/spawn performance measurements.

## Typed socket modes and encapsulated atomic arrays (2026-10-15)

- Replace numeric socket active/packet modes with byte-sized enums throughout
  parsing, socket options, worker reads and reactor dispatch.
- Make atomics/counters storage private, remove their redundant size fields,
  and centralize one-based cell access against the actual storage length.
- Derive metadata from storage and preserve direct atomic access and ordering,
  without additional locks or resource lookups.
- Add array-boundary and socket-mode checks and preserve native TCP and
  atomics/counters performance measurements, including contention.

## Checked TCP arguments and bounded initial receive allocation (2026-10-14)

- Reject non-integer and out-of-range TCP ports before network operations.
- Validate nonnegative receive lengths and share exact-read semantics between
  recv/2 and recv/3; recv/2 previously ignored its requested length.
- Bound the initial exact-read reservation to 4 KiB on reactor platforms and
  accumulate blocking reads progressively, avoiding allocation proportional to
  an enormous requested length when the peer closes without sending it.
- Add argument/domain and native TCP regressions and compare native receives
  from one byte to one MiB with alternating before/after measurements.

## Typed, bounded socket handles (2026-10-14)

- Introduce a transparent SocketHandle with a checked positive small-integer
  domain and propagate it through registry, reactor, I/O and TCP messages.
- Bound generations to 28 bits, permanently retire exhausted slots without
  retaining their socket owner, and reject index exhaustion before insertion.
- Preserve the two-state, 16-byte measured slot representation and direct
  lookup. Validate raw terms at the FFI boundary and encode typed handles directly.
- Add generation/domain, owner-release, free-list and native TCP regressions.
  Preserve paired registry/TCP measurements and a shared-owner contention control;
  document the possible listen/close cost and noisy separate-process results.

## Typed atomic resources and complete integer domains (2026-10-14)

- Distinguish ordinary references, atomics and counters before interpreting any
  native resource pointer. Keep two-word references and direct access without
  registry lookups or reference-count updates on the hot path.
- Preserve resource metadata through GC, message and ETS copies, and use stable
  reference identity for ETS keys. Cache atomics signedness in the descriptor.
- Accept the full signed/unsigned 64-bit storage domains and OTP arithmetic
  delta domains, wrap arithmetic modulo 64 bits, and promote results beyond
  small integers. Reject invalid values and options before mutation.
- Add wrong-resource, GC/copy, integer-boundary and no-mutation regressions,
  with paired native 1/2/4/8-thread performance measurements.

## Process identifier block reservations (2026-10-14)

- Reserve disjoint PID ranges of up to 64 per calling thread for multi-worker
  schedulers, reducing contention on the checked global allocator. Preserve
  the direct checked counter for single-worker schedulers.
- Bind each local reserve to a Weak allocator identity without retaining its
  owner or allowing address reuse. Keep final partial ranges within the PID
  domain and never recycle abandoned identifiers or wrap the global counter.
- Add concurrent refill, scheduler identity, owner-lifetime, partial-range and
  direct-path exhaustion regressions. Retain paired 1/2/4/8-thread measurements
  and a compiled Erlang benchmark executing 40,004 spawned processes.

## Project-configured EUnit runner (2026-10-14)

- Add `[test] framework = "eunit"`, modules and source directories to `nera test`.
  Reuse project build configuration and external dependencies, isolate generated
  build artifacts and require framework completion with successful exit.
- Preserve semantic compilation errors under `--no-warnings`, preventing
  undefined local test calls from being accepted as successful tests.
- Cover module selection, successful/failed assertions, exceptions, runtime
  timeout, compilation/configuration errors and unchanged legacy execution.
- Validate Cowlib HTTP functional suites through the public project runner,
  with original BEAM EUnit as reference and all third-party source external.

## Native OTP EUnit dependency and Cowlib validation (2026-10-14)

- Add explicit `project.defer_undefined` runtime-undef mode, retaining strict
  defaults, local-function diagnostics and export checks.
- Compile unmodified external EUnit/gb_trees sources; recognize default EUnit
  autoexport, generate module_info exports/module metadata, and accept ignored
  OTP documentation literals, parenthesized types and literal space characters.
- Add scoped runtime metadata, leaders, normal/low priority, wall-clock timing,
  Unicode/text I/O, and project-owned dictionary/set/proplist/list APIs.
- Stop protected try bodies after failed calls; retain exact numeric types in
  nested equality and rebuilt structural map keys, fixing EUnit group collation.
- Execute original Cowlib HTTP suites on both BEAM and native OTP EUnit:
  cow_http 102/102, cow_http_hd 396/396, with framework failure controls.
  Sources and adapted copies stay external; native PropEr remains out of scope.

## Typed execution status and bounded process identifiers (2026-10-14)

- Replace raw ProcessContext status codes with a repr(u32) ExecutionStatus enum,
  preserving the generated-code word layout and all seven discriminants.
- Introduce a transparent ProcessId with a checked 60-bit payload domain,
  reserving the external-term marker. Encode validated IDs without repeated
  checks and propagate the type through scheduler, timers, sockets and ETS owners.
- Centralize unique PID allocation in a bounded atomic counter. Exhaustion
  leaves the counter saturated; spawn paths report system_limit before publishing
  a new process. Keep owner/initial-PID atomic words behind typed accessors.
- Preserve high PID bits in ETF's existing ID/serial words and reject reserved
  bits on decode. Add domain, ABI, concurrency, exception and roundtrip tests,
  with reproducible measurements of allocation, encoding and process creation.

## Macro arguments containing keyword blocks (2026-10-14)

- Keep commas inside begin/end and nested keyword blocks within one macro
  argument; distinguish anonymous/named functions from fun references.
- Add a minimal PAIR/FORALL regression, lexical block/reference controls and
  direct BEAM/native execution checks.
- Replay all 26 cow_http_hd properties with original FORALL calls through
  Nera's preprocessor: 2,600/2,600 shared instances pass, plus 498 upstream
  assertions. Retain the historical closure envelope as an explicit option.
- Update capability/compatibility evidence without vendoring Cowlib or adding
  native EUnit/PropEr frameworks.

## Cowlib HTTP upstream validation (2026-10-14)

- Validate merged main with all 498 original HTTP/header assertion bodies
  (102 cow_http, 396 cow_http_hd) on BEAM EUnit and external Nera adapters.
- Replay all 26 original header property predicates on 100 reproducible shared
  inputs each: 2,600/2,600 pass on BEAM/PropEr and isolated Nera executions.
- Add a pinned external runner, seven adapter regressions and consolidated
  evidence. No Cowlib source or upstream test bodies are vendored.
- Record the unavailable native EUnit/PropEr frameworks and the macro-argument
  limit for commas inside begin/end blocks; no compiler change in this milestone.

## Exact ETS numeric ordering and unsafe compiled calls (2026-10-14)

- Make ordered ETS key equality agree with its numeric/structural order.
  Compare mixed integer/float values exactly without rounding the integer;
  preserve signed-zero equality and deterministic ordering for raw NaN inputs.
- Avoid BigInt allocation on small-integer comparison paths, and compare
  disjoint large magnitudes without allocating a converted float integer.
- Require unsafe calls through ErlFn, documenting context, argument, GC root,
  reentrancy and code-lifetime obligations at the compiled execution boundary.
- Add precision, ordering-law, ordered_set traversal and compile-fail regressions,
  and retain reproducible before/after comparison and ETS performance evidence.

## Small tuple send performance (2026-10-14)

- Keep complex tuple copying out of the inlined send path, recovering the small
  two-element tuple slowdown observed after the isolation fix on Apple M1.
- Preserve deep-copy ownership, self-send behavior and the standard GC.

## List-generator pattern filtering (2026-10-14)

- Validate comprehension generator patterns before destructuring or evaluating
  filters/body. Nonmatching elements are skipped with the accumulator unchanged.
- Reuse the pattern compiler, preserving generator shadowing and nested-helper
  traversal. Add a minimal binary-literal selector regression, controls for
  tuple shape, repeated variables, maps/binaries/nested generators, and IR
  structural verification.
- Fix the three Cowlib HTTP/2 Content-Length differences: unrelated headers no
  longer enter the content-length comprehension. All 136 cow_http corpus cases
  conform to BEAM (135 exact, one existing pseudo-header semantic comparison).
  No upstream code or tests are copied into the repository.

## Pseudo-header validation contract (2026-10-14)

- Correct the Cowlib `http_merge_request` comparator: map enumeration does not
  guarantee relative pseudo-header order. Compare only the valid leading
  pseudo-header prefix without ordering; retain exact bytes and regular order.
- Add a permutation regression and negative controls for malformed fields,
  duplicates, missing/changed values and misplaced pseudo-header fields.
- Relaunch all 136 cow_http cases: 133 conform semantically, including 132
  exact outcomes; retain raw order differences and the three genuine failures.
  No Nera or Cowlib implementation changes and no upstream source vendored.

## Empty-field trimming in binary split (2026-10-14)

- Honor `trim_all` in `binary:split/3`: remove empty parts after splitting,
  with or without `global`, independently of option order. Existing calls
  without `trim_all` continue to preserve empty parts.
- Add a minimal native regression verified against BEAM, covering consecutive
  separators, empty input and first-match/global splitting.
- Fix Cowlib's `http_request_spaces` case without changing upstream source or
  normalizing differential results.

## Binary tail call boundaries (2026-10-14)

- Materialize deferred binary tails before calling functions with variable
  fallback clauses, BIFs or opaque targets. Forward internal match cursors
  only to local argument positions matched by binary patterns in every clause.
- Preserve binary tails in returned values and badmatch reasons; add a native
  serialization regression and an IR check retaining safe binary-loop reuse.
- Materialize non-byte-aligned tails with their exact bit offset and length;
  keep aligned refcounted windows on the existing zero-copy path. Add a runtime
  regression spanning heap/refcounted sources and partial final bytes.
- Cowlib `cow_http_hd` now matches BEAM on all 63 corpus cases, including four
  malformed Content-Length reasons and three previously failing Range cases.
  No Cowlib source or upstream tests are copied into the repository.

## URI operations and lexical exception protection (2026-10-14)

- Add project-owned `uri_string:parse/1`, `normalize/1`, `recompose/1` and
  `resolve/3`, with binary/list components, URI maps, IPv6, percent quoting,
  dot-segment removal and relative-reference resolution.
- Add 172 reproducible differential URI cases against BEAM, with a generator
  and project-owned inputs; no OTP/Cowlib implementation or tests vendored.
- Record lexical try protection in IR for calls and potentially raising
  operations. Later guards no longer swallow exceptions from earlier calls,
  and intermediate operations in a try defer exceptions to its handler.
- Add a minimal exception-before-guard regression and IR scope tests.
- Reject missing keys during map pattern matching instead of raising badkey;
  add case/function/nested-pattern and badmatch regressions.
- Execute all 199 Cowlib HTTP cases: native headers 56/63, HTTP 131/136;
  BEAM 199/199, core 122/122 and nine direct cow_link URI assertions pass.
  Record the twelve remaining execution mismatches.
- Build upstream BEAM modules with an explicit all target and verify outputs
  before differential execution; add two harness prerequisite regressions.


## Fused tuple message isolation (2026-10-14)

- Deep-copy complex children when a fused tuple send writes directly to another
  process's heap, so sender exit or garbage collection cannot invalidate them.
- Preserve shared children, receiver-owned binary descriptors, self-send behavior
  and the send expression's return value. The GC and fragment path are unchanged.
- Add a regression for ownership after sender destruction and receiver collection,
  plus a BEAM-compatible Erlang test for queued messages after sender exit.

## Key-based list extraction (2026-10-13)

- Implement `lists:keytake/3` in the project-owned Erlang stdlib, with first
  match removal, numeric equality and stable order of remaining elements.
- Skip non-tuples and short tuples using a guard; reject invalid positions
  with `function_clause` and preserve improper tails after a successful match.
- Add BEAM/native regressions including dynamic calls, function references
  and a long prefix with a tail-recursive traversal.
- Rerun cow_http_hd then cow_http: all 199 BEAM expectations pass; native
  builds advance to four missing uri_string functions. Core remains 122/122.

## Typed boxed object subtypes (2026-10-13)

- Require an explicit BoxedSubtype enum when constructing runtime and compiler
  headers. Fallible raw-tag conversion rejects unknown values without masking.
- Preserve all existing tag values and object layouts. Keep raw header reads
  unchanged and resize headers without decoding or changing their subtype.
- Cover the tag ABI, invalid tags, arity limits, binary resizing and constructor
  compilation contracts; document focused release performance comparisons.

## Base64 default arities (2026-10-13)

- Add `base64:encode/1` and `base64:decode/1` using the standard alphabet and
  default padding through the existing Rust-backed arity-two implementation.
- Add BEAM/native regression coverage for RFC vectors, flat byte lists,
  whitespace, malformed inputs, dynamic calls and function references.
- Rerun cow_http_hd then cow_http: Base64 calls now resolve, but all 199 native
  cases remain blocked by lists:keytake/3 and four uri_string functions.
  BEAM passes all 199 expectations and the core remains 122/122.

## Process state encapsulation and term-pointer alignment (2026-10-13)

- Make lifecycle status and wait cause private, with read-only accessors. Public
  process guards no longer expose mutable dereferencing or whole-process
  replacement; full mutable guards are confined to runtime execution scopes.
- Replace Rust callback status writes with finish(), preserving worker publication,
  I/O wake CAS ownership, lock scopes and the generated-code context ABI.
- Check cons/boxed pointer alignment in public constructors in release as well
  as debug, and offer fallible try_cons/try_boxed encoders. Encoding an aligned
  address does not establish live storage or ownership.
- Keep allocation, GC and copying paths on internal unsafe aligned encoders,
  retaining their existing release encoding cost. Add compilation/state/alignment
  regressions and record targeted before/after performance measurements.

## Avoid redundant process garbage collections (2026-10-13)

- Give large live promotions headroom before the next major GC, and avoid
  repeated idle full sweeps of an unchanged live graph. Changed pointer roots,
  new promotions and allocation/heap-limit pressure still allow reclamation.
- Select at most one collection at each receive-wait boundary. Preserve
  explicit full collection and hibernation behavior.
- Add heap, process and single-scheduler regressions plus reproducible
  before/after latency measurements. Collection itself remains synchronous.

## Incremental Rust Base64 list decoding (2026-10-13)

- Route list inputs of `base64:decode/2` to Rust and remove the production
  Erlang decoding algorithm. Inspect at most 8,192 cells per invocation and
  charge one reduction per eight cells, including whitespace and invalid input.
- Root only the remaining list across suspension and GC; retain partial
  quartets and adopt the completed output without a full-size final copy.
- Match BEAM's list validation and error order, correcting former differences
  for malformed lists, floating-point elements and out-of-range integers.
  Preserve the frozen Erlang benchmark reference separately.
- Add direct BEAM comparisons, GC/resume and single-scheduler progress tests.
  GC and allocation remain synchronous; this change is not a latency bound
  and does not alter GC policy or collection mechanics.


## Process-entry guards and EXIT memory contract (2026-10-13)

- Tie process locks to their owning entry through ProcessGuard. Publish
  transitions and drain the inbox through that guard, preserving mutex scopes,
  wake CAS operations and nonblocking reentrant try_lock calls.
- Restrict raw EXIT traversal to the runtime with an explicit unsafe contract
  for current, initialized, owned terms and the lifetime of returned reasons.
  Preserve mailbox traversal, generated-code ABI and release checks.
- Add compilation regressions, guard ownership and collected EXIT-reason tests;
  record targeted performance measurements in the baseline report.

## Incremental Rust Base64 decoding (2026-10-13)

- Decode binary inputs natively in bounded, budgeted steps, including whitespace
  and malformed input. Preserve partial quartets and padding across suspension.
- Materialize native errors after releasing the input borrow; keep exception
  reasons intact on resume. No malformed-binary fallback or second full scan.
- Compare binary values and error class/reason with BEAM, retain the frozen
  Nera corpus for existing list semantics, and test one-scheduler fairness for
  large data, whitespace-only inputs and late errors. List decoding is unchanged.

## Cooperative native binary work (2026-10-13)

- Share native BIF budgeting, suspension, rooted input resumption and output
  ownership across byte-oriented operations; migrate Base64 encoding to it.
- Bound each step by the remaining reductions as well as the operation's maximum
  chunk, including exhausted budgets and boundaries inside an encoding group.
- Add a reproducible one-scheduler ping/pong latency benchmark for idle, repeated
  near-threshold encodings and large binaries. This is not a hard latency bound.

## Scheduler fairness and Base64 reductions (2026-10-13)

- Charge native Base64 encoding one reduction per 32 input bytes, with a minimum
  of one per call, including calls below the 48 KiB suspension threshold.
- Poll the global run queue after at most 16 dispatches even when local work
  remains, and drain at most 64 incoming entries before attempting execution.
  A yielding local process can no longer indefinitely exclude global work.
- Add single-worker progress regressions for repeated small encodings, large
  resumable encodings and global queue arrivals, plus a bounded-inbox test.
- Add `NERA_SCHEDULERS=1..255` for reproducible runtime scheduling and verify
  compiled Erlang progress against BEAM with one scheduler.

## Production Rust Base64 encoding (2026-10-13)

- Route `base64:encode/2` through a native Rust encoder while retaining Erlang
  option and flat-byte-list validation. Decoding remains unchanged.
- Process large inputs in 48 KiB blocks, yielding between blocks; root the input across GC and
  transfer the completed buffer into the result without a full-size copy.
  Check output-size overflow and report reservation failure as `system_limit`.
- Add BEAM comparisons, large/subbinary and dynamic-call regressions, and runtime
  tests for suspension, GC, cancellation cleanup and output-buffer adoption.
- Restore the prototype from PR #61 onto main's integration path and freeze its
  Erlang baseline so historical comparisons remain reproducible.

## Recursive bit matching (2026-10-13)

- Preserve the intra-byte offset when extracting integers from a reused binary
  match context. Recursive one-bit traversal previously reread bits from the
  beginning of the current byte. Keep logical sizes and context advances
  unchanged, with no binary copying or additional runtime allocation.
- Add a BEAM/native regression covering all eight starting offsets, byte
  boundaries, signed and little-endian extraction, terminal and recursive
  readers, and a large refcounted binary. Reproduction and comparison evidence:
  `docs/baselines/2026-10-13-bit-offset/`.

## Binary match-context allocation (2026-10-13)

- Preserve the current inline heap region when a binary match reuses an existing
  context or rejects a non-binary input. Previously every such slow-path call
  reserved another region (up to 4 KiB), inflating allocation and GC work in
  recursive binary consumers, including the Erlang Base64 encoder.
- Keep refilling after a fresh match-context allocation on the exhausted-region
  path. Add allocation-invariant tests and a compiled Erlang regression that
  interleaves binary iteration, tuple/list allocation, GC pressure and messaging.
- Record before/after allocation attribution and native Erlang benchmarks in
  `docs/baselines/2026-10-13-match-context-allocation/`.

## Hibernate retained roots and GC validity contracts (2026-10-13)

- Preserve queued messages and process dictionary keys/values during hibernation,
  including shared graphs, reference-counted binaries and borrowed ETS storage.
  Retain the direct heap reset for empty or immediate-only retained state.
- Resume hibernating processes when a message is already queued, and protect
  pending hibernate arguments before a moving collection can invalidate them.
- Require explicit unsafe contracts for raw-root collection and runtime execution
  APIs; document ownership, current pointers and exclusive access. Generated-code
  ABI remains unchanged.

## Mailbox tombstone and map pair contracts (2026-10-13)

- Reject tombstone sends and double removals before mailbox mutation in release
  as well as debug. Restrict GC root scattering to the runtime with an explicit
  unsafe validity contract, preserving its release cost and the mailbox layout.
- Require MapPairs for map allocation: typed entry arrays establish pairing
  directly, while flat buffers form a checked borrowed view without copying or
  allocation. Preserve the map payload layout and generated-code ABI.
- Migrate runtime consumers and build typed entries at the map-allocation FFI
  boundary. Add release rejection/state tests, compile-fail API checks and
  pointer/layout/GC retention tests for map views.

## Checked mailbox access and EXIT scan indices (2026-10-13)

- Check logical mailbox indices in release before narrowing or reading raw
  storage, preserving the slot-based API including interior tombstones.
- Scan stable physical mailbox slots when processing EXIT signals so head
  trimming cannot skip consecutive signals or read beyond initialized storage.
  Preserve FIFO handling, the generated receive ABI and allocation behavior.
- Add release regressions for invalid indices, empty/wrapped mailboxes,
  consecutive normal/kill signals, interior tombstones and trapping processes.

## Transferable fragment heap contracts (2026-10-13)

- Restrict heap adoption to internal allocation-only FragmentHeap storage, which
  cannot collect, promote objects or register borrowed external owners. Remove
  the public general-heap adoption API and its debug-only generation assumption.
- Use the restricted storage for message fragments, remote process-info snapshots,
  EXIT/DOWN signals and the legacy TCP reader, preserving storage transfer without
  another copy or Arc clone.
- Add adoption ownership tests, compile-fail API checks and a native snapshot
  regression that reads copied messages/dictionary graphs after source exit.

## Iolist byte size (2026-10-13)

- Implement auto-imported `erlang:iolist_size/1`, including dynamic calls and
  function references. Count binary metadata without copying payloads and
  traverse nested lists iteratively, with `badarg` for invalid iodata and
  checked size accumulation.
- Add BEAM/native regressions for binary tails, unaligned binary views, deeply
  nested lists and invalid terms; runtime tests cover shared payloads and the
  checked `system_limit` path.
- Rerun cow_http_hd then cow_http with unchanged external Cowlib sources:
  all 199 BEAM cases pass; Nera advances to missing Base64 arity-one functions,
  lists:keytake/3 and uri_string functions. Core corpus remains 122/122.

## Arbitrary-precision integer literals (2026-10-12)

- Accept decimal and base-N integer literals outside i64 without truncation,
  including negative values, patterns, guards, macros and inactive sections.
- Carry exact constants through AST/IR and materialize them from immutable
  object data using the bigint runtime; update formatting and source analysis.
- Correct boxed-integer binary construction, preserving truncation, byte order
  and partial-byte segments. Add native and lexer regressions against BEAM.
- Rerun cow_http_hd then cow_http: both advance to an undefined iolist_size/1
  dependency; the validated core corpus remains 122/122.

## Message fragment and binary size contracts (2026-10-12)

- Encapsulate heap fragment roots, inline tuple bounds, immediate terms and
  owned binary windows so safe message constructors cannot publish invalid ranges.
- Add borrowed term-to-fragment conversion without copying shared binary content.
- Reject oversized boxed-header arities and binary allocation sizes before heap
  mutation, retaining explicit bit-length encoding and padded word alignment.

## Mailbox growth capacity contract (2026-10-12)

- Reject overflowing mailbox capacity rounding and message-count increments
  in release builds before allocating a replacement ring buffer.
- Preserve queued messages, tombstones and active receive scans after a rejected
  growth request. Keep the checks on the resize path.

## Ordinary escaped character literals (2026-10-12)

- Accept ordinary escaped Erlang characters such as `$\#`, preserving named
  and octal escape values. Reserved incomplete hexadecimal/control escapes
  remain lexer errors.
- Add a minimal native regression and lexer tests for punctuation, letters,
  non-octal digits, Unicode, exact spans and incomplete escapes.

## Cowlib HTTP validation baseline (2026-10-12)

- Add opt-in differential corpora for `cow_http` (136 cases covering all public
  functions) and an initial `cow_http_hd` subset (63 cases, 12 public functions).
- Build complete external HTTP dependency closures and record exact blocked
  outcomes: BEAM satisfies all expectations; Nera rejects `$\#` in `cow_http`.
- Keep the validated 122-case core as the default CI workload, and record the
  successful Linux Base64url property gate from PR #48.

## Borrowed term copying and direct bigint storage (2026-10-12)

- Expose heap-bound term copying through `BorrowedTerm::copy_to_heap` and require
  explicit source validity guarantees for raw `deep_copy_term` calls.
- Retain hibernate arguments in owned fragments before discarding the source heap.
- Copy requested remote process-info term graphs under the source lock and
  transfer their storage to the caller without a second deep copy.
- Store bigint words directly from little-endian byte chunks, removing the
  intermediate limb vector while retaining zero padding and sign handling.

## Owned ETS word reservations (2026-10-12)

- Keep each ETS word pointer, reserved length and mapping owner together in a
  non-cloneable `EtsAllocation` with private fields.
- Consume allocations directly when constructing compact ETS and persistent-term
  heaps, removing the unsafe owner/pointer pairing contract without extra
  mappings, allocations or reference-count operations.

## Cowlib property CI and merged-main validation (2026-10-12)

- Require the existing upstream Base64url property replay in Linux CI, including
  cumulative execution, isolated inputs and four false-property controls.
- Pin rebar3 and PropEr preparation; preserve reports, inputs and diagnostic
  logs on failure, with all upstream code kept outside the repository.
- Consolidate the merged-main validation and distinguish the observed Linux
  corpus pass from the new property gate awaiting its first remote run.

## Bound binary matching (2026-10-12)

- Fix sized binary extraction when a recursive caller supplies an advanced
  match context: resolve the underlying binary and its current offset before
  comparing a bound variable such as `B = binary:copy(<<N>>, N)`.
- Materialize deferred binary tails for bound-variable equality checks,
  preserving their definitions for sibling match clauses.
- Add a minimal two-frame regression and nearby cases for empty/large inputs,
  nested and alternative patterns, exact types and expected `badmatch` reasons.

## Retain promoted binary storage during minor GC (2026-10-12)

- Fix premature freeing of reference-counted binary storage behind old
  match contexts and nested containers. Minor GC does not traverse those
  containers, so retain promoted backing stores until a major GC traces the
  whole heap. Unreachable young binary storage is still released immediately.
- Add a minimal Rust ownership regression that fails before the fix, plus
  a nested-container test covering young reclamation and major collection.
- Both upstream Base64url properties now pass all 1,563 inputs on BEAM and
  Nera, in cumulative and isolated execution, with exact encoding agreement.
  All 122 Cowlib differential cases also pass. No upstream source is vendored.

## Cowlib Base64url upstream property replay (2026-10-12)

- Add reproducible replay of both original Base64url round-trip properties
  using 1,563 deterministic binaries, plus the original RFC 7515 test.
  BEAM uses real PropEr; Nera executes unchanged predicates through an
  external macro adapter. No upstream code is vendored.
- Compare encoded bytes against an independent Base64 oracle and keep
  cumulative and isolated native execution results separate. A cumulative
  runtime failure remains a failed validation, regardless of isolated passes.
- Add harness regressions and independent false-property controls for both
  engines. Document the newly exposed cumulative binary-extraction blocker.

## Consolidated Cowlib corpus and CI (2026-10-12)

- Rebuild merged main `06ef709` and pass all 122 differential cases together:
  cow_qs 29/29, cow_cookie 34/34, cow_date 26/26, cow_base64url 33/33.
- Add the full corpus to Linux CI against OTP 29.0.5, with external pinned
  sources, failure propagation and retained reports/logs. Remote execution
  remains pending; the recorded run is macOS arm64.
- Consolidate evidence and compatibility documentation without changing the
  corpus or its Set-Cookie comparison contract. Upstream EUnit/PropEr suites
  were not rerun and their existing limitations remain explicit.

## Base64 options APIs (2026-10-12)

- Implement `base64:encode/2` and `base64:decode/2` in the project-owned
  Erlang stdlib, with standard/URL-safe alphabets and optional padding.
- Accept binaries and flat byte lists; decoding ignores space, tab, CR and LF
  and accepts nonzero unused padding bits as BEAM does.
- Add BEAM/native regressions for vectors, every prefix length from 0 to 256,
  all byte values, long input, dynamic calls and malformed-input error reasons.
  Validate decoded symbols explicitly before binary construction.
- `cow_base64url`: all 33 differential cases match BEAM, including five errors.
  No OTP or Cowlib implementation is copied into the repository.

## Cowlib integration and upstream cookie/date validation (2026-10-12)

- Integrate the reviewed Cowlib support chain with current main, preserving
  main's bigint and overflow fixes.
- Add an external adapter for all 54 cookie and 7 date functional tests,
  plus original BEAM EUnit and the date PropEr property (1,000 trials).
- Record the distinction between old main's compilation blockers and the
  integration candidate: native 53/54 cookie assertions and 7/7 date assertions.
  The remaining cookie assertion fails on attribute ordering; native PropEr
  is unvalidated. No upstream test assertions are rewritten or vendored.

## Cowlib Set-Cookie validation contract (2026-10-12)

- Correct a false positive in the differential harness: `cookie_set_build`
  compares complete attribute bytes independently of order, which OTP map
  iteration does not guarantee. Other cases keep their existing contract.
- Preserve raw outputs and report byte equality separately; reject duplicate
  attributes and regressions in cookie bytes, attributes, values or errors.
- Add eight harness regressions (27 tests pass). All 34 cookie corpus cases
  now pass against BEAM. No runtime or upstream Cowlib code is changed.

## UTC date/time APIs (2026-10-12)

- Add native `erlang:universaltime/0` and the Erlang stdlib wrapper
  `calendar:universal_time/0`, including remote funs and dynamic calls.
  Reentrant UTC conversion is independent of the process timezone.
- Add regressions for clock bracketing, non-UTC timezones, epoch/day/year
  boundaries, leap day, 2038 and subsecond flooring.
- Cowlib now executes all cookie cases: 33/34 match BEAM, including two new
  positive Max-Age checks. One strict Set-Cookie attribute-order mismatch
  remains. `cow_date` stays at 26/26. No upstream source is copied.

## Deterministic Gregorian calendar functions (2026-10-12)

- Add project-owned Erlang stdlib implementations of `day_of_the_week/1`,
  `datetime_to_gregorian_seconds/1` and `gregorian_seconds_to_datetime/1`.
  Arithmetic uses Gregorian 400-year cycles and mathematical floor division
  to support year zero, negative years/seconds and arbitrary-size integers.
- Compare a complete 146,097-day cycle with an independent BEAM checksum,
  plus leap-century boundaries, round trips, argument errors and dynamic calls.
  No OTP or Cowlib source code is copied into the repository.

## Unicode string trimming (2026-10-12)

- Add `string:trim/3` as a native stdlib BIF with leading, trailing and both
  directions, custom grapheme sets and Unicode 17 segmentation through pinned
  `unicode-segmentation` 1.13.3. No OTP or Cowlib code is copied.
- Add BEAM/native regressions for UTF-8, codepoint lists, combining marks,
  CRLF, mixed chardata, argument errors, dynamic calls and long inputs.
- List/mixed chardata may be normalized to flat codepoints. Invalid UTF-8
  is rejected eagerly, which differs from some OTP optimized paths.
- Cowlib rerun: cow_qs 29/29; cow_cookie BEAM 32/32, with all native cases
  still blocked by five missing calendar/time APIs.

## Binary search helpers (2026-10-12)

- Add runtime BIFs `binary:last/1` and `binary:match/2`, including dynamic
  calls and remote funs. Matching accepts a binary or a proper nonempty list
  of nonempty binaries and chooses the longest match at the earliest offset.
- Reject invalid types, empty patterns and improper pattern lists with
  `badarg`, including invalid entries after an earlier successful match.
  Compiled patterns and `binary:compile_pattern/1` remain unsupported.
- Add Rust unit tests and a BEAM/native regression for byte values,
  matching precedence, sub-binaries and argument errors.

## Octal character literals (2026-10-12)

- Recognize one to three octal digits in character literals, including
  `$\013` and `$\014` used by Cowlib. Preserve values through `$\777` (511)
  and leave subsequent digits for the next token, matching BEAM.
- Add lexer value/span regressions and a BEAM/native fixture covering
  expressions, binaries and guards.

## Cowlib query-string upstream validation (2026-10-12)

- Run all 72 cow_qs upstream functional tests: real EUnit on BEAM and original
  test closures through an external adapter in Nera, both passing.
- Preserve pinned sources outside the repository, check generator counts and
  completion markers, and document the exclusion of Horse performance tests.
  Native EUnit framework compatibility remains outside this verdict.

## Case-clause exception reasons (2026-10-12)

- Preserve the unmatched value in `{case_clause, Value}`, matching BEAM.
- Add a native regression for integer, binary and nested tuple values.
  All 29 Cowlib query-string cases now pass, including malformed hex escapes.

## lists:join/2 and Cowlib query strings (2026-10-12)

- Implement `lists:join/2` in the Erlang stdlib with tail recursion, preserving
  separator terms and BEAM's `function_clause` errors for improper lists.
- Add regressions for direct/dynamic calls, remote funs, nested separators,
  invalid input and 100,000-element lists, validated on BEAM and Nera.
- Cowlib `cow_qs` now compiles unchanged: 27/29 differential cases pass. Two
  malformed-escape cases reveal a separate `case_clause` reason-shape mismatch.

## Multiplication overflow (2026-10-11)

- Detect overflow before publishing a generated small-integer product. Products
  outside the signed 61-bit domain use the existing bigint fallback instead of
  returning a truncated immediate.
- Combine machine overflow and immediate-domain validation by multiplying one
  decoded operand with the other operand scaled by the tag width.

## Thoas full upstream revalidation (2026-10-11)

- Rebuild merged main and rerun all 338 ExUnit/StreamData tests without exclusions:
  BEAM passes 338; Nera through ETF passes 337, with only the documented memory
  observation limitation. All six properties and 318 JSONTestSuite cases pass.
- Revalidate native assertion bodies (36), differential cases (80) and direct
  memory scenarios (4) on the same build. Consolidate current scope and evidence,
  distinguishing historical blockers and successful PR 27 CI from this local run.

## Left-shift overflow (2026-10-11)

- Avoid truncating overflowing `bsl` constants during folding; retain runtime
  evaluation when the mathematical result does not fit an `i64` literal.
- Check generated small-integer shifts before publishing a tagged result,
  detecting both discarded bits and values outside the immediate domain.
  Overflow falls back to arbitrary-precision arithmetic.

## Bigint conversion and heap capacity contracts (2026-10-11)

- Expose borrow-checked bigint conversions on `BorrowedTerm`; require explicit
  lifetime and layout guarantees for raw conversions and their numeric callers.
- Reject overflowing young-generation reservations and GC allocation layouts in
  release builds, preserving the existing allocations after a rejected request.
- Keep allocation growth on a cold path and prevent adaptive GC thresholds and
  configured heap limits from wrapping.

## Bigint conversion buffer (2026-10-11)

- Fill the byte buffer directly from heap limbs in `term_to_bigint`, removing
  an intermediate limb allocation and traversal while retaining explicit
  little-endian conversion.

## Thoas native assertion corrections (2026-10-11)

- Construct default binary string segments from codepoints truncated to 8 bits,
  matching BEAM for non-ASCII literals while preserving explicit UTF-8 encoding.
- Resolve nested dynamic calls before continuing the caller, even inside an
  active tail-call loop. Keep tail dispatch iterative to preserve bounded stack use.
- Add Rust and Erlang regressions, including exceptions and 100,000 tail calls.
- All 36 Thoas assertion bodies pass directly in Nera through the external
  adapter. Remove the ten failure exemptions and require strict passing in CI.
  Native EUnit framework compatibility remains unvalidated.

## Thoas regression gates (2026-10-11)

- Require direct memory-sharing and real OTP header/assertion probes in Linux CI.
- Protect the 26 passing native Thoas assertion bodies. Run the ten known
  failures too, requiring their exact error signatures; regressions and
  unexpected successes fail the baseline check.
- Add six fail-closed harness tests and preserve native validation reports/logs
  as CI artifacts. Full native EUnit compatibility is still not established.

## Nested callee closure capture (2026-10-11)

- Include the callee expression when collecting free variables for closures.
  This fixes variables such as `Expected` used by an immediately invoked
  anonymous function inside an EUnit generator.
- Add a lowering regression and a native regression checking separate captures
  across two comprehension iterations.
- Add an external adapter to execute the pinned Thoas assertion bodies directly
  in Nera and compare with the original EUnit suite on BEAM. The upstream
  checkout stays unchanged and no upstream implementation is vendored.

## Explicit function imports (2026-10-11)

- Support `-import(Module, [Name/Arity])` for ordinary and tail calls, with
  explicit imports taking precedence over automatically imported BIFs.
- Reject duplicate imports, conflicts with local definitions, imported fun
  references, and non-Erlang imported functions in guards.
- Parse `true()`, `false()` and quoted atom calls as static calls, as needed by
  Thoas. Add Rust and Erlang regressions and a direct Thoas import probe.
- Native EUnit still stops on a nested closure capture (`Expected`); import
  support alone does not establish complete Thoas compatibility.

## Macro argument stringification (2026-10-11)

- Support `??Parameter` in function-like macros without evaluating or expanding
  the argument. Preserve caller text through nested headers and quote literals
  with normalized token spacing, including binary arguments containing commas.
- Add Rust, Erlang and real OTP assertion regressions checked against BEAM.
- Keep reserved atoms quoted during formatting (for example `'case'`), using
  the same quoting rule as macro stringification.
- Thoas tests now pass preprocessing; direct compilation is still blocked by
  unsupported `-import` calls. This does not establish native EUnit support.

## Nested OTP includes and overloaded macros (2026-10-11)

- Preserve the application resolver through nested includes, so EUnit's header
  can resolve `stdlib/include/assert.hrl`.
- Keep macro definitions by name and arity; `-undef` removes every overload.
- Add isolated preprocessor regressions, an Erlang macro regression and an OTP
  header smoke test. Direct Thoas EUnit compilation now reaches the unsupported
  `??Expression` stringification syntax; native EUnit is not yet validated.

## Binary sharing and Thoas memory validation (2026-10-11)

- Implement `binary:referenced_byte_size/1` for binaries and bitstrings, including
  direct calls, remote funs and dynamic application; invalid arguments raise `badarg`.
- Preserve the backing allocation in `binary:part/3` for refc slices larger than
  64 bytes, including nested slices. Small slices and `binary:copy/1` detach.
- Add GC lifetime and Erlang regressions, plus a pinned, in-process BEAM/Nera
  Thoas memory probe without vendoring upstream sources.

## Short float formatting fix (2026-10-11)

- Match BEAM's `float_to_binary/2` `[short]` spelling, including scientific
  notation, ties-to-even shortest digits, the `2^53` boundary and negative zero.
- Add minimal Rust/FFI and Erlang regressions. Revalidate Thoas encoding and its
  float roundtrip property against BEAM without vendoring upstream code.

## Integer formatting padding fix (2026-10-11)

- Apply integer field width, radix and padding in `io_lib:format/2`, fixing
  missing zeros in Thoas date and datetime encoding.
- Add Rust and Erlang regressions; all 36 upstream Thoas EUnit assertions now
  pass against BEAM and native Thoas. See [validation evidence](tests/integration/thoas/README.md#date-padding-correction--2026-10-11)
  for scope and remaining compatibility blockers.

## ETS errors and integer domain (2026-10-11)

- Reject invalid or unsupported `select_delete` specifications before mutation;
  distinguish an empty selection from an explicit match-all specification.
  Supported patterns remain wildcard tuples with distinct variables and one
  comparison guard; more complex specifications raise `badarg`.
- Promote ETS counters beyond the signed 61-bit immediate range to arbitrary
  precision integers, retaining the in-place small-counter path and atomic batches.
- Promote wide binary integers, system timestamps and atomic counter reads
  instead of truncating them; use an unsigned range check for unsigned binary
  integers in generated code.
- Validate counter positions and proper lists; preserve list-shaped results even
  for a single operation. Reject out-of-domain `ErlTerm::small_int` construction
  in release as well as debug, and provide `try_small_int` for fallible encoding.

## Heap allocation and ownership contracts (2026-10-11)

- Check compact ETS heap capacity in release builds, rejecting oversized requests
  before advancing the cursor, including requests that would overflow addition.
- Encapsulate process heap backing owners and centralize binary registration with
  virtual heap accounting, without adding allocations or reference-count changes.

## Pending TCP Accept Closure (2026-10-11)

- Wake every process awaiting `gen_tcp:accept/1` with `{error, closed}` when
  its listener closes, including accept registration races with closure.

## Concurrent TCP Accept Repair (2026-10-11)

- Preserve all processes waiting in `gen_tcp:accept/1` on a shared listener;
  registering another accept no longer strands an earlier process in I/O wait.
- Queue acceptors in arrival order and promote surviving requests after cancellation.
- Add Rust and Erlang regressions plus an opt-in cancellation scaling benchmark.

## ETS counter batch validation (2026-10-11)

- Validate all counter targets before applying a batch: an out-of-range position
  or non-integer field no longer leaves earlier updates applied on failure.
- Preserve the single-counter fast path, with no extra allocation or copying.

## Refc Binary Hash Fix & Arc Lifecycle Hardening (2026-04-12)

### Bug Fixes
- **Fix wrong hash for refc binaries**: `erlang:phash2` hashed the bytes of
  the `Vec` struct instead of the actual binary data for off-heap (refc)
  binaries. Caused by casting `*const Vec<u8>` directly to `*const u8`
  instead of dereferencing through the Vec first. Affects ETS lookups and
  maps with large binary keys.

### Safety Hardening
- Add `debug_assert!` in `clone_arc_from_raw()` verifying the Arc's strong
  count is ≥ 1 — catches use-after-free in debug/test builds.
- Restrict `clone_arc_from_raw()` visibility from `pub` to `pub(crate)`.
- Document the ownership invariant on `ProcessHeap::refc_binaries`.

### Tests
- New Rust unit test: `phash2_refc_binary_matches_heap_binary` — verifies
  same-content heap and refc binaries produce identical hashes.
- New e2e test: `phash2_refc_binary.erl` — end-to-end hash consistency.

## Full Tail Call Optimization (2026-04-12)

### Features
- **Complete TCO for all call targets**: Tail calls now avoid stack growth for
  all dispatch types, not just self-recursion and local calls:
  - **Cross-module**: `return_call` via Cranelift for co-compiled modules
    (including intra-module qualified calls like `maps:map(...)` from `maps.erl`)
  - **Dynamic apply**: `M:F(Args)` where M/F are runtime variables — trampoline
    dispatches from `run_slice` via `STATUS_TAIL_CALL` (6) + ProcessContext fields
  - **Closure calls**: `Fun(Args)` in tail position — same trampoline mechanism
  - **Unknown remote calls**: Dispatched via `nera_rt_apply` trampoline
- IR lowering now emits `TailCall` for all call targets in tail position
  (not just `CallTarget::Local`), with a `try_exception_targets` guard to
  prevent TCO inside try/catch bodies
- Unresolved-call validation now scans `TailCall` terminators too

### Tests
- 5 new e2e tests: `tco_cross_module` (100k mutual bounces), `tco_dynamic_apply`
  (100k dynamic M:F dispatch), `tco_closure_tail` (100k closure calls),
  `tco_trampoline_exception` (exception propagation through trampoline)
- **Total**: 1612 Rust unit tests + 280 end-to-end tests

## Monotonic Timers & Warning Fixes (2026-04-12)

### Bug Fixes
- **Monotonic timer deadlines**: Internal timer deadlines (`receive...after`,
  timer wheel, scheduler timeout checks) now use `std::time::Instant` (monotonic
  clock) instead of `SystemTime` (wall-clock). This makes timeouts immune to
  NTP step corrections, DST changes, and manual clock adjustments. The fix is
  confined to `nera-runtime` — no codegen changes needed.

### Maintenance
- Fix unused variable warning in IR lowering (`_operand` in `UnaryOp`).
- Fix unused import warning in IR warning tests (`HashSet`).
- Fix unused assignment warning in codegen (`needs_exception_check`).

## Unused Record Warning (2026-04-02)

### Features
- **`unused_record` warning**: Records defined via `-record(name, {...})` but
  never referenced anywhere in the module now produce a warning, matching OTP's
  `{unused_record, T}` diagnostic. All record usage sites are tracked: create,
  update, field access, index, and pattern matching.

### Tests
- 7 new tests: create (no warning), unused (warning), pattern (no warning),
  update (no warning), access (no warning), index (no warning), multiple
  records with one unused.
- **Total**: 1186 Rust unit tests + 240 end-to-end tests (1426 total)

## Exported Variables Warning (2026-04-02)

### Features
- **`exported_var` warning**: Variables bound in every branch of a `case`, `if`,
  `receive`, or `try` expression and then referenced after the construct now
  produce a warning, matching OTP's `{exported_var, V, {What, Where}}` diagnostic.
  While technically valid, exported variables are fragile — adding a branch that
  forgets to bind the variable causes a runtime crash. Underscore-prefixed
  variables are suppressed. Variables already in scope before the construct are
  not flagged.

### Tests
- 8 new tests: case export, not used (no warning), if export, receive export,
  underscore prefix (no warning), already in scope (no warning), nested in fun,
  try export.
- **Total**: 1179 Rust unit tests + 240 end-to-end tests (1419 total)

## Illegal Map Construction Error (2026-04-01)

### Features
- **`illegal_map_construction` error**: Using `:=` in map construction
  (`#{k := v}`) now produces an error, matching OTP's
  `illegal_map_construction` diagnostic. Map updates (`M#{k := v}`) remain
  valid.

### Tests
- 5 new tests: valid construction, illegal `:=`, update with `:=` (valid),
  mixed fields, nested map construction.
- **Total**: 1171 Rust unit tests + 240 end-to-end tests (1411 total)

## Record Redefinition & Duplicate Field Errors (2026-04-01)

### Features
- **`redefine_record` error**: Defining the same record name twice via separate
  `-record(...)` attributes now produces an error at the second definition,
  matching OTP's `{redefine_record, T}` diagnostic.
- **`redefine_field` error**: Duplicate field names within a single record
  definition now produce an error at the duplicate field, matching OTP's
  `{redefine_field, T, F}` diagnostic.

### Tests
- 6 new tests: single definition (no error), duplicate record name, different
  names (no error), duplicate field, unique fields (no error), combined
  duplicate record + duplicate field.
- **Total**: 1172 Rust unit tests + 240 end-to-end tests (1412 total)

## Underscore Variable Reuse Warning (2026-04-01)

### Features
- **`match_underscore_var` warning**: Warns when a `_`-prefixed variable (e.g.
  `_X`) appears in a pattern position but is already bound in scope. This matches
  OTP's `{match_underscore_var, V}` diagnostic. The programmer likely meant bare
  `_` (don't care) rather than an equality check against the existing value.
  Covers function head parameters, case/receive/try clause patterns, body-level
  match expressions, and list comprehension generator patterns. Fun clauses
  correctly start a fresh scope and do not trigger false warnings.

### Tests
- 9 new match_underscore_var tests: reuse in case clause, bound once (no warning),
  different clauses (no warning), body match reuse, fun clause (no warning),
  bare wildcard (no warning), non-underscore reuse (no warning), reuse across
  function head params, and reuse in list comprehension generator.
- **Total**: 1122 Rust unit tests + 240 end-to-end tests (1362 total)

## Behaviour Callback Validation (2026-04-01)

### Features
- **Behaviour callback validation**: Modules declaring `-behaviour(gen_server).`
  (or `-behavior(...)`) are now checked for required callback functions. Missing
  callbacks emit `missing_behaviour_callback` warnings at the `-behaviour`
  attribute span, matching OTP's `{undefined_behaviour_func, ...}` diagnostic.
  Supported behaviours: `gen_server` (`init/1`, `handle_call/3`, `handle_cast/2`),
  `gen_statem` (`init/1`, `callback_mode/0`), `supervisor` (`init/1`),
  `application` (`start/2`, `stop/1`), and `gen_event` (`init/1`, `handle_event/2`).
  Only strictly required callbacks are checked — optional ones like `terminate/2`
  and `handle_info/2` do not trigger warnings. Unknown (user-defined) behaviours
  are silently skipped.
- **`-behaviour`/`-behavior` attribute parsing**: These attributes are now fully
  parsed into the AST (`BehaviourAttr` struct with name and span) instead of being
  discarded as `Attr::Unknown`. Both British and American spellings are supported.

### Tests
- 7 new behaviour callback tests: all callbacks present (no warning), missing
  single callback, missing init for supervisor, dual behaviour validation,
  unknown behaviour (no warning), American spelling, optional callbacks omitted.
- **Total**: 1112 Rust unit tests + 240 end-to-end tests (1352 total)

## Module Name & Binary Segment Validation (2026-04-01)

### Features
- **Module name vs filename mismatch warning**: Warns when `-module(Name)` does
  not match the source filename (e.g., `-module(goodbye)` in `hello.erl`),
  matching OTP's `{module_name, Mod, Filename}` diagnostic. Emits a warning
  (not error) with code `module_name_mismatch` and a hint suggesting the fix.
  Works in both CLI compilation and LSP.
- **Binary segment type specifier validation**: Compile-time detection of invalid
  binary segment combinations that OTP's compiler rejects. Catches float segments
  with invalid sizes (must be 16, 32, or 64), signedness on non-integer types,
  endianness on binary segments, and size/unit on UTF types. Emits error
  diagnostics with code `invalid_binary_segment`.

### Tests
- 4 new module name mismatch tests: matching name, mismatched name (verifies
  warning severity), full path handling, and no-filename (None) case.
- 9 new binary segment validation tests covering all validation rules.

## Fault-Tolerant Parser (2026-04-01)

### Features
- **Fault-tolerant parsing**: `parse_module()` now always returns a partial AST
  alongside any errors, instead of discarding all valid forms on the first error.
  The parser synchronizes on `.` (form boundaries) and continues — if 1 out of
  20 functions has a syntax error, the other 19 still produce valid AST nodes.
- **LSP partial AST support**: The LSP server uses the partial AST for semantic
  tokens, hover, goto-def, completions, and project indexing even when the file
  contains syntax errors. Previously, a single parse error would cause all IDE
  features to stop working for the entire file.
- **New `ParseResult` type**: Replaces the previous `Result<Module, Vec<ParseError>>`
  return type with a struct that always contains both a `Module` and any `errors`.
  CLI compilation still blocks on parse errors.

### Tests
- 5 new parser unit tests for fault-tolerant parsing: partial parse with broken
  function in the middle, broken attribute recovery, multiple errors with valid
  forms preserved, error span verification, and no-regression on valid files.
- **Total**: 1077 Rust unit tests + 244 end-to-end tests (1321 total)

## OTP 21+ Storage Primitives (2026-03-28)

### Features
- **`persistent_term` module**: Read-optimized global key-value storage (OTP 21+).
  `put/2`, `get/1,2`, `erase/1`, `info/0`. Values are deep-copied into shared
  storage using ETS infrastructure (`TermKey`, `CompactHeap`, `deep_copy`).
  Fast path for immediate keys (atoms, small ints) avoids TermKey serialization.
  Uses `parking_lot::RwLock` for fast uncontended reads. Writes are 3x faster
  than BEAM; reads ~4x slower (deep-copy vs BEAM's zero-copy). Overwrite and
  erase are 900-4000x faster (no global GC).
- **`counters` module**: Lock-free atomic counter arrays (OTP 22+).
  `new/2`, `get/2`, `put/3`, `add/3`, `sub/3`, `info/1`. Raw pointer stored
  directly in Erlang reference term — zero-overhead access path with no
  registry lookup, RwLock, or Arc clone. On par with BEAM performance.
- **`atomics` module**: Atomic 64-bit integer arrays with CAS (OTP 22+).
  `new/2`, `get/2`, `put/3`, `add/3`, `sub/3`, `add_get/3`, `sub_get/3`,
  `exchange/3`, `compare_exchange/4`, `info/1`. Same direct-pointer
  optimization as counters. Supports `[{signed, true|false}]` option.
  On par with BEAM performance.
- **21 new BIF entries** registered in the unified BIF table with FFI bindings
  and `erlang:apply/3` dynamic dispatch support.

## `nera check` Command (2026-03-26)

### Features
- **`nera check`**: New subcommand that runs the full front-end pipeline
  (parse, preprocess, IR lowering, validation) without code generation or
  linking. Catches syntax errors, undefined variables, unresolved calls, and
  emits warnings — much faster than a full `nera build`. Supports the same
  source discovery flags as `build`: `--src`, `-I`/`--include`, `--deps-dir`,
  `--no-stdlib`, `--no-warnings`, and `-v`/`--verbose`.
- **Refactored compilation pipeline**: Extracted shared front-end into a
  reusable `check()` function. `compile()` now calls `check()` for parsing
  and validation, then continues with codegen and linking. This is the
  prerequisite for future LSP integration.

## Dependency Management (2026-03-23)

### Features
- **`nera init`**: Initialize a nera project in the current directory.
  Generates `nera.toml` with project name derived from directory name,
  creates `src/` if missing, and adds `.nera/` to `.gitignore`.
- **`nera deps get`**: Fetch all dependencies declared in the `[deps]`
  section of `nera.toml`. Supports three dependency sources:
  - **hex.pm**: `jsx = "3.1.0"` — downloads tarball from `repo.hex.pm`,
    extracts `contents.tar.gz` into `.nera/deps/<name>/`.
  - **git**: `{ git = "...", tag/branch/rev = "..." }` — shallow clone
    via `git` CLI, `.git/` removed after clone to save space.
  - **path**: `{ path = "../shared_utils" }` — symlinked into `.nera/deps/`.
- **`nera deps add <name> [version]`**: Add a hex.pm dependency to
  `nera.toml` and fetch it. Queries hex.pm API for latest version if
  no version specified. TOML manipulation preserves user comments and
  formatting.
- **Build integration**: `nera build` auto-discovers `.nera/deps/` when
  a `nera.toml` exists. Each subdirectory is scanned as an OTP application
  (`src/*.erl` + `include/`). No manual `--deps-dir` flag needed.
- **`nera new` updated**: `.gitignore` template now includes `.nera/`.

### Tests
- 39 new unit tests covering DepSpec deserialization (6 tests),
  find_deps_section_end (5), add_dep_to_toml (8), update_gitignore (6),
  resolve_path_dep (2), cmd_deps_get orchestration (5), cmd_deps_add (1),
  NeraToml integration (2), cmd_init (1), deterministic ordering (1),
  TOML round-trip validity (1), prefix collision (1).
- **Total**: 659 Rust unit tests + 232 end-to-end tests (891 total)

## Send Path Optimization (2026-03-23)

### Features
- **`direct_copy_message` in send path**: Direct-to-heap send now uses
  `direct_copy_message` instead of `deep_copy_term`, skipping the HashMap
  forwarding table for simple terms (tuples of immediates + refc binaries).
- **MakeTuple + Send fusion**: New IR optimization pass `fuse_make_tuple_send`
  detects single-use tuples immediately sent to another process and fuses them
  into a `SendDirect` instruction. The fused instruction constructs the tuple
  directly on the receiver's heap, avoiding sender heap allocation entirely.
- **`nera_rt_send_tuple` FFI**: New FFI function for fused sends (arity ≤ 4).
  Handles direct-to-heap, lock-free fragment, and self-send paths. Builds
  `InlineTuple`/`SimpleTuple` fragments directly from raw elements without
  intermediate tuple allocation.

## Embedded Stdlib (2026-03-23)

### Features
- **Embedded stdlib sources**: Standard library `.erl` files are now embedded
  directly in the compiler binary via `include_str!` and a `build.rs`
  auto-discovery script. The compiler no longer requires external stdlib files
  on the filesystem — it is fully self-contained for stdlib compilation.
  Filesystem stdlib still takes priority when present, allowing local overrides
  for development and debugging.

## Bitstrings & Performance Hardening (2026-03-23)

### Features
- **True bitstring support**: Binary header now stores bit count instead of byte
  count. `byte_size = (bits+7)/8`. `is_binary/1` requires `bits%8==0`;
  `is_bitstring/1` matches any binary/bitstring. Display shows trailing partial
  byte as `Value:Bits`. All binary IR, lowering, codegen, and runtime now use
  bit offsets throughout.
- **FastMailbox ring buffer**: Replaced `VecDeque<ErlTerm>` with `#[repr(C)]`
  power-of-2 ring buffer for the process mailbox. Index wrapping via `& (cap-1)`.
  GC uses `gather_roots()`/`scatter_roots()`. Field offsets exported for codegen.
- **Inline receive ops**: `receive_peek`, `receive_next`, `receive_accept`
  inlined in Cranelift IR. Fast path: direct ring buffer read/write (no FFI call).
  Slow path: FFI fallback for empty mailbox or selective receive middle-removal.
- **Direct-to-heap send**: `nera_rt_send_message` tries `entry.proc.try_lock()`
  when target is WAITING/READY. If successful, deep-copies directly to receiver's
  heap + mailbox (bypasses SegQueue entirely). Falls back to lock-free SegQueue
  on contention.
- **Register-based reduction counter**: Reduction counter kept in a Cranelift
  `Variable` instead of reloading from `ProcessContext` on every check. Only
  spilled on yield/call boundaries, reducing memory traffic in hot loops.
- **Redundant CheckReductions elimination**: Removed CheckReductions at function
  entry (covered by caller's check) and before self-tail-calls (loop body already
  has YieldCheck at back-edges).

### Bug Fixes
- **reds_var reload in self-tail-call loops**: Self-tail-call loops never yielded
  because `reload_reds_from_ctx()` was at the top of `loop_block`, resetting the
  reduction counter to 4000 on every iteration. Moved to `entry_cl_block` (runs
  once on function entry).
- **Head-recursive stdlib list functions**: Fixed `lists:seq/2,3`,
  `lists:duplicate/2`, and `lists:flatten/1,2` to be tail-recursive with
  accumulator + reverse, preventing stack overflow on large inputs.
- **Scheduler/GC latency**: Reduced p99.9 tail latency by tuning GC threshold
  ramp-up and scheduler wake-up timing.

### Benchmark Results (BEAM OTP 28)

| Benchmark                     | nera   | BEAM   | nera RSS | BEAM RSS |
|-------------------------------|--------|--------|----------|----------|
| binary_send (100K×10KB)       | 25ms   | 15ms   | 27MB     | 63MB     |
| binary_fanout (1000×100KB)    | 0ms    | 1ms    | 10MB     | 58MB     |
| binary_subbinary (100K×100KB) | 0ms    | 1ms    | 5MB      | 56MB     |
| tcp_ping_pong (50K×1KB)       | 1844ms | 1631ms | 7MB      | 56MB     |
| tcp_throughput (100MB bulk)   | 46ms   | 36ms   | 30MB     | 57MB     |
| tcp_connections (100 conns)   | 8ms    | 6ms    | 7MB      | 59MB     |

### Tests
- **Total**: 620 Rust unit tests + 232 end-to-end tests (852 total)

## Performance Optimization Sprint (2026-03-21)

### Runtime Optimizations
- **Generational GC**: Two-generation (young/old) copying garbage collector. Young
  gen uses multi-chunk `Vec<u64>`-backed bump allocator. No write barrier needed
  (Erlang immutability). Minor GC promotes live data to old gen; major GC copies
  everything. Result: tcp_ping_pong -15%, RSS -56%.
- **Lock-free MPSC mailbox**: `crossbeam_queue::SegQueue<MessageFragment>` replaces
  Mutex-based message delivery. Send path: deep-copy to fragment → lock-free push →
  CAS wake. Result: binary_send 170ms → 98ms.
- **Inline small-int arithmetic**: Add, Sub, Mul, Div, Rem, all comparisons, and
  bitwise ops inlined as Cranelift IR with fast-path tag checks. Overflow/mixed-type
  falls back to FFI. Result: binary_subbinary 19ms → 0ms.
- **Inline MakeCons**: Bump-pointer allocation in Cranelift IR with 4KB inline
  region from ProcessContext. Fast path: load cursor, check space, write, bump.
  Slow path refills from bumpalo.
- **Inline hd/tl**: Tag check + pointer load fast path, FFI fallback for badarg.
- **Inline binary ops**: BsMatchStart2, BsStartMatch, BinarySize, BsGetIntegerInline
  (8/16/32/64-bit) all emit Cranelift IR instead of FFI calls.
- **Binary match context**: Lazy sub-binary creation via BsMatchStart2/BsMatchCtxAdvance/
  BsMaterialize. Subbinary benchmark 20ms → 12ms.
- **SimpleTuple + InlineTuple message fragments**: Tuples of immediates/refc binaries
  avoid fragment heap allocation entirely. InlineTuple (arity ≤ 2) is zero-alloc.
  Boxed Heap variant shrinks enum from ~260 to ~80 bytes. Batch allocation in
  copy_to_heap (single alloc_words for tuple + all refc descriptors).
- **True TCO via return_call**: Non-self tail calls use Cranelift `return_call`.
- **Back-edge yield checks**: YieldCheck at CFG back-edges prevents tight-loop starvation.
- **Tail-recursive list comprehensions**: O(1) stack instead of O(N).
- **Batch startup**: Single FFI call for atom interning and function registration.
  Lazy I/O pool (threads spawned on first I/O).
- **Timer cancel fast path**: Atomic check skips Mutex when no timers active.
- **Binary vheap unique-Arc accounting**: GC threshold based on unique Arc count,
  not total descriptor count. Prevents spurious GC cycling when many messages share
  the same refc binary.
- **mio reactor**: Event-driven I/O via epoll/kqueue for TCP. Active-mode sockets
  use reactor-driven reads.

### Benchmark Results (BEAM OTP 28)

| Benchmark                     | nera   | BEAM   | nera RSS | BEAM RSS |
|-------------------------------|--------|--------|----------|----------|
| binary_send (100K×10KB)       | 36ms   | 13ms   | 38MB     | 58MB     |
| binary_fanout (1000×100KB)    | 1ms    | 2ms    | 30MB     | 59MB     |
| binary_subbinary (100K×100KB) | 0ms    | 1ms    | 5MB      | 58MB     |
| tcp_ping_pong (50K×1KB)       | 1597ms | 3194ms | 8MB      | 56MB     |
| tcp_throughput (100MB bulk)    | 41ms   | 37ms   | 11MB     | 56MB     |
| tcp_connections (100 conns)   | 7ms    | 7ms    | 8MB      | 58MB     |

### Tests
- **Total**: 601 Rust unit tests + 221 end-to-end tests

## Process Hardening (2026-03-19)

### Features
- **`process_flag(fullsweep_after, N)`**: Control how many minor GC cycles run
  before a forced major (full-sweep) GC. Wires the existing generational GC
  `max_minor_gcs` to the standard Erlang `process_flag/2` API. Returns the
  previous value.
- **`process_flag(max_heap_size, Words)`**: Kill runaway processes that exceed a
  memory limit. After each time-slice GC, if the heap still exceeds
  `max_heap_size * 8` bytes, the process is terminated with reason `killed`.
  Linked processes receive `{'EXIT', Pid, killed}`.
- **`erlang:hibernate/3`**: Shrink a process to zero heap and suspend until a
  message arrives. On wake, the process calls `M:F(A)` with a fresh heap.
  Preserves mailbox, OTP state (links, monitors, trap_exit), and process
  dictionary. Ideal for long-idle processes that should not hold memory.
- **`spawn_opt/2,4`**: Spawn a process with options. Supports `link`, `monitor`,
  `{fullsweep_after, N}`, `{max_heap_size, Words}`, and `{min_heap_size, Words}`.
  Returns `{Pid, Ref}` when `monitor` is in options, otherwise `Pid`.

### Tests
- 4 new end-to-end tests: `process_flag_fullsweep`, `max_heap_size`,
  `hibernate_basic`, `spawn_opt_basic`
- **Total**: 600 Rust unit tests + 220 end-to-end tests

## OTP Behaviours, ETS & OneCached (2026-03-01)

### Milestone: OneCached memcached server runs end-to-end
- ProcessOne's open-source memcached server compiled and running with nera
- All memcached text protocol commands working: SET, GET, ADD, REPLACE,
  DELETE, INCR, DECR, flush_all
- Uses gen_server, supervisor, application, ETS, gen_tcp, binary pattern
  matching — a real OTP application with supervision trees

### Features
- **OTP behaviours**: `gen_server` (start_link, call, cast, reply, full callback
  dispatch), `supervisor` (one_for_one strategy, max restart intensity),
  `application` (start/stop, app_name → app_name_app convention), `proc_lib`
  (spawn, spawn_link, init_ack). Implemented as compiled Erlang in
  `erlang/stdlib/`.
- **ETS tables**: `ets:new/2`, `ets:insert/2`, `ets:lookup/2`, `ets:delete/1,2`,
  `ets:delete_all_objects/1`, `ets:match_object/2`, `ets:select_delete/2`,
  `ets:tab2list/1`, `ets:info/1,2`, `ets:first/1`, `ets:next/2`. Named tables
  with `{keypos, N}` support.
- **error_logger**: Modern colored timestamped output for `info_msg`, `error_msg`,
  `warning_msg` — red for errors, yellow for warnings, with `HH:MM:SS [level]`
  prefix.
- **New BIFs**: `integer_to_binary/1`, `binary_to_integer/1`, `list_to_binary/1`,
  `binary_to_list/1`, `iolist_to_binary/1`, `binary:split/2,3` (with `[global]`),
  `binary:part/3`, `erlang:system_time/1`, `erlang:monotonic_time/1`,
  `erlang:send_after/3`, `erlang:process_info/1,2`,
  `erlang:is_process_alive/1`, `io_lib:format/2`.
- **gen_tcp enhancements**: `{packet, line}` option parsing in `listen/2`,
  option inheritance from listener to accepted sockets, `{active, once}` mode
  delivering `{tcp, Socket, Data}` messages, `inet:setopts/2`, iolist support
  in `send/2` (recursive flattening of nested lists and binaries).
- **io:format improvements**: Added `~p`, `~d`, `~f` format specifiers, binary
  support for `~s`, stdout flush after output.
- **String escape sequences**: Lexer and parser now correctly handle `\r`, `\n`,
  `\t`, `\\`, `\"`, `\'`, octal escapes in string literals.

### Bug Fixes
- **Multi-clause binary rest pattern**: Fixed pattern compiler scope leak where
  `R/binary` in one function clause would get the wrong VarId from another
  clause. Root cause: shared variable scope across clauses in `compile_column`.
  Fix: push/pop scope per clause isolates bindings.
- **iolist flattening**: `gen_tcp:send` now recursively handles nested lists of
  binaries and integers (was only handling flat lists of integers).
- **String escapes in lexer**: Lexer regex updated from `"[^"]*"` to
  `"([^"\\]|\\.)*"` to allow backslash-escaped characters in string literals.
- **recv_line CRLF stripping**: Active-mode line delivery strips trailing `\r\n`
  before delivering to process (workaround for binary pattern arithmetic size
  limitation).
- **apply/3 dispatch**: Fixed runtime function dispatch through `nera_rt_apply`
  for dynamic calls.

### Tests
- 3 new Rust unit tests for multi-clause binary pattern compilation
- 7 new Rust unit tests for string escape sequence handling
- 55 new end-to-end tests covering OTP behaviours, ETS, process management,
  binary operations, and concurrency patterns
- **Total**: 504 Rust unit tests + 150 end-to-end tests (654 total)

## Compiler Warnings & Diagnostics (2026-02-24)

### Features
- **Compiler warnings**: AST-level analysis pass detecting unused variables,
  unreachable clauses, shadowed variables, and unused functions. Warnings are
  printed to stderr and never block compilation.
- **Colored diagnostics**: Adopted `codespan-reporting` for rustc-quality error
  display with colored output, source snippets, and underlined spans.
- **Reserved keyword checks**: All 27 Erlang reserved words are rejected when
  used as unquoted atom names, with a helpful suggestion to use the quoted form.
- **Reduced parser diagnostic noise**: Redundant token descriptions removed
  from error messages.

## Tooling & Cross-Compilation (2026-02-18)

### Features
- **`nera test` subcommand**: Built-in parallel test runner with colored output,
  pattern filtering, per-test timing, `%% SKIP` support, and configurable timeout.
  Replaces the shell-based `run_tests.sh`.
- **Cross-compilation**: `--target <triple>` and `--linker <path>` flags for
  generating code for a different architecture. Supports macOS (arm64, x86_64),
  Linux (x86_64, arm64), and Windows (x86_64). `scripts/dist.sh` also supports
  `--target`.
- **CLI refactor**: Added subcommands (`new`, `build`, `run`) and unified
  project options via `nera.toml`.
- **`install-local.sh`**: Script for building and installing nera to a local
  prefix (default `~/.local`).
- **Project renamed** from `erlcc` to `nera` (Native Erlang Runtime
  Architecture).

## Async I/O & File Operations (2026-02-24)

### Features
- **Async I/O pool**: Fixed-size pool of OS threads for blocking I/O. FFI
  functions set `STATUS_IO_WAIT`; the codegen call-site bailout automatically
  suspends the process. On I/O completion, the result is written to `ctx.result`
  and the process is re-enqueued.
- **File I/O BIFs**: `file:read_file/1` and `file:write_file/2` for reading
  and writing files.
- **Float binary segments**: `<<X:64/float>>` for constructing and matching
  64-bit IEEE 754 floats in binaries.

### Bug Fixes
- Fixed binary string segment handling.
- Fixed `io_pool` shutdown race condition.

### Tests
- 113 new unit tests across nera-codegen, nera-ir, and nera-runtime.

## Standard Library & Timer (2026-02-23)

### Features
- **`lists` module**: 42 exported functions. Hot-path functions (`reverse`,
  `member`, `keyfind`, `keymember`, `keysearch`, `append/1`, `last`, `nth`)
  as Rust FFI BIFs; higher-order functions (`map`, `filter`, `foldl`, `sort`,
  etc.) as compiled Erlang in `erlang/stdlib/lists.erl`.
- **`maps` module**: 28 exported functions. Hot-path functions (`new/0`,
  `get/2,3`, `find/2`, `put/3`, `remove/2`, `is_key/2`, `keys/1`, `values/1`,
  `to_list/1`, `from_list/1`, `from_keys/2`, `size/1`, `merge/2`, `update/3`,
  `take/2`) as Rust FFI BIFs; higher-order functions (`map/2`, `filter/2`,
  `fold/3`, `foreach/2`, etc.) as compiled Erlang in `erlang/stdlib/maps.erl`.
- **`spawn_monitor/1,3`**: Atomically spawn and monitor a process, returning
  `{Pid, Ref}`.
- **Timer wheel**: Min-heap timer with dedicated timer thread and condvar-based
  worker notifications. Replaces busy-polling with precise deadline-based wakeups.
- **Type/spec parsing**: `-type`, `-spec`, `-opaque`, `-callback`, `-behaviour`,
  and `-export_type` declarations are parsed and silently skipped (nesting-aware
  skip logic).

### Performance
- Reduced scheduler termination latency from ~500ms to ~9ms.

## Registered Processes (2026-02-23)

### Features
- **Process registration**: `register/2`, `whereis/1`, `unregister/1`,
  `registered/0` BIFs for naming processes with atoms
- **Send to registered names**: `Name ! Msg` works when `Name` is a
  registered atom — `nera_rt_send_message` resolves atoms via the global
  registry before dispatching
- **Auto-cleanup on death**: Registered names are automatically removed when
  a process exits (normal, crash, or cascade kill via links)
- **Self-registration safety**: Registering the calling process avoids
  deadlock by using the `ProcessContext.otp_ptr` directly instead of
  re-locking the process Mutex (same pattern as self-send)

### Implementation
- Added `registered_names: Arc<RwLock<HashMap<u32, ProcessId>>>` to
  `SchedulerHandle` and `Scheduler`
- 4 new FFI functions: `nera_rt_register`, `nera_rt_whereis`,
  `nera_rt_unregister`, `nera_rt_registered`
- `unregister_dead_process()` called at all process exit points in
  `worker_loop` and `propagate_exit_signals`
- 4 new BIF table entries (all auto-imported from `erlang` module)
- 3 new end-to-end tests: `register_basic`, `register_send`, `register_process`

## Phase 2: Concurrency (2026-02-19)

### Milestone: ring.erl (100-process ring) runs end-to-end
- A ring of 100 processes passes messages around the ring 10 times
- Produces `Ring complete` on stdout
- Consistent results across repeated runs with 8 worker threads

### Milestone: ping_pong.erl (2-process message passing) runs end-to-end
- Two processes exchange 10 rounds of ping/pong messages
- Produces 21 lines of output: alternating Pong/Ping messages + "Ping done"
- Consistent results across repeated runs with multi-threaded scheduler

### Features
- **Work-stealing scheduler**: N worker threads (one per CPU core) with crossbeam-deque
  work-stealing queues. Atomic runnable_count for race-free termination detection.
- **Process spawn**: `spawn(Module, Function, Args)` creates new lightweight processes
  via the global function table
- **Message passing**: `Pid ! Message` deep-copies terms between process heaps;
  `receive` with pattern matching suspends processes until messages arrive
- **Receive-wait continuation**: Processes save a resume continuation when entering
  receive-wait, allowing the scheduler to re-invoke them at the correct point when
  messages arrive. Resume jumps skip pre-receive side effects via block splitting.
- **Self-tail-call optimization**: Tail-recursive calls to the current function are
  compiled as loops (jump back to entry block) with zero stack growth
- **Function registration**: Compiled functions are registered in a global function
  table at startup, enabling spawn to look up functions by module:function/arity
- **Atomic scheduler termination**: Uses `runnable_count` atomic counter combined with
  process table scan for reliable termination detection without races

### Bug Fixes
- Fixed atom ID mismatch in BinOp true/false and Branch terminator
- Fixed spawn/3 detection in IR lowering (was treated as local call)
- Fixed receive block splitting so resume skips pre-receive instructions
- Fixed `initial_fn` clearing after resume continuation completes normally
- Fixed scheduler premature termination race condition (non-atomic should_terminate)
- Fixed idle_spins never reaching termination threshold when polling waiting processes
- Fixed tail-position clause bodies missing Return terminators (caused SIGILL in ring.erl)
- Fixed linker preferring stale debug staticlib over fresh release build

### Build System
- Linker now prefers release staticlib over debug
- Linker uses executable-relative path discovery for runtime library
- Linker prints the canonical path of the runtime library being linked

## Phase 1: Sequential Programs (2026-02-18)

### Milestone: factorial.erl runs end-to-end
- Computes `10! = 3628800` using recursive function with pattern matching
- Multiple function clauses with guards compile correctly

### Milestone: hello.erl runs end-to-end
- First Erlang program compiled to a standalone native binary
- `io:format("Hello, World!~n")` produces correct output

### Features
- **Lexer** (`logos`): All Erlang tokens including operators, keywords, literals
- **Parser** (recursive descent with Pratt parsing): Modules, functions, pattern
  matching, case/if expressions, guards, all binary operators
- **AST**: Full representation of the supported Erlang subset
- **IR**: Flat SSA-like representation with basic blocks, pattern match compilation
  (Maranget algorithm), tail call detection
- **Cranelift codegen**: Function ABI with ProcessContext, reduction counting at function
  entry, runtime calls for heap allocation, BIF dispatch
- **Runtime**: Tagged 64-bit ErlTerm representation, per-process bump-allocated heaps
  (bumpalo), atom table, io:format BIF, cons/tuple/float allocation
- **Linker**: Produces standalone native binaries by linking with libnera_runtime.a
