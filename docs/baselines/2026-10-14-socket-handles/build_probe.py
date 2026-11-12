"""Compile the exact registry implementation as an isolated release probe."""
import itertools, pathlib, subprocess, sys
root = pathlib.Path(sys.argv[1])
label = sys.argv[2]
output = pathlib.Path(sys.argv[3])
source = (root/'crates/nera-runtime/src/socket.rs').read_text()
registry = source[source.index('const FREE_SENTINEL:'):source.index('/// Convenience: run a closure')]
checked = '.unwrap()' if 'SocketHandle' in registry else ''
harness = '''
fn main() {
    println!("slot_bytes {}", std::mem::size_of::<Slot>());
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let entry = Arc::new(SocketEntry {
        socket: Socket::Listener(Arc::new(mio::net::TcpListener::from_std(listener))),
        opts: Mutex::new(SocketOpts::default()),
    });
    let live = REGISTRY.write().insert(entry.clone())CHECKED;
    for threads in [1, 2, 4, 8] {
        let start = std::time::Instant::now();
        std::thread::scope(|scope| {
            for _ in 0..threads {
                scope.spawn(|| {
                    for _ in 0..(1_000_000 / threads) {
                        let owner = get_entry(std::hint::black_box(live)).unwrap();
                        std::hint::black_box(&owner);
                    }
                });
            }
        });
        println!("lookup_{threads} {}", start.elapsed().as_micros());
    }
    let mut registry = Registry::new();
    let mut handle = registry.insert(entry.clone())CHECKED;
    let start = std::time::Instant::now();
    for _ in 0..200_000 {
        let removed = registry.remove(std::hint::black_box(handle)).unwrap();
        handle = registry.insert(removed)CHECKED;
    }
    println!("reuse {}", start.elapsed().as_micros());
    let mut fresh = Registry::new();
    let start = std::time::Instant::now();
    for _ in 0..50_000 {
        std::hint::black_box(fresh.insert(entry.clone())CHECKED);
    }
    println!("fresh {}", start.elapsed().as_micros());
}
'''.replace('CHECKED', checked)
header = 'use std::{io, sync::Arc};\nuse parking_lot::{Mutex, RwLock};\nuse nera_runtime::socket::{Socket, SocketEntry, SocketOpts};\nuse nera_runtime::term;\n'
probe = output.with_suffix('.rs')
probe.write_text(header + registry + harness)
target = pathlib.Path('/tmp/nera-transfer-target/release')
args = ['rustc','--edition=2021','-O','-C','codegen-units=1','-A','dead_code','-A','unused_imports',str(probe),'-o',str(output),'-L',f'dependency={target}/deps','--extern',f'nera_runtime={target}/libnera_runtime.rlib']
candidates = [list((target/'deps').glob(f'lib{crate}-*.rlib')) for crate in ['parking_lot', 'mio']]
for libs in itertools.product(*candidates):
    extra = sum((['--extern',f'{crate}={lib}'] for crate,lib in zip(['parking_lot','mio'], libs)), [])
    result = subprocess.run(args + extra, cwd=root, capture_output=True, text=True)
    if result.returncode == 0:
        print(f'{label}: {output}; extracted registry: {probe}; dependencies: {libs}')
        break
else:
    raise RuntimeError(result.stderr)
