//! Project EUnit execution, using the same sources and dependencies as `build`.
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Framework {
    Eunit,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TestConfig {
    pub(crate) framework: Framework,
    pub(crate) modules: Vec<String>,
    /// Additional test source directories, relative to the project root.
    #[serde(default)]
    pub(crate) src: Vec<String>,
}

const MARKER: &str = "NERA_EUNIT_OK";

fn selected_modules(config: &TestConfig, patterns: &[String]) -> Result<Vec<String>, String> {
    if config.modules.is_empty() {
        return Err("[test] modules must not be empty".into());
    }
    let mut modules = config.modules.clone();
    for module in &modules {
        // Generate only unquoted Erlang atoms; never interpolate arbitrary code.
        if !module
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_lowercase)
            || !module
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(format!("invalid EUnit module name: {module}"));
        }
    }
    modules.sort();
    modules.dedup();
    if !patterns.is_empty() {
        modules.retain(|name| {
            patterns
                .iter()
                .any(|p| crate::test_runner::glob_match(p, name))
        });
        if modules.is_empty() {
            return Err("no configured EUnit modules match the requested patterns".into());
        }
    }
    Ok(modules)
}

fn driver(modules: &[String], verbose: bool) -> String {
    let options = if verbose { "[verbose]" } else { "[]" };
    format!("-module(nera_eunit_driver).\n-export([start/0]).\nstart() ->\n  ok = eunit:test([{}], {}),\n  eunit:stop(),\n  io:format(\"{}~n\").\n", modules.join(","), options, MARKER)
}

fn completed(code: Option<std::process::ExitStatus>, out: &str, err: &str) -> bool {
    code.is_some_and(|s| s.success())
        && out.lines().filter(|line| *line == MARKER).count() == 1
        && !err.contains("Error in process")
}

fn capture(
    command: &mut Command,
    timeout: u64,
) -> Result<(Option<std::process::ExitStatus>, String, String), std::io::Error> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let read = |mut stream: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stream.read_to_end(&mut bytes);
            String::from_utf8_lossy(&bytes).into_owned()
        })
    };
    let out = read(Box::new(stdout));
    let err = read(Box::new(stderr));
    let status = crate::test_runner::wait_with_timeout(&mut child, Duration::from_secs(timeout));
    Ok((
        status,
        out.join().unwrap_or_default(),
        err.join().unwrap_or_default(),
    ))
}

pub(crate) fn run(
    config: &TestConfig,
    root: &Path,
    opts: &crate::test_runner::TestOpts,
) -> Result<(), Box<dyn std::error::Error>> {
    match config.framework {
        Framework::Eunit => {}
    }
    let modules = selected_modules(config, &opts.patterns)?;
    if opts.list {
        println!("EUnit modules ({}):", modules.len());
        for module in modules {
            println!("  {module}");
        }
        return Ok(());
    }
    // Resolve production options before adding our generated entry source.
    // The build subprocess has an isolated working directory for object files.
    let build = crate::BuildOpts::resolve(
        vec![],
        vec![],
        vec![],
        vec![],
        None,
        None,
        false,
        false,
        opts.no_stdlib,
        opts.no_warnings,
        None,
        None,
        opts.verbose,
        Some(root),
    )?;
    let temp = tempfile::tempdir()?;
    let source = temp.path().join("nera_eunit_driver.erl");
    std::fs::write(&source, driver(&modules, opts.verbose))?;
    let binary = temp
        .path()
        .join(if cfg!(windows) { "eunit.exe" } else { "eunit" });
    let mut project = toml::Table::new();
    project.insert("name".into(), "nera_eunit_driver".into());
    project.insert("defer_undefined".into(), build.defer_undefined.into());
    let mut config_copy = toml::Table::new();
    config_copy.insert("project".into(), project.into());
    if let Some(ref otp_root) = build.otp_root {
        let mut otp = toml::Table::new();
        otp.insert(
            "root".into(),
            otp_root.to_string_lossy().into_owned().into(),
        );
        config_copy.insert("otp".into(), otp.into());
    }
    std::fs::write(
        temp.path().join("nera.toml"),
        toml::to_string(&config_copy)?,
    )?;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .current_dir(temp.path())
        .arg("build")
        .arg(&source)
        .arg("-e")
        .arg("nera_eunit_driver:start")
        .arg("-o")
        .arg(&binary);
    for dir in &build.src_dirs {
        command.arg("--src").arg(dir);
    }
    if config.src.is_empty() {
        if root.join("tests").is_dir() {
            command.arg("--src").arg(root.join("tests"));
        }
    } else {
        for dir in &config.src {
            command.arg("--src").arg(root.join(dir));
        }
    }
    for path in &build.include_paths {
        command.arg("-I").arg(path);
    }
    for path in &build.deps_dirs {
        command.arg("--deps-dir").arg(path);
    }
    if opts.no_stdlib {
        command.arg("--no-stdlib");
    }
    if opts.no_warnings {
        command.arg("--no-warnings");
    }
    let output = command.output()?;
    if !output.status.success() {
        eprint!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return Err("EUnit compilation failed (resolve dependencies with `nera deps get`)".into());
    }
    if opts.verbose {
        print!("{}", String::from_utf8_lossy(&output.stdout));
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }
    println!("Running EUnit: {}", modules.join(", "));
    let (status, out, err) = capture(Command::new(&binary).current_dir(root), opts.timeout)?;
    print!("{out}");
    eprint!("{err}");
    if !completed(status, &out, &err) {
        return Err(if status.is_none() {
            format!("EUnit timed out after {}s", opts.timeout)
        } else {
            "EUnit failed or did not complete".into()
        }
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_validates_atoms_filters_and_deduplicates() {
        let mut config = TestConfig {
            framework: Framework::Eunit,
            modules: vec![
                "beta_tests".into(),
                "alpha_tests".into(),
                "alpha_tests".into(),
            ],
            src: vec![],
        };
        assert_eq!(
            selected_modules(&config, &["alpha*".into()]).unwrap(),
            vec!["alpha_tests"]
        );
        assert!(selected_modules(&config, &["missing".into()]).is_err());
        config.modules = vec!["x),halt().".into()];
        assert!(selected_modules(&config, &[]).is_err());
        config.modules.clear();
        assert!(selected_modules(&config, &[]).is_err());
    }
    #[test]
    fn driver_requires_framework_success_before_completion() {
        let source = driver(&["alpha_tests".into()], true);
        assert!(source.contains("ok = eunit:test([alpha_tests], [verbose])"));
        assert!(source.find("eunit:stop()").unwrap() < source.find(MARKER).unwrap());
    }
}
