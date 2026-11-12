#!/usr/bin/env python3
"""Summarize completed measurements and bind their inputs/executables by SHA-256."""
import argparse
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--output', type=Path, default=HERE / 'results')
args = parser.parse_args()
OUT = args.output.resolve()
ROOT = HERE.parents[1]
meta = json.loads((OUT/'metadata.json').read_text())
work = Path(meta['workspace'])
summary = json.loads((OUT/'summary.json').read_text())
assert (OUT/'differential.log').read_text().strip() == 'DIFFERENTIAL_OK'
assert (OUT/'differential-allocations.log').read_text().strip() == 'DIFFERENTIAL_OK'
assert (OUT/'options.log').read_text().strip() == 'ok'
for source, dest in [('codec.rs','crates/nera-runtime/src/ffi/base64_codec.rs'),
                     ('adapter.rs','crates/nera-runtime/src/ffi/base64_prototype.rs'),
                     ('metrics.rs','crates/nera-runtime/src/base64_metrics.rs'),
                     ('base64_rust.erl','base64_rust.erl'),
                     ('bench.erl','bench.erl'), ('differential.erl','differential.erl')]:
    assert (HERE/source).read_bytes() == (work/dest).read_bytes(), source

def digest(p): return hashlib.sha256(p.read_bytes()).hexdigest()
artifacts = {
    'prototype_sources': {p.name:digest(p) for p in HERE.iterdir() if p.suffix in {'.rs','.erl','.py'}},
    'reference_options_test':digest(ROOT/'tests/base64_options.erl'),
    'executables': {name:digest(work/name) for name in ['bench-timing','bench-allocations','differential-timing','differential-allocations','options_test-timing']},
    'patched_runtime_sources': {str(p.relative_to(work)):digest(p) for p in (work/'crates/nera-runtime/src').rglob('*.rs')},
    'patched_bif_table':digest(work/'crates/nera-ir/src/bif_table.rs'),
    'evidence': {p.name:digest(p) for p in OUT.iterdir() if p.suffix in {'.json','.log'} and p.name != 'artifacts.json'},
}
(OUT/'artifacts.json').write_text(json.dumps(artifacts,indent=2)+'\n')
lines = ['# Base64 prototype results — 2026-10-13','',
    f"Reference source: `{meta['base_head']}`; {meta['platform']}; {meta['rustc']}.",
    'The source snapshot includes working-tree changes; source-manifest.json is authoritative, not HEAD alone.',
    'Release builds with LTO, one scheduler worker. Seven alternating-order samples; medians per call.',
    'Rust samples use 32 times the iteration count to exercise sustained allocation and avoid short timing blocks.',
    'The reference is the current Erlang stdlib compiled to native code by Nera, not BEAM.', '',
    '## Throughput', '', '| Operation | Raw bytes | Options | Erlang µs | Rust µs | Speedup |',
    '|---|---:|---|---:|---:|---:|']
for r in summary:
    lines.append(f"| {r['operation']} | {r['size']:,} | {r['mode']}, padding={r['padding']} | {r['erlang_us_per_op']:.3f} | {r['rust_us_per_op']:.3f} | {r['speedup']:.1f}× |")
lines += ['', '## Allocation traffic at 65,538 raw bytes, standard alphabet with padding', '',
          '| Operation | Metric per call | Erlang | Rust |', '|---|---|---:|---:|']
for r in summary:
    if r['size'] != 65538 or r['mode'] != 'standard': continue
    for key,label in [('alloc_calls','Global allocation/reallocation requests'),('alloc_bytes','Global requested bytes'),('heap_calls','Process-heap reservation calls'),('heap_bytes','Process-heap reserved bytes')]:
        lines.append(f"| {r['operation']} | {label} | {r['erlang_'+key+'_per_op']:,.1f} | {r['rust_'+key+'_per_op']:,.1f} |")
lines += ['', 'These counters measure cumulative traffic, not simultaneously live memory. Global and process-heap bytes overlap; do not add them. See the prototype README methodology for counter coverage and GC/inline-region details.', '',
    '## Correctness and checks', '',
    '- Differential suite: no value or exception-class/reason differences across more than 78,000 comparisons, in both builds.',
    '- Covers all prefixes of bytes 0–255, options, lists, wrong types, representative exhaustive short encodings, every byte in every quartet position, whitespace, noncanonical tails, subbinaries, and repeated large operations with GC pressure.',
    '- Existing `tests/base64_options.erl` adapted to the wrapper passes, including dynamic apply and remote fun calls.',
    '- Three focused Rust tests pass, including 16,388 length/option round trips. The mutation checks from the initial experiment are not rerun here.',
    '- Runtime Clippy passes with warnings denied. This rerun does not repeat the initial scoped rustfmt check.',
    '- Every benchmark row validates a consumed-output size checksum; full byte equality is checked separately by the differential suite.', '',
    '## Interpretation and limits', '',
    'This prototype removes intermediate Erlang lists/binaries and most runtime allocation work. The very large speedups are relative to this particular Nera implementation and compiler; they are not a general Rust-versus-Erlang result. The counters do not by themselves attribute all cost to individual logical objects or GC phases; see the separate match-context allocation investigation.',
    'Valid binary inputs take the native path. Lists and invalid inputs preserve the Erlang reference through fallback. Stack frames are different. Large binary calls are not preemptible; scheduling, very-large-input/resource limits and concurrent latency still require work before production integration.',
    'No production compiler/runtime files were changed. No commit, push or merge was performed. Full workspace, cross-platform and release gates were not run for this isolated prototype.', '',
    'Inputs, patched runtime sources, executables and evidence are fingerprinted in `artifacts.json`; the original source snapshot is in `source-manifest.json`. Raw timing/allocation rows are in `rows.json`.', '']
((HERE if OUT == HERE / 'results' else OUT)/'RESULTS.md').write_text('\n'.join(lines))
