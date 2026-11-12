# Native PropEr validation

## Initial bootstrap — 2026-10-15

The first native bootstrap exercises the **real `proper:quickcheck/2` engine**
with literal true and false properties. PropEr and OTP sources remain outside
this repository. This initial result did not validate generation or shrinking;
the next milestone is recorded below. Cowlib properties under native PropEr
remain outside both scopes.

Pinned dependencies:

- PropEr **1.5.0**, commit `5640d0715d4b346676267504d8e84398e2a29f75`.
- OTP **27.3 `rand` source**, commit
  `05737d130706c7189a8e6750d9c2252d2cc7987e`, SHA-256
  `7f956b3658f5b2054fe5809b7db1221e76109339a27ca728184d5a90fc679b0d`.
- Build tools / reference VM: OTP **29**; recorded platform: macOS arm64.

BEAM explicitly loads the same pinned `rand` source used by Nera, rather than
silently comparing two different OTP implementations. Its other standard
library modules come from the installed OTP. The native executable uses Nera's
runtime and standard library plus the prepared external dependencies.

| Check | BEAM | Nera |
| --- | --- | --- |
| `proper:quickcheck(true, [quiet, {numtests, 3}])` | `true` | `true` |
| `proper:quickcheck(false, [quiet, {numtests, 3}])` | `false` | `false` |

These properties contain no generated values, so their verdicts do not depend
on a random seed. They exercise initialization, real property execution and
cleanup. Both results and one final completion marker are required; a crash,
timeout, duplicate/missing result or process error never counts as rejection
of a false property. The runner clears `NUMWORKERS` and Erlang option variables
so ambient settings cannot change the workload.

The local checks pass: release build, Clippy with warnings denied, **1,974 Rust
checks** (one release-only measurement ignored), and **365 native Erlang tests**
(two existing Onecached exclusions). The seven new Erlang regression modules
also pass on BEAM; the two PropEr harness tests and 65 existing Cowlib harness
tests pass. An earlier TCP throughput check failed during concurrent Rust
builds; it passes in the final complete native run after those builds finished.

See [bootstrap evidence](bootstrap-evidence.json) for the exact candidate,
binary hashes, outcomes and regression checks. Other platforms are untested.

## Seeded native generation and shrinking — 2026-10-15

Based on merged main `4e14b65` (PR #97), the project-owned probe now executes
real PropEr generators and shrinkers in the native executable. It supplies no
BEAM-generated values to Nera. Both engines use the same external OTP 27.3
`rand` **and `ordsets`** sources at the revision above; `ordsets.erl` has SHA-256
`064acc495f9a5ddf0c1bdc9d97fa7d1b1b975b10076495dbce4babf73900f17d`.

For each seed `{101,202,303}`, `{7,11,13}`, `{999,1001,2027}`, the probe runs
the following twice, cumulatively in one process:

- 100 generated tuples containing a bounded integer, a list of bytes, and a
  variable-length binary. Their types, ranges, binary round-trip and variation
  are checked independently of engine agreement.
- A deliberately false integer property (`X < 10`), reduced to `[10]`.
- A deliberately false list property (`length(L) < 3`), reduced to `[[0,0,0]]`.
- A deliberately false binary property (`byte_size(B) < 3`), reduced to
  `[<<0,0,0>>]`.
- `proper:check/3` replay: each minimized counterexample must still fail;
  its adjacent passing example (9, a two-element list, a two-byte binary)
  must pass.

The public `?SETUP` mechanism (`proper:setup/2`) seeds `rand:seed(exsplus, Seed)`
after PropEr initialization. Each repeated seed must reproduce the entire
trace, including shrink attempts. This exercises **600 generated composite
instances, 18 failing/shrinking quickchecks and 36 replays per engine**. There
are 300 distinct seeded trial positions, repeated twice, rather than 600
independent random trials. The initial failing integer values are 83, 56, 77;
list/binary lengths are 10, 12, 7. All differ from their reduced examples.
The driver asserts this difference, so an unshrunk initial failure cannot pass.

The comparison decodes typed ETF and checks complete generated and shrink
traces across BEAM and Nera. It rejects crashes, timeouts, process errors,
missing/duplicate results, unexpected types/ranges, absent reduction and wrong
minima. Repetition is asserted inside each executable. All third-party source,
preprocessed source, manifests, BEAM artifacts and native binaries stay in the
external work directory.

The first native list shrink failed with `error:undef` for external
`ordsets:from_list/1`. Adding the original OTP module as a normal dependency
revealed missing `lists:sublist/2`; it is now implemented in Nera's own Erlang
stdlib. A minimal regression was confirmed failing before that implementation
and passing after it. Its BEAM comparison covers direct/dynamic calls, a remote
fun, zero/excessive lengths, heterogeneous terms, improper-list prefixes,
invalid arguments and a 100,000-element list. `lists:sublist/3` is not added.

Final local checks pass: release build, Clippy with warnings denied,
**1,977 Rust tests** (one existing ignored measurement), **372 native Erlang
tests** (two existing Onecached exclusions), **6 PropEr harness tests** and
**69 Cowlib harness tests**. The initial true/false native bootstrap still passes
with the extended runner. No CI workflow was enabled or changed.

See [generation evidence](generation-evidence.json) for the final release run,
source/binary hashes, seeds, trace digests, shrink results and regression checks.
This is a bounded native-engine milestone, not full PropEr compatibility or a
native rerun of the Cowlib property suites.

## Explicit BEAM preparation

PropEr's `proper_gen` uses the `vararg` parse transform to generate functions of
variable arity. Nera does not execute arbitrary OTP parse transforms itself.
The explicit, generic build helper
[`prepare_erlang_dependency.py`](../../../scripts/prepare_erlang_dependency.py)
uses standard **`erlc -P`** after building the original transform modules on
BEAM. It writes the resulting Erlang sources and a hash manifest into a
separate external directory. It does not rewrite PropEr-specific functions.
Nera consumes that directory as an ordinary project path dependency.

This requires BEAM **at build time**. The generated executable runs on Nera.
Preparation is opt-in (`--beam-parse-transforms` in the integration runner);
ordinary Nera builds do not silently invoke BEAM. The raw-source attempt is
also recorded: it still stops on untransformed `'MAKE_FUN'/3`. That failure is
not suppressed or passed off as a successful raw-source compilation.
The explicit `defer_undefined = true` mode remains necessary for unused remote
APIs in dependency modules. Missing local calls remain compilation errors.

The helper is reusable independently of PropEr:

```sh
python3 scripts/prepare_erlang_dependency.py \
  --source /tmp/my_external_dependency \
  --beam-path /tmp/my_external_dependency/_build/default/lib/my_external_dependency/ebin \
  --output /tmp/my_prepared_dependency
```

The dependency and its transform modules must already have been built on BEAM.
The output must be separate and empty. Use its path in normal configuration:

```toml
[project]
name = "my_property_project"
defer_undefined = true

[deps]
proper = { path = "/tmp/prepared-proper" }
otp_random = { path = "/tmp/prepared-otp-random" }
```

## Reproduce

Requires Erlang/OTP, `erlc`, `rebar3` and the pinned Rust toolchain. Keep all
third-party sources and generated sources outside the Nera repository:

```sh
git clone https://github.com/proper-testing/proper.git /tmp/nera-proper-src
git -C /tmp/nera-proper-src checkout 5640d0715d4b346676267504d8e84398e2a29f75
mkdir -p /tmp/nera-otp-rand
curl --fail --location \
  https://raw.githubusercontent.com/erlang/otp/05737d130706c7189a8e6750d9c2252d2cc7987e/lib/stdlib/src/rand.erl \
  --output /tmp/nera-otp-rand/rand.erl
shasum -a 256 /tmp/nera-otp-rand/rand.erl
cargo build --release --locked
python3 tests/integration/proper/native.py \
  --proper /tmp/nera-proper-src \
  --otp-rand /tmp/nera-otp-rand/rand.erl \
  --nera "$PWD/target/release/nera" \
  --beam-parse-transforms \
  --work-dir /tmp/nera-proper-native-run
python3 -m unittest discover -s tests/integration/proper -p 'test_*.py'
```

For the generation/shrinking milestone, fetch `ordsets` from the same OTP pin
and add the explicit generation flags (use a fresh external work directory):

```sh
curl --fail --location \
  https://raw.githubusercontent.com/erlang/otp/05737d130706c7189a8e6750d9c2252d2cc7987e/lib/stdlib/src/ordsets.erl \
  --output /tmp/nera-otp-rand/ordsets.erl
shasum -a 256 /tmp/nera-otp-rand/ordsets.erl
python3 tests/integration/proper/native.py \
  --proper /tmp/nera-proper-src \
  --otp-rand /tmp/nera-otp-rand/rand.erl \
  --otp-ordsets /tmp/nera-otp-rand/ordsets.erl \
  --nera "$PWD/target/release/nera" \
  --beam-parse-transforms --generation \
  --work-dir /tmp/nera-proper-generation-run
```

Verify both source hashes against the pins above. PropEr must be at the exact pin
with a clean checkout. The runner cleans and rebuilds its BEAM artifacts,
prepares both dependencies externally, resolves them via `nera deps get`, then
builds and runs the native probe. The work directory must be empty to prevent
stale results. `report.json` contains commands, diagnostics, source manifests,
binary hashes and separate raw/prepared outcomes. Exit 0 means both engine
controls completed successfully; incomplete/blocked attempts exit 1.

## Nera prerequisites covered by this candidate

- `?OTP_RELEASE` comes from the selected installation's
  `releases/start_erl.data`, including header and editor preprocessing.
  `-if(...)` supports integer-literal comparisons and boolean operators,
  expanded macros and nested inactive branches. Other constant guards and
  `-elif` are rejected; full Erlang conditional evaluation is not claimed.
- `get/0` returns process-local key/value pairs without removing them. Tests
  cover compound values, replacement, erasure, preserved snapshots, dynamic
  calls and process isolation; order is not assumed.
- `bitstring_to_list/1` preserves a residual short bitstring. Its tests exposed
  and cover the corrected binary-copy path for non-byte-aligned counts and
  offsets, including preservation of destination prefix bits.
- `binary_to_integer/2` handles bases 2–36, signs and arbitrary-size integers,
  rejects invalid digits/whitespace/underscores/partial bitstrings, and has
  direct and dynamic-call regression coverage against BEAM.
- `erlang:unique_integer/0` uses a runtime-wide atomic counter. Tests verify
  uniqueness across Erlang processes; the options arity is not implemented.
- `os:timestamp/0` reports wall-clock time. `os:getenv/1` reads Unicode
  environment values with proper nested character-list names and rejects
  invalid names. Raw non-Unicode OS environment values remain unsupported;
  `getenv/0`, mutation APIs and general `os` compatibility are not claimed.

The tests use project-owned fixtures and ordinary Erlang assertions. The helper
has an actual synthetic parse-transform regression on BEAM; no third-party
source bodies are stored as test fixtures.

## Cowlib Base64url upstream properties — 2026-10-15

The original Cowlib `prop_identity/0` and `prop_identity_no_padding/0` now run
with native PropEr from merged main `68247b9` (PR #98). Three seeds repeated
twice yield **6,000 passing trials per property per engine**. Input, encoding
and false-control shrink traces match BEAM exactly; 12 false controls reduce to
the empty binary and 24 seeded replays verify their rejection and the original
properties' acceptance. The original RFC 7515 assertion passes as well.

No runtime or compiler change was needed. The shared runner now accepts a
project-owned scenario module for an external dependency and result protocol;
its original smoke and generation/shrinking modes remain supported. Cowlib's
generator and predicate are preserved, with only observation around the original
predicate and test-entry exports in the external prepared listing. See the
[Cowlib native property scope and reproduction](../cowlib/README.md#cow_base64url-with-native-proper--2026-10-15)
and [evidence](../cowlib/native-base64url-proper-evidence.json).

## Cowlib date upstream property — 2026-10-15

The original `cow_date:prop_http_date/0` also runs through native PropEr from
merged main `306f5fe` (PR #99): **6,000/6,000 trials on each engine**, three
seeds repeated twice cumulatively, with identical input/parsed-date traces.
All three original formats execute, independently checked against their date
fields. Six false controls shrink to the minimum date components and 12 seeded
replays distinguish them from the original passing property. No compiler,
runtime or stdlib change is needed. See the
[date scope and reproduction](../cowlib/README.md#cow_date-with-native-proper--2026-10-15)
and [evidence](../cowlib/native-date-proper-evidence.json).

## Cowlib HTTP header upstream properties — 2026-10-15

All 26 original `cow_http_hd` properties now run with native PropEr from main
`211b1fb` (PR #101) plus `math:sqrt/1`: **600/600 trials per property per engine**,
three seeds repeated twice, with matching input/verdict/shrink traces. This
exposed the missing square-root primitive used by PropEr's unbounded integer
generation. The scalar BIF is now implemented with regression coverage against
BEAM. False wrappers and retained-counterexample replays exercise all properties;
shrinking is capped at 100 steps without a global-minimality claim.
See the [HTTP native property scope](../cowlib/README.md#cow_http_hd-with-native-proper--2026-10-15)
and [evidence](../cowlib/native-http-hd-proper-evidence.json).

## Remaining work

1. Extend the native property scope to additional Cowlib modules and larger
   workloads; the 26 header properties above are a finite seeded validation.
2. Support zipped generators (`&&`) to compile the installed OTP 29 `rand`.
   OTP 28's source also uses them; OTP 27.3 is the explicitly pinned bootstrap
   dependency, not evidence of OTP 29 `rand` compatibility.
3. Assess additional runtime APIs only when an executed property needs them.
   Successful compilation with deferred remote calls does not prove those
   unused paths work.
