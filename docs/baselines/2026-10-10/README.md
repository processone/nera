# Restart baseline — 2026-10-10

This snapshot records experiments against commit
`c5b89ee65d679da1f2fe257aef73f130859440bb`, tree
`39856e395ff12a1548741a2cd895975e5e80a856`. See
[PROJECT_STATUS.md](../../PROJECT_STATUS.md) for the active blocker list and
recommended release scope. These results establish a baseline, not readiness.

## Environment and dependency custody

| Item | Recorded value |
| --- | --- |
| OS | macOS 15.7.7, build 24G720, arm64 |
| Rust | rustc 1.97.1 (`8bab26f4f`, 2026-07-14) |
| Cargo | 1.97.1 |
| Linker | Apple clang 17.0.0 |
| Reference Erlang | OTP 29, Homebrew Erlang 29.0.5 |
| Source modifications during execution | None to compiler, runtime, existing test fixtures, dependencies or workflows |
| Pre-existing untracked directories | `.claude/`, `.lavish/`; not included in the source baseline |

The root `Cargo.lock` was already present locally but ignored by Git. Its exact
contents are preserved as [Cargo.lock.snapshot](Cargo.lock.snapshot), SHA-256:

```text
ad42464c59618c1e56a7102589e51a26ffac0f55401b8e4d7e24c039f110bb1d
```

This snapshot does not establish that a clone resolving dependencies from
scratch will choose the same versions. The clean packaging experiment used
`git archive` of the baseline commit plus this lockfile, with an empty target
directory and the existing Cargo dependency cache.

The existing sccache wrapper could not run within the initial sandbox. Commands
used `RUSTC_WRAPPER=` and permission-reviewed cache/network access. An initial
offline Rust test attempt failed on uncached `rand 0.9.3`; the locked online run
fetched test dependencies. These were environment failures, not source defects.

## Tests actually executed

From the repository root, with the recorded lockfile in place:

```sh
RUSTC_WRAPPER= cargo build --release --locked
RUSTC_WRAPPER= cargo test --workspace --locked
# The first test run stopped at the formatter target; run all remaining targets:
RUSTC_WRAPPER= cargo test --workspace --locked --offline --no-fail-fast
./target/release/nera test -j 4 --timeout 30
cargo fmt --all -- --check
RUSTC_WRAPPER= cargo clippy --workspace --all-targets --locked --offline -- -D warnings
```

The final complete Rust run: **1,671 passed, 6 failed, 0 ignored**.

| Target | Pass | Fail |
| --- | ---: | ---: |
| CLI | 286 | 0 |
| Codegen | 80 | 0 |
| Formatter unit | 121 | 0 |
| Formatter integration | 92 | 0 |
| Formatter property | 3 | 1 |
| IR | 364 | 5 |
| Runtime | 436 | 0 |
| Syntax | 289 | 0 |

Doc-test targets had no tests. See [test-summary.txt](test-summary.txt) for failure
names and the first end-to-end failure messages.

The first end-to-end run: **282 passed, 2 failed, 2 skipped / 286**, in 140.18s.
Other baseline builds were active. `tcp_reactor` timed out after 30s;
`tcp_reactor_perf` reported a batch-variance stall. Both original programs then
passed three individual runs each; see [reactor-repeats.json](reactor-repeats.json).
An instrumented copy of `tcp_reactor` completed all nine subtests and exited.

The same full end-to-end command, repeated after our builds finished:
**284 passed, 0 failed, 2 skipped / 286**, in 68.59s. This is a passing repeat,
not proof that the original failures are resolved. The machine was not reserved
for benchmarking; neither timing is a product performance measurement.

Skipped: `ets_onecached_full_startup` and `ets_onecached_pattern`, both dependent
on external OneCached modules. Helper files explain the difference between 296
tracked `.erl` files and 286 discovered tests.

Formatting check: differences in 123 files. Strict Clippy stops on eight
pre-existing syntax-crate findings (range checks, needless return, collapsible
conditionals, type complexity); later crates may have additional findings.
Neither command changed source files.

## Real dependency integrations

Copied the checked-in `nera.toml` and `src/` for each integration into separate
empty directories, then ran the newly built compiler by absolute path:

```sh
nera deps get
nera build -o integration-test
./integration-test
```

- **Thoas v1.2.1:** fresh fetch, build and execution pass, ending in
  `all tests passed`.
- **Poolboy 1.5.2:** fresh fetch succeeds, compilation fails on `ets:match/2`,
  `ets:select/2`, `supervisor:start_child/2`, `supervisor:terminate_child/2`, and
  `supervisor:which_children/1`. Its executable was not run.

The dependency manifests pin tags rather than immutable commit hashes. The
project's existing optional CI treatment of Poolboy is retained.

## Small correctness reproductions

Run from the repository root:

```sh
python3 docs/baselines/2026-10-10/reproduce.py --nera "$PWD/target/release/nera"
```

Requires `erl` and `erlc` on PATH. The script writes into a temporary directory,
compiles each diagnostic independently, prints JSON and exits 1 on behavioural
differences. It is intentionally red on the baseline, outside the regular test
suite. [Recorded output](probe-results.json).

| Probe | BEAM reference | Nera result |
| --- | --- | --- |
| Resumed exception | `before`, then `after` | `before` only; exit 0 |
| Caught gen_server timeout, then mailbox check | `no_late_reply` | No output; exit 0 before final check |
| Formatter guard, evaluated on BEAM | Original prints `matched` | Formatted source prints `fallback` |

[Control observations](continuation-probes.json) distinguish an immediate caught
exit and a delayed normal return (both continue) from the delayed caught exit
(does not continue). The late-reply probe is blocked by the continuation defect;
it does not yet demonstrate the anticipated stale-mailbox behaviour.

The formatter probe demonstrates a semantic change independently of the failed
randomized idempotence property test. The current CLI prints nothing under
`--stdout` for an unchanged file; the reproduction accounts for that behaviour.

## Clean package / actual installation

Using a clean `git archive` copy plus the recorded lockfile:

```sh
RUSTC_WRAPPER= CARGO_NET_OFFLINE=true bash scripts/dist.sh \
  --version 0.1.0-baseline --target aarch64-apple-darwin
```

This is the target argument used by the release workflow. The command succeeds
but prints `Cross-compiling from arm64-apple-darwin`. Its first compiler build
generates `EMBEDDED_RUNTIME: None`, then the runtime is built separately.

Unpacked with the system `tar` into a separate installation directory; compiled
this program from a separate working directory with a fresh `TMPDIR`:

```erlang
-module(hello).
-export([start/0]).
start() -> io:format("Hello, preview!~n").
```

The archive's `bin/nera build hello.erl -e hello:start -o hello -v` exits 1:
`runtime library not found`, then undefined runtime symbols at link time.
The runtime is present under `lib/nera/`, but the compiler searches `lib/`.

**One-variable control:** copy the existing archive library from
`lib/nera/libnera_runtime.a` to `lib/libnera_runtime.a` in the disposable
installation. Repeating the same compile succeeds; the binary prints
`Hello, preview!`. No production source or original archive was modified.

An earlier extraction via Python tarfile exposed macOS AppleDouble `._*.erl`
metadata entries and failed before linking. The authoritative installation test
above used the documented system `tar` extraction; it independently establishes
the runtime-path failure. Metadata-free archive creation is also worth checking
when making archives portable across extraction tools.

## Evidence and untested boundaries

- [results.json](results.json) binds command outcomes to the revision and lock
  checksum. `$REPO` and `$CLEAN_COPY` stand for the local checkout and clean copy.
- Raw logs and disposable installations remain locally under
  `target/preview-baseline-2026-10-10/` (ignored build artifacts).
- [raw-log-hashes.json](raw-log-hashes.json) records their SHA-256 digests; the
  concise evidence files here are the durable repository record.
- The initial release build used the existing checkout/target directory; the
  separate archive experiment used an empty target directory. Do not conflate
  them or claim a clean installed compiler passed without the path control.
- No Linux/Windows runtime execution, editor-plugin build, OneCached service
  soak, new throughput measurement, broad differential corpus, sanitizer run
  or fuzz campaign was performed.
- No tag, push, release, PR, visibility change or CI dispatch occurred.

The follow-up work is to repair the named defects and collect a new baseline
against the exact resulting source revision, not to label this snapshot green.

## Documentation handoff validation

`BASE_REF=main`; `BASE=c5b89ee65d679da1f2fe257aef73f130859440bb`;
`TARGET=the uncommitted documentation/evidence worktree`. The recorded source
tree above remains the tested production tree: no production files changed.
Local HEAD and cached `origin/main` have 0 ahead / 0 behind divergence.

The scoped change consists of the active status file, this dated evidence
directory, and corrections/cross-links in README, CONTRIBUTING, FEATURES,
ROADMAP, the EEP/Phase 8/release/testing documents, and the test/Poolboy readmes.
The pre-existing `.claude/` and `.lavish/` contents are unrelated.

Whitespace checks, local Markdown file-link checks, JSON parsing and Python
syntax validation passed across the 24 scoped documentation/evidence files.
The diagnostic runner was executed and produced the expected red comparisons.
No new committed tree exists; nothing was staged, committed or published.
