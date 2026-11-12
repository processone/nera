#!/usr/bin/env python3
"""Attribute allocation traffic and compare the original/fixed shared runtime path.
Requires the isolated compiler created by benchmarks/base64_prototype/run.py.
The existing prototype's workspace, logs and binaries are never modified.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import statistics
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
PRIOR = ROOT/'benchmarks/base64_prototype'
BASE = json.loads((PRIOR/'results/metadata.json').read_text())
ORIGINAL = Path(BASE['workspace'])
WORK = Path(tempfile.mkdtemp(prefix='nera-match-allocation-'))
TARGET = ROOT/'target/base64-prototype'
COMPILER = TARGET/'release/nera'
ENV = dict(os.environ, RUSTC_WRAPPER='', CARGO_TARGET_DIR=str(TARGET))
RESULTS = HERE/'measurements'
RESULTS.mkdir(exist_ok=True)

def run(cmd, log, timeout=900):
    print('Running:', ' '.join(map(str,cmd)), flush=True)
    with (RESULTS/log).open('w') as stream:
        p = subprocess.run(list(map(str,cmd)),cwd=WORK,env=ENV,text=True,stdout=stream,stderr=subprocess.STDOUT,timeout=timeout)
    assert p.returncode == 0, (cmd, p.returncode, RESULTS/log)
    return (RESULTS/log).read_text()

def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()

for name in ['Cargo.toml','Cargo.lock','rust-toolchain.toml']:
    shutil.copy2(ORIGINAL/name,WORK/name)
for name in ['crates','erlang']:
    shutil.copytree(ORIGINAL/name,WORK/name,ignore=shutil.ignore_patterns('target'))
for name in ['base64_rust.erl','differential.erl','options_test.erl']:
    shutil.copy2(ORIGINAL/name,WORK/name)
shutil.copy2(HERE/'probe.erl',WORK/'probe.erl')
shutil.copy2(ROOT/'tests/binary_match_reuse_alloc.erl',WORK/'reuse_test.erl')
p=WORK/'reuse_test.erl'
p.write_text(p.read_text().replace('-module(binary_match_reuse_alloc).','-module(reuse_test).').replace('-export([test/0]).','-export([start/0]).\nstart() -> test(), halt(0).'))
alloc = WORK/'crates/nera-runtime/src/ffi/alloc.rs'
old_alloc = subprocess.check_output(['git','show',BASE['base_head']+':crates/nera-runtime/src/ffi/alloc.rs'],cwd=ROOT,text=True)
new_alloc = (ROOT/'crates/nera-runtime/src/ffi/alloc.rs').read_text()
lib = WORK/'crates/nera-runtime/src/lib.rs'
heap = WORK/'crates/nera-runtime/src/heap.rs'
base_lib = lib.read_text().replace('static GLOBAL: base64_metrics::Counting = base64_metrics::Counting;', 'static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;')
base_heap = heap.read_text().replace('\n        crate::base64_metrics::heap(n * 8);','')
metrics = WORK/'crates/nera-runtime/src/base64_metrics.rs'
instrumented = (PRIOR/'metrics.rs').read_text().replace('[u64; 4]','[u64; 8]').replace('[0; 4]','[0; 8]')
instrumented = instrumented.replace('    static ACTIVE:', '    static IN_GC: Cell<bool> = const { Cell::new(false) };\n    static ACTIVE:')
instrumented = instrumented.replace('                c.set(v);', '                if index == 0 && IN_GC.with(Cell::get) { v[6] += 1; v[7] += bytes as u64; }\n                c.set(v);')
instrumented += '''
pub fn refill(bytes: usize) { add(4, bytes); }
pub struct GcScope(bool);
pub fn gc_scope() -> GcScope { GcScope(IN_GC.with(|c| c.replace(true))) }
impl Drop for GcScope { fn drop(&mut self) { IN_GC.with(|c| c.set(self.0)); } }
'''
rows=[]; artifacts={}
for kind in ['timing','allocations']:
    if kind == 'timing':
        metrics.write_text('pub fn start() {}\npub fn stop() -> [u64; 8] { [0; 8] }\n')
        lib.write_text(base_lib); heap.write_text(base_heap)
    else:
        metrics.write_text(instrumented)
        lib.write_text(base_lib.replace('static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;', 'static GLOBAL: base64_metrics::Counting = base64_metrics::Counting;'))
        s=base_heap.replace('        self.young_bytes_allocated += n * 8;', '        self.young_bytes_allocated += n * 8;\n        crate::base64_metrics::heap(n * 8);')
        s=s.replace('        let words = self.inline_chunk_words;', '        let words = self.inline_chunk_words;\n        crate::base64_metrics::refill(words * 8);')
        for name in ['minor','major']:
            marker=f'    fn collect_{name}(&mut self, roots: &mut [ErlTerm]) {{'
            assert marker in s
            s=s.replace(marker,marker+'\n        let _scope = crate::base64_metrics::gc_scope();')
        heap.write_text(s)
    for variant,source in [('before',old_alloc),('after',new_alloc)]:
        label=f'{variant}-{kind}'; alloc.write_text(source)
        run(['cargo','build','--release','--locked','-p','nera-runtime'],f'build-{label}.log')
        artifacts[label]={'runtime':sha(TARGET/'release/libnera_runtime.a'),'alloc_source':sha(alloc),'heap_source':sha(heap),'metrics_source':sha(metrics),'lib_source':sha(lib)}
        for module in ['probe','differential','options_test','reuse_test']:
            exe=WORK/f'{module}-{label}'
            run([COMPILER,'build',WORK/f'{module}.erl',WORK/'base64_rust.erl','-e',f'{module}:start','-o',exe],f'compile-{module}-{label}.log')
            artifacts[label][module]=sha(exe)
            if module != 'probe':
                expected='DIFFERENTIAL_OK' if module == 'differential' else 'ok'
                assert run([exe],f'{module}-{label}.log',300).strip()==expected
    # All builds complete before timing. Alternate complete executable runs.
    for variant in ['before','after','after','before']:
        rep=sum(1 for r in rows if r['variant']==variant and r['kind']==kind)//63
        text=run([WORK/f'probe-{variant}-{kind}'],f'probe-{variant}-{kind}-{rep}.log',600)
        lines=text.splitlines(); assert lines[-1]=='PROBE_OK'
        for line in lines[:-1]:
            v=line.split(); assert v[0]=='ROW' and len(v)==15,line
            row=dict(zip(['operation','size','repeat','iterations','us','alloc_calls','alloc_bytes','heap_calls','heap_bytes','refills','refill_bytes','gc_alloc_calls','gc_alloc_bytes','checksum'],v[1:]))
            for key in list(row)[1:]: row[key]=int(row[key])
            expected={'encode':row['size']//3*4,'decode':row['size'],'scan':row['size']//6*748}[row['operation']]
            assert row['checksum']==expected*row['iterations'],row
            row.update(variant=variant,kind=kind,run=rep); rows.append(row)
summary=[]
for op in ['encode','decode','scan']:
    for size in [1026,16386,65538]:
        item={'operation':op,'size':size}
        for variant in ['before','after']:
            for kind,names in [('timing',['us']),('allocations',['alloc_calls','alloc_bytes','heap_calls','heap_bytes','refills','refill_bytes','gc_alloc_calls','gc_alloc_bytes'])]:
                rs=[r for r in rows if (r['variant'],r['kind'],r['operation'],r['size'])==(variant,kind,op,size)]
                assert len(rs)==14
                for name in names: item[f'{variant}_{name}']=statistics.median(r[name]/r['iterations'] for r in rs)
        item['speedup']=item['before_us']/item['after_us']; summary.append(item)
(RESULTS/'rows.json').write_text(json.dumps(rows,indent=2)+'\n')
(RESULTS/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
(RESULTS/'artifacts.json').write_text(json.dumps({'base':BASE['base_head'],'workspace':str(WORK),'compiler':sha(COMPILER),'variants':artifacts,'sources':{p.name:sha(p) for p in HERE.iterdir() if p.suffix in {'.py','.erl'}}},indent=2)+'\n')
print(json.dumps(summary,indent=2))
