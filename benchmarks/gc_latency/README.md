# Redundant GC latency regression

This measures **the same production Rust Base64 list decoder**, before/after
GC trigger changes, with `NERA_SCHEDULERS=1`. No GC timers or allocator counters
are compiled into either measured runtime.

Use `run.py build --nera <release-compiler> --output <directory>` at each
revision after building the runtime and compiler (runtime first, so the compiler
also embeds the current library). Preserve the resulting executables before
switching revisions. Then run:

```sh
python3 benchmarks/gc_latency/run.py measure \
  --before /tmp/gc-before --after /tmp/gc-after --output /tmp/gc-results
```

Each case has one discarded fresh-process warmup per version, then five
fresh-process trials in alternating order. A parent measures ping/pong RTT to a
probe while a worker decodes lists for at least one second. The input contains
1,048,576 (`large`) or 8,388,608 (`huge`) cells; output size is checked. Time
includes the first worker collection; input construction and spawn copying
precede the timer. The deadline is checked after each complete decode, so a
single slow call may overrun one second. `waits` retains a 1,048,576-cell live
list over receive timeouts, and `idle` supplies the probe baseline. Keep other
builds and tests stopped during measurement.

The runner stores all RTT samples as compressed text and reports per-trial
p50, p99, maximum, sample count, elapsed time and operation count, plus median
p99, median maximum and worst maximum across trials. It validates executable
and workload hashes. The parent itself allocates a sample list, so its GC and
OS scheduling can contribute to outliers. These are observed latencies, not
bounds, and the GC collector remains synchronous.

See `docs/baselines/2026-10-13-gc-redundant/` for the recorded machine, source
revision, validation logs and before/after results.
