//! Check the real private EXIT adapter's unsafe call boundary independently of
//! its external visibility. Runtime tests own traversal and signal semantics.
use std::process::Command;

#[test]
fn raw_exit_scan_requires_an_unsafe_call() {
    let source = include_str!("../src/process.rs");
    let signature = source.find("fn process_exit_signals").unwrap();
    let start = source[..signature].rfind('\n').unwrap() + 1;
    let end = source[signature..].find('{').unwrap() + signature;
    let declaration = &source[start..end];
    // Only the declaration controls whether a call requires unsafe. Compile it
    // with the real runtime types, without duplicating private mailbox state or
    // substituting fake term/heap operations for the implementation.
    let adapter = format!("{declaration}{{ unreachable!() }}");
    let dependencies = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let mut dependency_dirs = vec![dependencies.clone()];
    if let Some(package_build_dir) = dependencies.parent().and_then(|dir| dir.parent()) {
        if package_build_dir
            .file_name()
            .is_some_and(|name| name == "nera-runtime")
        {
            if let Some(build_dir) = package_build_dir.parent() {
                for package in std::fs::read_dir(build_dir).unwrap().flatten() {
                    if let Ok(fingerprints) = std::fs::read_dir(package.path()) {
                        dependency_dirs.extend(
                            fingerprints
                                .flatten()
                                .map(|fingerprint| fingerprint.path().join("out"))
                                .filter(|output| output.is_dir()),
                        );
                    }
                }
            }
        }
    }
    let library = dependency_dirs
        .iter()
        .flat_map(|directory| std::fs::read_dir(directory).unwrap())
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "rlib")
                && path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("libnera_runtime-")
        })
        .expect("the runtime library built for this integration test");
    let directory = std::env::temp_dir().join(format!("nera-exit-contract-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let file = directory.join("contract.rs");
    let compile = |body: &str| {
        std::fs::write(
            &file,
            format!("use nera_runtime::{{process::Process, term::ErlTerm}};\n{adapter}\n{body}"),
        )
        .unwrap();
        let mut command = Command::new("rustc");
        command
            .args(["--edition=2021", "--crate-type=lib", "--emit=metadata"])
            .arg("--extern")
            .arg(format!("nera_runtime={}", library.display()));
        for directory in &dependency_dirs {
            command
                .arg("-L")
                .arg(format!("dependency={}", directory.display()));
        }
        command
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
