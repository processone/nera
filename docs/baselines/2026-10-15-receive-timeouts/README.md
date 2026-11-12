# Checked receive timeouts and unsafe helper contracts

2026-10-15. Base: main `4a8d33fd61546ed3c72274217f02ad2efa86e840` (PR #93).
Target: local worktree `mr/receive-timeout-contracts`,
`/tmp/nera-transferable-heap-fragments`. Rust 1.99.0, unchanged Cargo.lock,
Apple M1 / macOS ARM64. Final code/test fingerprints are preserved.

## Contract and implementation

ReceiveTimeout distinguishes Infinite from After(u32). The accepted domain is
infinity or an integer from zero through 4294967295 milliseconds, matching the
[Erlang reference](https://www.erlang.org/docs/29/system/expressions.html#receive).
Invalid terms raise timeout_value before mailbox scanning, so an existing
matching message does not hide the error or get consumed. Noninteger and negative
terms previously became zero or a huge unsigned delay; infinity became immediate.

The existing context representation is retained: zero means no deadline, finite
deadlines are positive monotonic milliseconds. Finite addition saturates safely;
the clock is not read for infinity. Accepting a message clears its deadline in
the Rust fallback and generated fast path, preventing stale deadlines from
leaking into later receives. The scheduler waiting loop and timer wheel layout
are unchanged. Two built-in atoms are appended (infinity=42, timeout_value=43);
existing built-in IDs remain unchanged. Compiler and runtime were built together.

Timeout setup can now fail. Generated code checks that error before scanning or
waiting, including direct propagation outside try and a jump to the lexical
exception landing pad inside try/catch. This is required in addition to validating
the FFI argument, since receive_wait would otherwise overwrite exception status.

The remaining safe-signature graph readers parse_socket_opts, the three private
pretty-format helpers and extract_line_from_info now explicitly require unsafe
calls. Their contracts require initialized/live input graphs, no GC movement,
and stable binary bytes until return. The socket parser takes ErlTerm instead
of raw u64. These signature/documentation changes add no runtime validation,
locks, ownership changes or restriction on scheduler reentrancy outside the
short non-reentrant graph-read scopes.

## RED/GREEN and validation

Both native regressions failed before implementation: receive_infinity ran its
after body, and receive_timeout_domain consumed a queued message for an invalid
timeout instead of raising timeout_value. See red.txt. Both pass after the fix.

- Workspace: **1,967 passed**, zero failed, one existing ignored.
- Runtime release: **626 passed**, zero failed, one existing ignored.
- Focused native regressions: **2 passed**; broader native set: **36 passed**.
  The regressions are included in the broader set: **36 distinct native tests**,
  zero failed/skipped.
- Workspace/all-targets Clippy with -D warnings and locked release build: passed.
- Runtime Intel macOS and x86_64 Windows MSVC compilation checks: passed;
  existing Windows networking fallback unused-code warnings remain.

Coverage includes zero, finite timeout/resumption, maximum finite input with a
queued message, infinity as a variable/literal after a finite receive, invalid
negative/oversized/float/noninteger terms, queued-message preservation, exceptions
inside try/catch and uncaught child errors via monitors. Unit coverage also checks
positive deadline sentinels, saturation and no mutation of an existing deadline
on invalid input. Broader native checks cover receive/selective receive, GC,
resumption, try/catch/stacktraces, formatting and TCP.

## Performance method and results

Three series of nine alternating before/after native pairs, with warm-up runs
for both binaries in every series: 27 observations per version. One scheduler,
same probe sources, real release runtime, no compilation/startup/printing inside
timers. Baseline runtime production code matches main; the additional main commit
between the prior build and this base changed only cowlib integration evidence
and its Python harness, not Rust compiler/runtime production sources.

The receive probe measures 100,000 self-send/accept cycles for ordinary receive,
after 0, after 1000 and after infinity, with 100 warm-up cycles. These cases
always have the matching message queued. It also times 10,000 empty after-0 polls
after 100 warm-ups. Every message/result is checked. The infinity measurement is
an equivalent queued-message comparison; the baseline blocking infinity behavior
was incorrect and is not a valid performance comparator.

The TCP probe uses 16 warm-ups followed by 2,000 verified transfers each for raw
active-once, line active-once, and continuous line mode. It includes setup changes
to the receive machinery as well as the helper-contract refactor; it does not
isolate the signature-only change. Socket/payload construction is outside timing.

| Case | Before median µs | After median µs | Pooled change | Series changes |
|---|---:|---:|---:|---:|
| after_finite | 3,730 | 3,699 | -0.83% | -1.10% to -0.24% |
| after_infinity | 3,889 | 2,066 | -46.88% | -47.12% to -46.84% |
| after_zero | 3,727 | 3,708 | -0.51% | -0.75% to -0.35% |
| empty_poll | 638 | 637 | -0.16% | -0.31% to +0.00% |
| plain | 1,913 | 1,944 | +1.62% | +0.62% to +1.71% |
| line_active | 63,519 | 64,123 | +0.95% | -2.52% to +2.85% |
| line_once | 60,717 | 59,509 | -1.99% | -2.84% to +2.49% |
| raw_once | 58,671 | 56,967 | -2.90% | -8.59% to -2.54% |

Ordinary receive is +1.62%, with all series positive (+0.62% to +1.71%). Retain
this as a possible cost of clearing the deadline on acceptance, rather than
claiming universal zero overhead. Finite setup and after-0 polling are close and
show no slowdown in these runs. Queued infinity is about 47% faster because it
no longer reads the clock; this is not a claim about blocking throughput. TCP
results change signs across series, so the pooled -2.90% to +0.95% establishes
no stable general improvement. Raw samples and binary hashes are retained.

The host was active: 1/5/15-minute load averages about 3.2/3.6/3.4 at start and
2.6/3.4/3.3 at end. No builds/tests were intentionally run during measurement.
Recheck on an isolated host before generalizing.

## Reproduction and coverage limits

With RUSTC_WRAPPER= and CARGO_TARGET_DIR=/tmp/nera-transfer-target:

```sh
cargo test --workspace --locked --no-fail-fast
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release --locked
cargo test -p nera-runtime --release --locked
/tmp/nera-transfer-target/release/nera test receive_timeout_domain receive_infinity -j 1 --timeout 30 --no-warnings
/tmp/nera-transfer-target/release/nera test 'receive*' 'selective_receive*' nested_receive gc_receive_compact 'try*' 'catch*' 'tcp_*' 'io_*' 'raise*' 'stacktrace*' -j 1 --timeout 30 --no-warnings
```

Cross-check nera-runtime with cargo check --locked --target
x86_64-pc-windows-msvc and x86_64-apple-darwin. Compile receive_probe.erl and
modes_probe.erl from /tmp, outside project discovery, using receive_probe:main
and modes_probe:main. Preserve /tmp/nera-receive-before|after and
/tmp/nera-timeout-modes-before|after, then run measure.py.

The full native suite, Linux/Windows execution, installed archives and contention
performance were not checked. CI/Release remain paused per CONTRIBUTING.md.
Independent review and explicit merge approval remain required before a merge.
This report records the validated pre-publication worktree. source-hashes.json
binds the code, fixtures and probes to the recorded validation and measurements.
