#!/usr/bin/env python3
"""Build a disposable workspace. Never patch the user's runtime/compiler sources."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import tempfile
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--output', type=Path, default=HERE / 'results')
parser.add_argument('--resume', action='store_true')
args = parser.parse_args()
OUT = args.output.resolve()
OUT.mkdir(parents=True, exist_ok=True)
RESUME = args.resume
WORK = Path(json.loads((OUT/'metadata.json').read_text())['workspace']) if RESUME else Path(tempfile.mkdtemp(prefix='nera-base64-'))
# Reuse dependency artifacts, not the normal target/release executable.
TARGET = ROOT / 'target' / 'base64-prototype'
ENV = dict(os.environ, CARGO_TARGET_DIR=str(TARGET), RUSTC_WRAPPER="", NERA_SCHEDULERS="1")

def run(args, log, cwd=WORK, timeout=900):
    print('Running:', ' '.join(map(str,args)), flush=True)
    p = subprocess.run(list(map(str,args)),cwd=cwd,env=ENV,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=timeout)
    (OUT/log).write_text(p.stdout)
    if p.returncode:
        raise RuntimeError(f'{args}: exit {p.returncode}; see {OUT/log}\n{p.stdout[-3000:]}')
    return p.stdout

def replace(path, old, new):
    p = WORK/path; s = p.read_text()
    assert old in s, (path,old)
    p.write_text(s.replace(old,new,1))

if not RESUME:
    for name in ['Cargo.toml','Cargo.lock','rust-toolchain.toml']:
        shutil.copy2(ROOT/name,WORK/name)
    for name in ['crates','erlang']:
        shutil.copytree(ROOT/name,WORK/name,ignore=shutil.ignore_patterns('target','.git','node_modules'))
    # Build-script inputs and compiler's source-relative metadata.
    for name in ['README.md','LICENSE','CHANGELOG.md']:
        shutil.copy2(ROOT/name,WORK/name)
    manifest = {str(p.relative_to(WORK)):hashlib.sha256(p.read_bytes()).hexdigest()
                for p in WORK.rglob('*') if p.is_file()}
    (OUT/'source-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
    meta = {'base_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
            'input_status':subprocess.check_output(['git','status','--short'],cwd=ROOT,text=True),
            'platform':platform.platform(),'machine':platform.machine(),'workspace':str(WORK),
            'rustc':subprocess.check_output(['rustc','--version'],cwd=ROOT,text=True).strip(),
            'scheduler_threads':1,'timing_instrumented':False,
            'erlang_baseline_sha256':hashlib.sha256((HERE/'base64_reference.erl').read_bytes()).hexdigest()}
    (OUT/'metadata.json').write_text(json.dumps(meta,indent=2)+'\n')
    ffi = WORK/'crates/nera-runtime/src/ffi'
    shutil.copy2(HERE/'codec.rs',ffi/'base64_codec.rs')
    shutil.copy2(HERE/'adapter.rs',ffi/'base64_prototype.rs')
    replace(Path('crates/nera-runtime/src/ffi/mod.rs'),'mod binary_ops;','mod binary_ops;\nmod base64_prototype;')
    entries = ''
    for f,arity in [('encode',3),('decode',3),('start',0),('stop',0)]:
        entries += f'    BifEntry {{ module: "nera_base64_native", function: "{f}", arity: {arity}, ffi_symbol: Some("nera_rt_base64_{f}"), auto_import: false, param_names: None, spec: None }},\n'
    replace(Path('crates/nera-ir/src/bif_table.rs'),'pub const BIF_TABLE: &[BifEntry] = &[','pub const BIF_TABLE: &[BifEntry] = &[\n'+entries)
    replace(Path('crates/nera-runtime/src/ffi/apply.rs'),'use super::binary_ops::*;','use super::binary_ops::*;\nuse super::base64_prototype::*;')
    entries = ''
    for f,arity in [('encode',3),('decode',3),('start',0),('stop',0)]:
        args = ''.join(f', arg_terms[{i}]' for i in range(arity))
        entries += f'        (Some("nera_base64_native"), Some("{f}"), {arity}) => Some(nera_rt_base64_{f}(ctx{args})),\n'
    replace(Path('crates/nera-runtime/src/ffi/apply.rs'),'let result = match (mod_name.as_deref(), fun_name.as_deref(), arity) {','let result = match (mod_name.as_deref(), fun_name.as_deref(), arity) {\n'+entries)
    replace(Path('crates/nera-runtime/src/lib.rs'),'pub mod term;','pub(crate) mod base64_metrics;\npub mod term;')
    metrics = WORK/'crates/nera-runtime/src/base64_metrics.rs'
    metrics.write_text('pub fn start() {}\npub fn stop() -> [u64; 4] { [0; 4] }\n')
    for name in ['base64_rust.erl','differential.erl','bench.erl','base64_reference.erl']:
        shutil.copy2(HERE/name,WORK/name)
(WORK/'erlang/stdlib/base64.erl').write_text((HERE/'base64_reference.erl').read_text().replace('-module(base64_reference).', '-module(base64).'))
metrics = WORK/'crates/nera-runtime/src/base64_metrics.rs'
compiler = TARGET/'release/nera'

def build(label):
    packages = ['-p','nera-runtime'] if RESUME or label == 'allocations' else ['-p','nera-runtime','-p','nera']
    run(['cargo','build','--release','--locked'] + packages,f'build-{label}.log')
def compile_erl(module,label):
    exe = WORK/f'{module}-{label}'
    run([compiler, 'build', WORK/f'{module}.erl',WORK/'base64_rust.erl','-e',f'{module}:start','-o',exe],f'compile-{module}-{label}.log')
    return exe

if RESUME:
    p = WORK/'crates/nera-runtime/src/lib.rs'
    p.write_text(p.read_text().replace('static GLOBAL: base64_metrics::Counting = base64_metrics::Counting;', 'static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;'))
    p = WORK/'crates/nera-runtime/src/heap.rs'
    p.write_text(p.read_text().replace('\n        crate::base64_metrics::heap(n * 8);',''))
    metrics.write_text('pub fn start() {}\npub fn stop() -> [u64; 4] { [0; 4] }\n')
    for name in ['base64_rust.erl','differential.erl','bench.erl','base64_reference.erl']:
        shutil.copy2(HERE/name,WORK/name)
    for name,dest in [('codec.rs','base64_codec.rs'),('adapter.rs','base64_prototype.rs')]:
        shutil.copy2(HERE/name,WORK/'crates/nera-runtime/src/ffi'/dest)
build('timing')
run(['cargo','test','--locked','-p','nera-runtime','base64_prototype'], 'runtime-tests.log')
diff = run([compile_erl('differential','timing')],'differential.log',timeout=300)
assert diff.strip() == 'DIFFERENTIAL_OK', diff[-2000:]
# Reuse the shipped explicit vectors against the wrapper, including errors and apply/fun.
s = (ROOT/'tests/base64_options.erl').read_text().replace('base64:', 'base64_rust:').replace('apply(base64,','apply(base64_rust,')
s = s.replace('-module(base64_options).','-module(options_test).').replace('-export([test/0]).','-export([start/0]).\nstart() -> test(), halt(0).')
(WORK/'options_test.erl').write_text(s)
assert run([compile_erl('options_test','timing')],'options.log').strip() == 'ok'
run([compile_erl('bench','timing')], 'timing.log', timeout=600)
# Counters only in a second build; do not use its durations for speed comparisons.
shutil.copy2(HERE/'metrics.rs',metrics)
replace(Path('crates/nera-runtime/src/lib.rs'),'static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;', 'static GLOBAL: base64_metrics::Counting = base64_metrics::Counting;')
replace(Path('crates/nera-runtime/src/heap.rs'),'        self.young_bytes_allocated += n * 8;', '        self.young_bytes_allocated += n * 8;\n        crate::base64_metrics::heap(n * 8);')
build('allocations')
assert run([compile_erl('differential','allocations')],'differential-allocations.log',timeout=300).strip() == 'DIFFERENTIAL_OK'
run([compile_erl('bench','allocations')], 'allocations.log',timeout=600)
rows = []
for kind in ['timing','allocations']:
    lines = (OUT/f'{kind}.log').read_text().splitlines()
    assert lines[-1] == 'BENCH_OK'
    for line in lines[:-1]:
        f = line.split(); assert f[0] == 'ROW' and len(f) == 14, line
        row = dict(zip(['module','operation','size','mode','padding','repeat','iterations','us','alloc_calls','alloc_bytes','heap_calls','heap_bytes','checksum'],f[1:]))
        for key in ['size','repeat','iterations','us','alloc_calls','alloc_bytes','heap_calls','heap_bytes','checksum']: row[key] = int(row[key])
        raw_size = row['size']
        expected_size = ((raw_size+2)//3)*4 if row['operation']=='encode' else raw_size
        assert row['checksum'] == expected_size*row['iterations'], row
        row['kind'] = kind; rows.append(row)
(OUT/'rows.json').write_text(json.dumps(rows,indent=2)+'\n')
summary = []
keys = sorted({(r['operation'],r['size'],r['mode'],r['padding']) for r in rows})
for op,size,mode,pad in keys:
    item = dict(operation=op,size=size,mode=mode,padding=pad)
    for module,label in [('base64','erlang'),('base64_rust','rust')]:
        for kind,metrics_names in [('timing',['us']),('allocations',['alloc_calls','alloc_bytes','heap_calls','heap_bytes'])]:
            subset = [r for r in rows if (r['kind'],r['module'],r['operation'],r['size'],r['mode'],r['padding'])==(kind,module,op,size,mode,pad)]
            assert len(subset)==7
            for metric in metrics_names:
                item[f'{label}_{metric}_per_op'] = statistics.median(r[metric]/r['iterations'] for r in subset)
    item['speedup'] = item['erlang_us_per_op']/item['rust_us_per_op']
    summary.append(item)
(OUT/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps(summary,indent=2))
print('Workspace preserved:',WORK)

run(['cargo','clippy','--locked','-p','nera-runtime','--lib','--','-D','warnings'], 'clippy.log')
subprocess.run([sys.executable, str(HERE/'report.py'), '--output', str(OUT)], check=True)
