//! Check the real private EXIT adapter requires an unsafe call inside the crate,
//! independently of its visibility to external library users.
use std::process::Command;

#[test]
fn raw_exit_scan_requires_an_unsafe_call() {
    let source = include_str!("../src/process.rs");
    let signature = source.find("fn process_exit_signals").unwrap();
    let start = source[..signature].rfind('\n').unwrap() + 1;
    let end = source[start..]
        .find("\n// ---------------------------------------------------------------------------")
        .unwrap()
        + start;
    let adapter = &source[start..end];
    let directory = std::env::temp_dir().join(format!("nera-exit-contract-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let file = directory.join("contract.rs");
    let prelude = r#"
mod atom_table {
    pub const ATOM_EXIT_SIGNAL: u32 = 1;
    pub const ATOM_KILL: u32 = 2;
    pub const ATOM_KILLED: u32 = 3;
    pub const ATOM_NORMAL: u32 = 4;
}
mod term {
    pub const BOXED_TUPLE: u64 = 1;
    pub const TOMBSTONE_VALUE: u64 = 7;
    pub struct BoxedHeader(pub u64);
    impl BoxedHeader {
        pub fn subtype(&self) -> u64 { 0 }
        pub fn arity(&self) -> usize { 0 }
    }
}
pub struct ErlTerm(pub u64);
impl ErlTerm {
    fn is_boxed(&self) -> bool { false }
    fn as_boxed_ptr(&self) -> Option<*const u64> { None }
    fn as_atom_id(&self) -> Option<u32> { None }
    fn atom(id: u32) -> Self { Self(id as u64) }
}
struct Mailbox { head: u32, capacity: u32, buf: *mut u64 }
impl Mailbox {
    fn slot_count(&self) -> u32 { 0 }
    unsafe fn remove_scanned_slot(&mut self, _: usize) {}
}
struct Otp { trap_exit: bool }
pub struct Process { mailbox: Mailbox, otp: Otp }
"#;
    let compile = |body: &str| {
        std::fs::write(&file, format!("{prelude}\n{adapter}\n{body}")).unwrap();
        Command::new("rustc")
            .args(["--edition=2021", "--crate-type=lib", "--emit=metadata"])
            .arg(&file)
            .arg("-o")
            .arg(directory.join("contract.rmeta"))
            .output()
            .unwrap()
    };
    let valid = compile("unsafe fn scan(p: &mut Process) { unsafe { process_exit_signals(p); } }");
    assert!(
        valid.status.success(),
        "{}",
        String::from_utf8_lossy(&valid.stderr)
    );
    let rejected = compile("fn scan(p: &mut Process) { process_exit_signals(p); }");
    let errors = String::from_utf8_lossy(&rejected.stderr);
    assert!(
        !rejected.status.success(),
        "safe raw EXIT traversal was accepted"
    );
    assert!(errors.contains("E0133"), "unexpected failure: {errors}");
    std::fs::remove_dir_all(directory).unwrap();
}
