//! Compile the actual private helper against minimal context/scheduler types.
//! This checks its lifetime contract without making an FFI helper public.
use std::process::Command;

#[test]
fn scheduler_reference_cannot_escape_callback() {
    let helpers = include_str!("../src/ffi/helpers.rs");
    let helper = helpers
        .split("\nuse crate::binary::BinaryData;")
        .next()
        .unwrap();
    let directory =
        std::env::temp_dir().join(format!("nera-scheduler-scope-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let source = directory.join("scope.rs");
    let prelude = r#"
mod scheduler {
    pub struct SchedulerHandle;
    pub fn global_scheduler() -> Option<&'static std::sync::Arc<SchedulerHandle>> { None }
}
mod process {
    pub struct ProcessContext { pub runtime: *const crate::scheduler::SchedulerHandle }
}
mod helpers {
"#;
    let compile = |body: &str| {
        std::fs::write(&source, format!("{prelude}{helper}\n}}\n{body}")).unwrap();
        Command::new("rustc")
            .args(["--edition=2021", "--crate-type=lib", "--emit=metadata"])
            .arg(&source)
            .arg("-o")
            .arg(directory.join("scope.rmeta"))
            .output()
            .unwrap()
    };
    // First prove the harness accepts an owned result from the same helper.
    let valid = compile(
        r#"
unsafe fn owned(ctx: *mut process::ProcessContext) -> bool {
    unsafe { helpers::with_sched_from_ctx(ctx, |scheduler| scheduler.is_some()) }
}
"#,
    );
    assert!(
        valid.status.success(),
        "{}",
        String::from_utf8_lossy(&valid.stderr)
    );
    for body in [
        r#"
unsafe fn escape(ctx: *mut process::ProcessContext) -> Option<&'static scheduler::SchedulerHandle> {
    unsafe { helpers::with_sched_from_ctx(ctx, |scheduler| scheduler) }
}
"#,
        r#"
unsafe fn stash(ctx: *mut process::ProcessContext) {
    let mut saved = None;
    unsafe { helpers::with_sched_from_ctx(ctx, |scheduler| { saved = scheduler; }); }
    std::hint::black_box(saved);
}
"#,
    ] {
        let rejected = compile(body);
        let errors = String::from_utf8_lossy(&rejected.stderr);
        assert!(!rejected.status.success(), "scheduler borrow escaped");
        assert!(
            errors.contains("lifetime may not live long enough")
                || errors.contains("borrowed data escapes outside of closure"),
            "unexpected compilation failure: {errors}"
        );
    }
    std::fs::remove_dir_all(directory).unwrap();
}
