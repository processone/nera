# Thoas JSON Integration Test

Tests that the [thoas](https://github.com/lpil/thoas) JSON library (3100 LOC)
compiles and runs correctly with Nera.

## Current validation — main, 2026-10-11

Fresh release build of `main` at `9b64f629dbc854014ad351a451145cd55df6a50c`,
macOS arm64, OTP 29.0.5 and Elixir 1.20.3. Thoas remains pinned to 1.2.1
(`7cdcec7e52b80d2869900893b2acf47199d84190`); StreamData is pinned to 0.5.0.
No upstream code, test fixtures or dependencies are stored in this repository.

| Scope | BEAM | Nera |
| --- | --- | --- |
| Full ExUnit + StreamData | 338/338 | 337/338 through ETF; one transport limitation |
| Ordinary encoder/decoder tests | 14/14 | 13/14; copying test cannot observe native sharing through ETF |
| JSONTestSuite | 318/318 | 318/318 |
| StreamData properties | 6/6, 100 trials each | 6/6, 100 trials each |
| Original EUnit through upstream runner | 36/36 | 36/36 through ETF |
| Original assertion bodies through native adapter | 36/36 reference | 36/36 executed directly |
| Differential corpus | 80 cases | 80/80 agree |
| Direct memory scenarios | 4/4 | 4/4, plus referenced-byte-size regression |

All 338 tests were executed, with **zero exclusions and zero skips**. Properties
use seed 0 and one test at a time. The native library handled **1,665 calls**
without a transport failure, native crash or timeout.

The raw upstream runner correctly exits **1** for Nera: `copying strings on
decode` observes the binary after ETF has copied it (101 referenced bytes,
where the assertion requires more than 203). This result is retained as a
failure, not skipped or relabeled as passing. The direct memory probes pass on
both engines and measure the native behavior without that transport boundary.
No additional functional mismatch was observed in this run.

ExUnit, StreamData and the upstream EUnit runner execute on BEAM; native mode
routes library calls to Thoas compiled by Nera. The separate native adapter
executes the 36 assertion bodies in Nera. Direct compilation of the EUnit test
module succeeds, but native EUnit runner/autoexport support remains unvalidated.

[Reproducible evidence](main-validation-evidence.json) records source, dependency,
compiler/runtime and report hashes. Full logs and external sources are under
`/private/tmp/nera-thoas-main-validation`; temporary files are not durable
artifacts. Reproduce the full run using the commands in
[upstream validation](#upstream-eunit-and-exunit-validation), then run the
[direct probes](#automated-regression-gates). Keep every work directory outside
this repository. The full suite remains a diagnostic with the documented ETF
failure; its result must not be presented as an entirely green native suite.

The [PR 27 CI run](https://github.com/processone/nera/actions/runs/38171086897)
passed all four jobs, including Linux corpus, memory, OTP and strict 36-assertion
checks. This is separate evidence from the full local ExUnit/StreamData run.

Dated results later in this document are **historical checkpoints**. Their
blockers and counts describe the corresponding compiler revision, not the
current status above.

## What it tests

- JSON encode/decode roundtrip for basic types (null, bool, int, string)
- Collection encoding (lists, maps)
- Nested structure roundtrip (map → JSON → map)
- Float encode/decode
- String escape handling
- Decode error handling
- Atom key encoding

## Why it matters

Thoas exercises several Nera features simultaneously:
- Binary pattern matching with `/bitstring` rest patterns
- Closure tail-calls through wrapper functions
- Multi-key map encoding via `maps:to_list`
- `iolist_to_binary` with deeply nested iodata
- Cross-module calls to `binary:at/2`, `binary:copy/1`
- BIFs: `integer_to_binary/2`, `binary_to_existing_atom/1`, `binary_to_float/1`,
  `atom_to_binary/2`, `float_to_binary/2`, `integer_to_list/2`

## Reproducible BEAM/Nera differential validation

From the repository root (Python 3.9+, Git, `erl`, `erlc`, and a C linker):

```sh
cargo build --release --locked
python3 -m unittest discover -s tests/integration/thoas/differential -p test_runner.py
python3 tests/integration/thoas/differential/runner.py
# Repeat using the verified source checkout, without network access:
python3 tests/integration/thoas/differential/runner.py --offline
# Reproduce one case (the report lists the selected IDs):
python3 tests/integration/thoas/differential/runner.py --offline --case decode_unicode_escape
```

`--nera /absolute/path/to/nera`, `--work-dir /absolute/path`, and `--timeout 60`
are available. Run only one invocation per work directory. A command timeout
fails validation (compilation gets at least 180 seconds). The runner creates
and replaces its own `build/` subdirectory; use a dedicated work directory.

### Source and reference versions

Both engines compile the **same unmodified upstream source files**, fetched
outside tracked sources into `target/thoas-differential/upstream/` by default.
No Thoas implementation is vendored. `differential/upstream.json` fixes Thoas
1.2.1 to commit `7cdcec7e52b80d2869900893b2acf47199d84190`; this version has no
runtime dependencies. An existing checkout must match the commit and contain
no modifications or extra files, including ignored files. A mismatch fails;
the runner does not reset or overwrite the checkout. Remove or choose another
work directory to change revisions deliberately.

CI uses OTP **29.0.5** on Linux x86-64. Local runs record the installed OTP/ERTS
version, so use that same OTP release when reproducing CI. The normal macOS
job still runs the existing Thoas integration and comparator unit tests; the
differential command can also be run manually on macOS arm64. A successful
cross-compilation is not a target execution result.

### Corpus and comparison contract

`differential/corpus.json` is our own deterministic 80-case corpus, not a copy
of Thoas source or its upstream test suite. Each record has a stable ID,
category, operation, Erlang expression, and expected outcome category.

- Valid inputs: literals, collections, nested maps/lists, whitespace, UTF-8,
  escapes, and surrogate pairs.
- Errors: truncated input, malformed numbers/objects/arrays, invalid UTF-8,
  invalid escapes/surrogates, unsupported encoder values, and overflow.
- Boundaries: signed zero, integer representation boundaries, arbitrary-size
  integers, minimum/maximum finite floats, underflow, duplicate keys,
  64 nested arrays, a long string, and a 256-element array.
- Options: copy modes, existing atom keys, and JSON/Unicode/HTML/JavaScript
  escaping. These tests check semantic output, not preferred escape spelling.

Large encoder integers are constructed with `binary_to_integer/1` so this
suite exercises Thoas and the runtime independently of the lexer's supported
integer-literal range.

The generated Erlang driver wraps returns and exceptions (class and reason,
without compiler-specific stack traces). It emits External Term Format as
byte arrays, one identified result per case, followed by a completion count.
The Python reader preserves Erlang types, binary contents, float bits, and
error details, while ignoring ETF representation choices and map order.
Missing, duplicate, unexpected, malformed, or post-completion output fails;
exit status zero alone cannot pass.

Decoder outcomes and exception outcomes must agree exactly. Successful
encoder outputs are parsed independently by Python's JSON parser and compared
as typed JSON values: object order and equivalent number spellings are ignored,
but booleans versus numbers, duplicate object members, arrays, and wrong values
remain distinct. Invalid JSON fails even if both engines produce it. Expected
outcome categories also prevent both engines failing a nominally valid case
from counting as success. This does not prove full JSON conformance or replace
the upstream test suite, property generation, or long-running stress tests.

### Evidence and failures

The command exits nonzero on setup/build/runtime/protocol/comparison failure.
`report.json` records the pinned source, tool versions, compiler binary hash,
corpus/runner/source hashes, Nera checkout commit and dirty status, commands,
platform, and verdict. The compiler hash identifies the executable actually
used; the checkout commit alone does not prove that executable was built from
it. Use the preceding fresh build for that association.

Raw stdout/stderr, decoded `beam.json` and `nera.json`, and the generated driver
remain under the work directory. CI uploads this evidence even on failure.
Failures are reported by case ID, with both outcomes. No known divergence is
silently skipped or accepted as a passing comparison.

To extend the corpus, add a record with a unique ID, run it on BEAM and Nera,
and review the expected category against the upstream API. Update the pinned
SHA and `nera.toml` together when intentionally upgrading Thoas.

### Initial local result — 2026-10-11 (before the case-state repair)

On macOS arm64 with OTP 29.0.5 and Nera freshly built from `c29c04c`:

- All 80 corpus expectations pass on BEAM.
- The full native run exits with SIGSEGV after 15 completed results, before
  `decode_unicode_escape` completes. This case decodes `"\u00e9\u65e5"`.
- Selecting only `decode_unicode_escape` reproduced the native crash. Its
  cause was not yet diagnosed at this initial baseline; see the repair below.
- A separate five-case selection (`decode_null`, `error_array_trailing_comma`,
  `boundary_huge_integer`, `encode_nested`, `encode_unsupported_tuple`) agrees
  on both engines. It does not substitute for the full corpus.
- All 14 harness unit tests pass. They include divergence, incomplete output,
  duplicate IDs, ETF type preservation, and independent JSON comparison.

This initial run did not establish a green verdict for the complete corpus;
cases after the crash were not counted as passing.

### Case-state repair — 2026-10-11

The complete unchanged corpus now agrees on BEAM and Nera on macOS arm64:
**80/80 cases**, comprising 30 valid, 30 error, 14 boundary and 6 option cases,
with OTP 29.0.5 and a fresh release build of the repaired worktree over `c29c04c`.

The defect was in `lower_expr_case`, not in the upstream Thoas implementation.
Lowering the first case arm consumed the shared deferred-binary-tail metadata
when materializing or advancing a rest segment. Later arms retained the
variable binding without its definition. Code generation supplied zero for
that undefined register; the binary type check then dereferenced a null
pointer. The fix restores the incoming match-context state independently for
each arm and at the non-tail join.

`tests/binary_rest_case.erl` reproduces the defect without Thoas. Its second-arm
tail call crashed before the repair; the Rust lowering test also failed on an
undeclared variable before passes. Both now pass, including direct returns,
post-case reuse, nested arms, and an explicit completion marker. The corpus
and comparison rules were not weakened, and no Thoas source was modified.

The Linux differential CI step remains required. These local results do not
claim Linux execution or a successful remote CI run.

## Upstream EUnit and ExUnit validation

The separate upstream runner fetches the same pinned Thoas commit and executes
its **unmodified assertions**. It requires Python 3.9+, Git, Erlang/OTP, Elixir,
Mix with Hex installed, and a freshly built Nera compiler and matching runtime:

```sh
# Prefer a dedicated target directory when other tasks are also building Nera.
CARGO_TARGET_DIR=/tmp/nera-thoas-target cargo build --release --locked
python3 -m unittest discover -s tests/integration/thoas/upstream -p test_run.py
python3 tests/integration/thoas/upstream/run.py \
  --nera /tmp/nera-thoas-target/release/nera \
  --work-dir /tmp/nera-thoas-upstream
# Repeat with the downloaded sources/dependencies, without fetching:
python3 tests/integration/thoas/upstream/run.py \
  --nera /tmp/nera-thoas-target/release/nera \
  --work-dir /tmp/nera-thoas-upstream --offline
```

Use a dedicated work directory and one invocation at a time. The runner
recreates its own Mix source workspace and per-engine call directories there.
It refuses a work directory inside the Nera repository. Neither Thoas source,
its test suite, its JSON fixtures, nor StreamData is vendored. The pristine
checkout is verified at the pinned SHA before a separate external workspace
is prepared for Mix. `mix.lock` pins StreamData **0.5.0** and must remain
unchanged. Reports include source/dependency/harness hashes, compiler and
native executable hashes, the linked runtime hash, tool versions and commands.
The checkout commit is contextual metadata; a recorded fresh compiler build
is needed to associate the executable with that source revision.

The upstream inventory at this revision is:

- **36 EUnit assertions**, from `test/thoas_encode_test.erl`.
- **332 ExUnit tests**: 14 ordinary encoder/decoder tests and 318 JSONTestSuite
  fixtures (95 `y_`, 188 `n_`, 35 `i_`). The upstream expectations decide the
  implementation-dependent fixtures; the runner does not reinterpret them.
- **6 StreamData properties**, with seed 0, 100 trials each and one test at a
  time. The runner requires StreamData to be loaded so the upstream conditional
  cannot silently omit the property module. Trials are not additional test
  functions and do not constitute exhaustive validation.

### What executes on each engine

On BEAM, the original EUnit/ExUnit frameworks execute the original tests
against the original Thoas modules. In native mode the **same test bodies and
assertions still execute on BEAM**, but generated API proxies send each call
to a separate executable containing the unchanged Thoas sources compiled by
Nera. An explicit dispatch table invokes the public `thoas` and `thoas_encode`
functions. Arguments, return values and exception classes/reasons use ETF.
The wrapper never computes a Thoas result on BEAM in native mode.

This validates the native library against upstream assertions; it does **not**
claim that Nera can compile Elixir, ExUnit or EUnit. The runner also attempts
to compile the Erlang test module directly and records that outcome separately.
Upstream test expressions themselves are compiled by BEAM in adapted mode.

ETF copies binaries between processes, so it cannot preserve backing storage
or sub-binary sharing. The upstream `copying strings on decode` test therefore
cannot establish native memory-sharing semantics through this adapter. It
still executes, and its failure is reported; it is not skipped or counted as
passing. The remaining comparisons retain the exact upstream expectations,
including the spelling of encoded floats, unlike the 80-case corpus's JSON
semantic comparison.

Each native invocation has a 10-second timeout; each ExUnit test has a
300-second timeout to accommodate process startup and property generation.
Each complete engine run has a 30-minute timeout. A native crash/timeout is
reported as a test failure and other tests continue. Each request, response,
exit status and process output remains under `nera-calls/` for reproduction.
Replay a saved request with the same executable from that call's directory:

```sh
cd /tmp/nera-thoas-upstream/nera-calls/CASE_ID
/tmp/nera-thoas-upstream/build/native
```

`report.json`, `beam.stdout`, `nera.stdout`, their stderr logs, and per-ExUnit-test
`*-tests.tsv` files preserve the results. Missing summaries/results, unexpected
test counts, excluded/skipped tests, framework failures and failed assertions
all prevent success. No known failure is whitelisted. A nonzero command exit
is expected while blockers remain. CI runs the harness unit tests; the full
upstream run is currently a manual diagnostic, not a green required gate.

### Upstream result — 2026-10-11

Final local execution on macOS arm64, OTP **29.0.5**, Elixir **1.20.3**, using
an isolated release rebuild of Nera commit
`326814af210ed7923c69dc53c0baa644ac149dbd`:

| Suite | BEAM | Nera library through the adapter |
| --- | --- | --- |
| Upstream EUnit | 36/36 pass | 33/36 pass; 3 date/datetime assertions fail |
| Upstream ordinary ExUnit tests | 14/14 pass | 12/14 pass; float spelling fails, sharing test cannot be validated through ETF |
| Upstream JSONTestSuite | 318/318 pass | 318/318 pass |
| Upstream StreamData properties | 6/6 pass, 100 trials each | 6/6 pass, 100 trials each |
| Existing differential corpus | 80/80 pass | 80/80 agree with BEAM |

The adapted native run completed **1,665 API calls**, with no native crash or
transport timeout. ExUnit reports 336/338 successes, two failures and zero
skipped/excluded tests; EUnit reports 33 successes and three failures. The
runner correctly exits **1**. These results do not establish full Thoas
compatibility.

Four blockers remain, with two demonstrated runtime behaviour differences:

1. **Date and datetime padding:** all three EUnit `types_test_` assertions fail.
   For example, `2023-08-07` becomes `2023-8-7`, and `19:02:24` becomes
   `19:2:24`. A standalone probe without Thoas reproduces the difference with
   `io_lib:format("~4..0b-~2..0b-~2..0b", [2023,8,7])`.
   `nera_rt_io_lib_format` in `crates/nera-runtime/src/ffi/io_format.rs` skips
   the width/precision/padding text instead of applying it.
2. **Short float spelling:** upstream expects `"9.9e100"`; Nera emits the
   expanded decimal with a trailing `.0`. A standalone
   `float_to_binary(9.9e100, [short])` probe reproduces the difference.
   The short branch of `nera_rt_float_to_binary_2` in
   `crates/nera-runtime/src/ffi/binary_ops.rs` uses Rust's decimal display.
   The result is valid JSON, which explains why a semantic JSON comparison
   can pass while the upstream exact string assertion fails.
3. **Binary sharing:** the upstream `copying strings on decode` assertion
   fails because ETF copies the returned binaries. This is an adapter
   limitation, not evidence that Thoas returns an incorrect decoded value.
   A separate native probe also fails to compile
   `binary:referenced_byte_size(<<0>>)`: that primitive is unresolved.
   Native backing-storage semantics therefore remain unvalidated.
4. **Direct framework compilation:** compiling the original EUnit test with
   Nera fails on `stdlib/include/assert.hrl`, followed by undefined assertion
   macros. The successful adapted tests do not remove this tooling blocker
   or establish support for compiling the Elixir suite with Nera.

The final compiler SHA-256 is
`4cbf7e9b1e5af4292da153da5ddba9f20f0ac5e8091f0db1fc3836391c964571`;
the linked runtime SHA-256 is
`936955ed0f9fd75ca79b69029abd3893492e71524a78b2e32d1192e405e65786`.
The source archive/build log and a separate `compiler-provenance.json` bind
these artifacts to the commit above. Exploratory executions using the shared
build directory had a different compiler hash and are superseded by this
isolated rebuild; their crashes are not attributed to the validated commit.

The harness's nine unit tests and the existing comparator's fourteen tests
pass. The full upstream validation remains a diagnostic with known failures;
no upstream assertion or Thoas source was changed to obtain a green result.


### Date-padding correction — 2026-10-11

The date blocker above is resolved in the follow-up runtime change.
`io_lib:format/2` now applies width, radix and padding for integer `~b`/`~B`
fields, including the zero padding used by Thoas. It handles left alignment,
negative values, narrow/zero-width fields, custom padding and dynamic fields.
The Rust regression first failed with `2023-8-7T19:2:24.65432Z`; it now returns
`2023-08-07T19:02:24.065432Z`. `tests/io_lib_integer_format.erl` independently
passes on both BEAM and Nera.

All **36 original upstream EUnit assertions now pass on both engines**, up
from 33/36 on Nera: all three date/datetime failures are fixed. This follow-up
ran the EUnit portion of `upstream/launch.exs`, with the same unchanged upstream
BEAM test module and the same native transport. It did not rerun ExUnit,
JSONTestSuite or properties; their previous results above remain historical.
The differential corpus was rerun and still agrees on **80/80** cases.

The runtime's **451 unit tests and 5 doctests pass**, as does Clippy for all
runtime targets with warnings denied. Socket tests required permission to open
local sockets. No full compiler-workspace suite or Linux execution is claimed.
The float, binary-sharing and direct native EUnit blockers remain open.

[Recorded evidence](date-padding-evidence.json) binds these results to the
runtime source and binary hashes. The frozen compiler from `326814a` was reused
with a freshly rebuilt release runtime from the modified worktree over
`4b35bf7`; compiler sources did not change. Full logs are locally retained in
`/private/tmp/nera-date-validation`, with the corpus report in
`/private/tmp/nera-date-corpus/report.json`.

To reproduce with a fresh compiler/runtime pair, use the dedicated-target
build and upstream runner commands above (the complete runner is still
expected to report the unrelated ExUnit failures), and run:

```sh
CARGO_TARGET_DIR=/tmp/nera-thoas-target cargo test -p nera-runtime --locked
/tmp/nera-thoas-target/release/nera test io_lib_integer_format
python3 tests/integration/thoas/differential/runner.py \
  --nera /tmp/nera-thoas-target/release/nera \
  --work-dir /tmp/nera-thoas-corpus
```


### Publication revalidation on current main

The PR was rebased onto `e046d620f7e6e1f552c30caf74fff0c453b3bcf4`.
The compiler and adjacent runtime were rebuilt in release mode from the
rebased sources (`0a150dc`, tree `3d3925f5280db12d21378cd43e8acc4baf573a62`).
The original pre-rebase evidence above remains historical; the
`publication_revalidation` entry in [the evidence file](date-padding-evidence.json)
records the new binary hashes and checks. Subsequent changes only update
this evidence and documentation.

On this base, the full Rust workspace passes **1,775 tests including doctests**,
with one opt-in cancellation-scaling benchmark ignored and no failures.
Workspace Clippy with all targets and warnings denied passes, as do the
**23 harness unit tests**. The upstream EUnit rerun passes **36/36 on each
engine**, and the differential corpus agrees on **80/80** cases.
The complete ExUnit/property/JSONTestSuite run was not repeated; the same
remaining limitations apply. Linux execution is left to PR CI.

The complete Erlang suite (`nera test -j 1 --timeout 30`) also passes:
**294 passed, zero failures, two skipped** out of 296. Both skips require
external OneCached modules. The new date-format regression passes in this
normal test runner. Its log is `/private/tmp/nera-date-pr-e2e.log`.


### Short-float correction — 2026-10-11

The `float_to_binary(Value, [short])` blocker is resolved in the follow-up
worktree over `1e7b5a4`. Rust's default float display expanded `9.9e100` into
fixed decimal notation and differed from BEAM on some halfway rounding cases.
The runtime now uses Ryū 1.0.23 for nearest, ties-to-even shortest digits and
applies Erlang's notation rules: prefer the shorter spelling, fixed on ties,
and always use scientific notation when the absolute value is at least `2^53`.
Both forms retain a decimal point and negative zero is preserved. See the
[Erlang formatting contract](https://www.erlang.org/doc/apps/erts/erlang.html#float_to_list/2).
Only Ryū was added to the dependency lock; existing versions are unchanged.

The minimal Rust FFI test and `tests/float_to_binary_short.erl` both failed
before the correction and pass afterwards. Boundary checks cover the notation
thresholds, `2^53`, halfway rounding, subnormals and the largest finite float.
An additional exact-text comparison agrees on **21,889 finite binary64 values**
between BEAM and the native FFI path. This is a deterministic sample, not an
exhaustive proof: Python `Random(0)` generates 20,000 bit patterns, supplemented
by each power of ten from `10^-323` through `10^308` and its two bit neighbours;
non-finite patterns are removed. Values are passed to Nera as an ETF list.

Targeted upstream results on macOS arm64 / OTP 29.0.5:

| Scope | BEAM | Nera |
| --- | --- | --- |
| EUnit assertions | 36/36 | 36/36 |
| Unchanged upstream `encode_test.exs` | 7/7 | 7/7 |
| Unchanged float roundtrip property | 100 trials pass | 100 trials pass |
| Existing differential corpus | 80/80 | 80/80 |

The targeted ExUnit run loads `encode_test.exs` and `property_test.exs` unchanged,
with seed 0, `max_runs: 100`, one concurrent test and these explicit filters:
`exclude: [:test]`, `include: [module: Jason.EncoderTest,
test: :"property float roundtrip"]`. It reports **8 passed, 5 excluded**, zero
failures and zero skips per engine. The five other properties were deliberately
outside this targeted run. EUnit/ExUnit still execute on BEAM and call native
Thoas through the existing ETF transport. No Thoas code or assertions were
copied into Nera or modified.

The runtime's **471 tests and 8 doctests pass**, with one opt-in scaling
benchmark ignored; runtime Clippy passes for all targets with warnings denied.
The full Erlang suite, complete ExUnit/JSONTestSuite and Linux execution were
not repeated for this focused fix. The remaining known blockers
are binary-sharing validation / `binary:referenced_byte_size/1` and direct
native EUnit include resolution. No claim of full Thoas compatibility is made.

[Recorded evidence](float-short-evidence.json) preserves source/binary/input
hashes and local log locations. The compiler from the preceding publication
validation is reused with a freshly built adjacent release runtime. To reproduce
with a fresh pair, use the dedicated build and upstream runner commands above;
the full runner also exercises the remaining memory-sharing failure. The focused
runtime regression can be run with:

```sh
CARGO_TARGET_DIR=/tmp/nera-thoas-target cargo test -p nera-runtime --locked
/tmp/nera-thoas-target/release/nera test float_to_binary_short
```

For publication, the branch was based on main `e15bd09`, whose tree is identical
to the original validation base `1e7b5a4`. The source hashes above were rechecked.
The full workspace passed **1,777 tests including doctests**, with zero failures
and one opt-in benchmark ignored; workspace Clippy with all targets and warnings
denied also passed. These checks are recorded under `publication_checks` in the
evidence file. The targeted Thoas/native results apply unchanged.

## Memory sharing validation — 2026-10-11

`memory/run.py` compiles the same pristine, pinned Thoas 1.2.1 sources on both
engines. Our own `thoas_memory.erl` probe checks contents and measures retained
storage **inside each engine**, before printing scalar results. No decoded
binary crosses ETF transport. Neither upstream sources nor upstream tests are
copied into this repository; the runner requires an external working directory.

```sh
CARGO_TARGET_DIR=/tmp/nera-memory-target cargo build --release --locked
python3 tests/integration/thoas/memory/run.py \
  --nera /tmp/nera-memory-target/release/nera --work-dir /tmp/nera-memory-check
# Repeat without fetching:
python3 tests/integration/thoas/memory/run.py \
  --nera /tmp/nera-memory-target/release/nera --work-dir /tmp/nera-memory-check --offline
```

The input is explicitly detached from builder capacity: 210 bytes of JSON,
with a 101-byte key and a 102-byte value. Both engines produce:

| Options | Key retained bytes | Value retained bytes |
| --- | ---: | ---: |
| Default/reference | 210 | 210 |
| Keys and strings copied | 101 | 102 |
| Keys copied | 101 | 210 |
| Strings copied | 210 | 102 |

The runner also compiles and executes `tests/binary_referenced_byte_size.erl`
on both engines, checking direct/dynamic calls, remote funs, nested views,
64/65-byte slicing, copy detachment, empty binaries, bitstrings and errors.
Rust regressions check that a nested view survives full GC and that copying
allows the original backing allocation to be freed.

This closes the previously unobservable Thoas copying behavior for these
scenarios. The original ExUnit copying assertion still cannot measure native
sharing through ETF; it is neither skipped nor declared passing by this probe.
Native EUnit compilation on `stdlib/include/assert.hrl` remains a separate
blocker. The full upstream suite was not rerun for this change.

[Recorded evidence](memory-evidence.json) includes engine/source hashes, commands
and observed sizes. Validation used a freshly built debug compiler with an
adjacent release runtime; the commands above build both in release mode.

Checks on the final memory-change worktree: **1,779 workspace Rust tests pass**,
with one opt-in benchmark ignored; workspace Clippy with warnings denied passes;
**19/19** targeted native Erlang binary tests pass. The memory runner passes all
four Thoas scenarios and the standalone regression on each engine.
The **80/80 differential corpus cases** also agree on BEAM and Nera with the same
compiler/runtime pair. The existing harness unit tests pass (14 differential,
9 upstream). No full native EUnit verdict is implied.

## Native EUnit preprocessing — 2026-10-11

The child preprocessor now inherits the `include_lib` application resolver.
Previously `eunit.hrl` was found but its nested `stdlib/include/assert.hrl`
include failed. A synthetic regression reproduces that exact error without
requiring OTP. A second fix keys macro definitions by name and arity, preventing
`assertEqual/3` from replacing `assertEqual/2`; `-undef` removes every overload.

The project-owned `eunit/header_probe.erl` compiles and prints
`EUNIT_HEADERS_OK` on both BEAM and Nera with the real OTP 29.0.5 headers.
It checks header loading only, **not EUnit execution**. The unchanged pinned
Thoas test module now reaches another unsupported preprocessor construct:
`??Expression` (macro argument stringification). Direct compilation reports
`undefined macro: ?`; **zero native EUnit assertions have been executed**.
The full EUnit framework and its parse transforms are not validated by this fix.

Reproduce from an external directory (to avoid the repository's project-wide
source discovery), using the existing pristine pinned checkout:

```sh
# Set absolute paths for your checkout, freshly built compiler, and pinned Thoas.
nera_repo=/absolute/path/to/nera
nera_bin=/absolute/path/to/nera/target/release/nera
thoas_src=/tmp/nera-thoas-validation/upstream
mkdir -p /tmp/nera-eunit-probe
cd /tmp/nera-eunit-probe
"$nera_bin" build "$nera_repo/tests/integration/thoas/eunit/header_probe.erl" \
  -e header_probe:test -o header-probe
./header-probe  # must print EUNIT_HEADERS_OK
erlc -o . "$nera_repo/tests/integration/thoas/eunit/header_probe.erl"
erl -noshell -pa . -eval 'header_probe:test(), halt().'
# Currently blocked on ??Expression, not include resolution:
"$nera_bin" build "$thoas_src"/src/*.erl "$thoas_src"/test/*.erl \
  -e thoas_encode_test:test -o direct-eunit
```

[Evidence](eunit-preprocessing-evidence.json) records source/engine fingerprints,
commands, the new diagnostic and checks. No Thoas or OTP source is vendored.
The 80-case corpus and full upstream suite were not rerun for this preprocessing
change; earlier results remain historical evidence for their recorded versions.

Checks: **1,782 Rust tests pass** (one benchmark ignored), workspace Clippy
passes, and both targeted native Erlang regressions pass. After strengthening
the undef test, all **293 syntax tests** pass again. Implementation is unchanged.

## Macro stringification — 2026-10-11

Nera now expands `??Parameter` inside a function-like macro to the textual form
of the argument, without evaluating it or expanding macros inside the quoted
argument. Regression cases compare BEAM and Nera for token spacing, integers
in another base, quoted atoms, escaped strings/chars, exponent spelling, binary
commas, comments, and using the same parameter as both text and executable code.
A Rust regression covers a forwarding macro from nested headers.
The formatter also preserves quotes around reserved atoms such as `'case'`;
the new fixture exposed its previous invalid unquoted output.

The project-owned `eunit/assertion_probe.erl` loads the **real OTP assert.hrl**,
runs a successful assertion and catches a deliberately failing assertion.
It verifies `expression = "40 + 2"`, `expected = 43`, and `value = 42`, then prints
`OTP_ASSERTIONS_OK` on both BEAM and Nera. These assertions execute in each VM;
there is no transport and no vendored OTP or Thoas implementation.

From an external working directory, using the absolute compiler/repository and
pinned Thoas paths defined in the preceding section:

```sh
"$nera_bin" build "$nera_repo/tests/integration/thoas/eunit/assertion_probe.erl" \
  -e assertion_probe:test -o assertion-probe
./assertion-probe  # require OTP_ASSERTIONS_OK
"$nera_bin" build "$nera_repo/tests/macro_stringify.erl" \
  -e macro_stringify:test -o macro-stringify
./macro-stringify  # require ok
erlc -o . "$nera_repo/tests/macro_stringify.erl" \
  "$nera_repo/tests/integration/thoas/eunit/assertion_probe.erl"
erl -noshell -pa . -eval 'macro_stringify:test(), assertion_probe:test(), halt().'
"$nera_bin" build "$thoas_src"/src/*.erl "$thoas_src"/test/*.erl \
  -e thoas_encode_test:test -o direct-eunit
```

The final command **still fails**, but preprocessing now succeeds. The next
observed blocker is unsupported `-import`: Thoas's `null/0` and `string/1` calls
are reported undefined. Full native EUnit execution, its autoexport transform,
and the 80-case corpus are not validated by these targeted checks.
[Recorded evidence](stringification-evidence.json) includes source/binary hashes,
checks, the successful probe markers and the remaining compilation diagnostic.

Validation: **1,784 workspace Rust tests pass** (one benchmark ignored),
workspace Clippy passes, all three targeted Erlang tests pass, and the formatted
stringification regression still executes successfully. A final syntax/formatter
rerun follows a Clippy-only conditional simplification; behavior is unchanged.

## Explicit imports — 2026-10-11

Nera resolves `-import(Module, [Name/Arity])` calls, including tail calls and
imports that override automatic BIF imports (`float/1` in Thoas). Duplicate
imports and conflicts with local functions are rejected. Like BEAM, Nera
rejects `fun Name/Arity` for imported names: use `fun Module:Name/Arity`.
Imported non-Erlang functions cannot be used as guard BIFs. `true()` and
`false()` are now parsed as static calls, as are quoted atom calls.

The project-owned `eunit/import_probe.erl` runs assertions directly in each
engine against **unmodified external Thoas 1.2.1 sources**, pinned at
`7cdcec7e52b80d2869900893b2acf47199d84190`. It checks the imported boolean/null
encoders, string encoding and `float/1`, including a qualified fun reference.
Require `THOAS_IMPORTS_OK`, not merely exit status zero.

From a fresh external working directory, set `nera_bin`, `nera_repo`, and
`thoas_src` to absolute paths (the compiler needs its matching runtime archive):

```sh
"$nera_bin" build "$thoas_src"/src/*.erl \
  "$nera_repo/tests/integration/thoas/eunit/import_probe.erl" \
  -e import_probe:test -o import-probe
./import-probe # require THOAS_IMPORTS_OK
erlc -o . "$thoas_src"/src/*.erl \
  "$nera_repo/tests/integration/thoas/eunit/import_probe.erl"
erl -noshell -pa . -eval 'import_probe:test(), halt().'
python3 "$nera_repo/tests/integration/thoas/differential/runner.py" \
  --nera "$nera_bin" --work-dir "$PWD/corpus" # clones the pinned source externally
"$nera_bin" build "$thoas_src"/src/*.erl "$thoas_src"/test/*.erl \
  -e thoas_encode_test:test -o direct-eunit
```

The last command still fails with `undefined variable: 'Expected'` at upstream
line 35. The same failure occurs with this independent minimal expression:

```erlang
[F] = [fun() -> (fun() -> Expected end)() end || Expected <- [42]],
42 = F().
```

It passes on BEAM. Native Thoas EUnit execution therefore remains **blocked**;
the autoexport transform and EUnit runner are not validated. No upstream source
or OTP implementation is vendored. See [recorded evidence](import-evidence.json)
for exact source/binary hashes, regression results and the remaining diagnostic.

Validation on macOS arm64 / OTP 29.0.5: **80/80 differential cases agree**,
`THOAS_IMPORTS_OK` is present on both engines, both targeted native regressions
pass, and **1,791 workspace Rust tests pass** (one ignored). Workspace Clippy
passes with warnings denied. Validation was performed on the import worktree over `7c45485`.

## Nested closure capture — 2026-10-11

The free-variable collector previously traversed call arguments but skipped
`CallTarget::Expr`, the callee expression. Thus `(fun() -> Expected end)()`
inside another closure did not capture `Expected` from its comprehension.
The minimal lowering regression failed with `UndefinedVariable("Expected")`
before the fix and passes afterward. `tests/closure_callee_capture.erl` also
checks that two iterations retain distinct values (42 and 99) on both engines.

The pristine upstream test module now compiles with Nera. Compilation alone
is not evidence that EUnit ran: the EUnit autoexport transform and runner remain
unvalidated. To run the assertion bodies directly, use the project-owned adapter:

```sh
python3 tests/integration/thoas/eunit/run_assertions.py \
  --upstream /absolute/path/to/clean/pinned/thoas \
  --nera /absolute/path/to/nera \
  --work-dir /absolute/external/validation/directory
```

The script verifies the upstream commit, runs the original EUnit suite on BEAM,
and writes a temporary test module outside both repositories. It preserves the
upstream test bodies, adds one export and a driver, then invokes the five simple
tests and all generated assertion closures in separate Nera processes. This
keeps one failed assertion from affecting later results. The final count must be 36;
missing completion, unexpected output or any failed assertion makes the script
fail. This validates native execution of assertion bodies, **not the full EUnit
framework**. No source is copied into our repository or changed in the checkout.

Native execution exposes a separate binary-literal discrepancy in the emoji
string assertion: BEAM evaluates `binary_to_list(<<"🤔">>)` as `[20]`, while
Nera returns `[240,159,164,148]`. The original test expects the default 8-bit
segment semantics. This issue is independent of closure capture; Thoas is not
yet fully compatible. See [capture evidence](capture-evidence.json).

Recorded result on macOS arm64 / OTP 29.0.5: **36/36 BEAM EUnit tests pass**;
**26/36 native assertion bodies pass**. Failed indices (one-based driver order)
are 21 (emoji string), 27–29 (non-recursive objects), 31–33 (encode), and 34–36
(types). The nine object-related cases raise `badmatch`; their root cause is
not established by this capture fix. Each case runs in a fresh OS process and
requires its own `THOAS_ASSERTION_OK:<index>` marker and zero exit status.
The script exits 1 for this recorded result, preserving all per-case logs.

The **80/80 differential corpus** still agrees with BEAM. **1,792 workspace Rust
tests pass** (one ignored), Clippy passes with warnings denied, and the native
capture regression prints `ok` on both engines. No complete native EUnit or full
ExUnit validation is claimed.

## Automated regression gates

Linux CI runs the four direct memory-sharing/copy scenarios, real OTP header
and assertion probes, and **all 36 native assertion bodies** using OTP 29.0.5.
The native assertion runner now runs in strict mode: every case must exit zero
and print its exact indexed completion marker. The former ten known-failure
entries have been removed; `eunit/known_failures.json` is empty.

The optional `--check-known-failures` mode remains available for investigating
future scoped baselines, but CI does not use it. Six harness tests verify that
missing/wrong markers, crashes, changed failure reasons and unexpected successes
cannot silently pass. The existing 23 harness tests remain. Reports and logs
are uploaded even when a CI check fails.

```sh
python3 tests/integration/thoas/memory/run.py \
  --nera /absolute/path/to/nera --work-dir /external/thoas-memory
python3 tests/integration/thoas/eunit/run_probes.py \
  --nera /absolute/path/to/nera --work-dir /external/thoas-otp
python3 tests/integration/thoas/eunit/run_assertions.py \
  --upstream /external/thoas-memory/upstream --nera /absolute/path/to/nera \
  --work-dir /external/thoas-assertions
python3 -m unittest discover -s tests/integration/thoas/eunit -p 'test_*.py'
```

Sources stay pinned and external. The adapter preserves the original test
bodies, adds an entry point outside both repositories, and runs each assertion
in a fresh Nera process. It does **not** validate the native EUnit framework.

## Native assertion corrections — 2026-10-11

The original EUnit suite passes **36/36 on BEAM**, and the external adapter now
passes **36/36 assertion bodies directly in Nera**, with no failure exemptions.
Two independently reproduced defects accounted for all ten previous failures:

- `<<"🤔">>` used UTF-8 source bytes instead of one codepoint truncated to 8 bits.
  Nera now produces `<<20>>`, matching BEAM; `<<"🤔"/utf8>>` retains UTF-8 bytes.
- A global trampoline re-entry guard deferred nested dynamic calls that needed
  to complete before the caller continued. The caller then received an internal
  escaping result instead of the complete encoded object. Tail-loop dispatch
  now leaves tail calls pending for its loop, while ordinary calls resolve their
  own chains before returning. The fix covers closure calls, remote fun refs
  and dynamic apply without making terminal call chains recursively grow the stack.

`tests/binary_string_codepoints.erl` covers the binary-literal correction.
`tests/nested_dynamic_tail_call.erl` checks retained caller results, dynamic
remote calls, exception propagation and 100,000 terminal closure calls. Both
run against BEAM and Nera; isolated Rust regressions failed before each fix and
pass afterward. [Recorded evidence](native-assertion-fixes-evidence.json) includes
compiler/runtime hashes and the final validation counts. The full ExUnit suite,
native EUnit runner and autoexport transform remain outside this verdict.

Final local validation: **1,794 Rust tests pass** (one ignored), workspace Clippy
passes, **29 harness tests pass**, and **15 targeted native tests pass**. The
**80/80 differential cases**, four direct memory cases and both OTP probes also
pass. These are the historical local macOS arm64 results. Linux CI subsequently
passed on PR 27; see the current validation summary at the top of this document.
