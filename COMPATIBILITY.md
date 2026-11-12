# Project compatibility

Last evidence review: **2026-11-12**. This register summarizes results recorded
in this repository; the review date does not mean every project was retested.
It describes native compilation and execution. Passing `nera check`, fetching
a dependency, or using the editor tools does not establish runtime compatibility.

## Status definitions

- **Tested — scoped pass:** the recorded checks passed for the stated version,
  platform and workload. This is not a claim that the entire project works.
- **Not working yet:** a recorded attempt failed; the blocker and last observed
  result are linked. A later fix needs a rerun before changing this status.
- **Not tested:** no reproducible project-level result is recorded here.
  This is neither a success nor a known failure.
- **Historical — needs revalidation:** older execution results exist, but do
  not establish compatibility with the current compiler.

## Recorded results

| Project / version | Status | Last recorded validation | Scope, limits and evidence |
| --- | --- | --- | --- |
| **Thoas 1.2.1** (`7cdcec7e52b80d2869900893b2acf47199d84190`) | **Tested — scoped pass** | 2026-10-11; main `9b64f62`; fresh release; macOS arm64; OTP 29.0.5, Elixir 1.20.3 | Full ExUnit/StreamData: BEAM 338/338; Nera library through ETF 337/338. Only the copying test fails because transport copies binaries; direct memory checks pass 4/4 on both engines. JSONTestSuite 318/318 and all six properties pass (100 trials each, seed 0); no skips/exclusions or transport failures. Corpus 80/80. [Current scope and reproduction](tests/integration/thoas/README.md#current-validation--main-2026-10-11), [evidence](tests/integration/thoas/main-validation-evidence.json). |
| **Thoas 1.2.1 — upstream assertion bodies** (same pinned commit) | **Tested — scoped pass** | 2026-10-11; main `9b64f62`; macOS arm64; OTP 29.0.5 | Original EUnit 36/36 on BEAM and all 36 assertion bodies directly in Nera via the external adapter. Native EUnit runner/autoexport remain unvalidated. PR 27 CI also passes the strict native assertion gate on Linux. [Consolidated results](tests/integration/thoas/README.md#current-validation--main-2026-10-11). |
| **Cowlib 2.22.0 example project** (`examples/cowlib_demo`, from hex.pm) | **Tested — scoped pass** (85 calls over all 25 modules; not upstream suites) | 2026-11-12; Nera main `3b2abf2`; macOS arm64; Rust 1.99.0; OTP 29.1.2 | `nera deps get`, `nera check` (0 errors, 57 warnings on Cowlib) and `nera build` succeed. All 85 calls return natively with no exception, stable over 10 runs. Against OTP 29, 75 lines were byte-identical; 8 printed the same value differently (6 map key orders, 2 `'HTTP/1.1'` atoms printed unquoted); 2 differ only in byte order because Cowlib walks an atom-keyed map, and are equivalent when sorted. Since atoms print quoted as on OTP, 77 lines are byte-identical and only the 6 map orders and 2 byte orders remain (rerun on macOS arm64, OTP 29.1.2). [`scripts/test-cowlib-demo.py`](scripts/test-cowlib-demo.py) checks the sorted output against OTP 29 (2 known lines allowed); [`benchmarks/cowlib_bench.sh`](benchmarks/cowlib_bench.sh) times it on demand. Upstream EUnit suites other than cookie/date (next row) and PropEr properties were not rerun on 2.22.0. [Example and result](examples/cowlib_demo/README.md). |
| **Cowlib 2.22.0 — cow_cookie / cow_date upstream EUnit** (`b92326f02df30dd62045d622a17f6ba52d96d53a`) | **Tested — scoped pass** (two original functional suites; date property blocked natively) | 2026-11-12; main `3b2abf2` plus the `filename:flatten/1` export, `~W`/`~P` depth fixes and three scheduler liveness fixes; macOS 27 arm64; Rust 1.99.0; OTP 29.1.2 / EUnit 2.11; PropEr 1.5.0 | Original suites through project-configured `nera test`: **157/157 cow_cookie** and **27/27 cow_date** on BEAM and natively, with EUnit framework controls passing on both; no skipped or normalized assertions. Native runs hung or exited early about once in 30 runs until the scheduler fixes; afterwards **1,200/1,200** consecutive native `cow_cookie` runs pass. The 2.20.0 Domain/HttpOnly failure no longer occurs in the suite, but the separate cookie probe still prints the attributes in another order than BEAM (`wire_equal` false). `prop_parse_date/0` passes on BEAM but cannot run natively: `rand:uniform/1` is now a built-in that `rand:seed/2` does not control, so seeded runs differ. The Linux workflow runs both suites. [Results, fixes and reproduction](tests/integration/cowlib/README.md#cow_cookie--cow_date-upstream-eunit-on-cowlib-2220--2026-11-12). |
| **Cowlib 2.20.0 — first four modules** (`c768a804565ff5b8178ed968a5921e469d6bd7b2`) | **Tested — scoped pass** (four named modules; not all Cowlib) | 2026-10-12; merged main `4f5a266`; fresh release; macOS arm64; OTP 29.0.5 | **122/122 differential cases pass:** qs 29/29, cookie 34/34, date 26/26, Base64url 33/33; no blocked or skipped cases. Existing Set-Cookie attribute-order normalization and two Max-Age time-window checks retained. The 122-case gate also passed on Linux in [PR #47 CI](https://github.com/processone/nera/actions/runs/38255964315), head `e76d893`. [Current evidence](tests/integration/cowlib/main-validation-evidence.json), [scope and reproduction](tests/integration/cowlib/README.md#current-validation--merged-main-2026-10-12). |
| **Cowlib cow_hpack upstream EUnit and native PropEr** (same pin) | **Tested — scoped pass** | 2026-10-19; main `c74fc02` plus five compatibility fixes; macOS 27 arm64; Rust 1.99.0; OTP 29.0.4 / EUnit 2.11; PropEr 1.5.0; pinned OTP 27.3 `rand`/`ordsets` | Original EUnit **74/74 on both engines**, no skipped or normalized cases. Both original string properties pass **6,000/6,000 each per engine**, three seeds repeated twice, with exact input/wire/decoded/verdict/shrink traces. **12** false controls and **24** seeded replays per engine pass. Five minimal blocker regressions fail before and pass after correction, including real full GC and integer/map heap-limit enforcement. Third-party sources stay external; CI stays paused. Final serial local gate: **391 passed, zero failed, two existing Onecached exclusions / 393** after the quiet-launch check; historical loaded TCP failures also reproduce on unchanged main and remain recorded. Scope excludes exhaustive API/protocol coverage, nondefault heap policies, unoptimized-compiler and other-platform execution, and the next HTTP/2 modules. [Results, fingerprints, limits and reproduction](tests/integration/cowlib/README.md#cow_hpack-campaign--2026-10-19). |
| **Cowlib cow_http2_machine upstream EUnit** (same pin) | **Tested — scoped pass** | 2026-10-19; merged main `9d0cbd8` (after HTTP/2) plus real message-timer cancellation; macOS 27 arm64; Rust 1.99.0; OTP 29.0.4 / EUnit 2.11 | Complete original suite **1/1 on BEAM and native Nera**, no skipped/normalized assertions; native EUnit controls pass. **No upstream PropEr properties exist** at this pin: trials, seeds and shrinking are not applicable. Covers termination and removal of queued timeout messages, not all 26 APIs, stream/flow-control or protocol behavior. `cancel_timer/2` was missing and `/1` a stub; both now really cancel registered message timers with async/info options. Two native/BEAM regressions, two deterministic runtime regressions and a racing-cancel check cover cancellation, cleanup and pending-work ownership. The race check observes a false result and pending ownership during publication, not a deterministic guarantee that cancellation blocks. Selection, dependency/helper and singular-summary regressions fail before and pass after correction; sources remain external and CI stays paused. [Inventory, results, fingerprints, limits and reproduction](tests/integration/cowlib/README.md#cow_http2_machine-upstream-eunit--2026-10-19). |
| **Cowlib cow_http2 upstream EUnit** (same pin) | **Tested — scoped pass** | 2026-10-19; merged main `522bf09` (after HPACK); unchanged compiler/runtime/stdlib; fresh release; macOS 27 arm64; Rust 1.99.0; OTP 29.0.4 / EUnit 2.11 | Original suite **3/3 on both engines** via real EUnit and project-configured native `nera test`, no blocked/skipped/normalized tests. Covers PING/WINDOW_UPDATE truncation, complete frames/remainders, and SETTINGS acknowledgement/stream rejection, not all 18 public APIs. **No upstream PropEr properties exist at this pin**; trials/seeds/shrinking are not applicable, not a property pass. General framework controls and the initial 20 mutations of actual engine readings pass; permanent completion tests now guard rejection without an external report (seven subcases fail with the old checks). Runner selection and duplicate-summary regressions fail before and pass after correction. At that revision, duration formatting differed because `io:format/2` ignored field width and precision, without affecting verdicts. The subsequent [I/O field regression](#io-field-formatting--2026-10-19) fixes the formatter; this historical campaign was not rerun. No runtime fix was needed for those verdicts and no third-party code is vendored. [Inventory, fingerprints, commands and limits](tests/integration/cowlib/README.md#cow_http2-upstream-eunit--2026-10-19). |
| **Cowlib Base64url with native PropEr** (same pin) | **Tested — scoped pass** | 2026-10-15; main `68247b9` (PR #98), unchanged compiler/runtime/stdlib; release; macOS arm64; OTP 29.0.5; PropEr 1.5.0; pinned OTP 27.3 `rand`/`ordsets` | Both original upstream properties pass **6,000/6,000 trials each per engine**, three seeds repeated twice cumulatively. Independent native generation, complete input/encoding/shrink traces matching BEAM, independent Base64url oracle; original RFC 7515 assertion passes. **12** false controls reduce to empty binary; **24** seeded false/original replays pass. **79** harness tests pass; previous native PropEr smoke/generation modes rerun. No third-party code vendored. Bounded macOS workload, not full Cowlib/PropEr or native date/HTTP property validation. [Evidence](tests/integration/cowlib/native-base64url-proper-evidence.json), [scope and reproduction](tests/integration/cowlib/README.md#cow_base64url-with-native-proper--2026-10-15). |
| **Cowlib Base64url upstream properties** (same pin) | **Tested — scoped pass** | 2026-10-12; merged main `4f5a266`; fresh release; macOS arm64; OTP 29.0.5; PropEr 1.5.0 | Both original predicates pass **1,563/1,563 inputs each** on BEAM/PropEr and cumulative/isolated Nera. RFC 7515 passes; four false-property controls are rejected. Encodings match an independent oracle. The GC and bound-variable matching fixes are merged. The Linux workflow now requires the property replay, including cumulative execution; it passed on Linux in [PR #48 CI](https://github.com/processone/nera/actions/runs/38267594628), head `299d008`. Native PropEr generation/shrinking is not validated. [Current evidence](tests/integration/cowlib/main-validation-evidence.json), [scope and reproduction](tests/integration/cowlib/README.md#current-validation--merged-main-2026-10-12). |
| **Cowlib cow_http1 / cow_http_te upstream EUnit** (same pin) | **Tested — scoped pass** | 2026-10-15; main `902702e` (PR #104) plus message/receive-timer liveness fixes; fresh release; macOS arm64; OTP 29.0.5 / EUnit 2.11 | Original suites **39/39 + 10/10** on BEAM and native `nera test`, no skipped/normalized assertions. EUnit positive/negative/timeout/isolation controls complete on both engines. Fix scheduler shutdown before pending `send_after` / `start_timer` deliveries; two minimal regressions fail before and pass after the fix, also pass on BEAM. Two deterministic receive-timer handoff regressions protect registration/expiration, single-wake accounting and untimed deadlock detection. Original fragmentation helpers preserved externally; no third-party source vendored. Release, Clippy, 1,994 Rust, 379 native Erlang and 85 harness tests pass (existing exclusions recorded). `cancel_timer/1` remains a stub; no full API/protocol, generic EUnit or other-platform claim. [Evidence](tests/integration/cowlib/http1-te-native-eunit-evidence.json), [scope/reproduction](tests/integration/cowlib/README.md#cow_http1--cow_http_te-upstream-eunit--2026-10-15). |
| **Cowlib HTTP modules** (same pin) | **Tested — scoped pass** (corpus; not full upstream suites) | 2026-10-14; main `9ebe3ff` (PR #82) plus generator-pattern fix; macOS arm64; OTP 29.0.5 | `cow_http`: **136/136 conformant** vs BEAM 136/136: 135 exact outcomes and the existing semantic pseudo-header prefix comparison, raw `wire_equal` false. The three HTTP/2 Content-Length cases now match exactly. `cow_http_hd`: 63/63 (12/68 public functions); core 122/122. All 321 corpus cases execute and conform under existing contracts, no blocked/skipped cases. Release, Clippy, 1,923 Rust tests, 340 native tests (2 Onecached skips) and 47 harness tests pass. Full upstream HTTP suites/properties and other platforms were not rerun. [Current evidence](tests/integration/cowlib/http2-content-length-evidence.json), [scope](tests/integration/cowlib/README.md#http2-content-length-filtering--2026-10-14). |
| **Cowlib cow_http_hd with native PropEr** (same pin) | **Tested — scoped pass** | 2026-10-15; main `211b1fb` (PR #101) plus `math:sqrt/1`; release; macOS arm64; OTP 29.0.5; PropEr 1.5.0; pinned OTP 27.3 `rand`/`ordsets` | All **26 original properties: 600/600 each per engine**, **15,600 total**, three seeds repeated twice, PropEr sizes 1–10. Independent native generation with original constraints; complete typed input/verdict/shrink traces match BEAM. **156** false controls and **312** seeded replays per engine pass; shrinking capped at 100 steps, no global-minimality claim. Missing `math:sqrt/1` fixed with type/domain/direct/dynamic/remote-fun regressions vs BEAM. No third-party source vendored. Finite workload, not full Cowlib/PropEr or cross-platform validation; previous EUnit evidence is separate. [Evidence](tests/integration/cowlib/native-http-hd-proper-evidence.json), [scope/reproduction](tests/integration/cowlib/README.md#cow_http_hd-with-native-proper--2026-10-15). |
| **Cowlib HTTP suites via native OTP EUnit** (same pin) | **Tested — scoped pass** | 2026-10-14; main `b772a55` plus native-EUnit and project-runner changes; fresh release; macOS arm64; OTP 29.0.5 / EUnit 2.11 | Original suites through project-configured `nera test`: BEAM and native Nera EUnit **102/102 cow_http**, **396/396 cow_http_hd**, with success/failure/generator/timeout/isolation controls. EUnit and OTP gb_trees are external dependencies; no Cowlib/OTP source is vendored. Explicit runtime-undef mode handles unused missing APIs; strict mode remains default. Scope excludes generic EUnit/OTP, captured I/O, distributed/port helpers, native PropEr, other platforms and Thoas framework reruns. [Project runner evidence](tests/integration/cowlib/project-eunit-evidence.json), [initial framework evidence](tests/integration/cowlib/native-eunit-evidence.json), [reproduction and limits](tests/integration/cowlib/README.md#project-configured-nera-test--2026-10-14). |
| **Cowlib HTTP upstream assertions and properties** (same pin) | **Tested — scoped pass** (external Nera adapters) | 2026-10-14; merged main `a40da33` (PR #84) plus macro-block argument fix; fresh release; macOS arm64; OTP 29.0.5; PropEr 1.5.0 | Original BEAM EUnit: `cow_http` **102/102**, `cow_http_hd` **396/396**; all **498** original assertions pass in Nera. All **26** header properties pass **100 shared seeded inputs each** (2,600/2,600) on both engines. Native copies retain original `?FORALL` calls: Nera now groups `begin … end` arguments, with no Python property-envelope splitting in macro mode. Native `eunit:test/2` and `proper:quickcheck/1` remain unavailable. Release, 1,925 Rust tests, Clippy, 341 native tests (2 Onecached skips) and 55 harness tests pass. No native framework, shrinking, cumulative/property generation, other-platform or full Cowlib claim. [Evidence](tests/integration/cowlib/macro-block-evidence.json), [scope](tests/integration/cowlib/README.md#macro-block-arguments-and-http-property-replay--2026-10-14). |
| **Cowlib cow_cookie via native EUnit** (same pin) | **Not working yet** (complete raw upstream functional suite) | 2026-10-14; merged main `c93eb65` (PR #88); fresh release; macOS arm64; OTP 29.0.5 / EUnit 2.11 | BEAM **54/54**, native project-configured `nera test` **53/54**, one failure, no skipped or blocked tests. Unchanged `setcookie_test_` requires Domain before HttpOnly; native map iteration emits the inverse order. Raw diagnostic confirms this; no normalization applied to the upstream suite. The separate semantic probe passes; this order difference is allowed by the checked maps/Cowlib contracts. The Max-Age test and framework controls pass. The earlier normalized corpus remains a separate contract. [Raw evidence](tests/integration/cowlib/cookie-date-native-eunit-evidence.json), [separate semantic evidence](tests/integration/cowlib/cookie-domain-semantic-evidence.json), [bilan and reproduction](tests/integration/cowlib/README.md#cookiedate-native-eunit-from-merged-main--2026-10-14). |
| **Cowlib Domain / HttpOnly semantic probe** (same pin) | **Tested — scoped pass** (one project-owned builder probe) | 2026-10-15; unchanged main `c93eb65` compiler/runtime; macOS arm64; OTP 29.0.5 / EUnit 2.11 | Existing `set_cookie_attributes` comparator: **1/1 semantic pass**, `wire_equal=false`. Name/value and exact attribute bytes match; only order is ignored and duplicates are rejected. Raw cookie EUnit remains **53/54**, raw validation exits 1, date remains 7/7. No runtime or upstream assertion change; 65 harness tests pass. [Evidence](tests/integration/cowlib/cookie-domain-semantic-evidence.json), [contract and scope](tests/integration/cowlib/README.md#domain--httponly-semantic-validation--2026-10-15). |
| **Cowlib cow_date with native PropEr** (same pin) | **Tested — scoped pass** | 2026-10-15; main `306f5fe` (PR #99), unchanged compiler/runtime/stdlib; release; macOS arm64; OTP 29.0.5; PropEr 1.5.0; pinned OTP 27.3 `rand`/`ordsets` | Original `prop_http_date/0` **6,000/6,000** on each engine, three seeds repeated twice cumulatively. Independent native generation retains the original three-format `oneof`; full input/parsed-date/shrink traces match BEAM and an independent wire/date-field oracle. Six false controls reach minimum date components; 12 seeded replays pass. Every upstream component value observed per format across the three streams; no exhaustive calendar/weekday claim. **83** harness tests and fresh native Base64url regression pass. Prior EUnit 7/7 is separate evidence. [Evidence](tests/integration/cowlib/native-date-proper-evidence.json), [reproduction and limits](tests/integration/cowlib/README.md#cow_date-with-native-proper--2026-10-15). |
| **Cowlib cow_date — upstream EUnit and property replay** (same pin) | **Tested — scoped pass** (functional suite and finite property instances) | 2026-10-15; merged main `49b9757` (PR #94); fresh release; macOS arm64; OTP 29.0.5 / EUnit 2.11; PropEr 1.5.0 | Original EUnit **7/7** on BEAM and native `nera test`. Original date property **3,000/3,000** on both engines, 1,000 seeded instances per original format generator, repeated generation byte-identical; parsed dates match exactly in cumulative execution. False-property controls rejected on both engines. All component values in upstream ranges observed, without exhaustive combination coverage. No native PropEr generation/shrinking or other-platform claim. Release and 69 harness tests pass. [Evidence](tests/integration/cowlib/date-property-evidence.json), [reproduction and scope](tests/integration/cowlib/README.md#cow_date-upstream-property-replay--2026-10-15). |
| **Cowlib cookie/date upstream suites** (same pin) | **Not working yet** (full upstream scope) | 2026-10-12; macOS arm64; OTP 29.0.5; main `bff545c`, integration candidate `c40c17d` | Historical main `bff545c`: BEAM cookie 54/54, date 7/7, PropEr 1,000 trials; all 61 native cases blocked by missing Cowlib support. Integration candidate (since merged into main): native cookie 53/54 (one unchanged assertion fails on attribute ordering), date 7/7; original assertions via external adapter, not native EUnit. PropEr remains BEAM-only. Candidate corpus passes cookie 34/34, date 26/26, qs 29/29. [Main and integration evidence](tests/integration/cowlib/README.md#main-integration-and-cookiedate-upstream-suites--2026-10-12). |
| **PropEr 1.5.0** (`5640d0715d4b346676267504d8e84398e2a29f75`) | **Tested — scoped pass** (native generation/shrinking probe) | 2026-10-15; main `4e14b65` plus `lists:sublist/2`; macOS arm64; build/reference OTP 29; pinned OTP 27.3 `rand` and `ordsets` | Real native generators/shrinkers: three seeds repeated twice, **600 composite trials**, **18 failing/shrinking quickchecks**, **36 counterexample/boundary replays** per engine. Complete generated/shrink traces match BEAM; integer/list/binary minima are 10, `[0,0,0]`, `<<0,0,0>>`. `lists:sublist/2` regression passes on both engines. Explicit BEAM build-time preparation and external dependencies remain required. No full PropEr, native Cowlib property-suite or other-platform claim; OTP 28/29 `rand` still needs zipped generators. [Reproduction and limits](tests/integration/proper/README.md), [generation evidence](tests/integration/proper/generation-evidence.json), [initial bootstrap](tests/integration/proper/bootstrap-evidence.json). |
| **Cowboy 2.20.0** (`0a51d96698e1496720f9e13b4c1b91106dcc8a49`) | **Not working yet** (analysis only; execution not reached) | 2026-10-23; resolver on main `a6dcd8c`; fresh runtime and release compiler; macOS 27 arm64; Rust 1.99.0; OTP 29.0.4 | The combined dependency check passes both Ranch BIF blockers and lowers all 83 modules: 68 external modules plus 15 bundled modules. It then exits 1 with 90 unresolved calls and 90 warnings. Remaining gaps include `zlib` compression APIs and other unsupported OTP calls; this is not successful Cowboy compilation. The earlier isolated HTTP/2 check reports 56 unresolved-call diagnostics. No server build, link or execution is established. [Pins, commands, regressions and limits](#cowboy-analysis--2026-10-20). |
| **Poolboy 1.5.2** | **Not working yet** | 2026-10-10; macOS arm64; original Nera baseline `c5b89ee` and subsequent repair baseline | Compilation blocked on `ets:match/2`, `ets:select/2`, `supervisor:start_child/2`, `supervisor:terminate_child/2`, and `supervisor:which_children/1`; execution not reached. [Integration instructions](tests/integration/poolboy/README.md), [baseline and follow-up](docs/PROJECT_STATUS.md). |
| **OneCached** (upstream revision not pinned in the restart baseline) | **Historical — needs revalidation** | Historical performance runs; not revalidated in the 2026-10-10 restart baseline | Native execution and benchmarking are described in the [performance record](docs/ONECACHED_PERFORMANCE_PLAN.md). The [restart assessment](docs/PROJECT_STATUS.md) requires pinned sources, dependencies and reproducible build/start/stop commands; external-module tests remain skipped there. These results are not a current release gate. |

Dated Thoas sections below preserve historical evidence; the table above and
the integration README summary define the current verdict.

## Cowboy analysis — 2026-10-20

Cowboy **2.20.0** (`0a51d96698e1496720f9e13b4c1b91106dcc8a49`)
requires Ranch **1.8.1** (`10b51304b26062e0dbfd5e74824324e9a911e269`)
and Cowlib **2.21.0** (`81161a5350dbec9d73a204c903cd23a2e3560e90`).
These are external, unmodified sources, not the separate Cowlib 2.20.0 test pin.

On **macOS 27.0 arm64**, **Rust 1.99.0**, a fresh release compiler/runtime
from unchanged main `24f046a2b2c17ee52138f67fd642bd7265fd7309` still panics
with exit **101** in `pattern.rs`: `Pattern::Record should have been desugared
to Pattern::Tuple`. Both the single HTTP/2 module (includes resolved) and the
full dependency-directory check reproduce it. In particular, Cowboy matches
`#{StreamID := Stream = #stream{...}}`; the record-normalization walk previously
skipped map values. It also skipped nested patterns in string-prefix tails.

With the normalization correction, the same single-module check exits **1**
with **56 unresolved-function diagnostics**, including source locations and
missing-module explanations (for example `cow_http:parse_fullpath/1`). An
include path resolves headers, not dependency modules. The dependency-directory
check stops at `cowboy_router.erl:107`, with exit **1**, **one explicit error**
(`binary_to_atom/2` undefined) and 11 warnings. A separate minimal conversion
module reproduces that error using the unchanged-main compiler.

At that revision, the command parsed all **68 external modules**, but reached
semantic checks for only the first **20**, in path order. It lowered **19**
before the semantic error in `cowboy_router` ended the check; this was not a
completed dependency-chain analysis or evidence that the whole chain is
panic-free. It never reached semantic checks or lowering for:

- `cowboy_static`, `cowboy_stream`, `cowboy_stream_h`, `cowboy_sub_protocol`,
  `cowboy_sup`, `cowboy_tls`, `cowboy_tracer_h`, `cowboy_websocket` or
  `cowboy_webtransport`;
- any module of **Cowlib 2.21.0**, including `cow_http3_machine`, which also
  contains a record in a map pattern;
- any module of **Ranch 1.8.1**.

**After `binary_to_atom/2`.** On the same platform, a fresh release from main
`222aea92746766b90d6d930598276740677adb3e` plus the `binary_to_atom/2` change
passes `cowboy_router`. The dependency-directory check then exits **1** with
**one error** and 96 warnings at `cow_multipart.erl:147`:
`undefined variable: 'Stream2'`. Lowering rejects
`<< Boundary:BoundarySize/binary, "--", Stream2/bits >>` when `Boundary` is
already bound. A minimal module matching `<<Boundary:4/binary, "--", Stream2/bits>>`
with a bound `Boundary` fails the same way with the unchanged-main compiler;
OTP 29 compiles and matches it.

The rerun reaches semantic checks for **48** of the 68 external modules and
lowers **47**: all **29** Cowboy modules, including the nine listed above, and
the first **18** Cowlib modules in path order, including `cow_http3_machine`.
`cow_multipart` is checked but not lowered. The check never reaches semantic
checks or lowering for `cow_qpack`, `cow_qs`, `cow_sse`, `cow_uri`,
`cow_uri_template`, `cow_ws` or any Ranch 1.8.1 module. The single-module
`cowboy_http2.erl` result is unchanged: 56 diagnostics, exit 1.

**After bound binary segment values.** On the same platform, a fresh release
from main `8d575de` (which includes `binary_to_atom/2`) plus the
bound-segment change passes `cow_multipart`. The dependency-directory check
then exits **1** with **one error** and 116 warnings at `cow_ws.erl:142`:
`function 'is_map_key/2' is undefined`, in a guard. A minimal module calling
`is_map_key/2` in a guard or a body fails the same way with the
unchanged-main compiler; OTP 29 compiles and runs it.

The rerun reaches semantic checks for **54** of the 68 external modules and
lowers **53**: all 29 Cowboy modules and the first **24** Cowlib modules,
including `cow_multipart`, `cow_qpack`, `cow_qs`, `cow_sse`, `cow_uri` and
`cow_uri_template`. `cow_ws` is checked but not lowered. The check never
reaches semantic checks or lowering for any Ranch 1.8.1 module. The
single-module `cowboy_http2.erl` result is unchanged: 56 diagnostics, exit 1.

**After `is_map_key/2` (2026-10-21).** A fresh release from main `ae0a2f8`
plus the map-key BIF correction passes `cow_ws.erl:142`. The same unchanged
main reproduces the preceding result: 53 external modules lowered, one error
and 116 warnings. With the correction, the check reaches semantic checks for
**59** of the 68 external modules and lowers **58**: all **29** Cowboy
modules, all **25** Cowlib modules, and Ranch's `ranch`, `ranch_acceptor`,
`ranch_acceptors_sup` and `ranch_app`. It then exits **1** with **two errors**
and 122 warnings in `ranch_conns_sup.erl`: undefined `node/1` at line 87 and
`get_keys/1` at line 249. The nine later Ranch modules are parsed but not
semantically checked or lowered. The single-module `cowboy_http2.erl` check
still reports 56 unresolved-call diagnostics, exit 1.

The correction registers both auto-imported `is_map_key/2` and qualified
`erlang:is_map_key/2`, sharing `maps:is_key/2`'s exact lookup and dynamic
runtime dispatch. Non-maps now raise `{badmap, Map}` in bodies; even negated
guards fail instead of accepting a non-map. On unchanged main, the
unqualified and qualified calls both fail compilation, while `maps:is_key/2`
incorrectly returns `false` for non-maps. The corrected original probe prints
`[absent,present,true]`, matching OTP 29. `tests/map_key_bif.erl` passes on
both engines with the explicit `map_key_bif_ok` completion marker. It covers
13 present/absent exact-key cases, eight non-map terms, direct/dynamic/fun
calls, positive/negated guards, guard error recovery and the Cowlib
negotiation conjunction. Platform: macOS 27 arm64, Rust 1.99.0, OTP 29.

**After `node/1` (2026-10-22).** A fresh release runtime/compiler from main
`dd7a00f` plus the local-node BIF implementation passes
`ranch_conns_sup.erl:87`: Ranch uses `node(SupPid)` in its `nodedown` exit.
The check still reaches semantic checks for **59** of the 68 external modules
and lowers **58**; `ranch_conns_sup` still cannot lower because `get_keys/1`
at line 249 is undefined (exit **1**, **one error**, **85 warnings**).
The nine later Ranch modules remain unchecked beyond parsing. The
single-module HTTP/2 result remains 56 unresolved-call diagnostics, exit 1.
The unchanged-main build reproduces both missing BIFs, with 85 warnings.

The minimal `node(self())` program fails compilation on unchanged main and
prints `nonode@nohost` with the correction, matching OTP 29. The registry's
existing `node/0` entry also lacked a runtime implementation: it compiled but
raised `undef`. Both arities now share the local `nonode@nohost` result.
`tests/bif_node.erl` passes natively and on BEAM, printing `bif_node_ok` after
checking live/dead PIDs, ordinary/monitor/atomics references, 17 invalid term
cases, unqualified/qualified calls, apply and fun references, Ranch's exit
shape and guard failure/recovery (including negation and inequality).
The supported scope is local PIDs and references only: Nera does not represent
ports or preserve distributed identifier origins. Platform: macOS 27 arm64,
Rust 1.99.0, OTP 29.0.4.

```sh
./target/release/nera test bif_node -j 1 --timeout 30
work=$(mktemp -d)
erlc -o "$work" tests/bif_node.erl
erl +S 1 -noshell -pa "$work" -eval 'ok = bif_node:test(), halt().'
rm -rf "$work"
```

**After `get_keys/1` (2026-10-22).** On macOS 27 arm64 with Rust 1.99.0,
a fresh locked release from main `dd7a00f` plus the value-filtered dictionary
lookup no longer reports undefined `get_keys/1` at `ranch_conns_sup.erl:249`.
The same pinned dependency check exits **1** with **one error** (`node/1`
at line 87, tracked separately as NERA-66) and 85 warnings. Unchanged main
`dd7a00f` reports both undefined calls, exit 1, with the same warning count.
The external-module coverage is unchanged: **59** semantically checked,
**58** lowered, and nine later Ranch modules parsed only. The single-module
HTTP/2 check still exits 1 with 56 unresolved-call diagnostics.

The minimal probe `put(child, active), get_keys(active)` fails compilation on
unchanged main; the correction returns `[child]`, matching OTP 29. Both
unqualified and qualified calls return that result. The shared native/BEAM
regression `tests/process_dict_get_keys.erl` reaches
`process_dict_get_keys_ok`. It covers direct, qualified, dynamic and fun calls;
multiple, empty and absent matches; integer/float distinctions at every depth;
compound, binary, bitstring, pid, reference and fun values; non-atom keys;
unchanged dictionary contents and `get_keys/0`; updates, erasure and process
isolation with Ranch's active/removed child-state pattern. Results are compared
without assuming key order.

OTP 29 also exposed a required correction in the existing exact comparator:
positive and negative float zero are distinct under `=:=` (OTP 27+), including
inside compounds. The native baseline prints `[true,true,true]` for direct
exact, tuple exact and numeric comparisons of opposite zeros; the corrected
runtime and BEAM both print `[false,false,true]`. The shared exact comparator
also governs pattern matching: float-literal patterns such as `f(0.0)` and
`case X of 0.0 -> ... end` no longer match `-0.0`. A variable already bound
to `0.0` likewise rejects `-0.0` in a pattern. Literal `=:=` and `=/=` folding
uses the same bit identity; numeric `==` remains unchanged. The existing
`list_operator_contracts.erl` regression now retains `{tag,0.0}` when
subtracting `{tag,-0.0}`, matching BEAM instead of its previous expectation
that also failed on OTP 29.

**Combined `node/1` and `get_keys/1` (2026-10-22).** A fresh locked release
at `1497b46`, based on main `e15b45b`, passes both former Ranch blockers at
`ranch_conns_sup.erl:87,249`. The same pinned check now lowers **83 modules**:
all **68 external modules** (29 Cowboy, 25 Cowlib and 14 Ranch), plus 15 bundled
modules. This includes `ranch_conns_sup` and the nine later Ranch modules that
the isolated corrections above could not check beyond parsing.

The check reaches global unresolved-call validation and exits **1** with
**90 unresolved calls** and **90 warnings**. Remaining gaps include
`zlib:open/0`, `zlib:deflateInit/6`, `zlib:inflateInit/2` and
`zlib:safeInflate/2`, as well as other unsupported OTP APIs. Compression support
is the starting point for further compatibility work, not part of this fix.
The pinned third-party sources remain unmodified and external. No server build,
linking, startup, HTTP request or runtime compatibility is established.

The full serial local gate at the same `1497b46` revision passes: locked runtime
and compiler release builds, strict workspace Clippy, **2,047 Rust tests**
(one ignored), allocator validation and **408 native tests** (two existing
OneCached skips). The expanded dictionary regression also passes on OTP 29;
both engines print `process_dict_get_keys_ok`. GitHub workflows remain paused.

**After OTP import resolution and main #135–137 (2026-10-23).** On
macOS 27.0 arm64, Rust 1.99.0 and OTP 29.0.4, the resolver based on main
`a6dcd8c` recognizes all 183 signatures from the pinned `erl_internal:bif/2`
catalogue without claiming missing native implementations. The preprocessed
production-source audit covers all **68 modules** and **54 distinct OTP
auto-imports**; none is absent from the catalogue.

The dependency-directory check parses and lowers **all 68 external modules**
(29 Cowboy, 25 Cowlib, 14 Ranch), plus 15 built-in stdlib modules. It exits **1**
with **90 unresolved-function errors and 90 warnings** at final capability
validation. Main #135 supplies native `get_keys/1`, which now resolves and
passes `process_dict_get_keys.erl`; neither `node/1` nor `get_keys/1` appears
among the diagnostics. Remaining targets include `erlang:send/3`,
`erlang:function_exported/3`, `ets:match/2`, `crypto:hash/2` and zlib APIs.
No successful Cowboy native build or server execution is claimed.

The complete final-version gate passes after building the runtime before the
compiler: release builds, workspace Clippy, **2,067 Rust tests** (one ignored
release-only measurement), allocator validation and **415 native tests**.
The same two external-OneCached native tests remain explicitly skipped; no
test fails. This includes the merged process-dictionary and binary-generator
regressions as well as both import-resolution completion fixtures.

The catalogue equals OTP 29.0.4's abstract-code enumeration. All **27 CLI
semantic probes** and five strict/deferred integration checks pass. They cover
ignored non-signature suppression entries and the now-available `get_keys/1`,
while compiler-specific record forms and missing native targets still receive
explicit errors. The two import fixtures also pass on BEAM. Linux/Windows
native execution and full Cowboy compatibility remain unverified.

Reproduce the catalogue and semantic checks below. The production-source audit
runs in the clone block that follows, before its sources are removed:

```sh
python3 scripts/update-otp-auto-imports.py --check
python3 scripts/test-import-resolution.py --nera ./target/release/nera
./target/release/nera test 'import_resolution*' append_element -j 1 --timeout 30
python3 tests/integration/eunit/deferred_calls.py --nera ./target/release/nera
```

Each fix requires rerunning the check before drawing conclusions about the
remaining modules. None of these results establishes successful Cowboy
compilation or runtime support.

Reproduce from the project root after the locked release build (third-party
sources stay outside the project):

```sh
cargo build --release --locked
NERA="$PWD/target/release/nera"
work=$(mktemp -d)
mkdir "$work/deps"
for spec in \
  cowboy:0a51d96698e1496720f9e13b4c1b91106dcc8a49 \
  ranch:10b51304b26062e0dbfd5e74824324e9a911e269 \
  cowlib:81161a5350dbec9d73a204c903cd23a2e3560e90
do
  name=${spec%%:*}; commit=${spec#*:}
  git init -q "$work/deps/$name"
  git -C "$work/deps/$name" remote add origin "https://github.com/ninenines/$name.git"
  git -C "$work/deps/$name" fetch --depth 1 origin "$commit"
  git -C "$work/deps/$name" checkout --detach FETCH_HEAD
done
escript scripts/audit-auto-imports.escript "$work/deps" crates/nera-ir/src/otp_auto_imports.rs
(cd "$work"; "$NERA" check deps/cowboy/src/cowboy_http2.erl -I deps)
# Expected: explicit diagnostics, exit 1; not panic/101.
(cd "$work"; "$NERA" check --deps-dir deps)
# With the resolver on main a6dcd8c: all 83 modules lower (68 external + 15 bundled).
# Both Ranch BIF blockers resolve. Final validation exits 1 with 90 unresolved
# native targets, including zlib APIs, and 90 warnings.
rm -rf "$work"
```

Run the value-filtered dictionary regression on both engines:

```sh
./target/release/nera test process_dict_get_keys process_dict comparison_ops map_key_bif -j 1 --timeout 30
work=$(mktemp -d)
erlc -o "$work" tests/process_dict_get_keys.erl
erl +S 1 -noshell -pa "$work" -eval 'ok = process_dict_get_keys:test(), halt().'
rm -rf "$work"
```

`tests/record_nested_patterns.erl` uses the same stream-map shape to check
record tags/arity, wildcarded omitted fields, aliases, repeated bindings,
missing keys and mismatches. It also exercises nested records/maps under
lists/tuples, matches, closures, generators, receive and try clauses, and
string-prefix tails. The original compiler panics while compiling this probe;
the corrected compiler executes all assertions and prints
`record_nested_patterns_ok`, matching **OTP 29 / ERTS 17.0.4**. The IR regression
lowers this same fixture and structurally verifies the result.

```sh
cargo test -p nera-ir --locked
./target/release/nera test record_nested_patterns record_basic include_record \
  'match_*' 'map_*' -j 1 --timeout 30
work=$(mktemp -d)
erlc -o "$work" tests/record_nested_patterns.erl
erl +S 1 -noshell -pa "$work" -eval 'record_nested_patterns:test(),halt().'
rm -rf "$work"
```

The targeted native run passes **25/25**. This is an analysis/pattern-lowering
fix, not a Cowboy HTTP/2 protocol campaign. Listener startup, OTP APIs, linking,
HTTP requests, TLS, WebSocket, other platforms and complete Cowboy compatibility
remain unverified. No upstream source is patched or vendored; CI remains paused.

`tests/bif_binary_to_atom.erl` uses the `cowboy_router` binding shape and checks
`binary_to_atom/1,2` with `utf8`, `unicode` and `latin1`, heap and
reference-counted sub-binaries, and qualified, `apply` and fun-reference calls.
It expects `badarg` for unknown encodings, non-binaries, bitstrings and invalid
UTF-8 up to 1,020 bytes. It expects `system_limit` above 255 characters and,
before decoding, for longer input, including a 16 MiB binary. The
unchanged-main compiler rejects it as undefined; the corrected compiler and
**OTP 29** both print `bif_binary_to_atom_ok`.

```sh
./target/release/nera test bif_binary_to_atom -j 1 --timeout 30
work=$(mktemp -d)
erlc -o "$work" tests/bif_binary_to_atom.erl
erl +S 1 -noshell -pa "$work" -eval 'ok = bif_binary_to_atom:test(), halt().'
rm -rf "$work"
```

`tests/try_catch_bound_vars.erl` checks bound variables in `try ... of` and
`catch Class:Pattern` against **OTP 29** on macOS 27 arm64, with Rust 1.99.0.
It covers bare reason variables
(including the former catch fast path), guarded and compound patterns,
integer/float exact equality, fixed/UTF/rest binary segments, preservation
of outer deferred tails, bound or repeated class names, fresh bindings and
re-raising when no clause matches. Try-of sees body bindings, but catch
clauses use only the bindings from before the protected body. The IR regression
also verifies that body-only registers cannot be read by catch patterns in
unsafe-name programs that Nera currently accepts but OTP rejects. Both runtimes print
`try_catch_bound_vars_ok`; before the correction, the four calls
`try_of(1,1)`, `try_of(1,2)`, `catch_throw(1,1)`, `catch_throw(1,2)` all
returned `matched` instead of `[matched,other,matched,other]`.

```sh
cargo test -p nera-ir --locked test_try_catch_bound_vars_lowers
./target/release/nera test try_catch_bound_vars -j 1 --timeout 30
work=$(mktemp -d)
erlc -o "$work" tests/try_catch_bound_vars.erl
erl +S 1 -noshell -pa "$work" -eval 'ok = try_catch_bound_vars:test(), halt().'
rm -rf "$work"
```

`tests/binary_bound_segment.erl` uses the `cow_multipart:parse_headers/2`
shape: a deferred tail from the function head, matched against
`<<Boundary:BoundarySize/binary, "--", Stream2/bits>>` and
`<<Boundary:BoundarySize/binary, Stream2/bits>>` with `Boundary` bound. It also
checks bound integer, float, UTF and tail segments with exact equality,
names repeated inside one binary or across a tuple, recursive heads that read
a reused match context, and the same rule in `case`, match expressions,
function heads, guards, `receive` and `maybe`. `maybe ?=` and list and binary
comprehension generators bind names before the match runs; they are covered
with repeated names, bound tails and bodies that do not use the name.
An outer deferred tail keeps its value after a `try ... of` or `catch` binary
pattern names it. Fun
heads and comprehension generators still introduce fresh variables,
including a name an earlier generator bound, and a later generator's size
may name an earlier generator's variable. The
unchanged-main compiler rejects it with `undefined variable: 'Stream2'`; the
corrected compiler and **OTP 29** both print `binary_bound_segment_ok`. With
only the lowering half of the change, the test fails at runtime because
`parse_headers(<<"--abce--x">>, <<"abcd">>)` returns `{done, <<"x">>}`
instead of `more`. The IR regression lowers this same fixture and
structurally verifies the result.

```sh
cargo test -p nera-ir --locked test_binary_bound_segment_lowers
./target/release/nera test binary_bound_segment -j 1 --timeout 30
work=$(mktemp -d)
erlc -o "$work" tests/binary_bound_segment.erl
erl +S 1 -noshell -pa "$work" -eval 'ok = binary_bound_segment:test(), halt().'
rm -rf "$work"
```

The map-key BIF regression uses the same source on native Nera and BEAM:

```sh
cargo test -p nera-ir --locked is_map_key_is_available_as_a_bif
./target/release/nera test map_key_bif -j 1 --timeout 30
work=$(mktemp -d)
erlc -o "$work" tests/map_key_bif.erl
erl +S 1 -noshell -pa "$work" -eval 'ok = map_key_bif:test(), halt().'
rm -rf "$work"
```

## I/O field formatting — 2026-10-19

The Cowlib HTTP/2 campaign exposed EUnit's literal `[done in ~.3f s]` timing
line and shifted arguments after width controls. A separate formatter regression
now compares `io:format/2`, `io:fwrite/2` and `io_lib:format/2` with BEAM
**OTP 29.0.4**, on **macOS arm64** with **Rust 1.99.0**. The EUnit timing example
prints `[done in 0.019 s]` for `19/1000`; `~5w ~w` prints `    a b` for `[a,b]`.
Static/dynamic width, precision and padding, decimal float rounding, string
truncation/adjustment, per-binary Unicode/Latin-1 fallback, integer radix and
prefix forms, accepted modifiers, overflow and later-argument alignment are
covered by `tests/io_format_fields.erl`. That regression fails on the previous
runtime and passes on BEAM and the corrected runtime. A debug Rust regression
also guards large-magnitude `~g` selection without arithmetic overflow.
Map-order modifiers are accepted and consume their arguments, but output order
is unchanged; the `~k`/`~K` sorting remains unimplemented.

Reproduce after `cargo build --release --locked`:

```sh
./target/release/nera test 'io*format*' -j 1 --timeout 30
work=$(mktemp -d)
erlc -o "$work" tests/io_format_fields.erl
erl -noshell -pa "$work" -eval 'io_format_fields:test(),halt().'
rm -rf "$work"
```

The test owns the exact stdout sequence and a completion marker, plus returned
io_lib strings and invalid-input checks. No Cowlib suite was rerun for this
isolated formatting fix; historical campaign results and fingerprints remain
unchanged. Full `~p` pretty layout, `~P`/`~W` depth controls, `error_logger`
formatting and other-platform execution are not established by these checks.

## Candidates without recorded project-level validation

Versions and platforms below are **not selected or validated**. These are
projects already mentioned in dependency examples or roadmaps, not an
exhaustive list of the Erlang ecosystem. Unlisted projects have unknown status.

| Project | Status | Reason for tracking / next validation scope |
| --- | --- | --- |
| **JSX** | Not tested | Appears in [dependency examples](docs/DEPENDENCY_MANAGEMENT.md). Pin a version and validate JSON encode/decode behavior. |
| **Ranch** | Not tested | Dependency example and a follow-on target in the [Poolboy integration notes](tests/integration/poolboy/README.md). Validate listener startup, connections and shutdown. |
| **Cowlib — remaining modules** | Not tested | The named utility, HTTP and HPACK workloads have scoped evidence above. `cow_http2` and `cow_http2_machine` have the three-test and one-test upstream EUnit results above, not exhaustive API/protocol validation. The Cowlib 2.22.0 example calls `cow_iolists` and every other module once or more, but that is a smoke check, not functional validation. Compilation as a dependency is not functional validation. |
| **ejabberd** | Not tested | [Flagship roadmap](docs/FLAGSHIP_ROADMAP.md) target. A demo or extracted module does not establish full ejabberd compatibility. |
| **RabbitMQ** | Not tested | [Flagship roadmap](docs/FLAGSHIP_ROADMAP.md) target; select and identify a bounded module/service scope first. |
| **EMQX** | Not tested | [Flagship roadmap](docs/FLAGSHIP_ROADMAP.md) target; select and identify a bounded module/service scope first. |

## Keeping this register useful

Update this file in the same change that adds a project integration, records
a new validation result, fixes a project blocker, or discovers a regression.
Do not promote a status merely because a missing API was implemented or a CI
job was added. Record an actual result, including failures and skipped checks.

For each new result, retain:

1. Upstream version and commit, dependencies, and any source patches or adapters.
2. Nera revision and whether the worktree was modified, with a fresh compiler
   and matching runtime build (or their recorded hashes).
3. Run date, OS/architecture, toolchain and BEAM/OTP version when comparing.
4. Reproduction commands, test/workload scope, pass/fail/skip counts, and a
   repository evidence file or durable CI run/artifact link.
5. Known limitations, the failing stage (check/build/link/run/behavior), and
   the next concrete blocker or issue when applicable.

Keep independent rows when platforms, versions or workloads have different
outcomes. Preserve earlier failures in linked evidence when a rerun supersedes
them. A successful cross-build is not execution on the target platform.

General compiler readiness remains in [project status](docs/PROJECT_STATUS.md);
this file is the entry point for compatibility of named external projects.

### Thoas memory sharing — 2026-10-11

The pinned Thoas 1.2.1 source is compiled unchanged on BEAM (OTP 29.0.5) and
Nera on macOS arm64. The project-owned probe measures decoded terms before any
transport: both engines retain 210 bytes for default key/value views, 101/102
bytes with full copy, and the corresponding mixed sizes for each partial-copy
option. This closes the memory-observation gap for these four scenarios;
it does not claim allocator equivalence for arbitrary binaries or a complete
native EUnit run. Nested includes and macro stringification are fixed; the remaining native assertion failures and EUnit runner gap are described in the capture validation below.
See [reproduction and evidence](tests/integration/thoas/README.md#memory-sharing-validation--2026-10-11).

### Thoas native EUnit preprocessing — 2026-10-11

The nested include failure and macro arity collision are fixed. A smoke test
using the installed OTP 29.0.5 `eunit.hrl` and `assert.hrl` runs on BEAM and Nera.
At this validation stage, the pristine Thoas test module still failed on
`??Expression` (`undefined macro: ?`), so no native EUnit assertions ran. This
preprocessing blocker was subsequently fixed; see the later validation stages.
See [reproduction and evidence](tests/integration/thoas/README.md#native-eunit-preprocessing--2026-10-11).

### Thoas macro stringification — 2026-10-11

`??Expression` is now supported. The project-owned OTP assertion probe runs on
BEAM and Nera and verifies both a successful assertion and the text/value fields
of a deliberately failing assertion. At this validation stage, the pristine
Thoas test module passed preprocessing but failed on `-import` calls (`null/0`
and `string/1` reported undefined). Import support was subsequently added. The EUnit runner and autoexport parse transform
remain unvalidated; no full native Thoas EUnit run is claimed.
See [evidence and reproduction](tests/integration/thoas/README.md#macro-stringification--2026-10-11).

### Thoas explicit imports — 2026-10-11

The original pinned sources are compiled outside this repository. A project-owned
probe directly exercises imported `true/0`, `false/0`, `null/0`, `string/1` and
`float/1` on BEAM and Nera, requiring `THOAS_IMPORTS_OK` on both engines.
The differential corpus passes again: **80/80 cases agree** with the updated compiler.
At this validation stage, the original EUnit module passed import resolution
but failed with `undefined variable: 'Expected'` at line 35. An isolated nested
closure reproduced the issue without Thoas or OTP headers. This capture defect
was subsequently fixed; see the current native assertion results below. The
EUnit autoexport transform and runner remain unvalidated.
[Commands and results](tests/integration/thoas/README.md#explicit-imports--2026-10-11),
[validation evidence](tests/integration/thoas/import-evidence.json).

### Thoas nested closure capture — 2026-10-11

The `Expected` capture defect is fixed. The original test module now compiles.
On BEAM, the original EUnit suite passes **36/36**. An external adapter executes
those assertion bodies individually inside Nera: **26 pass, 10 fail**. Assertion
21 exposes default 8-bit binary string literal semantics (`<<"🤔">>` gives `[20]`
on BEAM versus UTF-8 bytes on Nera); assertions 27–29 and 31–36 fail with
`badmatch` while encoding objects. Their root cause is not established here.
These results do not validate the native EUnit runner or autoexport transform.
The separate **80/80 differential corpus still passes**. The capture regression
passes on both engines; **1,792 Rust tests** and Clippy pass (one test ignored).
[Commands and scope](tests/integration/thoas/README.md#nested-closure-capture--2026-10-11),
[evidence](tests/integration/thoas/capture-evidence.json).

### Automated Thoas regression checks

Linux CI requires the direct memory and OTP probes and all 36 native assertion
bodies. The previous ten failure exemptions are removed. Any missing completion
marker, nonzero exit or assertion failure fails the check. This validates the
assertion bodies through the adapter, not the native EUnit framework.
[Policy and commands](tests/integration/thoas/README.md#automated-regression-gates).

### Thoas native assertion corrections — 2026-10-11

All 36 assertion bodies now pass in Nera. The emoji failure came from lowering
binary string segments as UTF-8 source bytes instead of codepoints truncated to
8 bits. The nine object-encoding failures shared a runtime defect: a global
trampoline re-entry guard skipped nested non-tail call resolution, losing the
caller continuation. The runtime now distinguishes iterative tail dispatch from
ordinary calls that must resolve their nested chains before returning.
Rust and Erlang regressions cover both defects, exception propagation and
100,000 dynamic tail calls. [Reproduction and scope](tests/integration/thoas/README.md#native-assertion-corrections--2026-10-11).

## Base64 list decoding — 2026-10-13

The incremental native implementation of `base64:decode/2` for list inputs is
checked directly against Erlang/OTP 29.0.5 on macOS arm64. The deterministic
corpus covers 67,529 values and exception class/reason results, including
alphabet/padding options, whitespace, partial quartets, large nonuniform data,
out-of-range integers, floats, improper tails and error precedence.

This intentionally corrects differences from Nera's former eager Erlang list
implementation: for example `[512]` and `[65.0]` raise `badarg`, and
`[$*,$A,$A,$A,bad]` raises `badarg` before reaching the trailing atom. The old
Erlang implementation remains only as a frozen benchmark reference; it is no
longer the list compatibility oracle. Binary decode and encode retain their
existing frozen-reference and BEAM checks.

Reproduce with `python3 scripts/test-base64.py --nera /absolute/path/to/nera`
and `python3 scripts/test-base64-scheduler.py --nera /absolute/path/to/nera`.
The latter checks logical progress with one scheduler on both runtimes. Native
work is bounded, but GC and allocator operations remain synchronous; no latency
upper bound is claimed. These checks do not establish whole-OTP compatibility
or other platform coverage. See the [validation evidence](docs/baselines/2026-10-13-base64-list-decode/README.md).

## Runtime input-domain note: Unicode lists (2026-10-17)

On macOS ARM64 with OTP 29, `list_to_atom([4294967361])` raises `badarg`;
Nera now agrees instead of truncating the value to `A`. Shared string conversion
also deliberately rejects malformed numeric lists: OTP 29 returns `1` for
`list_to_integer([4294967345])`, while Nera raises `badarg` because the element
is outside Unicode's domain. Valid numeric strings are unchanged. This scoped
observation does not change the project-level verdicts above.

Reproduce on OTP with `erl +S 1 -noshell -eval
'io:format("~p~n",[list_to_integer([4294967345])]),halt().'`; the Nera regression is
`nera test unicode_integer_scalar_domain -j 1 --timeout 30`. See the
[Unicode/materialization report](docs/baselines/2026-10-17-unicode-binary-materialization/README.md)
for the recorded version, tests, raw observations and performance limits.

## Runtime input-domain note: binary lists and ETF counts (2026-10-17)

On macOS ARM64, `binary_to_list_domain` and `etf_container_bounds` pass on
OTP 29 / ERTS 17.0.5. Nera rejects partial bitstrings in `binary_to_list/1`
and checks impossible ETF container counts before reserving memory. The
fixtures also cover valid byte-aligned conversions and minimal/empty ETF
containers; this is scoped evidence, not whole-ETF compatibility. Reproduce
with `nera test binary_to_list_domain etf_container_bounds -j 1 --timeout 30`;
see the [validation report](docs/baselines/2026-10-17-binary-list-etf-bounds/README.md)
for OTP commands, allocation evidence and performance limitations.

## Map iteration order — 2026-11-12

`maps:to_list/1`, `keys/1`, `values/1` (and so `fold`, `iterator`, `map`,
`filter`), `~w`/`~p` printing and `term_to_binary/1` now visit a map of up to
32 keys in ascending key order, as BEAM's flat maps do, instead of insertion
order. Non-atom keys follow Erlang term order and match BEAM exactly
(`tests/map_iteration_order.erl`, run on both). **Atom keys do not match BEAM
yet:** BEAM orders them by atom-table index, which depends on the order in
which erlc and the loader first saw each atom; Nera orders them by its own
atom-table index (first appearance in the compiled program). On the Cowlib
2.22.0 example (`examples/cowlib_demo`) this fixes the HTTP/2 SETTINGS frame
order, while the `set-cookie` attribute order, the HTTP/2 pseudo-header order
and several printed map orders still differ from OTP 29.1.2. Maps of 33 or
more keys iterate in insertion order; BEAM uses hash order there.
