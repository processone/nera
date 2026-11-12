# Cowlib differential validation

This project-owned corpus exercises the public APIs of `cow_qs`, `cow_cookie`,
`cow_date` and `cow_base64url`, with partial `cow_http_hd`/`cow_http` coverage,
against BEAM and Nera. Cowlib **2.20.0** is pinned
at **c768a804565ff5b8178ed968a5921e469d6bd7b2**. No Cowlib source, headers,
upstream tests or dependencies are copied into this repository.

**Build-path note (2026-10-20):** the allocator now includes jemalloc's
upstream whitespace fix. A fresh locked release can use the default target
path even when the working copy contains spaces. The external target paths
below record historical campaign commands, not a current build requirement.
See [the build regression](../../../CONTRIBUTING.md#building).

## cow_http2_machine upstream EUnit — 2026-10-19

Starting point: freshly fetched merged main **`9d0cbd87116da9d056921d479c84a0236aba0e84`**
(PR #113, after the separate HTTP/2 stage). Cowlib remains **2.20.0** at
`c768a804565ff5b8178ed968a5921e469d6bd7b2`. Both external checkouts were fetched
freshly on this machine. No third-party source, test, header or dependency is
vendored. CI and Release remain suspended.

### Inventory and actual scope

The pinned `cow_http2_machine` exports **26 function/arities**:

- Lifecycle: `init/2`, `terminate/1`.
- Streams: `init_stream/2`, `init_upgrade_stream/2`, `reset_stream/2`.
- Input: `frame/2`, `ignored_frame/1`, `timeout/3`.
- Output: `prepare_headers/5`, `prepare_push_promise/4`, `prepare_trailers/3`,
  `send_or_queue_data/4`.
- Flow control: `ensure_window/2,3`, `update_window/2,3`.
- Observation/state: `get_connection_local_buffer_size/1`, `get_local_setting/2`,
  `get_remote_settings/1`, `get_last_streamid/1`, `set_last_streamid/1`,
  `get_stream_local_buffer_size/2`, `get_stream_local_state/2`,
  `get_stream_remote_state/2`, `is_remote_concurrency_limit_reached/1`,
  `is_lingering_stream/2`.

Its complete upstream functional suite is **one direct EUnit test**,
`terminate_removes_stray_timeout_messages_test/0`. It creates two real timers,
queues timeout messages with their references, calls `terminate/1` and checks
that neither matching message remains. Its original `collect_timeout_messages/2`
helper and all assertions remain verbatim in the external native preparation.
No functions are removed. This is **not validation of all 26 APIs**, connection
establishment, stream transitions, HPACK exchanges, flow control or full HTTP/2
protocol behavior. The original test does not wait five seconds to independently
prove timer cancellation; the project-owned runtime regressions below cover
actual cancellation and pending-work ownership separately.

There are **zero upstream PropEr properties** for this module at this pin,
including outside its source file. The report records `properties: []`.
Trials, seeds, generation and shrinking are **not applicable**, not passing
property results. No substitute random property suite was invented.

Runtime dependencies include `cow_hpack`, `cow_http`, `cow_http2`, `cow_http_hd`,
`cow_iolists`, Erlang BIFs, `lists`, `maps` and `queue`. The runner includes the
complete Cowlib production closure, including the HTTP helpers' dependencies.
`file:name_all/0` is a type reference; optional debug output is not enabled.
The machine has no include files; HPACK's two headers come from external
Cowlib `src`. EUnit **2.11** and `gb_trees` come from the external OTP
**29.0.4 / ERTS 17.0.4**, stdlib **8.0.3** installation. PropEr **1.5.0** at
`5640d0715d4b346676267504d8e84398e2a29f75` is a required pinned runner input,
but none of its code executes here.

### Results and blocker regressions

The original BEAM suite passes **1/1**. On unchanged main's compiler/runtime,
the extended native runner reaches **0/1**, one failure, zero skips. A minimal
isolated native probe reads `cancel_timer/1 = false` and
`cancel_timer/2 = {error,undef}`. Both new direct-call regression programs fail
compilation at the missing second arity before the fix and pass on BEAM.

Message timers previously returned untracked references and slept in
fire-and-forget threads; `cancel_timer/1` was a documented success-independent
stub. Both timer APIs now register their returned reference before dispatch.
Cancellation and publication share one per-timer lock. A winning cancellation
removes the registration, releases pending scheduler work immediately and wakes
the callback without delivering its message. A winning delivery retains pending
ownership through inbox publication and scheduler wakeup. A canceller that finds
the registration serializes with that delivery. Receive-timer cancellation
remains a separate mechanism.
Callback completion, unwinding and unstarted-handle cleanup remove registrations.

`cancel_timer/1` now uses the same real cancellation as `cancel_timer/2`.
The second arity validates boolean `async`/`info` options, including proper-list
validation and last-option precedence. Synchronous information is a remaining
millisecond count or false; `info=false` returns ok. Asynchronous calls return
ok, with `{cancel_timer, Ref, Result}` only when information was requested.
Cancellation may complete immediately even with `async=true`. Already queued
messages are not retracted. Direct calls, dynamic apply and remote funs share
the implementation; references copied to another process still identify the
timer. Resource references are not interpreted as ordinary timer IDs.

`cancel_timer_sync` and `cancel_timer_async` verify these observable contracts,
unknown/already delivered references, invalid arguments, retained queued
messages and unrelated real timer delivery, with explicit completion markers.
Three Rust regressions cover cancelled callback suppression, immediate
pending-slot release, delivery ownership and callback/drop cleanup. Cancellation
and cleanup checks are deterministic. The racing-cancel check observes a false
result and pending ownership during publication; it does not deterministically
prove that cancellation waits for publication.
The existing delayed-message liveness regressions remain passing. These checks
do not establish all Erlang timer APIs, distributed references, resource limits,
hard timing bounds or timer service scalability; callbacks remain thread-based.

The runner also has three failing-before regressions for public suite selection,
the production closure and accidentally removed callable test helpers. A fourth
regression obtains a real single-test EUnit transcript: OTP prints
`Test passed.`, not `All 1 tests passed.`. Both completion gates now accept the
singular form only for one expected test while still rejecting duplicated,
contradictory, malformed or wrong-count summaries and missing/duplicate markers.
The general framework controls execute actual deliberate failures, timeouts,
isolation and unsupported descriptors; reporter failure cannot pass them.

The corrected campaign passes **1/1 on BEAM and native Nera**, exit **0**,
without skipped/blocked assertions or semantic normalization. Native execution
uses project-configured `nera test` and real OTP EUnit, not BEAM at runtime or
an assertion-closure adapter. Raw transcripts remain separate; matching verdicts
do not imply byte-identical diagnostic/duration formatting. This completes the
available pinned upstream suite, not general machine/API coverage or a PropEr
campaign.

### Final campaign fingerprints

The final campaign uses **macOS 27 arm64**, Rust **1.99.0**, a separately frozen
Nera **0.1.0** release compiler and its matching runtime. Both locked workspace
release-build passes succeed, followed by an explicitly ordered compiler-only build. The first
installation probe passed behavior checks but exposed different embedded and
external archive hashes. The compiler-only build embeds the already finished
runtime; a fresh-cache installation confirms their hashes match exactly.
The final workspace gate passes **2,028 Rust tests**, zero failures,
one existing ignored measurement, and Clippy with warnings denied passes.
The first workspace run caught a missing signature in the new BIF metadata;
adding the actual signature corrects that failure without changing its test.

| Original workload | BEAM | Native Nera |
| --- | ---: | ---: |
| Upstream termination test | **1/1** | **1/1** |
| EUnit positive/negative/timeout/isolation controls | **Pass** | **Pass** |
| Available upstream PropEr properties | **None** | **Not applicable** |

SHA-256 fingerprints bind this base-plus-fix production snapshot to the final
campaign; the source manifest is the sorted JSON source-to-hash map.

| Artifact | SHA-256 |
| --- | --- |
| Final frozen Nera release compiler | `c1f2d651e09e53fbeee6ab9d90787b39c1b97c5add624a9966bd2c15be4fb5a1` |
| Matching runtime | `3b7bdf75038c04e44a403e677a10a575dddc68d8ef1f285526724e548cc39878` |
| Nera production-source manifest | `4303e01178c183636f24d2ef68d42dc33eccf5091bf34f51f2eca9b849fae1bd` |
| Original `cow_http2_machine.erl` | `5ff9d6c3c520ca26d0a0090cc5d67ba6dc918fe7ee557083754e1d703cc14d3d` |
| External native `cow_http2_machine.erl` | `2406ee624af55e1e96b849abab398d011b0a4c4c6671f4361d468b5cc6c8215c` |
| External `gb_trees.erl` | `890b641fa4e901c4f11bf65288ea12e8a8c0480d3a4867274f699fb4ef9c60a3` |
| EUnit source/header manifest | `8853e5e8dbde8f34d5d5136ef306dc59c9573a4546f6d9f06696df12826cca4e` |

### Local gate, adjacent regression and installation

The final serial native gate passes **393 tests**, zero failures, **two existing
Onecached exclusions / 395 total**, exit **0**, including `tcp_reactor_perf` and
both new cancellation fixtures. The exclusions are `ets_onecached_full_startup`
and `ets_onecached_pattern`, requiring absent external modules; they are not
successful validation. No other task-owned build or campaign ran alongside
this gate. The final compiler also reruns the adjacent `cow_http2` suite:
**3/3 on both engines**, with its framework controls passing.

With both real reports, the Cowlib harness runs **98 tests: 97 pass, one optional
real-HPACK-report test is skipped**. Without reports, **95 pass, three optional
real-report checks are skipped**; permanent rejection checks still execute.
All **six PropEr harness tests** pass; no new native PropEr campaign is implied.
Both cancellation fixtures also pass on BEAM. The unchanged main compiler
fails both fixtures at the missing second arity; the isolated runtime probe
changes from `false` / `{error,undef}` to a remaining-time integer / `ok`.

A manually assembled native install-layout archive passes `scripts/test-dist.py`
with the external library and with the library removed and an empty runtime
cache. Both new cancellation fixtures also compile and run in both modes.
The extracted embedded runtime has the **same SHA-256** as the final external
runtime above. This verifies the local layout and final artifacts, not execution
of the release packaging workflow. Generic cross-object smokes and both new
fixtures emit verified macOS x86_64 and Linux aarch64 objects; these are not
other-target runtime builds or executions. Linux/Windows execution and remote
CI remain unclaimed; no workflow was enabled or changed.

```sh
CARGO_TARGET_DIR=/external/nera-target cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_TARGET_DIR=/external/nera-target cargo test --workspace --locked --no-fail-fast
/external/nera-target/release/nera test -j 1 --timeout 30
python3 -m unittest discover -s tests/integration/proper -p 'test_*.py'
# The previous stage is also a regression of the shared plural-summary gate:
# repeat native_eunit.py below with --modules cow_http2 and a separate work-dir.
# The native archive here is assembled locally, not downloaded or published:
python3 scripts/test-dist.py /external/nera-local-aarch64-apple-darwin.tar.gz
bash scripts/test-cross.sh /external/nera-target/release/nera x86_64-apple-darwin
bash scripts/test-cross.sh /external/nera-target/release/nera aarch64-unknown-linux-gnu
```

Reports, raw stdout/stderr, full command lists and manifests remain in the task's
external evidence folder, not the project or pull request. Earlier dated stub
notes describe their earlier binaries; the cancellation results above supersede
that limitation only for the APIs and workloads actually checked here.

### Reproduction

Build with Rust **1.99.0** and locked dependencies. Use a space-free external
target path where jemalloc cannot accept spaces. All checkouts, prepared sources,
compiled artifacts and campaign reports must remain external. Use a fresh empty
work directory. No PropEr preprocessing or build-time parse transform is needed
beyond supported EUnit autoexport for this suite.

```sh
CARGO_TARGET_DIR=/external/nera-target cargo build --release --locked
CARGO_TARGET_DIR=/external/nera-target cargo build --release --locked
CARGO_TARGET_DIR=/external/nera-target cargo build --release --locked -p nera
python3 tests/integration/cowlib/native_eunit.py --modules cow_http2_machine \
  --upstream /external/cowlib --proper /external/proper \
  --nera /external/nera-target/release/nera \
  --eunit /path/to/otp/lib/eunit-2.11 \
  --gb-trees /path/to/otp/lib/stdlib-8.0.3/src/gb_trees.erl \
  --work-dir /external/http2-machine-eunit
COWLIB_HTTP2_MACHINE_REPORT=/external/http2-machine-eunit/report.json \
  python3 -m unittest discover -s tests/integration/cowlib -p 'test_*.py'
/external/nera-target/release/nera test cancel_timer_sync cancel_timer_async \
  send_after_keeps_alive start_timer_keeps_alive -j 1 --timeout 30
```

`COWLIB_HTTP2_MACHINE_REPORT` enables positive checks against actual logs and
their recorded hashes, inventory and framework outcomes. Without a report this
integration-only test is explicitly skipped; the permanent single-test summary
and completion rejection regressions still execute. Earlier Cowlib campaigns
retain their dated results and were not combined into a fresh all-Cowlib run.
Other unvalidated modules remain unvalidated when compiled as dependencies.

## cow_http2 upstream EUnit — 2026-10-19

Starting point: freshly fetched merged main **`522bf09c6c08c1a8f3a800886a2409878d3740bc`**
(PR #112, after the HPACK stage). Cowlib remains **2.20.0**, commit
`c768a804565ff5b8178ed968a5921e469d6bd7b2`. The external Cowlib and PropEr
checkouts were fetched afresh; no previous machine's caches are required.
Compiler, runtime and Nera stdlib sources are unchanged from this main revision.
CI and Release remain suspended.

### Inventory and scope

`cow_http2` has **18 public function/arities**:

- Parsing: `parse_sequence/1`, `parse/1,2`, `parse_settings_payload/1`.
- Building: `data/3`, `data_header/3`, `headers/3`, `priority/4`,
  `rst_stream/2`, `settings/1`, `settings_payload/1`, `settings_ack/0`,
  `push_promise/3`, `ping/1`, `ping_ack/1`, `goaway/3`, `window_update/1,2`.

The complete upstream functional suite contains **three direct tests**:
`parse_ping_test/0`, `parse_windows_update_test/0` (upstream spelling), and
`parse_settings_test/0`. The first two require `more` for every nonempty
incomplete prefix, then verify the complete decoded frame and trailing bytes.
The settings test checks a truncated acknowledgement, the full acknowledgement,
and rejection of a nonzero connection stream ID. These tests are **not full
coverage of those 18 APIs or HTTP/2 protocol conformance**.

There are **zero upstream PropEr properties** for this module at the pin,
including outside its source file. The campaign records `properties: []`;
no PropEr trials, generation, shrinking or seed coverage are claimed. Seeds
are **not applicable** to these deterministic EUnit tests. No substitute
property suite was invented to turn this absence into a pass.

The original production module uses Erlang BIFs, `binary`, `lists` and `maps`.
Its `cow_http:fin/0` references are types, not runtime calls; it does not need
HPACK or an HTTP dependency closure for this suite. It has no include files.
Native OTP EUnit **2.11** and `gb_trees` come from the external OTP **29.0.4 /
ERTS 17.0.4** source installation (stdlib **8.0.3**). PropEr **1.5.0** at
`5640d0715d4b346676267504d8e84398e2a29f75` is still a required runner input
and its pin is checked, but none of its code executes in this campaign.

### Result and false-success checks

Validated on **macOS 27 arm64**, Rust **1.99.0** with locked dependencies, using
a freshly built, separately frozen release compiler and matching runtime.

| Workload | BEAM | Native Nera |
| --- | ---: | ---: |
| Original `cow_http2` EUnit tests | **3/3** | **3/3** |
| EUnit positive/negative/timeout/isolation controls | **Pass** | **Pass** |
| Upstream PropEr properties | **None at this pin** | **Not applicable** |

Both the initial and final campaigns complete with `status: passed`, exit
**0**, no skipped/blocked tests and **no semantic normalization**. Native
execution uses the public, project-configured `nera test` command and real
OTP EUnit, not an assertion-closure adapter or BEAM at runtime. The external
native copy adds only TEST and EUnit autoexport; it removes **zero functions**
and retains all original assertions and production code. A test-verdict excerpt
from both engines follows; it is not their complete raw output:

```text
cow_http2: parse_ping_test...ok
cow_http2: parse_windows_update_test...ok
cow_http2: parse_settings_test...ok
All 3 tests passed.
```

At that revision the duration line differed: BEAM printed `[done in 0.019 s]`,
while Nera printed `[done in ~.3f s]` because `io:format/2` ignored field width
and precision. The raw transcripts retain this historical difference; it did
not change the EUnit verdicts. The subsequent shared formatter fix is covered
by [`tests/io_format_fields.erl`](../../io_format_fields.erl), including the
same EUnit timing example on BEAM and Nera; this campaign was not rerun.

The original runner rejected `--modules cow_http2` with exit **2**, before
compilation. A public-selector regression first fails for that same reason,
then passes after the narrow suite-list extension. No compiler/runtime fix
was needed for these test verdicts; full output parity remains incomplete.

Mutations of the actual first BEAM/native transcripts exposed a second harness
blocker: the old substring check accepted duplicated or contradictory success
summaries. The initial before-run records four failing subcases; prefixed-marker
and malformed-summary mutations were added afterward. Both result gates now
require exactly one standalone success summary for the expected count and
exactly one standalone completion marker, plus exit zero. The initial campaign's
real-reading probe then accepted the original readings and rejected **20**
altered readings.

The always-run completion tests own all rejection
cases: nonzero exit, timeout, empty output, wrong/prefixed/duplicate completion,
wrong count, malformed summary, duplicate summary and contradictory summary.
They require no external report and run in ordinary `unittest discover`.
Running these permanent tests against the unchanged main `522bf09` substring
checks produces **seven failing subcases**; all pass against the corrected
checks. The optional HTTP/2 real-report test now owns only actual positive
readings, their hashes, suite inventory and framework results, not duplicate
rejection coverage. The unchanged general EUnit controls also exercise actual
deliberately failing assertions, generators, exits, timeout, isolation and
invalid descriptors; a reporter crash cannot pass those controls.

With the HTTP/2 report, the Cowlib harness gate runs **93 tests: 92 pass, one
explicitly skipped** (the existing optional real-HPACK-report check). Without
any reports, **91 pass, two optional real-report checks are skipped**; the
permanent rejection regressions still run. This task did not rerun the separate
HPACK property campaign. Reports, full command transcripts and source/binary
manifests stay in the task's external evidence folder, not in the project or
pull request.

Campaign SHA-256 fingerprints:

| Artifact | SHA-256 |
| --- | --- |
| Frozen Nera release compiler | `389022a60af365f670dfd1aee7b6e4f5144bab94e72c789256b7836b0b0c9ab1` |
| Matching runtime | `62dc77af3d143e3538ddeb348e80588cebb97d5aada28f36ade1b9dbd6034fbd` |
| Nera source manifest (sorted JSON source-to-hash map) | `ce827354d4eace31b7fd83dba691475b46d0a5e6eeedb5950ecedee8f71697b3` |
| Original `cow_http2.erl` | `edeb6bef50b963312e92021ed2edd029628c51752fa75b527557832912f452d6` |
| External native `cow_http2.erl` | `14337c59f5666ce70439021951ded820b184e101d51477f7f3949c525bee84c8` |
| External `gb_trees.erl` | `890b641fa4e901c4f11bf65288ea12e8a8c0480d3a4867274f699fb4ef9c60a3` |
| EUnit source/header manifest (sorted JSON source-to-hash map) | `8853e5e8dbde8f34d5d5136ef306dc59c9573a4546f6d9f06696df12826cca4e` |

### Local gate

The exact compiler/runtime source used above passes both locked release build
passes, workspace Clippy with warnings denied, and **2,025 Rust tests** (zero
failed, one existing ignored measurement). The final complete serial native
gate passes **391 tests, zero failed, two existing Onecached exclusions / 393**,
exit **0**, including the TCP performance test. The exclusions are
`ets_onecached_full_startup` and `ets_onecached_pattern`, which require absent
external modules; they are not successful validation. This task ran no other
build or campaign alongside the serial native gate. All **six PropEr harness
tests** also pass; this is not a new native PropEr campaign.

```sh
CARGO_TARGET_DIR=/external/nera-target cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_TARGET_DIR=/external/nera-target cargo test --workspace --locked --no-fail-fast
/external/nera-target/release/nera test -j 1 --timeout 30
python3 -m unittest discover -s tests/integration/proper -p 'test_*.py'
```

No compiler, runtime, packaging or platform code changed, so cross-object and
installed-archive checks were not rerun. Linux/Windows execution and remote CI
validation remain unclaimed; no workflow was enabled or changed.

### Reproduction and limits

Use clean pinned external checkouts and a fresh external work directory. The
OTP installation must contain EUnit headers/sources and `gb_trees.erl`. This
module requires neither PropEr preprocessing nor build-time parse transforms
beyond the supported EUnit autoexport. All third-party sources, prepared
modules and executables remain outside the project.

```sh
# On paths containing spaces, choose a space-free target path for jemalloc.
CARGO_TARGET_DIR=/external/nera-target cargo build --release --locked
CARGO_TARGET_DIR=/external/nera-target cargo build --release --locked
python3 tests/integration/cowlib/native_eunit.py --modules cow_http2 \
  --upstream /external/cowlib --proper /external/proper \
  --nera /external/nera-target/release/nera \
  --eunit /path/to/otp/lib/eunit-2.11 \
  --gb-trees /path/to/otp/lib/stdlib-8.0.3/src/gb_trees.erl \
  --work-dir /external/http2-eunit
COWLIB_HTTP2_REPORT=/external/http2-eunit/report.json \
  python3 -m unittest discover -s tests/integration/cowlib -p 'test_*.py'
```

`COWLIB_HTTP2_REPORT` enables positive checks using the actual campaign report
and its adjacent stdout logs, whose hashes must match the report. Without it,
this integration-only check is explicitly skipped, never counted as run;
completion rejection regressions remain in the always-run unit tests. Historical
Cowlib results remain separate campaigns. This result excludes exhaustive
public API/protocol coverage, other platforms, `cow_http2_machine` (the next
separate stage), and the remaining unvalidated modules. `cancel_timer/1`
remains the documented stub; this campaign supplies no new timer API evidence.

## cow_hpack campaign — 2026-10-19

Scope: pinned Cowlib **2.20.0** at
`c768a804565ff5b8178ed968a5921e469d6bd7b2`, starting from merged main
`c74fc024d8dfca558742c635882bef6bb7d0d75f` (PR #111, including #107).
The external dependencies were fetched afresh on this machine; no historical
cache or path is required. CI and Release remain suspended.

### Scoped result

Validated on **macOS 27 arm64**, Rust **1.99.0** with locked dependencies,
OTP **29.0.4 / EUnit 2.11**, PropEr **1.5.0** and pinned OTP **27.3** random-source
dependencies. A frozen compiler/runtime pair prevents concurrent rebuilds from
changing the upstream campaigns' executables.

| Original upstream workload | BEAM | Native Nera |
| --- | ---: | ---: |
| EUnit functional cases | **74/74** | **74/74** |
| `prop_str_raw/0` | **6,000/6,000** | **6,000/6,000** |
| `prop_str_huffman/0` | **6,000/6,000** | **6,000/6,000** |
| EUnit positive/negative/timeout/isolation controls | **Pass** | **Pass** |
| False properties reduced to empty binary | **12/12** | **12/12** |
| False/original seeded counterexample replays | **24/24** | **24/24** |

Both strict campaign reports finish `status: passed`, exit **0**, with no skipped,
blocked or semantically normalized upstream cases. Full typed property traces
match exactly, including generated input, wire bytes, decoded value/remainder,
verdict and shrinking. All **91 Cowlib harness tests** pass with the real-campaign
report supplied, including **28** mutations of actual BEAM/native readings.
The six focused Erlang regression modules pass on both engines; each of the
five blockers has a failing-before reproduction recorded in the task evidence.

Campaign fingerprints (SHA-256):

| Artifact | SHA-256 |
| --- | --- |
| Nera compiler used for both upstream campaigns | `ccf7764fe726bbb0bb9bc9b83cb8ed225d1be23944959df01e107788f2856eb5` |
| Matching runtime | `6c6e9ce59debfdb786f4c8176861e8ca5dc9911ebdc92d1474cacb6fa5b526b7` |
| Nera source snapshot (sorted source-hash manifest) | `ce827354d4eace31b7fd83dba691475b46d0a5e6eeedb5950ecedee8f71697b3` |
| Identical typed property traces, each engine | `58046f52625fe051d463b4b5d6efad327fd6c8e8827eecefe732c44b26713134` |
| Original `cow_hpack.erl` | `6c9d5714790d8951fa57b793bb8f6c0f0433f7bbc15f33284e3b289b264bfa4d` |
| Original common header | `c53242bd29cad93135050d5daf9f73e9b571da56a992dbc49870e7e53c1d164c` |
| Original Huffman lookup header | `80fb656300ee1288bf34268e88c2b0886760ab6a794eeca60d84a94ae0f11652` |

The campaign compiler above incorporates the rebuilt matching runtime.
All final campaigns and local gates use this production source at `06c4483`;
subsequent result-documentation edits do not change its source-hash manifest. Local native-install-layout checks
pass with both the external and incorporated runtime; a fresh-cache, compiler-only
installation also compiles/runs all six blocker regressions. Its extracted runtime
hash equals the campaign runtime above. Cross-object smokes pass for macOS x86_64
and Linux aarch64; these are object-generation checks, not target execution.
The local layout archive was assembled from these exact artifacts; this is not
a claim that release packaging workflows ran.

### Final local gates and loaded-run comparison

The final production source at `06c4483` passes the prescribed locked release
build (two passes to refresh the incorporated runtime), workspace Clippy with
warnings denied, and **2,025 Rust tests** (one existing ignored measurement).
The six PropEr harness tests pass. The original generic native generation/
shrinking scenario also passes on both engines, retaining its **60-second** bound.

The final complete serial native gate passes: **391 passed, zero failed,
two existing Onecached exclusions / 393**, exit **0**, including
`tcp_reactor_perf`. The exclusions are `ets_onecached_full_startup` and
`ets_onecached_pattern`, requiring absent external Onecached modules; they are
not counted as successful validation.

After all builds and campaigns ended, the gate launched once after three
consecutive one-minute host-load readings below 10: **9.68, 8.28, 6.94**, sampled
20 seconds apart on 10 logical CPUs. No other task-owned workload ran alongside
it. During the gate, load readings span **6.94–48.71**; low launch load does not
mean the machine remained idle while compiling/running the suite.

The review corrections also have direct behavior checks: the real formatter
retains integer/base-N/float source spelling; the unchanged process-lifecycle
benchmark completes 5,000 heap kills with **zero** release kill diagnostics,
versus **5,000** before the debug-only guard was restored. Nominal
`error_logger=true` and integer/map enforcement pass on BEAM and native Nera.
The normal-worker GC regression fails if its safe-point call is removed or the
BIF is replaced with a success-only stub, then passes after restoration.

The earlier full serial run at `1d5c8e8` remains recorded: **390 passed, one
failed, two existing Onecached exclusions / 393**, exit **1**. The unchanged
`tests/tcp_reactor_perf.erl` raises `{throughput_regression,492,10}`: 600 sustained
rounds take 492 ms versus 10 ms for the 200-round warmup, exceeding its 9x limit.
The failure is retained, not replaced by a later successful attempt.

An initial comparison of the identical 12-test TCP group passes **12/12** on
preserved, unmodified main `c74fc02`. Fixed-count repetitions of the original
fixtures then produce:

| Engine | Ordered TCP groups | Group cases | Isolated `tcp_reactor_perf` |
| --- | ---: | ---: | ---: |
| Patch at `1d5c8e8` | **2/3 pass** | **35/36 pass** | **4/5 pass** |
| Unmodified main `c74fc02` | **2/3 pass** | **35/36 pass** | **4/5 pass** |

The patch fails with batch maximum/median **565/22 ms** and **209/7 ms**.
Main fails with sustained/warmup **7,640/814 ms** and batch maximum/median
**402/10 ms**. All selected tests execute, in identical order, without skips,
normalization, sleeps or retry-until-pass. Separately instrumented logging probes
are not included in these counts.

One-minute host loads at launch span **163.6–247.8** for the patch and
**171.5–237.5** for main, on **10 logical CPUs**. The original failure families
are demonstrably pre-existing on main under high load, consistent with the
[recorded P1-1 baseline](../../../docs/PROJECT_STATUS.md#p1-1--tcp-stability-and-trustworthy-completion-checks).
After the captain-authorized termination of the nine identified CPU-consuming
shells, the quiet-launch full gate passes. The repeated main failures establish
pre-existing TCP instability observed under severe load; the final pass supports
load sensitivity, not a proven reactor/OS root cause. An unstable loaded main
is not a valid known-good endpoint for binary pass/fail bisection. The final
serial gate is successful; none of the earlier failures is waived or hidden.

Reference compiler SHA-256 is
`d3e94a3b79aaa674a05305e9754a15bc5eed19538af6bdf7c588db2100de46ec`;
reference runtime is
`f82813337ecc87a6eee177bd8d5832cd079ff195066d67995370c56fce823a36`.
The fixture SHA-256 matches main:
`811f9d5861cf140d1fb1d2a85aad43e42e4cb8dbc91fe510dd10f362ed23e5f1`.
The scoped HPACK result, successful final local gate and historical loaded-run
failures remain separate readings; there is no CI or other-platform execution claim.

```sh
CARGO_TARGET_DIR=/tmp/nera-hpack-target cargo build --release --locked
CARGO_TARGET_DIR=/tmp/nera-hpack-target cargo build --release --locked
CARGO_TARGET_DIR=/tmp/nera-hpack-target cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_TARGET_DIR=/tmp/nera-hpack-target cargo test --workspace --locked --no-fail-fast
/tmp/nera-hpack-target/release/nera test -j 1 --timeout 30
# Using separately preserved, verified main artifacts, not this branch's runtime:
/external/main/nera test 'tcp_*' -j 1 --timeout 30
```

This is a bounded release-compiler HPACK result, not full Cowlib, EUnit, PropEr,
protocol/API or other-platform compatibility. Earlier Cowlib campaigns retain
their own dated results and were not consolidated into this run. `cow_http2` and
`cow_http2_machine` remain next steps; `cancel_timer/1` remains the documented stub.

### Inventory

`cow_hpack` exports nine APIs: `init/0,1`, `set_max_size/2`, `decode/1,2,3`
and `encode/1,2,3`. Its **26 EUnit entries expand to 74 tests**: 23 direct
functions and three generators (37 allowlisted names, nine never-indexed static
names, five never-indexed new names). They cover RFC request/response examples,
raw/Huffman strings, dynamic tables and size updates, invalid EOS/oversized
integers, binary retention, iodata and indexing policy. Two original PropEr
properties, `prop_str_raw/0` and `prop_str_huffman/0`, generate arbitrary binaries
and check the internal string codec. They do not validate all public API or
state-machine behavior.

Both HPACK headers (`cow_hpack_common.hrl`,
`cow_hpack_dec_huffman_lookup.hrl`) are loaded from external Cowlib `src`.
Runtime calls use `binary`, `lists`, `maps` and Erlang BIFs; `cow_http:headers/0`
is a type reference, not a runtime dependency. EUnit additionally uses its
external OTP sources/headers and `gb_trees`. PropEr **1.5.0** uses the pinned
OTP **27.3** `rand`/`ordsets` sources described in the
[PropEr guide](../proper/README.md). Horse benchmarks are not functional tests.
The native EUnit preparation keeps `encode_indexed_names/0` and
`dyn_field_names/1` verbatim, including all clauses.

The follow-on inventory is recorded without claiming execution: `cow_http2`
has 18 exports, three direct tests and no properties; `cow_http2_machine` has
26 exports, one direct test and no properties. They remain separate steps.
Their dependencies include `cow_hpack`, HTTP/header helpers, maps/queues and
message timers; compiling them previously does not validate them.

### Property workload and controls

Each original property runs 1,000 trials for each seed `{101,202,303}`,
`{7,11,13}`, `{999,1001,2027}`, repeated twice cumulatively in one process per
engine: **6,000 trials per property per engine**, not 6,000 independent inputs.
Both properties use the same binary generator, so their input streams also
overlap. PropEr sizes grow from 0 to 256; observed binary lengths across the
three streams are 0–252, with every byte value observed per stream/property.
This is finite generated coverage, not exhaustive HPACK or arbitrary-size data.

An explicit setup callback seeds `rand` after PropEr initialization. Nera
generates its own inputs. The driver executes the original predicate and also
records input, encoded string, decoded value/remainder and verdict. Each repeat
must match inside its engine; typed ETF compares the entire generation and
shrink traces across engines. No semantic normalization is applied. Python
independently checks raw payload bytes and both string length/flag formats;
there is no independent Huffman encoder oracle.

**12 deliberately false quickchecks per engine** execute the original predicate
then reject their input. Every control must shrink a nonempty binary to `[<<>>]`.
**24 seeded replays per engine** require the false wrapper to reject that minimum
and the original property to accept it. Exit status, exact result count/order,
seeds and one completion marker are mandatory. The harness additionally mutates
real captured readings to reject wrong counts, verdicts, decoded values, wire
bytes, seeds, minima, missing shrinking and missing/duplicate completion. It
never substitutes fabricated positive traces for those readings.

### Initial blockers and regressions

The initial BEAM EUnit run passed **74/74**; both seeded BEAM properties also
completed. Nera executed **zero upstream EUnit tests/property trials** at this
stage: source compilation rejected `10_000`, and the preprocessed property
module exposed missing `map_get/2` and `garbage_collect/0`. Deferred remote
calls do not suppress these undefined local BIFs.

Project-owned `number_separators`, `map_get_bif` and `garbage_collect_bif`
regressions reproduce all three compilation failures on the initial compiler
and pass on BEAM. The lexer now recognizes underscores only between valid
digits, including base-N and float components. `map_get/2` reuses `maps:get/2`
with its existing argument order, errors and guard semantics, including dynamic
calls and remote funs.

`garbage_collect/0` suspends through the normal continuation protocol. Only
once compiled callers have saved their live values does the scheduler perform
a full collection; resumption returns `true`. A deterministic Rust regression
retains a live binary in the process dictionary, promotes a second binary then
makes it unreachable, and proves the latter is reclaimed before resumption.
The Erlang regression additionally retains values across caller locals, mailbox
and process dictionary. A second regression invokes the C-ABI BIF through the
normal scheduler worker and observes old backing-storage reclamation inside the
resumed caller, before process cleanup, with no automatic-GC pressure. Removing
the worker's explicit-collection call or substituting a success-only BIF makes
this regression fail; restoring the real path passes. This is not a no-op or
an in-FFI moving collection.
Other GC arities remain outside this addition.

The upstream indexing tests also call `binary:longest_common_prefix/1`.
A minimal dynamic native call returned `{error,undef}` before its implementation;
the direct-call regression failed compilation. Its BEAM reference establishes
that an empty list is `badarg` (not zero), and checks byte counts, nonempty
lists containing empty binaries, long/shared/shifted binaries and invalid
values even after the prefix becomes empty. The runtime validates the complete
proper list without allocating while borrowing binary bytes; direct calls,
`apply` and remote funs share the implementation. This is not a latency bound
for arbitrarily large input lists/binaries.

After these fixes, native EUnit reached **73/74**, with no skips. The remaining
failure occurred before the oversized-integer assertion: the existing
`process_flag(max_heap_size, Value)` accepted an integer but rejected the original
`#{size => 10000, kill => true}` map. The project-owned `max_heap_size_map`
regression reproduces `error:badarg` at that call and passes on BEAM. The fix
supports the default-policy size map and returns the previous normalized flag
map for both input forms. It preserves the kill-on-limit policy and the existing
debug-only kill diagnostic; release builds do not print per-kill messages.
`error_logger=true` is accepted as the nominal default, not a claim of full
error-logger semantics. Nondefault `kill`, `error_logger` and `include_shared_binaries`
policies are explicitly rejected, not silently accepted; complete flag-policy
compatibility is not claimed. The existing `max_heap_size` regression now checks
that both forms actually kill an allocating linked worker and requires a final
completion marker; merely returning a plausible flag map cannot pass it.

Two runner regressions first failed on suite selection and deleted functional
helpers, then passed after the narrow extension. EUnit assertion bodies remain
verbatim; properties run separately with their original generator and predicate.
The first full native property run exceeded the common 60-second execution
limit after completing the raw-string streams. Running the same executable with
a measured 600-second bound completed in about 249 seconds and produced exact
BEAM traces. The HPACK scenario now declares that finite bound explicitly;
other scenarios retain 60 seconds. A timeout still fails, even with partial
results. The report records both declared and command-specific bounds.

An additional unoptimized-compiler diagnostic exceeded its stack while compiling
the 4096-clause Huffman lookup table. A project-owned generated table reproduces
this during IR generation in the unoptimized compiler; the initial release
compiler emits its IR successfully. This campaign uses release Nera and does not
claim unoptimized-compiler coverage.

### Reproduction

Use fresh empty external campaign directories and a freshly built Nera. On a
working copy whose path contains spaces, use a space-free external
`CARGO_TARGET_DIR`; jemalloc's configure rejects spaces in its output prefix.
All third-party source, prepared listings, executables and full reports remain
external. Never add those files to the project.

```sh
CARGO_TARGET_DIR=/tmp/nera-hpack-target cargo build --release --locked
python3 tests/integration/cowlib/native_eunit.py --modules cow_hpack \
  --upstream /external/cowlib --proper /external/proper \
  --nera /tmp/nera-hpack-target/release/nera \
  --eunit /path/to/otp/lib/eunit-2.11 \
  --gb-trees /path/to/otp/lib/stdlib-8.0.3/src/gb_trees.erl \
  --work-dir /external/hpack-eunit
python3 tests/integration/cowlib/native_hpack.py \
  --upstream /external/cowlib --proper /external/proper \
  --nera /tmp/nera-hpack-target/release/nera \
  --otp-rand /external/otp-random/rand.erl \
  --otp-ordsets /external/otp-random/ordsets.erl \
  --beam-parse-transforms --samples 1000 --work-dir /external/hpack-proper
COWLIB_HPACK_REPORT=/external/hpack-proper/report.json \
  python3 -m unittest discover -s tests/integration/cowlib -p 'test_*.py'
python3 -m unittest discover -s tests/integration/proper -p 'test_*.py'
```

The environment variable enables the real-campaign mutation tests; without a
report those integration checks are explicitly skipped, never counted as run.

## cow_http1 / cow_http_te upstream EUnit — 2026-10-15

Validated from merged main **`902702e` (PR #104)** plus message-timer and
receive-timer handoff liveness fixes, with a fresh release build, macOS arm64, OTP **29.0.5 / EUnit 2.11**,
pinned Cowlib **2.20.0** (`c768a804565ff5b8178ed968a5921e469d6bd7b2`).
The existing `native_eunit.py` runner now selects both modules.

| Original upstream functional suite | BEAM | Native `nera test` |
| --- | ---: | ---: |
| `cow_http1` | **39/39** | **39/39** |
| `cow_http_te` | **10/10** | **10/10** |
| Total assertions | **49/49** | **49/49** |
| General EUnit framework controls | **Pass** | **Pass** |

The final runner exits **0**, with `status: passed`, no skipped assertions and
no semantic normalization. Each module retains all eight original test entry
points. HTTP/1 covers request/status lines, headers and versions, including
malformed/truncated inputs. Transfer encoding covers identity, chunk
encoding/decoding, extensions, trailers, partial reads, byte-by-byte dripfeeding,
large chunk sizes and error cases. These are the upstream assertions, not
exhaustive API or protocol coverage. Neither module contains an upstream
`prop_*` property at this pin.

External native preparation preserves the exact test bodies and the original
`dripfeed` / `do_body_to_chunks` helpers, including recursive clauses. Only
`horse_*` benchmarks are removed. It enables TEST and EUnit autoexport; EUnit
runs as an external project dependency through generic `nera test` with
explicit `defer_undefined = true`. The original BEAM suite uses TEST/autoexport
and the same pin. The complete production HTTP dependency closure is included,
without stubs. Third-party source, headers, prepared copies, dependencies and
executables stay outside this repository.

### Native EUnit timeout blocker and regression

Before the fix, both Cowlib suites already passed, but the general EUnit
fixture [`eunit_native.erl`](../eunit/eunit_native.erl) stopped during
`eunit:test({timeout, 0.01, fun() -> receive never -> ok end end})`.
Native execution returned 0 without `eunit_native_ok`; the runner correctly
reported a failing campaign. The same incomplete execution was reproduced in
an EUnit-only project without Cowlib.

`send_after/3` and `start_timer/3` delivered messages from sleeping Rust threads
that were invisible to scheduler deadlock detection. Once every Erlang process
was waiting, the scheduler could terminate before the timeout message arrived.
The fix registers pending message-timer work before spawning the callback and
retains it through message publication and wake dispatch. A guard releases the
pending slot on callback completion or unwinding. Receive-timer cancellation
remains separate and cannot erase pending message delivery.

Two minimal project-owned regressions wait for a delayed message **without a
receive-after fallback**, so another receive timer cannot mask the defect:
[`send_after_keeps_alive.erl`](../../send_after_keeps_alive.erl) and
[`start_timer_keeps_alive.erl`](../../start_timer_keeps_alive.erl). Before the
fix, both fail on missing expected `ok` output. Afterward both pass on native
Nera and BEAM; the `start_timer` test also checks the exact returned reference.
A Rust regression checks multiple pending callback lifetimes and independence
from receive-timer cancellation/removal. The unchanged EUnit control fixture
now reaches its timeout, isolation, unsupported-descriptor and final-marker
checks on both engines, including its deliberately failing tests.

The scheduler also preserves liveness across both **receive-timer handoffs**:
WAITING publication before heap registration, and extraction of an expired
timer before its wake is published. A process with a published timed-receive
wait reason is not deadlocked, even when the timer heap/counters are empty;
that ownership ends when the wake CAS publishes READY. Untimed empty receives
retain the existing deadlock behavior. This avoids adding delays or relying
on repeated timing checks to make either transition safe.

Two deterministic Rust regressions replay these exact states through the
production transition, timer insertion/extraction, process-state termination
predicate and wake functions. All state is local: no sleeps, global reactor,
background I/O or scheduling luck can mask the result. Both tests fail at the
intended liveness assertion without the fix and pass afterward. They also
check that a duplicate wake creates exactly one runnable slot/queue entry,
that READY remains live and that an untimed empty receive really is blocked.
The full 15-test scheduler group, including actual timeout wakeup and deadlock
execution, passes as well.

These are liveness fixes, not complete timer API compatibility. In particular,
`cancel_timer/1` remains the existing stub returning false without cancellation;
the feature table now states that limit. The callback still uses the existing
thread-based implementation. Long uncancelled timers can keep an otherwise
waiting scheduler alive until they deliver.

Final local checks: release build, Clippy with warnings denied, **1,994 Rust
tests** (one existing ignored measurement), **379 native Erlang tests** (two
existing Onecached exclusions) and **85 Cowlib harness tests** pass. Three new
harness tests first fail on the prior runner, then pass with suite selection,
dependency closure and verbatim helper retention. Existing checks reject
missing completion markers, wrong counts, timeouts and reporter crashes.
Harness/preparation/compiler and upstream source fingerprints are checked for
changes during execution. Other platforms and earlier Cowlib suites/properties
were not rerun. CI remains paused under repository policy.

```sh
python3 tests/integration/cowlib/native_eunit.py \
  --modules cow_http1 cow_http_te \
  --upstream /tmp/cowlib-pristine --proper /tmp/nera-cowlib-main-proper \
  --nera "$PWD/target/release/nera" \
  --eunit /path/to/otp/lib/eunit-2.11 \
  --gb-trees /path/to/otp/lib/stdlib-8.0.3/src/gb_trees.erl \
  --work-dir /tmp/nera-cow-http1-te-timer-fixed
./target/release/nera test send_after_keeps_alive start_timer_keeps_alive -j 1 --timeout 5
python3 -m unittest discover -s tests/integration/cowlib -p 'test_*.py'
```

Use pristine pinned external sources, the [dependency setup](../proper/README.md)
and a fresh work directory. The [evidence](http1-te-native-eunit-evidence.json)
records the final passing campaign, the initial blocker, inventories, controls,
regressions and source/binary fingerprints. This establishes the finite listed
upstream suites on macOS arm64, not all Cowlib, EUnit, timer or protocol behavior.

## cow_http_hd with native PropEr — 2026-10-15

Validated on merged main **`211b1fb`** (PR #101) plus the `math:sqrt/1` runtime
addition, release Nera, macOS arm64, OTP **29.0.5**, pinned Cowlib **2.20.0**,
PropEr **1.5.0** and original OTP **27.3 `rand`/`ordsets`** sources.

All **26 original upstream properties pass 600/600 trials per engine**:
**15,600/15,600 on BEAM and native Nera/PropEr**. Each property uses seeds
`{101,202,303}`, `{7,11,13}`, `{999,1001,2027}`, 100 trials per seed, repeated
twice cumulatively in one process per engine. There are 300 seeded trial
positions per property, repeated twice; these are not 600 independent trials.
PropEr size grows from 1 to 10. This size parameter is not a global bound on
header length: upstream generators also choose their own vector lengths.

The inventory includes Accept and charset/encoding/language variants, access
control request headers, Allow, Cache-Control, Connection, Content-Language,
Content-Length, Content-Range, Content-Type, ETag, Expect, Host, If-Match,
Max-Forwards, Origin, Range, WebSocket extensions/protocol/version fields, TE,
Transfer-Encoding and Upgrade. The pinned inventory is checked before building;
an explicit `--properties` subset is reported as a subset and does not count as
validation of all 26.

The actual original generator and predicate are retained, including upstream
constraints and `begin ... end` bodies. A setup callback seeds `rand` after
PropEr initialization. Observation records each input and predicate verdict;
Nera generates its own inputs. Both engines repeat the entire trace identically
for each seed, and typed ETF compares generated values, true verdicts and
false-control shrink attempts exactly. The functional assertions are the
original upstream predicates; this does not independently strengthen them.

**156 deliberately false quickchecks per engine** still execute the original
predicate, then reject their input. Shrinking is bounded to 100 steps: 144
controls change their initial counterexample, while the other 12 retain it.
No global-minimality claim is made for these complex generators. **312 seeded
replays per engine** require the false wrapper to reject the retained example
and the original property to accept it. Crashes, missing results or timeouts
never count as a successful false control.

The first native run stopped in the Cache-Control generator on missing
`math:sqrt/1`, used by PropEr's nonnegative-integer generator. The new scalar
Rust runtime BIF handles nonnegative integers/floats, preserves negative zero,
returns `badarg` for nonnumbers and `badarith` for negative/out-of-conversion-
range numbers. It is available by direct call, dynamic `apply` and remote fun;
other `math` functions are not implied. The focused regression fails at its
prefix assertion before implementation and passes on Nera and BEAM after it.

Source preparation remains ordinary explicit BEAM preprocessing/parse
transforms. Original Cowlib headers are read from the external include
directory. Only property exports are added to the external prepared listing;
no generator, predicate or macro is replaced. Cowlib/PropEr/OTP source, generated
input traces and executables stay outside this repository. Both engines use the
same prepared Cowlib module and pinned random-source dependencies. Native
execution uses no BEAM process.

Final local checks pass: release build, Clippy with warnings denied,
**1,981 Rust tests** (one existing ignored measurement), **375 native Erlang
tests** (two existing Onecached exclusions), **82 Cowlib / 6 PropEr harness
tests**. Fresh native date and Base64url property regressions also pass on both
engines. CI workflows remain manually suspended under repository policy; no
workflow was enabled or changed.

See [native HTTP property evidence](native-http-hd-proper-evidence.json) for
exact source/binary/runner fingerprints, per-property/seed trace hashes,
controls, the initial blocker and regression checks. These are finite property
runs on macOS arm64, not exhaustive header validation, full Cowlib/PropEr or
cross-platform compatibility. Earlier HTTP EUnit and predicate-replay evidence
below remain separate results.

```sh
python3 tests/integration/cowlib/native_http_hd.py \
  --upstream /tmp/cowlib-pristine \
  --proper /tmp/nera-proper-src \
  --nera "$PWD/target/release/nera" \
  --otp-rand /tmp/nera-otp-rand/rand.erl \
  --otp-ordsets /tmp/nera-otp-rand/ordsets.erl \
  --beam-parse-transforms --samples 100 --max-size 10 \
  --work-dir /tmp/nera-cow-http-hd-native-proper
python3 -m unittest discover -s tests/integration/cowlib -p 'test_*.py'
python3 -m unittest discover -s tests/integration/proper -p 'test_*.py'
```

Use fresh external directories and the pinned [PropEr setup](../proper/README.md).

## cow_date with native PropEr — 2026-10-15

Validated from merged main **`306f5fe`** (PR #99), macOS arm64, OTP **29.0.5**,
release Nera, pinned Cowlib **2.20.0**, PropEr **1.5.0** and original OTP
**27.3 `rand`/`ordsets`** sources. Compiler, runtime and stdlib are unchanged.
The actual upstream `prop_http_date/0` generator and predicate run under native
PropEr; BEAM does not supply inputs to Nera. The generator retains its original
`oneof` choice among all three formats, rather than enforcing balanced samples.

| Observed format | BEAM | Native Nera/PropEr |
| --- | ---: | ---: |
| IMF-fixdate | **2,000/2,000** | **2,000/2,000** |
| RFC 850 | **2,042/2,042** | **2,042/2,042** |
| asctime | **1,958/1,958** | **1,958/1,958** |
| Total original property trials | **6,000/6,000** | **6,000/6,000** |

Seeds **`{101,202,303}`**, **`{7,11,13}`**, **`{999,1001,2027}`** each run
1,000 trials with sizes 1–50, repeated twice cumulatively in one process per
engine. There are 3,000 seeded trial positions repeated twice, not 6,000
independent random trials. A `proper:setup/2` callback explicitly seeds
`rand:seed(exsplus, Seed)` after initialization, including all replay controls.
Full input/date/false-control shrink traces repeat identically and match BEAM.

The observation wrapper records each original `{Date, DateBin}` input and the
actual parsed date, then executes the original predicate. An independent
project-owned Python format oracle checks binary syntax, zero/space padding,
month/weekday names, RFC 850's 50/51 century boundary and exact date fields.
Across the three seed streams, **each format** observes every upstream component
value: years 1951–2050, months 1–12, days 1–31, hours 0–23, minutes 0–59 and
seconds 0–60. This includes single-digit asctime days and second 60. It does
not establish exhaustive combinations, actual leap-second dates, calendar
validity or weekday consistency: the original generators choose components
independently, and the oracle preserves that contract.

The original property passes and therefore has no failing counterexample.
**Six deliberately false wrappers** execute the original predicate then reject
the generated case. PropEr reduces every control to
`{{{1951,1,1},{0,0,0}}, <<"Mon, 01 Jan 1951 00:00:00 GMT">>}`. The driver
requires a change from the original failure. **12 seeded replays** require the
false wrapper to reject the minimized input and the original property to accept
it. Timeouts/process crashes are never counted as a successful false control.

Original Cowlib/PropEr/OTP sources, normal BEAM-prepared source listings and
compiled artifacts stay external. The only change to Cowlib's prepared listing
is an export for the original property. The generator, predicate and `FORALL`
macro are not replaced. The shared preparation helper also passes a fresh
Base64url regression: both original properties retain 6,000/6,000 passing trials
per engine, matching traces and their false controls.

Release build and **77 Cowlib / 6 PropEr harness tests** pass. Date harness tests
cover invalid/incomplete results, wrong parsed dates, wrong seeds/counts,
missing formats, absent reduction, incorrect minima and the independent oracle.
This integration-only change does not rerun or claim a new full Rust/Erlang,
EUnit, HTTP-property or cross-platform suite. The earlier cow_date EUnit 7/7 and
finite predicate replay below remain separate historical evidence.

```sh
python3 tests/integration/cowlib/native_date.py \
  --upstream /tmp/cowlib-pristine \
  --proper /tmp/nera-proper-src \
  --nera "$PWD/target/release/nera" \
  --otp-rand /tmp/nera-otp-rand/rand.erl \
  --otp-ordsets /tmp/nera-otp-rand/ordsets.erl \
  --beam-parse-transforms --samples 1000 \
  --work-dir /tmp/nera-cow-date-native-proper
python3 -m unittest discover -s tests/integration/cowlib -p 'test_*.py'
python3 -m unittest discover -s tests/integration/proper -p 'test_*.py'
```

Use pristine pinned dependencies from the [PropEr guide](../proper/README.md)
and a fresh empty external work directory.
[Native date evidence](native-date-proper-evidence.json) records source/binary,
runner, trace and regression fingerprints, format counts, seeds and controls.

## cow_base64url with native PropEr — 2026-10-15

Validated from merged main **`68247b9`** (PR #98), macOS arm64, release build,
OTP **29.0.5**, pinned Cowlib **2.20.0** and PropEr **1.5.0**. Compiler, runtime
and stdlib sources are unchanged. PropEr runs natively in Nera; inputs are
independently generated by each engine, with no BEAM-to-Nera input replay.

| Original upstream property | BEAM | Native Nera/PropEr |
| --- | ---: | ---: |
| `prop_identity/0` (with padding) | **6,000/6,000** | **6,000/6,000** |
| `prop_identity_no_padding/0` | **6,000/6,000** | **6,000/6,000** |
| Original `rfc7515_test/0` | **pass** | **pass** |

Each property runs 1,000 trials for each seed **`{101,202,303}`**,
**`{7,11,13}`**, **`{999,1001,2027}`**, repeated twice in one cumulative
process per engine. Thus each property has 3,000 seeded trial positions,
repeated twice, not 6,000 independent trials. Both properties use the same
binary generator, so their input streams also overlap by design. Size starts
at zero and grows to 256; observed lengths are 0–252, covering the empty binary,
all three length residues modulo three and all 256 byte values for every
property/seed. These observations do not claim exhaustive binary coverage.

A `proper:setup/2` callback calls `rand:seed(exsplus, Seed)` after initialization.
The driver obtains the **original generator and predicate** from each upstream
property and wraps the predicate only to observe inputs and encodings. It calls
that original predicate for every generated value. Every repeated run must
match its entire previous trace. Typed ETF results compare all inputs,
encodings and false-control shrink attempts across engines. Python's independent
Base64url oracle checks both padded and unpadded encodings for every input.

The original properties pass, so they have no counterexamples to shrink.
Separate deliberately false wrappers still execute the original predicate:
**12 false-property quickchecks per engine** are rejected and reduced from
nonempty binaries (initial lengths 10, 12, 7 by seed) to **`[<<>>]`**. An
additional **24 seeded replays** require the false wrapper to reject the
minimum and the original property to accept it. Crashes/timeouts never count
as property rejection. The original RFC 7515 assertion also executes unchanged.

Both engines load the same original **OTP 27.3 `rand` and `ordsets`** sources,
pinned and hashed in the [PropEr setup](../proper/README.md). Generic explicit
BEAM preparation remains a build-time requirement for PropEr's parse transforms.
Cowlib's original module is preprocessed/transformed by ordinary `erlc -P`
with `TEST`; the external listing receives only exports for the two properties
and the RFC test. Both engines compile that same prepared listing. No property
body, generator or `FORALL` macro is replaced. Sources, generated listings,
compiled modules, executables and full traces all stay outside this repository.

The strict harness checks malformed/incomplete/duplicate results, wrong seeds,
wrong trial counts, encoding errors, missing padding boundaries, absent
reduction and wrong minima. **73 Cowlib harness tests** and **6 PropEr harness
tests** pass. The existing literal true/false and seeded generation/shrinking
PropEr scenarios are rerun on both engines. No compiler/runtime/stdlib change
was necessary, so this task does not rerun or claim a new full Rust/Erlang,
HTTP/EUnit, cross-platform or full-Cowlib validation.

Reproduce after building Nera release and fetching the external pinned PropEr
and OTP dependencies as described in the [PropEr guide](../proper/README.md):

```sh
python3 tests/integration/cowlib/native_base64url.py \
  --upstream /tmp/cowlib-pristine \
  --proper /tmp/nera-proper-src \
  --nera "$PWD/target/release/nera" \
  --otp-rand /tmp/nera-otp-rand/rand.erl \
  --otp-ordsets /tmp/nera-otp-rand/ordsets.erl \
  --beam-parse-transforms --samples 1000 \
  --work-dir /tmp/nera-cow-base64url-native-proper
python3 -m unittest discover -s tests/integration/cowlib -p 'test_*.py'
python3 -m unittest discover -s tests/integration/proper -p 'test_*.py'
```

Use a fresh empty external work directory and pristine pinned source checkouts.
[Native property evidence](native-base64url-proper-evidence.json) records source,
compiler/runtime, runner and trace fingerprints, seeds, coverage and controls.
Earlier Base64url predicate replay results below remain historical evidence;
this result adds native PropEr execution, not a full PropEr or Cowlib claim.

## cow_date upstream property replay — 2026-10-15

Fresh release from merged main **`49b9757`** (PR #94), macOS arm64, OTP
**29.0.5**, PropEr **1.5.0** (`5640d0715d4b346676267504d8e84398e2a29f75`),
Cowlib **2.20.0** (`c768a804565ff5b8178ed968a5921e469d6bd7b2`). Compiler,
runtime and stdlib sources are unchanged. Both the original EUnit assertions
and the original `prop_http_date/0` predicate now have current recorded passes:

| Scope | BEAM | Nera |
| --- | ---: | ---: |
| Original EUnit functional assertions | **7/7** | **7/7** via `nera test` |
| Property inputs from `fixdate_gen/0` (IMF-fixdate) | **1,000/1,000** | **1,000/1,000** |
| Property inputs from `rfc850_gen/0` | **1,000/1,000** | **1,000/1,000** |
| Property inputs from `asctime_gen/0` | **1,000/1,000** | **1,000/1,000** |

The runner calls the **original upstream generators on BEAM**, taking balanced
samples of each format with seed `{19001, sample_index, format_index}` and
size `1 + ((sample_index - 1) rem 50)`. It repeats generation in an independent
BEAM invocation and requires a byte-identical ETF input file. All **3,000**
inputs are replayed through real `proper:check/3` on BEAM and the unchanged
property predicate on Nera. Each engine uses one cumulative process for the
whole corpus. Results also carry the actual parsed date, checked exactly
against the expected date in the input and against the other engine; a true
property boolean alone cannot hide an incorrect parse.

Observed component coverage, separately for every format: all years
**1951–2050**, months **1–12**, days **1–31**, hours **0–23**, minutes **0–59**
and seconds **0–60**, including the RFC 850 century boundary and the spacing
of single-digit asctime days. This is component coverage, not exhaustive date
combinations. Upstream generators choose fields independently, so this
property does not establish calendar validity of every generated date.

The input ETF SHA256 is
`56548684f5ce603bda39c407ae61b95127a00266d70f10d9810216c225f204d4`.
Original property bodies remain unchanged in the external Nera source:
`?FORALL` is retained and Nera's preprocessor replaces its envelope with a
closure receiving the shared input; unused generation and benchmark helpers
are removed only from that external copy. This is **finite property predicate
replay**, not native PropEr generation/shrinking. No Cowlib/OTP/PropEr code or
generated input data is copied into this repository.

Both engines reject an explicitly false property through the same result
gate. Native and BEAM EUnit positive/negative framework controls also pass.
Release and **69 harness tests** pass, with protocol, false-property and
incorrect-parsed-date controls. All expected property inputs execute, without
skips or blocks. [Evidence](date-property-evidence.json) contains seeds,
coverage, source/binary/output hashes, commands and the EUnit rerun.

```sh
python3 tests/integration/cowlib/upstream_date.py \
  --upstream /tmp/cowlib-pristine --proper /tmp/proper-1.5.0 \
  --nera ./target/release/nera --samples-per-format 1000 \
  --work-dir /tmp/nera-cow-date-property
python3 tests/integration/cowlib/native_eunit.py --modules cow_date \
  --upstream /tmp/cowlib-pristine --proper /tmp/proper-1.5.0 \
  --nera ./target/release/nera --eunit /path/to/otp/lib/eunit-2.11 \
  --gb-trees /path/to/otp/lib/stdlib-8.0.3/src/gb_trees.erl \
  --work-dir /tmp/nera-cow-date-eunit
python3 -m unittest discover -s tests/integration/cowlib -p 'test_*.py'
```

Use fresh external work directories and the pinned dependency setup below.
This completes the **recorded finite upstream validation scope for cow_date**;
it does not establish a native PropEr runtime, shrinking, exhaustive input
coverage or other platforms. The cookie raw EUnit result **53/54** and its
separate Domain/HttpOnly semantic verdict remain unchanged and were not rerun
in this task. CI/Release remain manually paused.

## Domain / HttpOnly semantic validation — 2026-10-15

The runner now reports a **separate semantic result** for the project-owned
Domain/HttpOnly builder probe, using the existing `runner.cookie_attributes`
comparator (`set_cookie_attributes`). The compiler/runtime binary hashes are
unchanged from the merged-main `c93eb65` validation below; no runtime or
upstream test code is changed.

| Validation contract | Result |
| --- | --- |
| Original `cow_cookie` EUnit assertions | **BEAM 54/54; Nera 53/54**, one failed, zero skipped |
| Original `cow_date` EUnit assertions | **BEAM 7/7; Nera 7/7** |
| Separate Domain/HttpOnly semantic probe | **1/1 pass**, `wire_equal = false` |

The probe verifies the expected cookie name/value and exact attribute bytes,
allowing only attribute reordering. Both executions must succeed and provide
exactly one framed output. Missing, extra or duplicated attributes, changed
values, malformed output and failed executions are rejected. Output framing
removes only its final newline; an invalid CR in the header is not stripped.
The semantic result cannot promote the raw EUnit assertion to a pass: the
runner's overall raw status remains **failed**, with exit code **1**.

This classifies the Domain/HttpOnly difference as a representation difference
allowed by the checked contracts, rather than a Nera ordering defect:

- [`maps:to_list(Map)` does not define tuple order](https://www.erlang.org/doc/apps/stdlib/maps.html#to_list/1).
  Ordered traversal requires an explicitly ordered iterator.
- [Cowlib 2.20.0 constructs attributes from `maps:to_list(Opts)` without sorting](https://github.com/ninenines/cowlib/blob/c768a804565ff5b8178ed968a5921e469d6bd7b2/src/cow_cookie.erl#L389-L425),
  and its [public setcookie documentation](https://github.com/ninenines/cowlib/blob/c768a804565ff5b8178ed968a5921e469d6bd7b2/doc/src/manual/cow_cookie.setcookie.asciidoc)
  specifies no fixed attribute order.
- The [RFC 6265 parsing/storage rules](https://www.rfc-editor.org/rfc/rfc6265.html#section-5.2)
  treat Domain and HttpOnly as distinct attributes. Duplicate attributes are
  deliberately rejected by our comparator rather than silently reordered.

The semantic scope is this **one project-owned probe**. It is not a claim of
54/54 unchanged upstream cookie assertions, a new full corpus result, or a
general-purpose Set-Cookie equivalence parser. Raw upstream tests remain
unmodified and separately visible. The cookie/date reproduction command
below now includes this semantic result automatically under
`cookie_attribute_order.semantic_validation`; HTTP remains the default suite.

[Evidence and commands](cookie-domain-semantic-evidence.json) record the raw
counts, both output strings, separate semantic/wire verdicts and source/binary
hashes. The rerun confirms all framework controls on both engines and **65
harness tests pass**, including negative controls and exact-wire equality.
The date PropEr property and other platforms remain outside this run.

## Cookie/date native EUnit from merged main — 2026-10-14

Fresh release from main **`c93eb65`** (PR #88, including the typed atomic
resources in PR #87), macOS arm64, OTP **29.0.5** / EUnit **2.11**. Compiler,
runtime and Nera stdlib sources are unchanged from main. Cowlib remains pinned
to **2.20.0 / c768a804565ff5b8178ed968a5921e469d6bd7b2**; PropEr **1.5.0**
is used only to compile the original BEAM TEST source.

| Original functional suite | BEAM EUnit | Nera `nera test` | Failed / skipped in Nera |
| --- | ---: | ---: | ---: |
| `cow_cookie` | **54/54** | **53/54** | **1 / 0** |
| `cow_date` | **7/7** | **7/7** | **0 / 0** |

Both suites compile and execute cumulatively through the project EUnit runner,
without per-case closure adapters. The one remaining failure is the first
case generated by `setcookie_test_`: the unchanged assertion expects
`Domain=acme.com; HttpOnly`, while Nera emits `HttpOnly; Domain=acme.com`.
Cowlib constructs attributes using `maps:to_list(Opts)`. No sorting,
normalization or assertion modification is applied to this native suite.
The original `setcookie_max_age_test` passes, including its expiration/payload
checks and rejection of negative Max-Age.

A separate project-owned diagnostic reproduces the raw order with its own
inputs, independently of the upstream assertion literals:

```text
BEAM: sample=value; Domain=example.org; HttpOnly
Nera: sample=value; HttpOnly; Domain=example.org
```

The two emitted attributes match in value; their byte order differs.
**cow_date has a scoped functional-suite pass; cow_cookie does not yet have a
complete raw upstream EUnit pass.** The earlier 34/34 cookie corpus pass uses
an explicit attribute-order comparison contract and does not remove this gap.

The native external date copy enables TEST/EUnit autoexport and removes only
PropEr generation/property and Horse benchmark helpers; every functional test
body and production function is retained unchanged. Original source and
adapted copies stay outside this repository. Positive and deliberate-failure
EUnit controls pass on both engines. The runner exits **1**, accurately
reflecting the cookie failure, and retains the generator inventory, actual
pass/fail/skip counts, raw diagnostics, source/compiler/runtime hashes and
commands in [cookie-date-native-eunit-evidence.json](cookie-date-native-eunit-evidence.json).
The release build and **62 project-owned harness tests** pass.

```sh
python3 tests/integration/cowlib/native_eunit.py \
  --modules cow_cookie cow_date \
  --upstream /tmp/cowlib-pristine --proper /tmp/proper-1.5.0 \
  --nera ./target/release/nera --eunit /path/to/otp/lib/eunit-2.11 \
  --gb-trees /path/to/otp/lib/stdlib-8.0.3/src/gb_trees.erl \
  --work-dir /tmp/nera-cowlib-cookie-date-native
python3 -m unittest discover -s tests/integration/cowlib -p 'test_*.py'
```

Use a fresh external work directory and the same source/dependency setup as
the HTTP native validation below. Omitting `--modules` retains the HTTP default.
This run does not execute `cow_date:prop_http_date/0` on either engine, rerun
the HTTP suites or 122-case corpus, or establish Linux/Windows coverage.
The historical BEAM property run below remains historical; native PropEr and
shrinking are still unvalidated. CI/Release remain manually paused.

## Project-configured `nera test` — 2026-10-14

The HTTP functional suite validation now invokes the public `nera test`
command with a generic project configuration rather than a harness-generated
Cowlib entry point:

```toml
[test]
framework = "eunit"
modules = ["cow_http", "cow_http_hd"]
```

The external project still resolves OTP EUnit and `gb_trees` as dependencies
and explicitly opts into `project.defer_undefined`. Nera generates an isolated
entry, inherits project source/include/OTP/dependency paths and executes all
selected modules cumulatively. The differential harness executes each whole
suite through this command and compares its total with original BEAM EUnit.
The reproduction command in the next section uses this path automatically.
See [generic configuration](../../README.md#project-eunit-configuration) and
[project runner regression](../eunit/project_test.py) for positive/negative,
selection, configuration, compilation, timeout and legacy-mode controls.
The undefined-local-call control exposed an existing `--no-warnings` defect:
semantic errors were suppressed along with warnings. The compiler now keeps
those errors active, preventing a false EUnit success for a missing tail call.
Failure counts and command statuses are checked; detailed `~P`/stacktrace
formatting remains partial.

This validation remains on main base `b772a55` plus the native
EUnit and project runner changes in this PR. It does not incorporate subsequent main
commits, broaden the native EUnit API scope or rerun the property replay.
Sources remain external; source and command hashes are recorded in
[project-eunit-evidence.json](project-eunit-evidence.json).

Final results: **102/102 cow_http** and **396/396 cow_http_hd** on both engines,
with no blocked/skipped assertions. Positive and deliberate-failure framework
controls pass on both BEAM and Nera. Local checks pass: release, **1,950 Rust
tests** (1 ignored), Clippy with warnings denied, **353 native tests** under
`--no-warnings` (2 external Onecached skips), **10 project-runner checks**,
**4 deferred-call checks** under `--no-warnings` and **59 harness tests**.
The standard sequential `nera test -j 1 --timeout 30` publication check also
passes all 353 native tests, with the same two exclusions.
The final code/test source SHA256 is
`4a2ee76a2d6f7bce1e404f53b2a09a995b3af85d6c1f020545872b36c735b472`.

## Native OTP EUnit — 2026-10-14

The native framework gap is closed for these two suites. From main `b772a55` (PR #85, including merged PR #86) plus this change, the
original pinned Cowlib functional assertions execute under **OTP EUnit itself**
on BEAM and Nera, with no per-case closure execution adapter.

| Suite | BEAM EUnit | Nera EUnit |
| --- | ---: | ---: |
| `cow_http` | **102/102** | **102/102** |
| `cow_http_hd` | **396/396** | **396/396** |

No assertions are blocked, skipped or modified. On Nera, each entire suite
executes cumulatively in one native program. EUnit **2.11**, OTP **29.0.5**
`gb_trees` and all Cowlib source stay outside this repository. Only the
nonfunctional Horse/PropEr generator helpers are removed from external Cowlib
copies; the test bodies remain verbatim. Default EUnit autoexport and Nera's
compiler-generated `module_info(exports)` perform test discovery.

`native_eunit.py` checks the Cowlib/PropEr pins, builds original TEST modules on
BEAM, derives generator counts, and requires the expected EUnit total, a single
completion marker and successful exit. It also executes project-owned framework
controls on both engines: successful tests/generators/nested groups, deliberate
assertion and generator failures, exits, timeout, isolation and invalid test
descriptors. A reporter crash fails the controls. Hashes and commands are in
[native-eunit-evidence.json](native-eunit-evidence.json); full logs and third-party
sources remain external. The platform tested is macOS arm64.

The explicit `[project] defer_undefined = true` mode lets unused EUnit helpers
reference unavailable port/node/file APIs without blocking compilation. Actual
calls still raise `error:undef`. Strict validation remains the default. Nera's
own stdlib supplies the collection subset used here; OTP EUnit is not copied,
rewritten or replaced. An early grouped run stalled because map list keys used
address equality: EUnit stored `[1]` and later looked it up using a reconstructed
list. `tests/maps_structural_keys.erl` fails before that fix and now passes on
both engines. Other minimal regressions cover try-call short circuiting, exact
nested numeric equality, module metadata, leaders, priority, time, Unicode,
formatting, suffixes and repeated properties.

This does not claim all EUnit/OTP APIs, I/O capture, priority-policy equivalence,
other Cowlib modules, native PropEr generation/shrinking or other platforms.
Previous property replay evidence below remains historical and was not rerun
as part of this native framework validation. Thoas's framework is not rerun.
GitHub CI/Release remain manually paused.

```sh
python3 tests/integration/cowlib/native_eunit.py \
  --upstream /tmp/cowlib-pristine --proper /tmp/proper-1.5.0 \
  --nera ./target/release/nera --eunit /path/to/otp/lib/eunit-2.11 \
  --gb-trees /path/to/otp/lib/stdlib-8.0.3/src/gb_trees.erl \
  --work-dir /tmp/nera-cowlib-native-eunit
python3 tests/integration/eunit/deferred_calls.py --nera ./target/release/nera
python3 -m unittest discover -s tests/integration/cowlib -p 'test_*.py'
```

Final local checks on the recorded code/test source hash pass: release build,
**1,947 Rust tests** (1 ignored), Clippy with warnings denied, **353 native tests**
(2 external Onecached skips), **58 Cowlib harness tests** and four deferred-call
checks. The first sandboxed native run rejected nine local TCP socket tests with
`eacces`; the complete run with local socket access passes. The later PR #87
atomic-resource changes on main are outside this pinned `b772a55` validation.

The OTP installation must include Erlang source (`src`) and EUnit headers.
Build the pinned PropEr dependency first using the existing upstream HTTP
runner instructions; BEAM needs it to compile the untouched upstream TEST
modules, even though this run does not execute properties. Use a fresh external
work directory for each compiler/runtime revision.

## Macro block arguments and HTTP property replay — 2026-10-14

From merged main `a40da33` (PR #84) plus the macro argument fix, Nera preserves
commas inside keyword blocks when collecting function-like macro arguments.
The collector tracks blocks terminated by `end`, including nested blocks and
anonymous/named functions, alongside punctuation nesting. Function references
such as `fun F/A` and `fun M:F/A` do not introduce a block. A comma separates
arguments only outside both kinds of nesting.

The minimal project-owned `tests/macro_block_arguments.erl` reproduces the
previous failure: `?PAIR(begin X = 1, X + 1 end, 3)` reports
`undefined macro: PAIR` before the fix. The focused Rust regression fails for
the same reason. Both now pass; the Erlang regression also passes on BEAM.
Controls cover nested `case`, anonymous/named functions and function references;
lexical unit tests additionally cover `if`, `receive`, `try` and `maybe`
argument boundaries. These lexical checks do not add execution support for
otherwise unsupported language features.

The HTTP runner defaults to `--property-envelope macro`: it preserves all
26 original `?FORALL` calls and predicate text in the external Nera source,
replacing the PropEr include with a three-argument macro that returns the
predicate closure. **Nera's preprocessor now collects these arguments**;
the Python `property_closure` splitter is not used in this mode.
`--property-envelope closure` retains the previous workaround for historical
reproduction. All upstream code, generated input and adapted copies remain
external, at the same Cowlib/PropEr pins documented below.

| Revalidation | BEAM | Nera |
| --- | ---: | ---: |
| All 26 `cow_http_hd` properties, 100 shared seeded instances each | **2,600/2,600** | **2,600/2,600** |
| `cow_http` original assertions | **102/102** | **102/102** |
| `cow_http_hd` original assertions | **396/396** | **396/396** |

There are no blocked/skipped assertions or property instances. Input generation
uses the same positive sizes and seeds as the prior replay; the ETF input hash
is unchanged (`31199f5b961e6b2bcd68a1f1cf31cf24ef8a79cf09f3effba0ef37151776a66e`).
The deliberately false BEAM property and false Nera adapter assertion are still
rejected. Native instances run in isolated processes; cumulative execution,
PropEr scheduling and shrinking remain outside this validation.

Local checks pass a fresh release build, workspace Clippy with warnings denied,
**1,925 Rust tests** (1 ignored), **341 native tests** (2 external Onecached
skips), and **55 harness tests**. Native EUnit and PropEr framework probes still
fail to link (`eunit:test/2`, `proper:quickcheck/1` unavailable). This fixes macro
argument collection; it does not provide those frameworks. Horse benchmarks,
other Cowlib suites/corpora, Linux/Windows, cross targets and installed archives
were not rerun. GitHub workflows remain paused.

```sh
RUSTC_WRAPPER= cargo build --release --locked
./target/release/nera test macro_block_arguments -j 1 --timeout 30
python3 tests/integration/cowlib/upstream_http.py \
  --upstream /tmp/cowlib-pristine --proper /tmp/proper-1.5.0 \
  --nera ./target/release/nera --work-dir /tmp/nera-macro-block-cowlib \
  --samples 100 --property-envelope macro
python3 -m unittest discover -s tests/integration/cowlib -p 'test_*.py'
```

[Evidence](macro-block-evidence.json) binds source/compiler/runtime hashes,
RED/GREEN checks, inventories, original macro calls, shared inputs and the
complete external report. Earlier sections retain their historical results;
the macro limitation described in the PR #84 section is resolved here.

## HTTP upstream assertions and property replay — 2026-10-14

Validation starts from merged main `c0f3b57` (PR #83 included), with a fresh
release build on macOS arm64, OTP 29.0.5. Cowlib remains pinned to 2.20.0 at
`c768a804565ff5b8178ed968a5921e469d6bd7b2`; PropEr 1.5.0 is pinned at
`5640d0715d4b346676267504d8e84398e2a29f75`. Checkouts, adapted modules,
inputs, dependencies and executables stay outside the Nera repository.

| Suite | BEAM | Nera | Scope |
| --- | ---: | ---: | --- |
| `cow_http` EUnit | **102/102** | **102/102** | Original EUnit on BEAM; original test bodies via external adapter on Nera |
| `cow_http_hd` EUnit | **396/396** | **396/396** | Same contract; no blocked/skipped assertions |
| `cow_http_hd` properties | **2,600/2,600** | **2,600/2,600** | All 26 original predicates, 100 shared seeded instances each |

The five `cow_http` test functions cover `parse_fullpath`, `ensure_scheme` and
`ensure_token`; they do not cover every HTTP semantic-processing API. The
`cow_http_hd` run inventories every `_test`/`_test_` function in the pinned
module and checks each generator's cardinality. Tests succeed on normal
completion, as in EUnit; a non-`ok` value is allowed, an exception is not.
Each Nera assertion runs in its own process, and exact completion markers,
exit status and output length prevent incomplete runs from passing.

For properties, the original PropEr generators produce inputs on BEAM with
seed `{17001, sample_index, property_index}` and size
`1 + ((sample_index - 1) rem 50)`. Positive sizes support `non_empty`
generators. BEAM uses `proper:check/3` against the original property for each
input; Nera evaluates the exact original predicate on the same ETF input.
Generation repeated with the same seeds produced the same input SHA-256
`31199f5b961e6b2bcd68a1f1cf31cf24ef8a79cf09f3effba0ef37151776a66e`.
This is finite, reproducible property-instance replay, not native PropEr
random generation, scheduling or shrinking. Native runs are isolated;
cumulative execution of these HTTP properties is not claimed.

The external adapter preserves production functions and assertion bodies.
It removes Horse benchmarks and PropEr generator helpers from the Nera
copies; the property copy retains required pure helpers. It replaces only
`?FORALL(Variable, Generator, Predicate)` with `fun(Variable) -> Predicate end`,
keeping predicate text intact. No expected values or comparators are changed.
A deliberately false PropEr predicate is rejected on BEAM, and a false
assertion in the Nera property adapter is rejected rather than counted as a
successful completion. The 54 harness tests pass, including preservation,
tuple binders, block predicates, conditional rejection and completion checks.

Framework limitations remain:

- The bundled Nera stdlib does not provide `eunit:test/2`: both native EUnit
  framework probes fail to link. The 498 Nera results above are assertion-body
  executions, not a native EUnit runner or autoexport validation.
- It does not provide `proper:quickcheck/1`. Native PropEr is unavailable;
  generator execution and `proper:check/3` run only on BEAM.
- Nera macro argument splitting does not account for commas inside
  `begin … end` blocks: a three-argument `FORALL` macro reports
  `undefined macro: FORALL`. A minimal project-owned external probe reproduces
  this. The narrow property-envelope adapter bypasses the limitation; no
  compiler fix is included in this validation change.

There are no observed assertion or property-instance mismatches in this
scope. Horse benchmarks, other Cowlib upstream suites, full core/native Rust
regressions, Linux/Windows, cross targets and installed archives were not
rerun. The previous 321-case corpus result below remains its dated result;
this run expands upstream assertion/property coverage and does not replace
that corpus with a claim of full Cowlib compatibility. GitHub workflows
remain manually paused.

Reproduction (all source/dependency paths must be external and pinned):

```sh
RUSTC_WRAPPER= cargo build --release --locked
# Build the pinned PropEr checkout once with rebar3 compile.
python3 tests/integration/cowlib/upstream_http.py \
  --upstream /tmp/cowlib-pristine \
  --proper /tmp/proper-1.5.0 \
  --nera ./target/release/nera \
  --work-dir /tmp/nera-http-upstream --samples 100 --property-envelope closure
python3 -m unittest discover -s tests/integration/cowlib -p 'test_*.py'
```

The runner checks clean dependency checkouts and pins, builds dependencies
with upstream `make all`, and compiles the original TEST modules for BEAM.
It records commands, inventories, source/header/adapter/compiler/runtime
hashes, seeded input hashes and every case result in the external report.
[Consolidated evidence](http-upstream-evidence.json) records the totals,
framework probes and hashes of the complete external report/logs.

## HTTP/2 Content-Length filtering — 2026-10-14

From merged main `9ebe3ff` (PR #82 included), the compiler now validates
comprehension generator patterns before destructuring and evaluating filters
or the body. A nonmatching element resumes the helper with the accumulator
unchanged. This reuses the shared pattern compiler and preserves generator
shadowing, nested iteration and result order.

The three former HTTP/2 failures had the same cause: the comprehension selecting
only `content-length` values also collected unrelated `accept`, `cookie` and
`te` values because Nera ignored the binary literal in its generator pattern.
`http_h2_get`, `http_h2_cookie_duplicate` and `http_h2_te` now return exactly
the BEAM result, including ordinary headers, pseudo-header maps and `undefined`
expected size. No Cowlib or comparator changes were made for this fix.

On macOS arm64, OTP 29.0.5, unchanged external Cowlib 2.20.0 at the pin above:

| Module | BEAM | Nera conformant | Blocked/skipped |
| --- | ---: | ---: | ---: |
| cow_http | 136/136 | **136/136** | 0 |
| cow_http_hd | 63/63 | **63/63** | 0 |
| cow_qs | 29/29 | **29/29** | 0 |
| cow_cookie | 34/34 | **34/34** | 0 |
| cow_date | 26/26 | **26/26** | 0 |
| cow_base64url | 33/33 | **33/33** | 0 |

All 321 corpus cases conform under their existing comparison contracts.
For `cow_http`, **135 outcomes are exact**, and `http_merge_request` retains
the existing semantic prefix-order comparison with raw `wire_equal: false`.
The four-module core retains its existing Set-Cookie order comparison and
Max-Age time-window assertions. No additional normalization was introduced.
This validates the corpus, not all Cowlib or the full upstream HTTP suites;
`cow_http_hd` coverage remains 12/68 public functions.

The project-owned `tests/lc_generator_pattern.erl` minimal selector fails on
the old compiler with `[7]` instead of `[]` and passes on BEAM and corrected
Nera. Controls cover mixed tuple shapes, skipped literals before filters,
repeated variables, maps, binary tails, nested generators, binary output and
intentional generator shadowing. The new fixture is also checked by the IR
structural verifier. No upstream implementation or test bodies are copied.

Local checks pass the release build, workspace Clippy with warnings denied,
**1,923 Rust tests** (1 ignored), **340 native tests** (2 external Onecached
skips), and **47 harness tests**. The full native suite runs in series and
includes the URI regressions. GitHub workflows remain manually paused.

```sh
RUSTC_WRAPPER= cargo build --release --locked
./target/release/nera test lc_generator_pattern lc_nested -j 1 --timeout 30
python3 tests/integration/cowlib/runner.py --module cow_http \
  --nera ./target/release/nera --work-dir /tmp/nera-http2-pattern-cowlib
TZ=Pacific/Honolulu python3 tests/integration/cowlib/runner.py \
  --module cow_http_hd --module cow_qs --module cow_cookie --module cow_date --module cow_base64url \
  --nera ./target/release/nera --work-dir /tmp/nera-http2-pattern-surrounding
```

[Evidence](http2-content-length-evidence.json) binds source/executable hashes,
commands, RED/GREEN checks, exact raw before/after HTTP/2 outcomes and the
surrounding rerun. Checkouts and builds remain external; no Cowlib code or
headers are vendored. Upstream suites/properties, Linux/Windows, cross-target,
installed archives and dedicated performance benchmarks were not rerun.
Older sections below retain their dated results and validation contracts.

## Pseudo-header comparison contract — 2026-10-14

From merged main `514b35b` (PR #80 included), all **136** `cow_http` cases
execute on BEAM and Nera with unchanged external Cowlib 2.20.0 at the pin above,
macOS arm64, OTP 29.0.5. BEAM satisfies 136/136 expectations; Nera is
**133/136 semantically conformant: 132 exact outcomes plus one pseudo-header
prefix permutation**. Both dependency builds succeed, with no blocked/skipped
cases. Raw results remain in the report; `http_merge_request` still has
`wire_equal: false`.

The former ordering mismatch was an invalid expectation in the comparator.
Cowlib's merge builder folds over `maps:to_list/1`. Its enumeration order is
[undefined without an ordered iterator](https://www.erlang.org/doc/apps/stdlib/maps.html#to_list/1).
[HTTP/2 section 8.3](https://www.rfc-editor.org/rfc/rfc9113.html#section-8.3)
requires pseudo-header fields to precede regular fields and prohibits duplicate
pseudo-header names; it does not prescribe relative order within that prefix.
Both observed outputs satisfy these constraints and contain the same bytes.
Nera and upstream implementations are unchanged.

Only `http_merge_request` opts into `request_pseudo_headers`. The comparator
sorts the leading request pseudo-header pairs after rejecting duplicates,
unknown names and any pseudo-header after a regular field. Names and values
remain byte-exact. The regular suffix retains exact order, bytes and repeated
fields. Malformed pairs, non-binary fields and exceptions are rejected.
All other HTTP cases keep exact comparison; the existing scoped Set-Cookie
attribute comparison remains unchanged for the core corpus.

A minimal permutation regression fails before implementing this mode. The
additional negative controls reject changed names/values, missing or added
fields, duplicate pseudo fields, malformed pairs and reordered regular fields.
A corpus guard enforces that only this one case opts in. **47 harness tests
pass** after the fix, and a fresh release build from main succeeds. This is a
validation-harness correction; the full Rust/native suites were not rerun.

The three remaining exact mismatches are `http_h2_get`,
`http_h2_cookie_duplicate` and `http_h2_te`, all concerning HTTP/2 content-length
handling. They are not normalized, exempted or counted as passes. The corpus
runner correctly exits 1 for these failures.

```sh
RUSTC_WRAPPER= cargo build --release --locked
python3 -m unittest discover -s tests/integration/cowlib -p 'test_*.py'
python3 tests/integration/cowlib/runner.py --module cow_http \
  --nera ./target/release/nera --work-dir /tmp/nera-pseudo-headers-cowlib
```

[Evidence](pseudo-header-order-evidence.json) records exact raw before/after
outputs, the comparison rationale, source/executable hashes, commands and
remaining failures. No upstream code, headers or tests are vendored; checkouts
and builds remain external. Other Cowlib corpora and full upstream HTTP suites
were not rerun. Older sections retain their dated results and contracts.

## Repeated request-line spaces — 2026-10-14

From merged main `36f6d5d` (PRs #78 and #79 included), `cow_http` now passes
**132/136** project-owned corpus cases against **136/136** BEAM expectations,
using unchanged external Cowlib 2.20.0 at the pin above, macOS arm64 and
OTP 29.0.5. Both dependency builds succeed; all 136 native cases execute with
no blocked or skipped cases. No comparator normalization was introduced.

`http_request_spaces` now returns
`{<<"GET">>, <<"/x">>, 'HTTP/1.1', <<>>}` exactly like BEAM. Cowlib uses
`binary:split/3` with `[trim_all, global]`, but Nera previously ignored
`trim_all`: empty fields between repeated spaces caused a `badmatch`.
The runtime now removes empty parts only when that option is present, with or
without `global` and regardless of option order. Calls without it retain empty
parts. No compiler or Cowlib source changes are needed.

The project-owned `tests/binary_split_trim_all.erl` regression fails before
the fix and passes on BEAM and corrected Nera. It covers consecutive, leading
and trailing separators, all-empty/empty input, no match, first-match mode,
option order and preserved empty parts without `trim_all`.

Four exact mismatches remain unchanged: `http_merge_request` (pseudo-header
order), `http_h2_get`, `http_h2_cookie_duplicate` and `http_h2_te`
(HTTP/2 content-length handling). They are recorded without normalization.

Local checks pass the release build, **1,916 Rust tests** (1 ignored),
workspace Clippy with warnings denied and three focused native fixtures:
`binary_split`, `binary_split_trim_all` and `uri_string_regression` (172 URI
cases). Publication validation also passes the full serial native suite:
**338 passed, 0 failed, 2 skipped / 340**; the skips require external Onecached
modules. Production and regression sources are unchanged between these runs.
Other Cowlib corpora and upstream HTTP suites were not rerun. Other split
options and list/compiled patterns remain outside this change's scope.

```sh
RUSTC_WRAPPER= cargo build --release --locked
./target/release/nera test binary_split binary_split_trim_all uri_string_regression -j 1 --timeout 30
python3 tests/integration/cowlib/runner.py --module cow_http \
  --nera ./target/release/nera --work-dir /tmp/nera-http-spaces-cowlib
```

[Evidence](http-request-spaces-evidence.json) preserves source/executable
hashes, the failing regression, exact before/after HTTP outcomes, commands and
remaining mismatches. Upstream checkout/builds remain external; no Cowlib code
or upstream tests are copied into this repository. Earlier sections below
retain their dated validation scopes, including the prior header corpus pass.

## Binary tail boundaries — 2026-10-14

Starting from merged main `bc8c3b8` (PR #77), `cow_http_hd` passes **63/63**
project-owned corpus cases against **63/63** BEAM expectations on macOS arm64,
OTP 29.0.5, with unchanged external Cowlib 2.20.0 at the pin above. All native
cases execute; no blocked/skipped cases or comparator normalization.
This covers 12/68 public functions, not the full upstream EUnit/PropEr suites.

The four malformed Content-Length inputs now preserve exact reasons:

| Input | BEAM and Nera error reason |
| --- | --- |
| `<<"3.0">>` | `{badmatch, {3, <<".0">>}}` |
| `<<"3 ">>` | `{badmatch, {3, <<" ">>}}` |
| `<<"3, 3">>` | `{badmatch, {3, <<", 3">>}}` |
| `<<"3\r\n">>` | `{badmatch, {3, <<"\r\n">>}}` |

An internal match cursor was forwarded on a tail call to a function with a
variable fallback. The fallback exposed that cursor in a tuple: internal
binary equality accepted it, while ETF encoded it as `undefined`. Tail-call
lowering now forwards cursors only for local argument positions matched by
binary patterns in every clause; other callees receive materialized tails.
This also fixes the three valid Range cases: their tail calls to
`binary_to_integer/1` previously received internal cursors and raised `badarg`.
Safe local binary-only loops retain cursor reuse. The full native suite also
exposed a latent defect in materializing bitstrings: offsets and lengths were
rounded down to byte boundaries. Materialization now packs the exact bit
window and preserves its bit count; aligned refcounted tails keep zero-copy
sharing. A runtime regression covers all starting bit positions, partial final
bytes and heap/refcounted inputs, including match-cursor inputs.
The conservative boundary
may allocate more sub-binaries; no performance benchmark was run.

`tests/binary_tail_guard_fallback.erl` is a project-owned minimal recursive
scanner, not a copy of Cowlib: it checks serialization of returned tails and
badmatch reasons, plus ordinary local, BIF and remote call boundaries. Its
serialization assertion fails on the pre-fix compiler with `{badmatch,false}`
and passes on BEAM and the corrected Nera. An IR regression verifies both the
safe optimization and required materialization boundaries.

Reproduce from the repository root (upstream checkout/builds stay external):

```sh
RUSTC_WRAPPER= cargo build --release --locked
./target/release/nera test binary_tail_guard_fallback -j 1
python3 tests/integration/cowlib/runner.py --module cow_http_hd \
  --nera ./target/release/nera --work-dir /tmp/nera-content-length-cowlib
```

The surrounding rerun preserves core **122/122** and `cow_http` **131/136**
against BEAM (136/136). Its five exact mismatches are unchanged: request
whitespace, pseudo-header order and three HTTP/2 content-length results.

Final local checks pass the release build, **1,916 Rust tests** (1 ignored),
workspace Clippy with warnings denied and **337 native tests** (2 external
Onecached skips), including the 172 URI regressions. A parallel native run
failed `tcp_reactor_perf`'s batch-variance threshold; the isolated rerun and
complete final serial run pass. The failed attempt remains in the evidence
and is not counted as a successful gate. No remote CI result is claimed under
the temporary workflow pause.

[Evidence](content-length-evidence.json) records source and executable hashes,
exact typed outcomes, commands and local regression checks. Prior dated
sections below retain historical results.

## URI support and native HTTP execution — 2026-10-14

From merged main `20afa67`, the project-owned stdlib now exports
`uri_string:parse/1`, `normalize/1`, `recompose/1` and `resolve/3`. It handles
URI references and component maps, binary/list representations, authority and
IPv6, component quoting, UTF-8 percent-encoding, percent normalization, dot
segments and RFC 3986 relative-reference resolution. Other URI APIs and
exhaustive malformed-input/scheme compatibility remain outside this scope.
Design follows the [documented OTP API](https://www.erlang.org/doc/apps/stdlib/uri_string.html);
no OTP or Cowlib implementation is copied.

The reproducible project-owned URI corpus passes **172/172** on BEAM and
Nera, including normal and abnormal RFC resolution examples, map/list/binary
forms, mixed component representations and errors. Generate its expected
values with the installed BEAM version:

```sh
escript scripts/generate-uri-regression.escript tests/uri_string_regression.erl
nera test uri_string_regression exception_before_guard map_missing_key_pattern -j 1 --timeout 30
```

The generator inputs are `scripts/fixtures/uri_cases.erl`. Numeric rendering
preserves Unicode codepoints independently of source-string formatting.
Nine project-owned assertions in `cowlib_uri_probe.erl` also pass on both
engines while calling unchanged external `cow_link`: absolute normalization,
relative resolution, query/fragment handling, anchors and anchor rejection.
The complete external HTTP dependency closure is linked for this probe.
After running the HTTP corpus into an external workdir, reproduce it with:

```sh
python3 tests/integration/cowlib/uri_probe.py \
  --nera /absolute/path/to/nera --work-dir /tmp/cowlib-uri-validation
```

The probe reuses that workdir's pinned upstream and verified BEAM build,
checks source hashes, compiles its own assertion module externally and
requires the final `ok` marker from both engines.

Two compiler defects surfaced and were corrected with minimal regressions:

- Calls before a later guard were mistakenly treated as try-protected;
  their exceptions were swallowed and subsequent clauses produced other
  errors. IR now records lexical protection for calls and potentially raising
  operations, including intermediate operations within a try expression.
- Map patterns extracted missing keys with `maps:get`, raising `badkey`.
  A presence check now rejects the pattern and selects the next clause;
  a failed match expression still raises `badmatch` as on BEAM.

The final strict rerun executes every native HTTP case:

| Target | BEAM expectations | Nera result |
| --- | ---: | --- |
| `cow_http_hd` initial subset, run first | 63/63 | 56/63 pass; 7 mismatches |
| `cow_http`, run second | 136/136 | 131/136 pass; 5 mismatches |
| Four-module core | 122/122 | 122/122 pass |

No native HTTP case is blocked or skipped. Cowlib HTTP is **not fully
validated**: 12 differences remain, with exact typed outcomes preserved in
[the evidence](http-uri-evidence.json):

- Four malformed Content-Length cases return `{badmatch,{3,undefined}}`
  instead of retaining the remaining binary: `hd_length_float`,
  `hd_length_space`, `hd_length_list`, `hd_length_control`.
- Three valid Range cases raise `badarg`: `hd_range_one`, `hd_range_open`,
  `hd_range_multi`.
- `http_request_spaces` retains empty split fields and fails matching.
- `http_merge_request` returns a different pseudo-header order.
- `http_h2_get`, `http_h2_cookie_duplicate`, `http_h2_te` return incorrect
  content-length error tuples despite valid unrelated regular headers.

The next scoped investigation is the lost binary remainder and valid Range
failures. The causes of these remaining differences have not been established;
they must not be normalized away or counted as passes. The header corpus is
still only 12/68 public functions. Native upstream HTTP EUnit/PropEr suites and
Base64url properties were not rerun.

Local checks pass the release build, **1,912 Rust tests** (1 ignored), workspace
Clippy with warnings denied, **335 native tests** (2 external Onecached skips)
and **39 harness tests**. The compiler and regression source hashes are bound
to these results. Linux/Windows, installed-archive and cross-target execution
are not claimed; GitHub CI/Release remain manually paused.

One invalid intermediate validation attempt had a successful `make` exit but
no BEAM modules; every BEAM case returned `undef`. Those results are excluded
from the compatibility verdict. The harness now prefers GNU `gmake` when
available, explicitly builds `all`, and rejects missing `.beam` files before
executing cases. Two regression tests cover this prerequisite check. Final
runs use fresh pristine external Cowlib 2.20.0 checkouts, with the original pin.
No upstream source, tests, headers or dependencies are vendored.

Publication validation after rebasing onto main `2cf9df0` (PR #76) also passes:
1,914 Rust tests (1 ignored), workspace Clippy, 336 native tests (2 Onecached
skips), 39 harness tests, all 172 URI cases and the nine direct `cow_link`
assertions. The core remains 122/122 and HTTP results remain 56/63 and 131/136.
The evidence records the exact validated commit/tree and rebuilt executable
hashes; later edits are documentation/evidence only.

Earlier sections retain historical results and blockers.

## Keytake support — 2026-10-13

On merged main `8c0e3a2` plus this change, `lists:keytake/3` is implemented
in Nera's project-owned Erlang stdlib. It removes the first matching tuple,
returns `{value, Tuple, Remaining}` or `false`, and preserves the order of
remaining elements. Matching uses `==`, so integer and float keys compare
numerically. Guards skip non-tuples and short tuples without raising errors.
Invalid positions raise `function_clause`; a successful match preserves an
improper tail, while reaching an invalid tail without a match fails.

The project-owned `tests/lists_keytake.erl` passes on BEAM and Nera: first,
middle and last matches, duplicate keys, numeric and nested-list equality,
non-tuples/short tuples, huge positions, invalid positions and lists, improper
tails, dynamic calls, function references and a 20,000-element prefix. The
fixture failed before implementation because `lists:keytake/3` was unresolved.
Nineteen native list/guard regressions pass, along with the release build,
1,910 Rust tests (1 ignored), workspace Clippy with warnings denied and all
37 harness tests.

The fresh release rerun gives:

| Target | BEAM expectations | Nera result |
| --- | ---: | --- |
| `cow_http_hd` initial subset, run first | 63/63 | 63 blocked; 0 executed |
| `cow_http`, run second | 136/136 | 136 blocked; 0 executed |
| Four-module core | 122/122 | 122/122 pass |

The complete native HTTP builds no longer report `lists:keytake/3` as missing.
Their remaining build blockers are four functions in `cow_link`:
`uri_string:parse/1`, `normalize/1`, `recompose/1` and `resolve/3` at lines
247, 249, 265 and 299. These URI operations are the next scoped step before
native HTTP execution. This is still an initial subset of 12/68 public header
functions, not validation of the full upstream HTTP suites.

Cowlib remains pinned at 2.20.0, unchanged in external checkouts; no upstream
source, tests or dependencies are copied into this repository. GitHub CI and
Release remain paused. The full native suite, Base64url properties,
scheduler fairness and cross-platform/archive checks were not rerun for this
change. [Evidence](http-keytake-evidence.json) records exact source and artifact
hashes, commands and results. Earlier sections retain historical blockers.

## Base64 default arities — 2026-10-13

Starting from merged main `fadb662`, `base64:encode/1` and `base64:decode/1`
now delegate to the existing arity-two API with an empty options map: standard
alphabet and default padding. This retains the recently merged Rust binary
codecs and incremental-list decoder without changing their algorithms.

The project-owned `tests/base64_default.erl` passes on BEAM and Nera. It covers
RFC vectors, standard-alphabet bytes, binary and flat-list input, whitespace,
missing padding, invalid terms, dynamic calls and function references. An
isolated build with the original main Base64 module fails precisely at the
two missing arities using the same compiler and unchanged fixture.

| Target | BEAM expectations | Nera result |
| --- | ---: | --- |
| `cow_http_hd` initial subset, run first | 63/63 | 63 blocked; 0 executed |
| `cow_http`, run second | 136/136 | 136 blocked; 0 executed |
| Four-module core | 122/122 | 122/122 pass |

The complete native HTTP dependency closures no longer report missing Base64
functions. Five unresolved functions remain: `lists:keytake/3` in
`cow_link.erl:82` and `uri_string:parse/1`, `normalize/1`, `recompose/1`,
`resolve/3` in `cow_link.erl:247,249,265,299`. Both builds exit 1, so none of
the 199 blocked native cases counts as a pass. The next scoped step is
`lists:keytake/3`, followed by the required URI operations. Cowlib 2.20.0 remains
pinned and unchanged in external checkouts; no upstream source is vendored.

Local validation passes the release build, 1,904 Rust tests (1 ignored),
workspace Clippy with warnings denied, four focused native regressions, all
four existing Base64 production fixtures against BEAM (including large binary
inputs and list decoding), and 37 harness tests. Actionlint reports an inherited
ShellCheck SC2012 finding in unchanged `release.yml:138`; YAML validation with
ShellCheck disabled passes. The full native suite, scheduler fairness,
Base64url properties, upstream HTTP suites and cross-platform/archive checks
were not rerun. Existing out-of-range list differences documented in FEATURES
remain outside this change. GitHub CI/Release remain manually paused.

[Evidence](http-base64-default-evidence.json) records source/compiler/runtime
hashes, exact commands, the RED counter-verification and report hashes. Work
is isolated from unrelated local binary changes. Earlier sections are historical.

## Iolist size fix — 2026-10-13

`erlang:iolist_size/1` is now implemented and auto-imported. It counts binary
metadata without flattening or copying payloads, traverses nested lists
iteratively, accepts binary tails and rejects invalid iodata with `badarg`.
Checked accumulation raises `system_limit` if the host size limit is exceeded.
The project-owned `tests/iolist_size.erl` regression passes on BEAM and Nera,
including dynamic calls, function references, byte boundaries, binary storage
sizes, a binary view at a non-byte-aligned offset, 20,000 nested lists and
invalid roots, elements and tails. Runtime tests exercise shared payloads and
size-limit rejection without requiring an enormous allocation.

A fresh release from main `964dc19` plus this fix gives:

| Target | BEAM expectations | Nera result |
| --- | ---: | --- |
| `cow_http_hd` initial subset, run first | 63/63 | 63 blocked; 0 executed |
| `cow_http`, run second | 136/136 | 136 blocked; 0 executed |
| Four-module core | 122/122 | 122/122 pass |

The complete native HTTP dependency closures pass the `iolist_size/1` step,
but remain blocked at function resolution by seven missing functions:

- `base64:decode/1` in `cow_http_hd.erl:1029` and `base64:encode/1` in
  `cow_http_struct_hd.erl:474`; our Base64 module currently exports arity two.
- `lists:keytake/3` in `cow_link.erl:82`.
- `uri_string:parse/1`, `normalize/1`, `recompose/1` and `resolve/3` in
  `cow_link.erl:247,249,265,299`; Nera has no `uri_string` module.

The strict runner exits 1 and counts no blocked native case as a pass. These
are build blockers, not observed HTTP execution failures. The next scoped
step is Base64 arity-one support, then `lists:keytake/3` and the required URI
operations before rerunning the same HTTP commands below. Cowlib remains
pinned at 2.20.0 and all upstream sources stay unchanged outside this repository.
The header corpus still covers only 12 of its 68 public functions.

Local validation passes 1,828 Rust tests (1 ignored), workspace Clippy with
warnings denied, three focused native regressions and 37 harness tests.
The full native suite initially gives 309 passes, nine TCP `eacces` failures
and two skips; rerunning those nine tests outside the socket-restricted sandbox
passes 9/9. Combined coverage is 318 passed, zero unresolved failures and two
Onecached tests skipped because their external modules are absent.
[Recorded evidence](http-iolist-size-evidence.json) binds source and executable
hashes to this worktree, records the failing regression before implementation,
and retains exact rerun commands and report hashes. GitHub CI/Release remain
manually paused; no remote CI, cross-platform result or upstream HTTP suite
execution is claimed. Earlier sections retain historical results.

## Large integer literal fix — 2026-10-12

The integer-literal ceiling is resolved. Decimal and `Base#Digits` literals
outside `i64` retain their exact value through lexing, expressions, patterns,
macros and IR. Compiled object data supplies immutable decimal bytes to the
arbitrary-precision runtime. Small integers retain their original fast path.
The formatter and source analysis understand the new literal variant too.

The project-owned `tests/integer_large_literal.erl` regression passes on BEAM
and Nera: i64 boundaries, large positive/negative values, base notation, macro
expansion/stringification, inactive TEST sections, patterns, guards, maps and
arithmetic. It also exposed a boxed-integer binary construction bug that wrote
zero; the fix preserves low bits, sign extension, byte order and partial bytes.
Binary tests cover values exceeding i64 and 128 bits. Formatting the fixture
and executing the result on BEAM preserves its behavior.

A fresh release on PR #53 head `bb8a71f` plus the large-literal changes gives:

| Target | BEAM expectations | Nera result |
| --- | ---: | --- |
| `cow_http_hd` initial subset, run first | 63/63 | 63 blocked; 0 executed |
| `cow_http`, run second | 136/136 | 136 blocked; 0 executed |
| Four-module core | 122/122 | 122/122 pass |

Both complete HTTP dependency builds now pass lexing and reach semantic
validation. Their next common blocker is **`iolist_size/1`**, an undefined
BIF in `cow_http2.erl:412`. The strict runner exits 1 and counts no blocked
native case as a pass. All upstream sources remain unchanged outside this
repository. The next priority is supporting `iolist_size/1`, then rerunning
`cow_http_hd` before `cow_http` with the same commands below.

Validation passes **1,822 Rust tests** (1 ignored, serial), workspace Clippy,
four native regressions and all 37 harness tests. The focused large-literal
lexer test failed before the fix; the original inactive-TEST probe now passes
Nera checking. [Large-literal evidence](http-big-literal-evidence.json) records
source/compiler/runtime hashes, before/after failures and exact results.
Publication validation after integrating main `fd364b5` also passes 1,825 Rust
tests (1 ignored), workspace Clippy and 317 native tests. Two native Onecached
tests are skipped because their external modules are absent. The 122-case core
passes again; all 199 BEAM HTTP expectations pass, and both native HTTP builds
remain blocked on `iolist_size/1`. These local macOS results are recorded in
the evidence; GitHub CI/Release are manually paused, and no new remote execution
or cross-target/installed-archive validation is claimed.

Earlier sections retain the prior blockers and their validation revisions.

## Escaped-character fix — 2026-10-12

Nera now accepts ordinary escaped character literals such as `$\#`. The minimal
`tests/char_escaped_ordinary.erl` regression passes on BEAM and Nera, including
punctuation, letters, non-octal digits, Unicode, binary construction and guards.
Named and octal escapes retain their values. Incomplete reserved escapes remain
errors. The focused lexer test failed before the fix and passes afterward.

With a release rebuild of main `4b612a7` plus the recorded lexical change, both
HTTP targets now pass the escaped-character step. Their complete dependency
builds encounter the next blocker in `cow_http_hd`: seven occurrences of the
integer literal `99999999999999999999` exceed the lexer's `i64` parser. These
occur in upstream TEST sections, but lexing happens before inactive sections
are removed. An independent project-owned inactive-TEST probe confirms this:
BEAM compiles it without TEST, while Nera fails at the large literal.

| Target | BEAM expectations | Nera result after the fix |
| --- | ---: | --- |
| `cow_http_hd` initial subset | 63/63 | 63 blocked; 0 executed |
| `cow_http` | 136/136 | 136 blocked; 0 executed |
| Four-module core | 122/122 | 122/122 pass |

Validation also passes 1,815 Rust tests (1 ignored, serial), all 299 syntax
crate tests, syntax Clippy, three native regressions and all 37 Python harness
tests. [Escaped-character fix evidence](http-escape-evidence.json) binds these
results to source/compiler hashes and preserves both RED/GREEN evidence and
the next failure. No Cowlib source was changed or copied into the repository.
The next priority is large integer literals, then rerun `cow_http_hd` first.

Publication validation after integrating main `e739528` (PR #49) also passes:
1,820 Rust tests (1 ignored), three native regressions and the 122-case core.
BEAM again satisfies all 199 HTTP expectations; Nera reports the same seven
large-literal errors before execution. The evidence records this separate
commit/tree and rebuilt compiler/runtime hashes.

## HTTP validation baseline — 2026-10-12

At this initial baseline the next priority was `cow_http_hd`, especially
`Content-Length`, `Connection`,
content types and host parsing, before broadening HTTP validation. The first
baseline also covers all 16 public functions of `cow_http`, including its
HTTP/1 aliases and HTTP/2+ header semantics. It uses Cowlib 2.20.0 unchanged
at the pin above and a release rebuild of merged main `4b612a7` (after #48).

| Module | Project-owned cases | BEAM expectations | Nera execution |
| --- | ---: | ---: | --- |
| `cow_http` | 136; all 16 public functions represented | 136/136 | 136 blocked at compilation; 0 executed |
| `cow_http_hd` | 63; initial subset of 12/68 public functions | 63/63 | 63 blocked at compilation; 0 executed |

The `cow_http` cases include complete/truncated request and status lines,
headers and iodata builders, exact error reasons, path/query splitting, token
and scheme syntax, duplicate/invalid pseudo headers, CONNECT, trailers and
content-length rules. The initial `cow_http_hd` subset covers numeric limits
(including 20-digit lengths), token lists, quoting, content types, cache
control, host/port, ranges, entity tags and invalid inputs. This is a
project-owned corpus, not a replay of the upstream test suites.

Both builds include the complete runtime dependency closure:
`cow_http`, `cow_http1`, `cow_http_hd`, `cow_cookie`, `cow_date`, `cow_http2`,
`cow_http_struct_hd` and `cow_link`. No source is pruned, stubbed or edited.
The closure contains a cycle through `cow_link` back to `cow_http`; selecting
`cow_http_hd` therefore encounters the same first blocker.

Nera rejects the valid Erlang escaped character **`$\#`** used in
`cow_http.erl:328`. A project-owned three-line probe with `35 = $\#` passes
on BEAM but fails Nera's lexer. This is the first observed blocker, not an
exhaustive inventory of missing HTTP support. Fix the shared lexer blocker,
rerun `cow_http_hd` first, then rerun `cow_http`; later failures must retain
separate compilation/runtime/behavior verdicts.

The two new modules are opt-in. Omitting `--module` still runs the validated
four-module core (122 cases), which remains the mandatory CI corpus. Its
rerun on this main revision also passes 122/122; the 37 harness tests and
workflow validation pass. No blocked
HTTP case is counted as a pass or silently added to that core verdict.
[HTTP baseline evidence](http-baseline-evidence.json) records both scopes,
compiler/runtime and source hashes, exact failures and the minimal probe.
All sources, dependencies, generated probes and executables remain outside
this repository.

Reproduce after rebuilding Nera:

```sh
python3 tests/integration/cowlib/runner.py \
  --module cow_http_hd \
  --nera /path/to/rebuilt/nera \
  --work-dir /tmp/nera-cow-http-hd-validation
python3 tests/integration/cowlib/runner.py \
  --module cow_http \
  --nera /path/to/rebuilt/nera \
  --work-dir /tmp/nera-cow-http-validation
```

Both commands exited 1 at this baseline because native compilation was blocked.
The escaped-character fix above resolves its first blocker; compilation still
fails on the subsequently exposed large integer literals. BEAM
expectations cover return versus exception outcomes; HTTP semantic errors
returned as `{error, Reason}` are return values. Native comparison retains
exact term/list order, bytes and exception class/reason; builder cases compare
the resulting wire bytes after `iolist_to_binary/1`.

## Current validation — merged main, 2026-10-12

Nera was rebuilt from merged main **`4f5a266e291d5b4c3dc56cfc060663000bb59d9c`**
(after PRs #46 and #47), on macOS arm64 with OTP 29.0.5 and rebar3 3.27.0.
Cowlib 2.20.0 and PropEr 1.5.0 retain their pinned revisions. Upstream sources,
adapted modules, inputs and executables stay in external temporary directories.
The compiler, runtime, standard library, corpus and runners are unchanged from
that main revision; this follow-up changes only CI, documentation and evidence.

The four-module differential corpus passes **122/122**: `cow_qs` 29/29,
`cow_cookie` 34/34, `cow_date` 26/26 and `cow_base64url` 33/33. No case is
blocked or skipped. The existing Set-Cookie attribute-order normalization and
two Max-Age time-window checks remain unchanged.

The original Base64url predicates pass **1,563/1,563 inputs each** with and
without padding on BEAM/PropEr and in one cumulative Nera process. Both encoded
outputs also match the independent Python oracle. The deterministic corpus and
external adapter are unchanged; this establishes finite predicate replay,
not native PropEr generation or shrinking.

The isolated Nera replay also passes 1,563/1,563 inputs per property. The RFC
7515 test passes in all three modes, all four false-property controls are
rejected, and both runners exit 0. [Current evidence](main-validation-evidence.json)
records the revision/tree, source and executable hashes, results and commands.

The Linux workflow now runs `upstream_base64url.py` as a required step after
the 122-case corpus. It builds pinned PropEr with rebar3 3.27.0 and reuses the
verified external Cowlib checkout. It retains the cumulative run, isolated
runs and all four deliberately false-property controls: an isolated pass
cannot mask a cumulative failure. The property step has a 20-minute timeout;
PropEr preparation has a 10-minute timeout. Artifact
`cowlib-base64url-properties-linux` preserves the report, framed inputs and
stdout/stderr logs even on failure, without uploading upstream source.

The existing 122-case gate already passed on Linux in
[PR #47 CI](https://github.com/processone/nera/actions/runs/38255964315), at
`e76d8938fac59e3503d819c214471e1296a90baf`. The property gate also passed
on Linux in [PR #48 CI](https://github.com/processone/nera/actions/runs/38267594628),
head `299d008`, including pinned PropEr preparation, cumulative replay and
evidence upload. Local workflow validation (`actionlint`) and all
37 harness tests pass. Use the property replay command below to reproduce;
first build Nera and prepare the pinned external dependencies.

Earlier upstream assertion results are unchanged and were not rerun here:
`cow_qs` 72/72, `cow_cookie` 53/54 (one attribute-order assertion), `cow_date`
7/7 through external adapters. Native EUnit/PropEr frameworks and the remaining
Cowlib modules are still outside the validated scope. Historical results below
retain their original revisions and verdicts.

## Bound binary matching fix — 2026-10-12

The separate `B = binary:copy(<<N>>, N)` failure found during GC reduction is
resolved. It was not a difference in equality semantics: recursive binary
matching reused an advanced match context, while sized segment extraction
still treated its input as an ordinary binary and returned an empty result.
Both pattern-lowering paths now use context-aware extraction. A related bound
tail case could fail compilation because its binary was still deferred;
pattern comparisons now materialize that value without consuming the slice
definition needed by sibling clauses.

The minimal reproducer consumes two frames from `<<0,1,1>>`: an empty frame,
then a one-byte frame. Before the fix Nera raises `{badmatch, <<1>>}` on the
second frame; BEAM returns 2. The new `tests/binary_bound_match.erl` covers
this case plus lengths 0/1/2/64/65/255, malformed frames, unequal bytes and
exact types, deferred tails, nested repeated variables and alternative clauses.
It passes on BEAM and rebuilt Nera. The original 129-frame reproducer recorded
in the GC evidence also passes unchanged on both engines.

[Bound-match evidence](bound-match-evidence.json) records the source hashes,
RED/GREEN results and current validation. The historical evidence below is
preserved, including the original failed reduction. No Cowlib code is vendored.

Current validation also passes **122/122 Cowlib differential cases**, 12 native
regressions, 1,813 Rust tests (1 ignored, serial), IR Clippy and all 37 Python
harness tests. These are local macOS arm64 results. The 1,563-input upstream
property replay below was not rerun for this compiler fix.

Reproduce the focused regression after rebuilding:

```sh
nera test binary_bound_match -j 1 --timeout 60
mkdir -p /tmp/nera-bound-match-beam
erlc -o /tmp/nera-bound-match-beam tests/binary_bound_match.erl
erl -noshell -pa /tmp/nera-bound-match-beam \
  -eval 'ok = binary_bound_match:test(), halt().'
```

## Binary GC fix — Base64url properties pass, 2026-10-12

The runtime fix on validation base `7efc0ea` resolves the cumulative failure
recorded below. Minor GC skipped old container contents but swept backing
stores based only on the pointers it encountered. A live binary behind an
old match context could therefore lose its backing storage. Nera now retains
promoted backing stores until major GC traces the whole heap; unreachable
young backing stores can still be freed at the next minor collection.

The **unchanged** property runner now exits **0** with the rebuilt compiler:

| Execution | With padding | Without padding | RFC 7515 test |
| --- | ---: | ---: | --- |
| BEAM, real PropEr replay | 1,563/1,563 | 1,563/1,563 | Pass |
| Nera, cumulative process | 1,563/1,563 | 1,563/1,563 | Pass |
| Nera, fresh process per input | 1,563/1,563 | 1,563/1,563 | Pass |

The cumulative BEAM/Nera output hashes are identical. Both encodings also
agree with the independent oracle, and all four false-property controls are
rejected. The same **122/122 differential corpus cases** pass with the existing
comparison contract. No Cowlib source or property expression was changed.
[GC fix evidence](base64url-gc-evidence.json) records the changed runtime source
hash, compiler/runtime hashes, exact results and reproduction commands.
Use the property replay command below with a freshly rebuilt compiler.

The minimal Rust regression promotes a match context and its binary, runs a
second minor collection, and checks ownership before reading the bytes. It
fails before the fix and passes afterward. A second test covers nested
maps/lists/tuples, reclaiming young garbage, retention after major promotion,
and freeing dead old storage at the next major collection. Validation also
passes 1,813 Rust tests (1 ignored, serial), runtime Clippy, five existing native
binary/closure regressions and all 37 harness tests. These are local macOS
arm64 results; native PropEr and a remote Linux verdict remain outside them.

Historical reduction finding (resolved by the bound binary matching fix above): in the project-owned loop preserved
in the evidence, `B = binary:copy(<<N>>, N)` raises `badmatch`, while the equality
form succeeds. This occurred before and after the GC fix; its cause was not
yet established during that run. The upstream predicates use equality, so this does not alter the
property verdict. The exploratory native fixture was not retained as a GC
regression because its equality form also passed before the fix.

## Base64url upstream property replay — 2026-10-12

`upstream_base64url.py` runs the two original properties, `prop_identity/0`
and `prop_identity_no_padding/0`, on **1,563 identical deterministic binaries**
per engine. It also executes the original `rfc7515_test/0`. Cowlib 2.20.0
and PropEr 1.5.0 are pinned to the revisions already used above; all upstream
sources and adapted copies remain outside the repository.

A fresh release build from merged main `15a7bcf` gives:

| Execution | With padding | Without padding | RFC 7515 test |
| --- | ---: | ---: | --- |
| BEAM, real PropEr replay | 1,563/1,563 | 1,563/1,563 | Pass |
| Nera, fresh process per input | 1,563/1,563 | 1,563/1,563 | Pass |
| Nera, cumulative process | Fails at index 67, then crashes | Incomplete | Pass before crash |

The **overall verdict is failed**, runner exit **1**. In the cumulative run,
the padded predicate becomes false at zero-based index 67 (67 bytes), and its
encoded output differs too; the following iteration aborts in
`nera_rt_binary_get_binary` at `binary_ops.rs:557`, trying to use offset 2554
in a backing buffer of length 92. That same input passes in isolation. This
points to a cumulative execution/binary lifetime problem; GC or liveness
corruption is a hypothesis, not a proven cause. No runtime fix is included.
Both complete isolated encodings match BEAM and the independent oracle for
all inputs. All four false-property controls are rejected. The **37 harness
tests pass**, including seven new tests.
[Recorded evidence](base64url-upstream-evidence.json) preserves the failing
input, raw result, panic, compiler/source hashes and separate execution counts.

The input generator uses SHAKE-256 with seed `nera-cow-base64url-v1`, plus
explicit patterns: every length 0..256, all 256 individual byte values, all
bytes together, repeated 0/255/85/170, boundaries around powers of two up to
16,385 bytes, and 1,000 generated binaries of length 0..4096. The framed
input file has SHA-256
`0ea75031528097cb037ca7295e2a7ae416521d1e2c6a9b67e4e3c18d9e533ced`.

BEAM compiles the unchanged upstream source with its real PropEr header and
replays each input using `proper:check/3`. Nera's external adapter replaces
only the PropEr header with a `FORALL` macro returning a closure; both original
property expressions and the RFC test body remain unchanged. This bypasses
the generator, not the predicate. It does **not** validate native PropEr,
random generation or shrinking. Both encoded outputs are additionally checked
byte for byte against Python's URL-safe Base64, to avoid two inverse functions
hiding the same bug. Exact completion markers and exit zero are mandatory.

The runner executes both a cumulative native process and fresh native
processes per input. A cumulative failure keeps the overall result failed,
even when isolated inputs pass. Each property is also independently forced
false in external negative-control copies, on both BEAM/PropEr and Nera;
the normal validator must reject all four controls.

Reproduce (build Nera from the revision to evaluate first):

```sh
# Reuse the pinned pristine Cowlib checkout prepared by runner.py.
git clone --depth 1 --branch v1.5.0 https://github.com/proper-testing/proper.git /tmp/nera-proper
(cd /tmp/nera-proper && rebar3 compile)
python3 tests/integration/cowlib/upstream_base64url.py \
  --upstream /tmp/nera-cowlib-validation/upstream \
  --proper /tmp/nera-proper \
  --nera /tmp/nera-cowlib-target/release/nera \
  --work-dir /tmp/nera-base64url-upstream
python3 -m unittest discover -s tests/integration/cowlib -p 'test_*.py'
```

The external report retains corpus lengths/hash, executable and dependency
hashes, commands, engine results, isolated failures and negative controls.
Per-input native logs are named `isolated-<index>.stdout/.stderr` (zero-based).
The helper's seven unit tests cover deterministic generation, binary framing,
truncated input, ETF output, false predicates and crashed/incomplete processes;
they run automatically under the existing CI harness test step. At that
baseline the property execution was a manual diagnostic. The current validation
and CI gate at the top of this document supersede that limitation.

## Consolidated main corpus — 2026-10-12

After merging PR #43, a release rebuild from main
`06ef709a0e0417c66505151ea4ba5f33887130c7` passes **122/122 cases** in a
single differential run on macOS arm64 / OTP 29.0.5, with
`TZ=Pacific/Honolulu`. All four native modules compile; no case is blocked
or skipped, and the strict runner exits **0**.

| Module | BEAM expectations | Nera differential agreement |
| --- | ---: | ---: |
| cow_qs | 29/29 | 29/29 |
| cow_cookie | 34/34 | 34/34 |
| cow_date | 26/26 | 26/26 |
| cow_base64url | 33/33 | 33/33 |
| **Total** | **122/122** | **122/122** |

The existing comparison contract is unchanged: 121 outcomes are exactly
identical; `cookie_set_build` ignores only attribute order and retains both
raw results. Two positive Max-Age cases check time-window invariants.
[Consolidated evidence](main-corpus-evidence.json) records the main commit/tree,
compiler/runtime and source hashes, every case status, and the raw cookie
order difference. Compiler, stdlib, corpus and harness sources were unchanged
during this run. No upstream code is stored in the repository.

The Linux CI workflow now includes this full differential run after installing
OTP 29.0.5, using an external work directory and preserving reports/logs even
on failure. The 30 harness unit tests and workflow validation (`actionlint`)
pass locally. At this historical baseline, remote Linux execution was pending.
It has since passed on PR #47; see the current validation above.

This historical run consolidates the **project-owned corpus only**. Upstream
suites were not rerun: the last cookie assertion result remains 53/54 (attribute order),
date 7/7 and qs 72/72 through the native adapters. Native EUnit framework and
PropEr support and other Cowlib modules remain unvalidated. Base64url upstream
property instances have since been validated as documented above. The dated
sections below preserve historical results, including blockers that are now
resolved on main.

Reproduce with the commands under [Reproduction](#reproduction), omitting
`--module` to run all four modules. The recorded local run reused the pristine
pinned checkout with `--offline`; CI fetches and verifies that same pin.

## Base64 options — cow_base64url passes, 2026-10-12

Starting from merged main `ef5e43b`, the project-owned Erlang stdlib now
implements `base64:encode/2` and `base64:decode/2`. Both accept binary or
flat byte-list inputs and return binaries. Options select standard/URL-safe
alphabets and optional padding; decoding ignores space, tab, CR and LF and
accepts nonzero unused padding bits, following observed OTP 29.0.5 behavior.
No OTP implementation or Cowlib source has been copied.

A rebuilt release compiler passes **33/33 cow_base64url corpus cases on
BEAM and Nera**, including all five expected errors; strict runner exit **0**.
The original corpus and its exact comparison contract are unchanged.
[Evidence](base64-options-evidence.json) records the main base, changed source
hashes, compiler/runtime hashes and every case result. Reproduce using the
corpus command below with `--module cow_base64url`.

`base64_options.erl` passes against BEAM and native Nera: known vectors,
standard and URL alphabets, both padding modes, all bytes, prefix lengths
0..256, 16-KiB input, flat lists, whitespace, noncanonical padding bits,
direct/dynamic calls and malformed-input error reasons. The pre-change
compiler rejects it because both functions are unresolved. The native
`binary_last_match` regression also passes, as do all 30 harness unit tests.
Only Erlang stdlib and test/documentation files changed; Rust tests were not
rerun for this addition.

Only the two requested arities are supported: MIME decoding, string-returning
APIs and arity-1 variants remain absent. Out-of-range/non-byte lists are not
covered by the compatibility claim. Other module results below are from
previous runs; the upstream Base64url EUnit/PropEr suite and other platforms
have not been validated by this corpus run.

## Main integration and cookie/date upstream suites — 2026-10-12

The earlier Cowlib PRs were merged into their respective parent branches,
not into `main`. An explicit `pull --ff-only origin main` still resolved to
`bff545cccb090b15446e9ad7784d5fa54d1944a2`. A fresh release build of that
clean main checkout cannot compile the suites: cookie is blocked by octal
character literals, date by `calendar:day_of_the_week/1`. Zero native tests
ran on that main revision. BEAM passed **54/54 cookie tests**, **7/7 date
tests**, and **1,000 trials** of `cow_date:prop_http_date/0`.
[Main baseline evidence](upstream-main-baseline-evidence.json).

The integration candidate `c40c17d` combines that main with the reviewed
Cowlib chain. Its rebuilt compiler gives:

| Scope | BEAM | Nera |
| --- | --- | --- |
| cow_cookie upstream EUnit / original assertion bodies | 54/54 | 53/54 |
| cow_date upstream EUnit / original assertion bodies | 7/7 | 7/7 |
| cow_date original PropEr property | 1,000 trials pass | Not executed |
| Project corpus: cookie / date / qs | 34/34, 26/26, 29/29 | 34/34, 26/26, 29/29 |

The single native upstream failure is `setcookie_test_` entry 1 (adapter
index 36): expected `Customer=WILE_E_COYOTE; Domain=acme.com; HttpOnly`,
actual `Customer=WILE_E_COYOTE; HttpOnly; Domain=acme.com`. Unlike the
project corpus, the upstream assertion is preserved verbatim and still
fails with `badmatch`; no normalization or exclusion hides it.
All 61 functional cases execute; none is blocked in this candidate.

The candidate passes 1,811 Rust tests (1 ignored, serial), seven focused
native regressions, runtime/IR Clippy, and 30 harness unit tests.
[Integration evidence](upstream-integration-evidence.json) is explicitly
separate from the main baseline. This does not establish a fully passing
upstream suite or native PropEr support.

`upstream_cookie_date.py` checks the pinned pristine Cowlib checkout and
PropEr **1.5.0** (`5640d0715d4b346676267504d8e84398e2a29f75`). On BEAM it
compiles original source with TEST and EUnit autoexport, then runs EUnit and
the original date property. PropEr uses its time-based default seed, so the
exact generated examples vary between runs. On Nera it adds an external
entry point to invoke every original functional test body, keeping assertions
unchanged. It checks all generator cardinalities, includes the two direct
cookie tests, and requires an exact completion marker from each process.

The native adapter removes nine date Horse benchmarks and the PropEr
helper/property region; neither is counted as a native pass. No cookie test
body is removed or normalized. Native EUnit framework execution and native
PropEr remain unvalidated. The runner exits nonzero while that full scope is
incomplete, even if all functional assertions pass.

Reproduce (all generated/upstream files stay outside the repository):

```sh
git clone --depth 1 --branch v1.5.0 https://github.com/proper-testing/proper.git /tmp/nera-proper
(cd /tmp/nera-proper && rebar3 compile)
# Build Nera in a clean checkout of the exact revision being evaluated.
TZ=Pacific/Honolulu python3 tests/integration/cowlib/upstream_cookie_date.py \
  --upstream /tmp/nera-cowlib-validation/upstream \
  --proper /tmp/nera-proper \
  --nera /tmp/nera-cowlib-target/release/nera \
  --nera-source /tmp/nera-clean-checkout \
  --work-dir /tmp/nera-cookie-date-upstream
```

## Set-Cookie comparison contract — 2026-10-12

The previous attribute-order mismatch was a false positive in the validation
contract, not a missing Nera map-order guarantee. Cowlib builds attributes
from `maps:to_list/1`. Since OTP 26, atom-key order is explicitly undefined
and may vary between VM invocations; even BEAM does not promise lexical order.
See [OTP 26 highlights](https://www.erlang.org/blog/otp-26-highlights/).

Only `cookie_set_build` opts into `set_cookie_attributes` comparison. The
cookie name/value and each complete attribute are compared byte for byte,
ignoring only the permutation of attributes. Duplicate attribute names
(including case variants) are rejected because their order can affect
interpretation. Missing or extra attributes, changed values/case/whitespace,
malformed delimiters, CR/LF and non-binary outcomes still fail. This is a
comparator for the corpus's builder output, not a general HTTP parser.
All other cases retain their previous comparison contract.

Raw BEAM/Nera results are preserved, with `comparison` and `wire_equal` fields
making semantic agreement distinct from wire equality. The regression fails
before the fix on the two recorded cookie outputs; all **27 harness tests**
pass afterward. No runtime or Cowlib source is changed for this correction.

The same 34-case cookie corpus now passes **34/34 on BEAM and Nera**,
with exit **0**, on macOS arm64 / OTP 29.0.5. `cookie_set_build` is explicitly
recorded as `wire_equal: false`; the other 33 case outcomes are equal.
[Evidence](cookie-order-evidence.json) records the comparison mode, raw
cookie outputs and source/executable hashes. Reproduce with the command
below, selecting only `--module cow_cookie`. Other modules and upstream
suites were not rerun.

## UTC clock — cow_cookie executes, 2026-10-12

`erlang:universaltime/0` now returns the current UTC datetime through the
native runtime, and the project-owned `calendar:universal_time/0` delegates
to it. Direct calls, remote funs and `apply/3` are supported. UTC conversion
uses reentrant platform functions and does not change the process timezone.

The 32 original cookie cases now execute: **31/32 match BEAM**. Two added
positive Max-Age cases also pass, bringing **cow_cookie to 33/34**; BEAM
satisfies **34/34** expectations. Parsing checks an expiry 111 seconds ahead;
building checks the complete cookie against the valid expiry dates 86417
seconds ahead. Each assertion brackets the call with system-clock reads,
so ordinary second/day rollovers do not cause spurious failures.

The remaining strict mismatch is `cookie_set_build`, the order of attributes:

- BEAM: `sid=abc; Path=/; HttpOnly; SameSite=Strict; Secure`
- Nera: `sid=abc; Secure; HttpOnly; SameSite=Strict; Path=/`

The attribute values agree. Cowlib builds attributes from `maps:to_list/1`,
whose iteration order differs here; this is separate from UTC conversion.
The harness still compares wire bytes exactly, without reordering attributes.
**cow_date remains 26/26**. The combined runner exits **1**, with no blocked
case. This is a corpus result, not validation of the upstream cookie suite.
[Evidence](universal-time-evidence.json) records the mismatch and hashes.

Reproduce after rebuilding Nera:

```sh
TZ=Pacific/Honolulu python3 tests/integration/cowlib/runner.py \
  --nera /tmp/nera-cowlib-target/release/nera \
  --work-dir /tmp/nera-cowlib-validation --offline \
  --module cow_cookie --module cow_date
TZ=Pacific/Honolulu /tmp/nera-cowlib-target/release/nera test universal_time calendar_deterministic -j 1 --timeout 60
```

`universal_time.erl` passes on BEAM and Nera with `TZ=Pacific/Honolulu` and
`TZ=Asia/Kolkata`. Two runtime tests cover epoch, day/year and leap-day
boundaries, 2038 and subsecond flooring. Workspace Rust tests pass
**1,803, with 1 ignored** (serial execution); runtime/IR Clippy and all
19 Cowlib harness tests pass. Cowlib source remains unchanged outside this
repository. Other Cowlib modules and upstream suites were not rerun.

## Gregorian calendar — cow_date passes, 2026-10-12

The project-owned Erlang `calendar` module now implements the three
deterministic functions needed here: `day_of_the_week/1`,
`datetime_to_gregorian_seconds/1` and `gregorian_seconds_to_datetime/1`.
It uses 400-year cycles with floor division, supporting year zero and
negative years/seconds as in the [OTP 29 calendar contract](https://www.erlang.org/doc/apps/stdlib/calendar.html).
No OTP implementation or Cowlib source is copied into the repository.

`calendar_deterministic.erl` passes on BEAM and native Nera. It checks epoch
values, leap and non-leap centuries, year transitions, normalization, errors,
dynamic calls and round trips for seconds through plus/minus 10^20. All
146,097 days of a Gregorian cycle are checked for round trips and weekdays;
a rolling checksum of the resulting dates matches the independently evaluated
BEAM value **998368685**. This checks the dates themselves rather than relying
only on two inverse functions agreeing with each other.

The unchanged Cowlib corpus now gives **cow_date 26/26 on both engines**.
**cow_cookie passes 32/32 on BEAM but all 32 native cases remain blocked at
compilation**, now only by `calendar:universal_time/0` and
`erlang:universaltime/0`. The combined strict runner exits 1, as expected for
a blocked module; no cookie case is counted as a native pass.
[Evidence](calendar-evidence.json) records hashes and each case's status.
Reproduce with the differential command below using
`--module cow_cookie --module cow_date`. The cow_date upstream suite and other
modules were not rerun; the scoped pass covers the 26 project-owned cases.

## cow_cookie — string:trim/3 added, 2026-10-12

The native `string:trim/3` BIF removes the next compilation blocker. It handles
valid Unicode binaries and chardata, all three directions and custom grapheme
sets using pinned `unicode-segmentation` 1.13.3 (Unicode 17). Combining marks
and CRLF are indivisible graphemes, following the
[OTP string contract](https://www.erlang.org/doc/apps/stdlib/string.html#trim/3).
No upstream implementation was copied.

`string_trim.erl` passes on BEAM and Nera: empty strings/sets, directions,
UTF-8, flat lists, combining accents, CRLF, errors, mixed chardata, long input,
`apply` and remote funs. Mixed/nested chardata results are compared as text:
Nera may flatten them into a codepoint list rather than preserve OTP's layout.
Malformed UTF-8 is rejected eagerly; some OTP fast paths preserve invalid
bytes instead, so this is not an exact compatibility claim for malformed text.
Only `trim/3`, not `trim/1`, `trim/2` or the full `string` module, is added.

Two Rust unit tests exercise grapheme boundaries and direction handling.
The full workspace passes **1,801 tests, 1 ignored**, with `--test-threads=1`;
runtime/IR Clippy passes. The original regression could not compile before
the implementation because `string:trim/3` was unresolved.

The unchanged differential corpus gives **cow_qs 29/29 on both engines;
cow_cookie 32/32 on BEAM, all 32 blocked at compilation in Nera**. Remaining
APIs: `calendar:datetime_to_gregorian_seconds/1`, `calendar:day_of_the_week/1`,
`calendar:gregorian_seconds_to_datetime/1`, `calendar:universal_time/0` and
`erlang:universaltime/0`. The strict runner exits 1; no native cookie case ran.
[Evidence](string-trim-evidence.json) records hashes, per-case status and
limitations. Reproduce with the differential command below using
`--module cow_cookie --module cow_qs`. Other modules and upstream suites were
not rerun for this change.

## cow_cookie — binary helpers added, 2026-10-12

`binary:last/1` and `binary:match/2` now resolve in the unchanged Cowlib source.
The latter accepts a binary or a proper nonempty list of nonempty binaries,
using the earliest match and the longest pattern at that offset, as specified
in the [OTP binary documentation](https://www.erlang.org/doc/apps/stdlib/binary.html#match/2).
Compiled patterns remain unsupported (`binary:compile_pattern/1` is absent).

The project-owned `binary_last_match.erl` fixture passes on BEAM and Nera:
byte values, empty input, overlapping and multiple patterns, retained binary
views, invalid types, empty/improper pattern lists, and direct/dynamic/remote
fun calls. Rust unit tests exercise all 256 byte values and pattern precedence
and ensure an invalid later pattern still raises `badarg`. The full Rust
workspace passes **1,799 tests, 1 ignored**, with `--test-threads=1` to avoid
the previously observed parallel socket-test failure. Runtime/IR Clippy passes.

The differential rerun gives **cow_qs 29/29 on both engines; cow_cookie 32/32
on BEAM, all 32 blocked at compilation in Nera**. The remaining missing APIs
are `calendar:datetime_to_gregorian_seconds/1`, `calendar:day_of_the_week/1`,
`calendar:gregorian_seconds_to_datetime/1`, `calendar:universal_time/0`,
`erlang:universaltime/0` and `string:trim/3`. The strict runner exits 1.
[Evidence](binary-last-match-evidence.json) records source/compiler hashes,
per-case status and the remaining blockers. No Cowlib source was altered.
Reproduce with the differential command below using
`--module cow_cookie --module cow_qs`. Other modules and upstream suites were
not rerun for this change.

## cow_cookie — octal characters fixed, 2026-10-12

The lexer now accepts one to three octal digits in character literals. The
eight lexer errors on `$\013` and `$\014` in the original pinned Cowlib source
are resolved. Two lexer regressions check values and token boundaries; they
failed before the fix. All 297 syntax tests and crate Clippy pass, and the
`char_octal.erl` regression passes on both BEAM and Nera (values, binaries,
arithmetic and guards).

The unchanged 32-case `cow_cookie` corpus was rerun: **BEAM 32/32; Nera 32
blocked at compilation**, with no native case executed. The newly exposed
missing functions are:

- `binary:last/1`, `binary:match/2`;
- `calendar:datetime_to_gregorian_seconds/1`, `calendar:day_of_the_week/1`,
  `calendar:gregorian_seconds_to_datetime/1`, `calendar:universal_time/0`;
- `erlang:universaltime/0`;
- `string:trim/3`.

The build includes the original `cow_date` dependency, including functions
outside the exercised corpus; it is not pruned or stubbed to bypass blockers.
The strict runner exits 1. [Evidence](cookie-octal-evidence.json) records
compiler/source hashes, each case's BEAM result and native blocked status.
Reproduce using the differential command below with `--module cow_cookie`.
Other modules were not rerun for this lexer change.

## Upstream functional suite — 2026-10-12

**72/72 upstream EUnit tests pass on BEAM, and all 72 original test closures
pass directly in Nera.** This complements the 29/29 project-owned differential
cases. All eight generators are checked for the expected number of tests:

| Generator | Tests |
| --- | ---: |
| parse_qs_test_ | 24 |
| parse_qs_identity_test_ | 5 |
| qs_test_ | 16 |
| qs_identity_test_ | 5 |
| urldecode_test_ | 7 |
| urldecode_identity_test_ | 5 |
| urlencode_test_ | 5 |
| urlencode_identity_test_ | 5 |

Reproduce with the same external pinned checkout prepared by `runner.py`:

```sh
python3 tests/integration/cowlib/upstream.py \
  --upstream /tmp/nera-cowlib-validation/upstream \
  --nera /tmp/nera-cowlib-target/release/nera \
  --work-dir /tmp/nera-cow-qs-upstream
python3 -m unittest discover -s tests/integration/cowlib -p 'test_*.py'
```

BEAM compiles the original source with `TEST` and `eunit_autoexport` using a
focused Make target, then runs the real EUnit runner. For Nera, an external
copy enables `TEST` and adds an entry point. Only the 17 Horse benchmark
functions at the ends of the four test sections are removed, avoiding an
unrelated performance-tool dependency; functional generators, test data,
assertions and library code remain unchanged. No upstream code is stored here.
The adapter rejects an unexpected generator or benchmark inventory.

Each native test executes in a separate process, requires a matching completion
marker and exit zero, and has a 15-second timeout. Generator counts are checked
on both engines. Tests are not required to return `ok`: as in EUnit, normal
completion is sufficient, and a failed match/exception fails the test.
A deliberate wrong expected value in a separate external negative-control copy
caused a native `badmatch` and no success marker, confirming that assertions run.

[Evidence](upstream-evidence.json) records the pinned source/header hashes,
adapted-source and executable hashes, per-test outcomes and scope. Both complete
runs passed locally on macOS arm64 / OTP 29.0.5 with the compiler from the
case-clause fix (same binary hashes as its evidence). Nera emitted 22 warnings
for unused macros/variables and non-tail recursion; these did not prevent the
build or tests. Logs remain under `/private/tmp/nera-cow-qs-upstream` and are
temporary. CI runs the 30 harness unit tests; this upstream execution has no
Linux CI verdict yet. Native EUnit framework compatibility, benchmarks and
other Cowlib modules remain outside this result.

## Additional compiler and stdlib regressions — 2026-10-12

The project-owned `tests/lists_join.erl` and `tests/case_clause_reason.erl`
regressions also pass on BEAM and Nera. They were extended after reviewing
OTP 29.0.5's [lists suite](https://github.com/erlang/otp/blob/OTP-29.0.5/lib/stdlib/test/lists_SUITE.erl)
and [exception suite](https://github.com/erlang/otp/blob/OTP-29.0.5/erts/emulator/test/exception_SUITE.erl).
No OTP test code was copied.

`lists:join/2` now checks improper-list and function separators, bitstring
elements, invalid binary input, a long improper list, and order/length properties
for every input length from 0 through 128, in addition to the existing
100,000-element case. `case_clause` checks maps, improper lists within values,
bitstrings, empty lists, rejected guards, non-tail calls, nested failures and
single evaluation of the case expression.

A Rust lowering unit test covers tail, non-tail and try/catch case expressions.
It fails when the old atom-only exception behavior is temporarily restored;
the production implementation was restored before the final run. All **429
nera-ir tests** pass (403 unit tests and 26 integration tests). These checks
extend compiler/stdlib coverage without changing the Cowlib adapter or its
previously recorded 72-test result.

## Differential corpus — case_clause correction, 2026-10-12

**All 29 cow_qs cases now agree with BEAM**, including the five error cases.
Nera's case-expression lowering now raises `{case_clause, Value}` instead of
`case_clause`, preserving the unmatched value as BEAM does. Both malformed
hexadecimal escape cases pass without changing Cowlib, the corpus or comparison
rules. The strict runner exits zero.

The minimal `case_clause_reason.erl` regression checks integers, binaries and
nested tuples and passes on BEAM and Nera. Four nearby native exception/matching
tests, 428 nera-ir tests and crate Clippy pass. [Recorded evidence](case-clause-evidence.json)
binds these results to the compiler, runtime and changed source hashes.
The other three Cowlib modules remain outside this focused rerun; their earlier
compilation blockers are not resolved by this result.

## lists:join baseline — cow_qs, 2026-10-12

After adding `lists:join/2` to Nera's Erlang stdlib, the unchanged `cow_qs`
module compiles and all 29 native cases execute. **27/29 agree with BEAM**;
BEAM satisfies all 29 expectations. The two remaining mismatches are error
reasons for invalid hexadecimal escapes:

| Case | BEAM error reason | Nera error reason |
| --- | --- | --- |
| `qs_bad_hex` (`x=%GG`) | `{case_clause, 71}` | `case_clause` |
| `qs_decode_bad` (`%X0`) | `{case_clause, 88}` | `case_clause` |

Both engines reject these inputs, but the reason terms differ; the runner
correctly exits 1. No comparison is relaxed or exempted. All eight valid and
all sixteen boundary cases pass, along with three of five error cases.
`lists_join.erl` passes on both engines, including direct/dynamic/remote calls,
separator identity, improper lists and a 100,000-element list. The existing
`lists_bifs`, `lists_stdlib` and `lists_large` tests also pass in Nera.
[Evidence](lists-join-evidence.json) records the changed stdlib and regression
hashes, rebuilt compiler/runtime hashes and per-case outcomes.

The other three modules were not rerun for this focused change; their initial
compilation blockers below remain the last recorded results. The baseline
section is historical, not a statement that cow_qs is still blocked.

## Reproduction

Requires Python 3.9+, Git, Make, Erlang/OTP 29 (reference run: 29.0.5), a C
linker and a freshly built Nera compiler with its matching runtime archive.
From the Nera repository root:

```sh
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-cowlib-target cargo build --release --locked
python3 -m unittest discover -s tests/integration/cowlib -p test_runner.py
python3 tests/integration/cowlib/runner.py \
  --nera /tmp/nera-cowlib-target/release/nera \
  --work-dir /tmp/nera-cowlib-validation
# Repeat without fetching sources:
python3 tests/integration/cowlib/runner.py \
  --nera /tmp/nera-cowlib-target/release/nera \
  --work-dir /tmp/nera-cowlib-validation --offline
# Investigate one module:
python3 tests/integration/cowlib/runner.py \
  --nera /tmp/nera-cowlib-target/release/nera \
  --work-dir /tmp/nera-cowlib-validation --offline --module cow_qs
```

Use a dedicated external work directory and one run at a time. The runner
refuses a directory inside the repository or one containing it. It verifies
an independent, clean checkout at the pinned commit. BEAM uses upstream `make`
in a disposable external copy; Nera compiles the original module and headers
without rewriting them. `cow_cookie` also compiles its `cow_date` dependency.
The upstream `TEST` macro is not enabled.

The work directory contains `report.json`, build logs, generated project-owned
drivers, executables and separate stdout/stderr logs per case and engine.
Subsequent runs replace those outputs. Reports include source and harness
hashes, compiler/runtime hashes, OTP version, commands, per-case typed results
and build failures. The recorded Git HEAD is context, not proof of executable
provenance: rebuild before evaluating a different compiler revision.

## Corpus and comparison contract

`corpus.json` contains **321 independently authored cases**, including the
122-case core used by default and 199 explicitly selected HTTP cases:

| Module | Cases | Valid | Boundary | Errors |
| --- | ---: | ---: | ---: | ---: |
| cow_qs | 29 | 8 | 16 | 5 |
| cow_cookie | 34 | 15 | 15 | 4 |
| cow_date | 26 | 3 | 14 | 9 |
| cow_base64url | 33 | 4 | 24 | 5 |
| cow_http_hd | 63 | 29 | 10 | 24 |
| cow_http | 136 | 48 | 22 | 66 |

Boundary cases may deliberately expect an exception. Each case specifies
whether it must return or throw; BEAM must satisfy that expectation even when
native compilation is blocked. Every exported function/arity in these four
core modules is exercised. HTTP module coverage is partial as described above.

- Query strings: flags, duplicate keys, empty values/names, separators, `+`,
  percent escapes, arbitrary bytes, malformed escapes; limits 0/1/2/3 and
  default-limit inputs with 99/100/101 keys.
- Cookies: whitespace, quoted/empty values, duplicates, control characters,
  cookie limits, Set-Cookie attributes, fixed expiry dates, deletion and
  invalid options, and positive Max-Age parsing/building checked against
  surrounding clock reads. Returned `ignore` is compared as an ordinary value.
- Dates: IMF-fixdate, RFC850 and asctime; all three builders; zero padding,
  leap day/second, century boundaries, the two-digit year pivot, out-of-range
  fields and invalid syntax. Accepted oddities such as day zero are compared
  with Cowlib's actual behavior, not interpreted as RFC conformance.
- Base64url: default/options APIs, padding enabled/disabled, lengths modulo
  three, URL alphabet, all 256 byte values, roundtrips and malformed inputs.

Each case runs in a fresh OS process with a timeout (15 seconds by default).
Explicit indexed result and completion markers plus exit zero are mandatory.
Returns are wrapped as `{return, Value}`; exceptions as
`{exception, Class, Reason}`. Stack traces are intentionally excluded.
Our existing Thoas ETF decoder preserves Erlang types and binary bytes while
normalizing map order and equivalent ETF encodings. No JSON or text
normalization is applied by default. Two cases explicitly select scoped semantic
comparison: `cookie_set_build` ignores attribute order while retaining exact
cookie/attribute bytes, and `http_merge_request` ignores only the order of a
valid leading pseudo-header prefix while retaining its exact pairs and the
regular suffix. Both contracts and rejection controls are described above.
The two positive Max-Age
cases assert time-window invariants separately on each engine and return `ok`;
wall-clock-dependent timestamps are not compared across processes. Cookie
builders are explicitly flattened from public iodata to wire bytes in the corpus expressions.

A mismatch, unexpected exception, timeout, crash or missing marker fails the
run. A failed native build leaves its cases **blocked**, never passed. The
runner continues with other modules and still runs all BEAM reference cases.
Exit zero requires all selected cases to pass on both engines. There are no
known-failure exemptions or skipped cases. CI runs the 30 harness unit tests
on Linux and macOS. The Linux job also
runs this entire corpus against OTP 29.0.5 and the freshly built native
compiler, without module filtering or `continue-on-error`. A mismatch,
blocked build or runner failure fails the job. The work directory is
`$RUNNER_TEMP/cowlib-differential`, outside the checkout; reports and stdout/
stderr logs are uploaded as `cowlib-differential-linux` even after failures.
Upstream source trees are not uploaded. The first remote CI execution of this
new step remains to be observed; the local result is macOS arm64 only.

## Recorded baseline — 2026-10-11

macOS arm64, OTP 29.0.5, release compiler/runtime rebuilt from merged Nera main
`9b64f629dbc854014ad351a451145cd55df6a50c`. The checkout subsequently contains
the documentation-only commit `811ce9a`; compiler and stdlib sources are
unchanged. [Evidence](baseline-evidence.json) records the exact hashes.

BEAM satisfies **120/120** case expectations. Nera cannot yet compile any of
the four complete modules, so **120 cases are blocked and zero native cases
have been executed**. The runner correctly exits 1.

| Module | Observed compilation blocker |
| --- | --- |
| cow_qs | Unresolved `lists:join/2` |
| cow_cookie | Eight lexer errors on octal character literals (`$\013`, `$\014`) |
| cow_date | Unresolved `calendar:day_of_the_week/1` |
| cow_base64url | Unresolved `base64:encode/2` and `base64:decode/2` |

These are first observed blockers, not an exhaustive list of missing support.
Fixes can reveal further dependencies or runtime differences. No Cowlib code
was patched, no missing API was stubbed and no fallback to BEAM was used.

## Scope limits

The differential corpus is separate from the upstream functional suite above.
The latter covers all EUnit tests in the pinned cow_qs module, not all Cowlib
modules or their PropEr suites. Positive cookie `max_age` values are checked
with surrounding system-clock reads (111 and 86417 seconds); these checks
assume the system clock does not jump during the call. Zero, negative and
malformed cases remain covered by deterministic comparisons. Memory sharing,
performance, protocol conformance and other platforms are not established by these results.
