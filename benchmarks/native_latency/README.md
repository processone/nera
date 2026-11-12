# Native work latency

Run `python3 benchmarks/native_latency/run.py --nera /absolute/path/to/nera
--output /tmp/latency.json` on a quiet machine after building the release runtime
and compiler. `NERA_SCHEDULERS=1` is set for every measured process.

A parent and probe exchange ping/pong messages while another Erlang process
encodes either 2,048 binaries of 49,149 bytes or sixteen binaries of 8 MiB.
Input construction precedes timing; output allocation and GC during encoding
are included. Round trips are collected until the encoder completes (including the final
in-flight round trip). The idle case collects 2,000 round trips without encoding. One warmup is discarded
per case, followed by five fresh-process runs. Results include raw round-trip
samples, nearest-rank p50/p95/p99/max in microseconds, total elapsed time,
platform and source/compiler hashes. Output printing occurs after timing.

This measures responsiveness of runnable processes, not timer deadline accuracy,
external request latency, or a worst-case bound. Maximum and sample count are reported alongside percentiles. OS scheduling, allocation, GC and memory pressure affect results.
Do not run builds/tests concurrently. Compare runs on the same machine.

The decoder cases each perform sixteen decodes of 8 MiB: valid `A` symbols,
spaces only, and spaces followed by `Y===` (expected `badarith`). Input setup
precedes timing, but output reservation, error construction and GC are included.
They exercise useful output, scanning without output, and an error after many
suspensions. There is no fallback to the Erlang decoder for binary input.
