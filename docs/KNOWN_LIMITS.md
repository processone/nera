# Known limits of Nera

Updated **2026-11-12**. Nera is **experimental**. This page lists what does not
work, what is unverified and what behaves differently from Erlang/OTP, so you
can decide whether Nera fits before you try it. It is written for the first
release, `v0.1.0`, and is revised with every release after it.

Nera is not a drop-in replacement for the BEAM. Everything here describes native
compilation and execution. Passing `nera check`, or using the editor plugins,
does not mean a program runs.

## What "experimental" means

- No compatibility promise. Language, command-line, `nera.toml` and runtime
  behaviour can change between releases without a deprecation period.
- Wrong results and hangs are still being found. Compare important output with
  the BEAM before relying on it.
- The first release, `v0.1.0`, is a full GitHub release but stays experimental
  until 1.0. Later tags with a suffix, such as `v0.2.0-experimental`, are GitHub
  prereleases and are never stable releases.
- Do not use Nera for production services yet.

## Platforms

| Platform | State |
| --- | --- |
| macOS arm64 | Developed and tested here. All recorded results come from this platform. |
| Linux x86-64 | Built and tested natively in CI, and the release job compiles and runs a program with the installed archive. The detailed results on this page come from macOS arm64. |
| macOS x86-64, Linux arm64 | Cross-built only. Archives have never been executed. |
| Windows x86-64 | Not in the release archives. The compiler does not link on Windows, and the archive was never executed. |

CI runs on pull requests, and the Release workflow builds and publishes each
`v*` tag. See [RELEASE.md](RELEASE.md).

## Libraries and applications

See [COMPATIBILITY.md](../COMPATIBILITY.md) for each result with its version,
platform and scope.

- **Cowboy (HTTP server): does not run.** Cowboy 2.20.0 with Ranch 1.8.1 and
  Cowlib 2.21.0 lowers all 68 modules, but `nera check` still reports about 70
  OTP functions that Nera does not implement:
  - about 30 needed for plain HTTP: OTP behaviours, sockets and a few small
    functions;
  - about 20 optional: static files and tracing;
  - about 18 for TLS: the whole `ssl` module.

  A native "hello world" server links but never opens a socket. Plain HTTP over
  Cowboy is the next milestone; Cowboy with TLS is further out.
- **Cowlib:** 2.22.0 builds and runs natively. `nera check` reports no errors
  and the [`examples/cowlib_demo`](../examples/cowlib_demo) project makes 85
  calls across all 25 modules without an exception. Its output matches OTP
  except for map iteration order: Cowlib walks atom-keyed maps, and Nera orders
  atom keys differently from OTP, so `Set-Cookie` attributes and HTTP/2
  pseudo-headers come out in another byte order (equivalent when sorted).
  Cowlib's upstream `cow_cookie` and `cow_date` EUnit suites pass natively on
  2.22.0 (157/157 and 27/27, as on BEAM). Cowlib's seeded date property
  passes natively, with the same 6,000 trials as on BEAM. The other upstream
  suites were last run on the 2.20.0 pin.
- **Poolboy:** does not compile.
- **Thoas** (JSON) passes its recorded checks. **PropEr** passes a scoped probe.
- **OneCached:** worked in early runs, which have not been revalidated.

## Missing or partial OTP

- **`ssl` / TLS:** not implemented.
- **Distributed Erlang** (`net_kernel`, `node/0`, remote spawn, `rpc`): not
  implemented. Nera runs single-node programs.
- **Hot code upgrade and code loading:** not supported.
- **`supervisor`:** only `one_for_one` is honoured; other strategies are
  accepted but ignored. Map-form supervisor flags crash with
  `function_clause`, and map-form child specs are refused with
  `{start_spec, {invalid_child_spec, Spec}}`. `start_child/2`,
  `terminate_child/2` and `which_children/1` are missing.
- **`proc_lib`:** `start_link/3,5` reports a child dying before `init_ack`,
  but `start_link/5` still ignores its timeout and spawn-option arguments.
- **`application`:** `get_env/1` is missing.
- **ETS:** `ets:match/2` and `ets:select/2` are missing. `ets:new/2` without
  `named_table` returns an integer where BEAM returns a reference.
- **Sockets:** `gen_tcp:listen/2` ignores the `{port, P}` option when given in
  the options list. `{active, 1}` returns `ok` but delivers no data and no
  `tcp_passive` message. `{active, once}` and `{packet, line}` work.
- **Late replies:** after a `gen_server:call` times out, the late reply still
  arrives in the caller's mailbox. OTP's process aliases prevent that, and
  Nera does not use them.
- **`binary`:** `compile_pattern/1`, `matches/2,3` and `replace/3,4` are
  missing.

## Wrong results and crashes still known

These are real differences from the BEAM that we have reproduced.

- Terms nested thousands of levels deep (an iolist built by prepending, for
  example) survive garbage collection, can be sent to other processes,
  captured in spawned funs, and stored in and read back from ETS and
  `persistent_term`. Comparing head-nested iolists with `<` or `>`, encoding
  them with `term_to_binary/1`, flattening them with `iolist_to_binary/1`
  and hashing them with `erlang:phash2/1` work at 200,000 levels, with
  BEAM-equivalent results. `==`, `=:=` and `iolist_size/1` also work at that
  depth. The ordering fix also applies to head-nested `ordered_set` keys.
  This is not a guarantee for every graph shape: sorting map keys still
  invokes nested comparisons, and deeply nested ETF decoding remains recursive.
- `erlang:phash2/1,2` use OTP's portable hash for data terms, replacing the
  earlier Nera-specific hash. Hashes previously stored by Nera may change.
  Native funs lack BEAM's module/index metadata, so their hashes are not
  BEAM-compatible. Process and reference identities are local to each runtime.
- Printing differs in some cases: a binary of printable bytes prints as
  `<<98>>`, not `<<"b">>`.

## Reliability and performance

- TCP tests failed twice in one early full run for reasons that were never
  explained. They have passed since, but the cause is not proven.
- No long-running soak test and no fuzzing campaign has been run. Fuzz targets
  and property tests exist; their existence is not evidence of a campaign.
- Benchmark figures are historical, from one Apple M4 Pro, and
  have not been revalidated.
- Binary size: a hello-world program links to a 47 MB executable.

## Tooling

- The formatter (`nera fmt`) was repaired for the reproduced guard-grouping
  defect. Full formatter equivalence with the compiler is not proven; review
  formatted code before committing it.
- Compiler warnings can differ from `erlc`. Report any false warning.

## Reporting a problem

When Nera gives a different result from the BEAM, the most useful report is a
single `.erl` file with the output of both. Open an issue in the project's
GitHub repository.
