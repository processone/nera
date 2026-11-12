"""Control for separate-process allocation/scheduling bias in lookup measurements."""
import itertools,pathlib,subprocess
folder=pathlib.Path(__file__).resolve().parent
root=folder.parents[2]
parts=['use std::sync::Arc;\nuse parking_lot::Mutex;\nuse nera_runtime::socket::{Socket,SocketEntry,SocketOpts};\nuse nera_runtime::term;\n']
for label in ['before','after']:
    source=(folder/f'{label}-registry-probe.rs').read_text().split('fn main() {')[0]
    checked='.unwrap().as_raw()' if label=='after' else ''
    raw='SocketHandle::try_from_raw(1 << 32).unwrap()' if label=='after' else '1 << 32'
    wrappers=f'''pub(super) fn install(entry: Arc<SocketEntry>) {{ assert_eq!(REGISTRY.write().insert(entry){checked}, 1 << 32); }}
#[inline(never)] pub(super) fn lookup() -> Arc<SocketEntry> {{ get_entry({raw}).unwrap() }}
'''
    parts.append(f'mod {label} {{\n'+source+wrappers+'}\n')
parts.append('''
fn measure(threads: usize, lookup: fn() -> Arc<SocketEntry>) -> u128 {
    let start=std::time::Instant::now();
    std::thread::scope(|scope| { for _ in 0..threads { scope.spawn(move || {
        for _ in 0..(1_000_000 / threads) { let entry=lookup(); std::hint::black_box(&entry); }
    }); }});
    start.elapsed().as_micros()
}
fn main() {
    let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap(); listener.set_nonblocking(true).unwrap();
    let entry=Arc::new(SocketEntry { socket:Socket::Listener(Arc::new(mio::net::TcpListener::from_std(listener))), opts:Mutex::new(SocketOpts::default()) });
    before::install(entry.clone()); after::install(entry);
    for threads in [1,2,4,8] {
        measure(threads,before::lookup); measure(threads,after::lookup);
        for pair in 0..9 {
            let order: [(&str, fn()->Arc<SocketEntry>);2]=if pair%2==0 { [("before",before::lookup),("after",after::lookup)] } else { [("after",after::lookup),("before",before::lookup)] };
            for (name, lookup) in order { println!("{name} {threads} {pair} {}",measure(threads,lookup)); }
        }
    }
}
''')
probe=pathlib.Path('/tmp/nera-socket-control.rs');probe.write_text(''.join(parts));target=pathlib.Path('/tmp/nera-transfer-target/release')
args=['rustc','--edition=2021','-O','-C','codegen-units=1','-A','dead_code','-A','unused_imports',str(probe),'-o','/tmp/nera-socket-control','-L',f'dependency={target}/deps','--extern',f'nera_runtime={target}/libnera_runtime.rlib']
for libs in itertools.product(*[list((target/'deps').glob(f'lib{c}-*.rlib')) for c in ['parking_lot','mio']]):
    extra=sum((['--extern',f'{c}={lib}'] for c,lib in zip(['parking_lot','mio'],libs)),[])
    r=subprocess.run(args+extra,cwd=root,capture_output=True,text=True)
    if r.returncode==0: break
else: raise RuntimeError(r.stderr)
