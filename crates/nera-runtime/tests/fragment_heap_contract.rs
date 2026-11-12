//! Compile the real private builder API without exposing it to library clients.
use std::process::Command;

#[test]
fn fragment_heap_rejects_collection_mutable_access_and_general_heap_conversion() {
    let source = include_str!("../src/heap.rs");
    let start = source
        .find("/// Allocation-only storage for message fragments")
        .unwrap();
    let end = source[start..]
        .find("impl Default for ProcessHeap")
        .unwrap()
        + start;
    let builder = &source[start..end];
    let dir = std::env::temp_dir().join(format!("nera-fragment-contract-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("contract.rs");
    let prelude = r#"
mod term {
    pub struct ErlTerm;
    pub struct BorrowedTerm<'a>(std::marker::PhantomData<&'a ()>);
    impl BorrowedTerm<'_> {
        pub fn copy_to_heap(self, _: &mut crate::heap::ProcessHeap) -> ErlTerm { ErlTerm }
    }
}
mod heap {
    pub struct ProcessHeap;
    impl ProcessHeap {
        pub fn new() -> Self { Self }
        pub fn new_with_capacity(_: usize) -> Self { Self }
        pub fn alloc_words(&mut self, _: usize) -> *mut u64 { std::ptr::null_mut() }
        pub fn alloc_tuple(&mut self, _: &[crate::term::ErlTerm]) -> crate::term::ErlTerm { crate::term::ErlTerm }
        pub fn alloc_binary_auto(&mut self, _: &[u8]) -> crate::term::ErlTerm { crate::term::ErlTerm }
    }
}
mod message {
    pub unsafe fn deep_copy_term(_: crate::term::ErlTerm, _: &mut crate::heap::ProcessHeap) -> crate::term::ErlTerm { crate::term::ErlTerm }
}
mod fragment {
    use crate::{heap::ProcessHeap, term::ErlTerm};
"#;
    let compile = |body: &str| {
        std::fs::write(&file, format!("{prelude}{builder}\n}}\n{body}")).unwrap();
        Command::new("rustc")
            .args(["--edition=2021", "--crate-type=lib", "--emit=metadata"])
            .arg(&file)
            .arg("-o")
            .arg(dir.join("contract.rmeta"))
            .output()
            .unwrap()
    };
    let valid = compile(
        "fn allocate() { fragment::FragmentHeap::new_with_capacity(64).alloc_tuple(&[]); }",
    );
    assert!(
        valid.status.success(),
        "{}",
        String::from_utf8_lossy(&valid.stderr)
    );
    for (body, error) in [
        ("fn collect() { fragment::FragmentHeap::new_with_capacity(64).collect_full(&mut []); }", "E0599"),
        ("fn mutable_access() { let mut heap = fragment::FragmentHeap::new_with_capacity(64); let _ = &mut heap.heap; }", "E0616"),
        ("fn convert() { let _: fragment::FragmentHeap = heap::ProcessHeap::new().into(); }", "E0277"),
    ] {
        let rejected = compile(body);
        let errors = String::from_utf8_lossy(&rejected.stderr);
        assert!(!rejected.status.success(), "fragment contract was bypassed");
        assert!(errors.contains(error), "unexpected failure: {errors}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}
