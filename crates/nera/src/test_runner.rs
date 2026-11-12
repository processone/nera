//! Built-in test runner for nera end-to-end tests.
//!
//! Discovers `.erl` test files in the `tests/` directory, compiles and
//! executes each one in parallel, and reports results with colored output.

use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

// ── Public interface ─────────────────────────────────────────────────

pub struct TestOpts {
    pub patterns: Vec<String>,
    pub list: bool,
    #[allow(dead_code)]
    pub verbose: bool,
    pub no_stdlib: bool,
    pub no_warnings: bool,
    pub timeout: u64,
    pub jobs: usize,
    /// Run only part `K` of `N` (1-based), see [`parse_shard`].
    pub shard: Option<(usize, usize)>,
}

/// Parse a `K/N` shard selector with `1 <= K <= N`.
pub fn parse_shard(text: &str) -> Result<(usize, usize), String> {
    let bad = || format!("invalid shard '{}': expected K/N with 1 <= K <= N", text);
    let (k, n) = text.split_once('/').ok_or_else(bad)?;
    let k: usize = k.trim().parse().map_err(|_| bad())?;
    let n: usize = n.trim().parse().map_err(|_| bad())?;
    if k == 0 || k > n {
        return Err(bad());
    }
    Ok((k, n))
}

/// Keep every `n`th item starting at the `k`th (1-based). Items are dealt
/// round-robin so that each lands in exactly one part and neighbouring (often
/// similarly slow) tests spread across parts.
fn select_shard<T>(items: Vec<T>, (k, n): (usize, usize)) -> Vec<T> {
    items.into_iter().enumerate().filter(|(i, _)| i % n == k - 1).map(|(_, t)| t).collect()
}

/// Run the test suite. Returns `Ok(())` on success, `Err("")` if any test failed.
pub fn run_tests(opts: &TestOpts) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(path) = crate::find_project_toml(None) {
        let config = crate::load_project_toml(&path)?;
        if let Some(test) = config.test {
            return crate::eunit_runner::run(&test, path.parent().unwrap(), opts);
        }
    }
    let c = Colors::detect();
    let test_dir = find_test_dir()?;
    let nera_bin = std::env::current_exe()
        .map_err(|e| format!("cannot determine nera binary path: {}", e))?;

    // Resolve project include paths so that -include() in test modules works.
    let include_paths = resolve_test_include_paths(&test_dir);

    let mut tests = discover_tests(&test_dir)?;

    // Filter by patterns
    if !opts.patterns.is_empty() {
        tests.retain(|t| opts.patterns.iter().any(|p| glob_match(p, &t.name)));
    }

    if tests.is_empty() {
        println!("No tests found.");
        return Ok(());
    }

    tests.sort_by(|a, b| a.name.cmp(&b.name));
    if let Some(shard) = opts.shard {
        tests = select_shard(tests, shard);
        if tests.is_empty() {
            println!("No tests in shard {}/{}.", shard.0, shard.1);
            return Ok(());
        }
    }

    // --list mode
    if opts.list {
        println!("Available tests ({}):", tests.len());
        for t in &tests {
            if let Some(ref reason) = t.skip_reason {
                println!("  {:<40} {}(SKIP: {}){}", t.name, c.dim, reason, c.reset);
            } else {
                println!("  {}", t.name);
            }
        }
        return Ok(());
    }

    let total = tests.len();
    let num_threads = opts.jobs.min(total).max(1);

    let tmp_dir = std::env::temp_dir().join(format!("nera_tests_{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir)?;

    println!(
        "{}Running {} tests ({} threads){}",
        c.bold, total, num_threads, c.reset
    );
    println!("{}", "─".repeat(60));

    // Parallel execution via thread pool + channel
    let (tx, rx) = mpsc::channel::<TestResult>();
    let work = Arc::new(Mutex::new(tests.into_iter()));

    let include_paths = Arc::new(include_paths);
    let handles: Vec<_> = (0..num_threads)
        .map(|_| {
            let work = Arc::clone(&work);
            let tx = tx.clone();
            let nera_bin = nera_bin.clone();
            let tmp_dir = tmp_dir.clone();
            let include_paths = Arc::clone(&include_paths);
            let no_stdlib = opts.no_stdlib;
            let no_warnings = opts.no_warnings;
            let timeout = opts.timeout;

            std::thread::spawn(move || loop {
                let test = { work.lock().unwrap().next() };
                let Some(test) = test else { break };
                let result = run_single_test(
                    &test, &nera_bin, &tmp_dir, &include_paths, no_stdlib, no_warnings, timeout,
                );
                if tx.send(result).is_err() {
                    break;
                }
            })
        })
        .collect();
    drop(tx);

    // Collect and print results as they stream in
    let mut passed = 0u32;
    let mut failed = 0u32;
    let mut compile_failed = 0u32;
    let mut skipped = 0u32;
    let mut count = 0u32;
    let mut failures: Vec<(String, String, Option<String>)> = Vec::new();

    for result in rx {
        count += 1;
        let counter = format!("[{}/{}]", count, total);

        match &result.outcome {
            TestOutcome::Pass => {
                passed += 1;
                println!(
                    "  {}PASS{}  {:<40} {}{:>6}  {}{}",
                    c.green, c.reset, result.name,
                    c.dim, format_duration(result.duration), counter, c.reset,
                );
            }
            TestOutcome::Fail { reason, detail } => {
                failed += 1;
                if reason.contains("compile") {
                    compile_failed += 1;
                }
                failures.push((result.name.clone(), reason.clone(), detail.clone()));
                println!(
                    "  {}FAIL{}  {:<40} {}{:>6}  {}{}",
                    c.red, c.reset, result.name,
                    c.dim, format_duration(result.duration), counter, c.reset,
                );
            }
            TestOutcome::Skip { reason } => {
                skipped += 1;
                println!(
                    "  {}SKIP{}  {:<40} {}({}){}",
                    c.yellow, c.reset, result.name, c.dim, reason, c.reset,
                );
            }
        }
    }

    for h in handles {
        let _ = h.join();
    }

    // Summary
    println!("{}", "─".repeat(60));
    println!(
        "{}Results:{} {} passed, {} failed ({} compile), {} skipped / {} total",
        c.bold, c.reset, passed, failed, compile_failed, skipped, total,
    );

    // Failure details
    if !failures.is_empty() {
        println!();
        println!("{}Failures:{}", c.bold, c.reset);
        for (name, reason, detail) in &failures {
            println!("  {}{}{}: {}", c.red, name, c.reset, reason);
            if let Some(d) = detail {
                for line in d.lines().take(5) {
                    println!("    {}", line);
                }
            }
        }
    }

    println!();
    let _ = std::fs::remove_dir_all(&tmp_dir);

    if failed > 0 {
        println!("{}{}SOME TESTS FAILED{}", c.red, c.bold, c.reset);
        return Err("".into());
    }

    println!("{}{}ALL TESTS PASSED{}", c.green, c.bold, c.reset);
    Ok(())
}

// ── Internals ────────────────────────────────────────────────────────

struct TestDescriptor {
    name: String,
    path: PathBuf,
    skip_reason: Option<String>,
    expected_output: Option<String>,
    /// If set, the test is expected to fail compilation and stderr must
    /// contain this substring.
    expected_compile_error: Option<String>,
    extra_modules: Vec<PathBuf>,
}

enum TestOutcome {
    Pass,
    Fail {
        reason: String,
        detail: Option<String>,
    },
    Skip {
        reason: String,
    },
}

struct TestResult {
    name: String,
    outcome: TestOutcome,
    duration: Duration,
}

struct Colors {
    red: &'static str,
    green: &'static str,
    yellow: &'static str,
    bold: &'static str,
    dim: &'static str,
    reset: &'static str,
}

impl Colors {
    fn detect() -> Self {
        if std::io::stdout().is_terminal() {
            Colors {
                red: "\x1b[31m",
                green: "\x1b[32m",
                yellow: "\x1b[33m",
                bold: "\x1b[1m",
                dim: "\x1b[2m",
                reset: "\x1b[0m",
            }
        } else {
            Colors {
                red: "",
                green: "",
                yellow: "",
                bold: "",
                dim: "",
                reset: "",
            }
        }
    }
}

/// Locate the `tests/` directory, searching CWD then walking up to the project root.
fn find_test_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let tests = cwd.join("tests");
    if tests.is_dir() {
        return Ok(tests);
    }
    // Walk up to find nera.toml project root
    let mut dir = cwd;
    loop {
        if dir.join("nera.toml").is_file() {
            let t = dir.join("tests");
            if t.is_dir() {
                return Ok(t);
            }
            break;
        }
        if !dir.pop() {
            break;
        }
    }
    Err("no tests/ directory found (run from project root)".into())
}

/// Resolve include paths for compiling test files.
///
/// The project root is the parent of `test_dir` (i.e. `tests/`'s parent).
/// We add the same conventional directories that `BuildOpts::resolve` uses:
/// `include/`, `src/`, and `tests/` itself. If a `nera.toml` exists we also
/// read its `[project] include` list.
fn resolve_test_include_paths(test_dir: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let project_root = match test_dir.parent() {
        Some(p) => p,
        None => return paths,
    };

    // Read nera.toml include dirs if present
    let toml_path = project_root.join("nera.toml");
    let mut found_toml_includes = false;
    if toml_path.is_file() {
        if let Ok(toml) = crate::load_project_toml(&toml_path) {
            if let Some(ref include_dirs) = toml.project.include {
                for dir in include_dirs {
                    let abs = project_root.join(dir);
                    if abs.is_dir() {
                        paths.push(abs);
                        found_toml_includes = true;
                    }
                }
            }
        }
    }

    // Convention: add include/ if nera.toml didn't specify includes
    if !found_toml_includes {
        let inc = project_root.join("include");
        if inc.is_dir() {
            paths.push(inc);
        }
    }

    // Add src/ and tests/ directories
    let src = project_root.join("src");
    if src.is_dir() {
        paths.push(src);
    }
    if test_dir.is_dir() && !paths.contains(&test_dir.to_path_buf()) {
        paths.push(test_dir.to_path_buf());
    }

    paths
}

/// Discover test files and parse their metadata comments.
fn discover_tests(test_dir: &Path) -> Result<Vec<TestDescriptor>, Box<dyn std::error::Error>> {
    let mut tests = Vec::new();

    let mut entries: Vec<_> = std::fs::read_dir(test_dir)?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("erl") {
            continue;
        }
        let name = path
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();
        // Skip helper modules (prefixed with _)
        if name.starts_with('_') {
            continue;
        }

        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;

        let mut skip_reason = None;
        let mut expected_output = None;
        let mut expected_compile_error = None;
        let mut extra_modules = Vec::new();

        for (i, line) in content.lines().enumerate() {
            // SKIP marker: first line only (matches shell script `head -1 | grep`)
            if i == 0 && line.contains("%% SKIP:") {
                skip_reason = Some(
                    line.split("%% SKIP:")
                        .nth(1)
                        .unwrap_or("")
                        .trim()
                        .to_string(),
                );
            }
            // EXPECT and MODULES can appear on any line
            if line.contains("%% EXPECT:") && expected_output.is_none() {
                expected_output = Some(
                    line.split("%% EXPECT:")
                        .nth(1)
                        .unwrap_or("")
                        .trim()
                        .to_string(),
                );
            }
            // EXPECT_COMPILE_ERROR: test expects compilation to fail with this substring in stderr
            if line.contains("%% EXPECT_COMPILE_ERROR:") && expected_compile_error.is_none() {
                expected_compile_error = Some(
                    line.split("%% EXPECT_COMPILE_ERROR:")
                        .nth(1)
                        .unwrap_or("")
                        .trim()
                        .to_string(),
                );
            }
            if line.contains("%% MODULES:") {
                let modules_str = line.split("%% MODULES:").nth(1).unwrap_or("").trim();
                for m in modules_str.split_whitespace() {
                    extra_modules.push(test_dir.join(m));
                }
            }
        }

        tests.push(TestDescriptor {
            name,
            path,
            skip_reason,
            expected_output,
            expected_compile_error,
            extra_modules,
        });
    }

    Ok(tests)
}

/// Simple glob matching supporting `*` wildcards.
pub(crate) fn glob_match(pattern: &str, name: &str) -> bool {
    if !pattern.contains('*') {
        return name == pattern;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut pos = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            if !name.starts_with(part) {
                return false;
            }
            pos = part.len();
        } else if i == parts.len() - 1 {
            if !name[pos..].ends_with(part) {
                return false;
            }
            return true;
        } else {
            match name[pos..].find(part) {
                Some(idx) => pos += idx + part.len(),
                None => return false,
            }
        }
    }
    true
}

/// Compile and execute a single test, returning the result.
fn run_single_test(
    test: &TestDescriptor,
    nera_bin: &Path,
    tmp_dir: &Path,
    include_paths: &[PathBuf],
    no_stdlib: bool,
    no_warnings: bool,
    timeout_secs: u64,
) -> TestResult {
    let start = Instant::now();

    // Handle skip
    if let Some(ref reason) = test.skip_reason {
        return TestResult {
            name: test.name.clone(),
            outcome: TestOutcome::Skip {
                reason: reason.clone(),
            },
            duration: start.elapsed(),
        };
    }

    // Per-test temp subdirectory (avoids .o file collisions between parallel builds)
    let test_tmp = tmp_dir.join(&test.name);
    if std::fs::create_dir_all(&test_tmp).is_err() {
        return TestResult {
            name: test.name.clone(),
            outcome: TestOutcome::Fail {
                reason: "cannot create temp dir".to_string(),
                detail: None,
            },
            duration: start.elapsed(),
        };
    }

    let binary = test_tmp.join(&test.name);
    let entry = format!("{}:test", test.name);

    // Make source paths absolute (CWD will be the per-test tmp dir)
    let abs_test_path = std::fs::canonicalize(&test.path)
        .unwrap_or_else(|_| test.path.clone());
    let abs_extra: Vec<PathBuf> = test
        .extra_modules
        .iter()
        .map(|m| std::fs::canonicalize(m).unwrap_or_else(|_| m.clone()))
        .collect();

    // ── Compile ──────────────────────────────────────────────────
    let mut cmd = std::process::Command::new(nera_bin);
    cmd.current_dir(&test_tmp);
    cmd.arg("build");
    cmd.arg(&abs_test_path);
    for m in &abs_extra {
        cmd.arg(m);
    }
    cmd.arg("-o");
    cmd.arg(&binary);
    cmd.arg("-e");
    cmd.arg(&entry);
    for inc in include_paths {
        cmd.arg("-I");
        cmd.arg(inc);
    }
    if no_stdlib {
        cmd.arg("--no-stdlib");
    }
    if no_warnings {
        cmd.arg("--no-warnings");
    }
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    match cmd.output() {
        Err(e) => {
            return TestResult {
                name: test.name.clone(),
                outcome: TestOutcome::Fail {
                    reason: format!("compile spawn error: {}", e),
                    detail: None,
                },
                duration: start.elapsed(),
            };
        }
        Ok(output) if !output.status.success() => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let detail = if !stderr.is_empty() {
                stderr.to_string()
            } else {
                stdout.to_string()
            };

            // If the test expects a compile error, check that the expected
            // substring appears in the compiler output.
            if let Some(ref expected) = test.expected_compile_error {
                if detail.contains(expected.as_str()) {
                    return TestResult {
                        name: test.name.clone(),
                        outcome: TestOutcome::Pass,
                        duration: start.elapsed(),
                    };
                } else {
                    return TestResult {
                        name: test.name.clone(),
                        outcome: TestOutcome::Fail {
                            reason: "compile error did not match expected".to_string(),
                            detail: Some(format!(
                                "expected stderr to contain: {}\n     got: {}",
                                expected,
                                detail.lines().take(5).collect::<Vec<_>>().join("\n"),
                            )),
                        },
                        duration: start.elapsed(),
                    };
                }
            }

            return TestResult {
                name: test.name.clone(),
                outcome: TestOutcome::Fail {
                    reason: "compile error".to_string(),
                    detail: if detail.trim().is_empty() {
                        None
                    } else {
                        Some(detail)
                    },
                },
                duration: start.elapsed(),
            };
        }
        Ok(_) => {
            // If the test expected a compile error but compilation succeeded, fail.
            if test.expected_compile_error.is_some() {
                return TestResult {
                    name: test.name.clone(),
                    outcome: TestOutcome::Fail {
                        reason: "expected compile error, but compilation succeeded".to_string(),
                        detail: None,
                    },
                    duration: start.elapsed(),
                };
            }
        }
    }

    // ── Execute ──────────────────────────────────────────────────
    let mut child = match std::process::Command::new(&binary)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return TestResult {
                name: test.name.clone(),
                outcome: TestOutcome::Fail {
                    reason: format!("cannot execute: {}", e),
                    detail: None,
                },
                duration: start.elapsed(),
            };
        }
    };

    // Read stdout/stderr in separate threads to avoid pipe buffer deadlock
    let child_stdout = child.stdout.take();
    let child_stderr = child.stderr.take();

    let stdout_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut r) = child_stdout {
            let _ = r.read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).to_string()
    });
    let stderr_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut r) = child_stderr {
            let _ = r.read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).to_string()
    });

    let timeout = Duration::from_secs(timeout_secs);
    let status = wait_with_timeout(&mut child, timeout);

    let stdout_str = stdout_thread.join().unwrap_or_default();
    let stderr_str = stderr_thread.join().unwrap_or_default();

    match status {
        None => TestResult {
            name: test.name.clone(),
            outcome: TestOutcome::Fail {
                reason: format!("timeout after {}s", timeout_secs),
                detail: captured_output(&stdout_str, &stderr_str),
            },
            duration: start.elapsed(),
        },
        Some(exit) if !exit.success() => {
            let exit_code = exit.code().unwrap_or(-1);
            TestResult {
                name: test.name.clone(),
                outcome: TestOutcome::Fail {
                    reason: format!("runtime crash, exit={}", exit_code),
                    detail: captured_output(&stdout_str, &stderr_str),
                },
                duration: start.elapsed(),
            }
        }
        Some(_) => {
            // Success — check expected output if specified
            if let Some(ref expected) = test.expected_output {
                if !stdout_str.contains(expected.as_str()) {
                    return TestResult {
                        name: test.name.clone(),
                        outcome: TestOutcome::Fail {
                            reason: "output mismatch".to_string(),
                            detail: Some(format!(
                                "expected: {}\n     got: {}",
                                expected,
                                stdout_str.lines().next().unwrap_or("")
                            )),
                        },
                        duration: start.elapsed(),
                    };
                }
            }
            TestResult {
                name: test.name.clone(),
                outcome: TestOutcome::Pass,
                duration: start.elapsed(),
            }
        }
    }
}

/// Keep both diagnostic streams, including partial output from a killed test.
fn captured_output(stdout: &str, stderr: &str) -> Option<String> {
    let mut sections = Vec::new();
    if !stdout.is_empty() {
        sections.push(format!("stdout:\n{stdout}"));
    }
    if !stderr.is_empty() {
        sections.push(format!("stderr:\n{stderr}"));
    }
    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n"))
    }
}

/// Poll-based wait with timeout. Returns `None` on timeout (child is killed).
pub(crate) fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Option<std::process::ExitStatus> {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return None,
        }
    }
}

fn format_duration(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 1000 {
        format!("{}ms", ms)
    } else {
        format!("{:.1}s", d.as_secs_f64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn shards_cover_every_test_exactly_once() {
        // CI splits `nera test` into parts; a test must never be dropped or doubled.
        let names: Vec<String> = (0..466).map(|i| format!("t{i}")).collect();
        for n in 1..=7 {
            let mut seen: Vec<String> = Vec::new();
            for k in 1..=n {
                seen.extend(select_shard(names.clone(), (k, n)));
            }
            seen.sort();
            let mut want = names.clone();
            want.sort();
            assert_eq!(seen, want, "n={n}");
        }
    }

    #[test]
    fn shard_selector_parses_only_valid_parts() {
        assert_eq!(parse_shard("2/3"), Ok((2, 3)));
        for bad in ["0/3", "4/3", "1", "a/b", "1/0", "/", "1/2/3"] {
            assert!(parse_shard(bad).is_err(), "{bad}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn timeout_preserves_stdout_and_stderr() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let compiler = tmp.path().join("compiler");
        // Exercise the real compile/execute/kill/report path with a tiny fixture.
        fs::write(&compiler, r#"#!/bin/sh
while [ "$1" != "-o" ]; do shift; done
shift
cat > "$1" <<'PROGRAM'
#!/bin/sh
echo 'TCP phase: waiting for close'
echo 'reactor diagnostic: pending read' >&2
exec sleep 30
PROGRAM
chmod +x "$1"
"#).unwrap();
        fs::set_permissions(&compiler, fs::Permissions::from_mode(0o755)).unwrap();
        let source = tmp.path().join("timeout_probe.erl");
        fs::write(&source, "").unwrap();
        let test = TestDescriptor {
            name: "timeout_probe".into(),
            path: source,
            skip_reason: None,
            expected_output: None,
            expected_compile_error: None,
            extra_modules: vec![],
        };
        let result = run_single_test(&test, &compiler, tmp.path(), &[], false, false, 1);
        match result.outcome {
            TestOutcome::Fail { reason, detail } => {
                assert_eq!(reason, "timeout after 1s");
                let detail = detail.expect("timeout must retain diagnostics");
                assert!(detail.contains("TCP phase: waiting for close"), "{detail}");
                assert!(detail.contains("reactor diagnostic: pending read"), "{detail}");
            }
            _ => panic!("fixture must time out"),
        }
    }

    /// Helper: create a temp project layout with an optional nera.toml.
    fn setup_project(dir: &std::path::Path, toml_content: Option<&str>, subdirs: &[&str]) {
        fs::create_dir_all(dir).unwrap();
        if let Some(content) = toml_content {
            fs::write(dir.join("nera.toml"), content).unwrap();
        }
        for sub in subdirs {
            fs::create_dir_all(dir.join(sub)).unwrap();
        }
    }

    // ── resolve_test_include_paths tests ────────────────────────

    #[test]
    fn test_include_paths_convention_discovers_include_src_tests() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        setup_project(&root, None, &["src", "include", "tests"]);

        let paths = resolve_test_include_paths(&root.join("tests"));
        assert!(paths.contains(&root.join("include")),
            "should discover include/ by convention, got: {:?}", paths);
        assert!(paths.contains(&root.join("src")),
            "should discover src/, got: {:?}", paths);
        assert!(paths.contains(&root.join("tests")),
            "should include tests/ itself, got: {:?}", paths);
    }

    #[test]
    fn test_include_paths_convention_no_include_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        setup_project(&root, None, &["src", "tests"]);

        let paths = resolve_test_include_paths(&root.join("tests"));
        assert!(!paths.iter().any(|p| p.ends_with("include")),
            "should not invent include/ when it doesn't exist, got: {:?}", paths);
        assert!(paths.contains(&root.join("src")));
        assert!(paths.contains(&root.join("tests")));
    }

    #[test]
    fn test_include_paths_toml_overrides_convention() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        let toml = r#"
[project]
name = "myapp"
src = ["src"]
include = ["headers", "proto/include"]
"#;
        setup_project(&root, Some(toml), &["src", "include", "headers", "proto/include", "tests"]);

        let paths = resolve_test_include_paths(&root.join("tests"));
        // nera.toml-specified dirs should be present
        assert!(paths.contains(&root.join("headers")),
            "should use nera.toml include dirs, got: {:?}", paths);
        assert!(paths.contains(&root.join("proto/include")),
            "should use nera.toml include dirs, got: {:?}", paths);
        // Convention include/ should NOT be present when toml specifies includes
        assert!(!paths.contains(&root.join("include")),
            "should not fall back to convention include/ when toml specifies include, got: {:?}", paths);
        // src/ and tests/ should always be present
        assert!(paths.contains(&root.join("src")));
        assert!(paths.contains(&root.join("tests")));
    }

    #[test]
    fn test_include_paths_toml_without_include_field_falls_back() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        let toml = r#"
[project]
name = "myapp"
src = ["src"]
"#;
        setup_project(&root, Some(toml), &["src", "include", "tests"]);

        let paths = resolve_test_include_paths(&root.join("tests"));
        // No include field in toml → falls back to convention include/
        assert!(paths.contains(&root.join("include")),
            "should fall back to convention include/ when toml has no include field, got: {:?}", paths);
        assert!(paths.contains(&root.join("src")));
        assert!(paths.contains(&root.join("tests")));
    }

    #[test]
    fn test_include_paths_toml_skips_nonexistent_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        let toml = r#"
[project]
name = "myapp"
src = ["src"]
include = ["exists", "ghost"]
"#;
        // Only create "exists", not "ghost"
        setup_project(&root, Some(toml), &["src", "exists", "tests"]);

        let paths = resolve_test_include_paths(&root.join("tests"));
        assert!(paths.contains(&root.join("exists")),
            "should include directories that exist, got: {:?}", paths);
        assert!(!paths.iter().any(|p| p.ends_with("ghost")),
            "should skip directories that don't exist, got: {:?}", paths);
    }

    // ── glob_match tests ────────────────────────────────────────

    #[test]
    fn test_glob_match_exact() {
        assert!(glob_match("arith_basic", "arith_basic"));
        assert!(!glob_match("arith_basic", "arith_float"));
    }

    #[test]
    fn test_glob_match_wildcard() {
        assert!(glob_match("arith_*", "arith_basic"));
        assert!(glob_match("arith_*", "arith_float"));
        assert!(!glob_match("arith_*", "guard_basic"));
    }

    #[test]
    fn test_glob_match_prefix_wildcard() {
        assert!(glob_match("*_basic", "arith_basic"));
        assert!(glob_match("*_basic", "guard_basic"));
        assert!(!glob_match("*_basic", "arith_float"));
    }

    #[test]
    fn test_glob_match_middle_wildcard() {
        assert!(glob_match("arith_*_test", "arith_basic_test"));
        assert!(!glob_match("arith_*_test", "arith_basic_check"));
    }

    #[test]
    fn test_glob_match_star_only() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*", ""));
    }
}
