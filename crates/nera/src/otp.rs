//! OTP installation detection and standard library app scanning.
//!
//! Provides utilities to locate the Erlang/OTP installation root and
//! enumerate the standard library applications (kernel, stdlib, etc.)
//! installed within it.  Used by the application registry to resolve
//! `-include_lib("app/path")` directives.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Attempt to locate the Erlang/OTP root directory.
///
/// Tries the following sources in order:
/// 1. Explicit path (from `nera.toml` `[otp] root`)
/// 2. `NERA_OTP_ROOT` environment variable
/// 3. Probing `erl` on PATH via `code:root_dir()`
/// 4. Well-known platform installation paths
///
/// Returns `None` if no OTP installation can be found.
pub fn find_otp_root(explicit: Option<&Path>) -> Option<PathBuf> {
    // 1. Explicit path from config
    if let Some(root) = explicit {
        if root.join("lib").is_dir() {
            return Some(root.to_path_buf());
        }
    }

    // 2. NERA_OTP_ROOT environment variable
    if let Ok(val) = std::env::var("NERA_OTP_ROOT") {
        let p = PathBuf::from(&val);
        if p.join("lib").is_dir() {
            return Some(p);
        }
    }

    // 3. Probe `erl` on PATH
    if let Some(root) = probe_erl_root_dir() {
        if root.join("lib").is_dir() {
            return Some(root);
        }
    }

    // 4. Well-known platform paths
    well_known_otp_paths().into_iter().find(|candidate| candidate.join("lib").is_dir())
}

/// Read the active release from the selected installation, never from another erl on PATH.
pub fn otp_release(root: &Path) -> Option<u32> {
    let data = std::fs::read_to_string(root.join("releases/start_erl.data")).ok()?;
    let release = data.split_whitespace().nth(1)?;
    release.split('.').next()?.parse().ok()
}

/// Scan the OTP `lib/` directory and return a map of app name → app directory.
///
/// Directory names use the OTP convention `appname-version` (e.g. `kernel-9.2`).
/// The version suffix is stripped to produce the app name key.
pub fn scan_otp_apps(otp_root: &Path) -> HashMap<String, PathBuf> {
    let mut apps = HashMap::new();
    let lib_dir = otp_root.join("lib");

    let entries = match std::fs::read_dir(&lib_dir) {
        Ok(e) => e,
        Err(_) => return apps,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let dir_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let app_name = strip_version_suffix(&dir_name);
        apps.insert(app_name, path);
    }

    apps
}

/// Strip the OTP version suffix from a directory name.
///
/// E.g. `"kernel-9.2"` → `"kernel"`, `"stdlib-5.0.1"` → `"stdlib"`.
/// If there is no recognisable version suffix the name is returned as-is.
fn strip_version_suffix(name: &str) -> String {
    // OTP convention: last '-' followed by a digit starts the version.
    if let Some(pos) = name.rfind('-') {
        let after = &name[pos + 1..];
        if after.starts_with(|c: char| c.is_ascii_digit()) {
            return name[..pos].to_string();
        }
    }
    name.to_string()
}

/// Probe the Erlang runtime on PATH for its root directory.
fn probe_erl_root_dir() -> Option<PathBuf> {
    let output = Command::new("erl")
        .args([
            "-noshell",
            "-eval",
            r#"io:format("~s", [code:root_dir()])"#,
            "-s",
            "init",
            "stop",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let root = String::from_utf8(output.stdout).ok()?;
    let root = root.trim();
    if root.is_empty() {
        return None;
    }
    Some(PathBuf::from(root))
}

/// Return well-known OTP installation paths for the current platform.
fn well_known_otp_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // Standard Linux / system installs
    paths.push(PathBuf::from("/usr/local/lib/erlang"));
    paths.push(PathBuf::from("/usr/lib/erlang"));

    // macOS Homebrew (Intel + Apple Silicon)
    for prefix in &["/opt/homebrew/lib/erlang", "/usr/local/Cellar/erlang"] {
        let p = PathBuf::from(prefix);
        if p.is_dir() {
            // Homebrew Cellar has a version subdir; try to find the latest.
            if prefix.contains("Cellar") {
                if let Some(latest) = find_latest_subdir(&p) {
                    let lib_erlang = latest.join("lib/erlang");
                    if lib_erlang.is_dir() {
                        paths.push(lib_erlang);
                    }
                }
            } else {
                paths.push(p);
            }
        }
    }

    // asdf installs
    if let Ok(home) = std::env::var("HOME") {
        let asdf_base = PathBuf::from(&home).join(".asdf/installs/erlang");
        if asdf_base.is_dir() {
            if let Some(latest) = find_latest_subdir(&asdf_base) {
                let lib_erlang = latest.join("lib/erlang");
                if lib_erlang.is_dir() {
                    paths.push(lib_erlang);
                }
            }
        }

        // kerl installs
        let kerl_base = PathBuf::from(&home).join(".kerl/installs");
        if kerl_base.is_dir() {
            if let Some(latest) = find_latest_subdir(&kerl_base) {
                let lib_erlang = latest.join("lib/erlang");
                if lib_erlang.is_dir() {
                    paths.push(lib_erlang);
                }
            }
        }
    }

    paths
}

/// Find the lexicographically latest subdirectory (used for version dirs).
fn find_latest_subdir(dir: &Path) -> Option<PathBuf> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    entries.sort();
    entries.into_iter().next_back()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_comes_from_selected_installation() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(otp_release(root.path()), None);
        std::fs::create_dir(root.path().join("releases")).unwrap();
        let data = root.path().join("releases/start_erl.data");
        for (text, expected) in [("17.0.5 29\n", Some(29)), ("14.2 26.2.1\n", Some(26)), ("bad", None), ("17.0 latest", None)] {
            std::fs::write(&data, text).unwrap();
            assert_eq!(otp_release(root.path()), expected);
        }
    }

    #[test]
    fn test_strip_version_suffix() {
        assert_eq!(strip_version_suffix("kernel-9.2"), "kernel");
        assert_eq!(strip_version_suffix("stdlib-5.0.1"), "stdlib");
        assert_eq!(strip_version_suffix("crypto-5.4"), "crypto");
        assert_eq!(strip_version_suffix("myapp"), "myapp");
        assert_eq!(strip_version_suffix("my-app-1.0"), "my-app");
    }
}
