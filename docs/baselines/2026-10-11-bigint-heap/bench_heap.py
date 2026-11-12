"""Compare the actual BumpChunk/YoungGen implementations against the pinned base.
Run from the worktree root. Only extracted allocator code is benchmarked.
"""
import json, pathlib, subprocess, tempfile, statistics
source = pathlib.Path('crates/nera-runtime/src/heap.rs')
versions = {'before': subprocess.check_output(['git', 'show', 'ab347d5a9f6dad929e68cc0a3bfde7782bc2740e:'+str(source)], text=True), 'after': source.read_text()}
harness = '''
fn main() {
    let start = std::time::Instant::now();
    let mut checksum = 0usize;
    for _ in 0..2000 {
        let mut heap = YoungGen::new(256);
        for _ in 0..10000 {
            checksum ^= std::hint::black_box(heap.alloc_words(std::hint::black_box(2))) as usize;
        }
        std::hint::black_box(&heap);
    }
    std::hint::black_box(checksum);
    println!("{}", start.elapsed().as_nanos());
}
'''
with tempfile.TemporaryDirectory() as directory:
    paths = {}
    for name, text in versions.items():
        code = text[text.index('struct BumpChunk {'):text.index('/// Clone an `Arc<BinaryData>`')]
        path = pathlib.Path(directory)/name
        path.with_suffix('.rs').write_text('#![allow(dead_code, unused_imports)]\nuse std::alloc::Layout;\n'+code+harness)
        subprocess.run(['rustc','--edition=2021','-O',str(path.with_suffix('.rs')),'-o',str(path)],check=True)
        paths[name]=path
        subprocess.check_output([path])
    samples = {name:[] for name in paths}
    for pair in range(9):
        for name in (['before','after'] if pair%2==0 else ['after','before']):
            samples[name].append(int(subprocess.check_output([paths[name]])))
    medians={name:statistics.median(values) for name,values in samples.items()}
    print(json.dumps({'samples_ns':samples,'medians_ns':medians,'percent_change':100*(medians['after']/medians['before']-1)},indent=2))
