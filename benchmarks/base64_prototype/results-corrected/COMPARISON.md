# Base64 after the allocation and bit-offset fixes

Same workload and prototype as the initial experiment. The new source snapshot includes both uncommitted fixes; see `source-manifest.json`. Historical results in `../results/` are preserved.

## Standard alphabet with padding, 65,538 raw bytes

| Operation | Previous Nera µs | Corrected Nera µs | Rust µs | Rust speedup over corrected Nera |
|---|---:|---:|---:|---:|
| decode | 19081.067 | 18586.200 | 360.700 | 51.53× |
| encode | 15827.067 | 5028.733 | 58.177 | 86.44× |

| Operation | Allocation metric, bytes/call | Previous Nera | Corrected Nera | Rust |
|---|---|---:|---:|---:|
| decode | Global allocator requests | 30,409,890.5 | 30,409,890.5 | 65,664.0 |
| decode | Process-heap reservations | 5,290,387.7 | 5,290,387.7 | 106.3 |
| encode | Global allocator requests | 264,158,669.0 | 5,979,956.7 | 87,507.0 |
| encode | Process-heap reservations | 90,128,682.1 | 1,057,267.7 | 105.3 |

The encoder is 3.15× faster than in the historical run. Its cumulative global allocation traffic falls by 97.74%, and process-heap reservations by 98.83%. Decoder allocation traffic is unchanged; its small timing difference is not evidence of a material improvement. Historical and current timings were measured in separate runs, so environmental noise remains.

The Rust codec is unchanged and is still substantially faster. The earlier allocation investigation explains the eliminated encoder cost: repeatedly reserving an inline heap region on match-context reuse. Remaining Erlang intermediate objects and GC still contribute to cumulative traffic. The bit-offset correction restores correctness; this byte-aligned workload does not isolate a performance benefit from that correction.

Counters measure cumulative requested bytes, not peak/live memory. The global and process-heap counters overlap and must not be added. Timings come exclusively from the uninstrumented build; allocation counters come from a separate build. Seven samples, medians per call, one scheduler worker.

Both differential runs complete without value or exception-class/reason differences. Existing Base64 options tests and the prototype Rust tests pass. Full sizes/options and raw results are in [RESULTS.md](RESULTS.md), `summary.json` and `rows.json`.

Reproduce from the repository root:

```sh
python3 benchmarks/base64_prototype/run.py --output benchmarks/base64_prototype/results-corrected
```
