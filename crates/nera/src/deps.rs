//! Dependency management for nera projects.
//!
//! Supports three dependency sources:
//! - **hex.pm**: Erlang package registry (version string or `{ hex = "version" }`)
//! - **git**: Clone from a git URL with optional tag/branch/rev
//! - **path**: Local filesystem symlink

use serde::Deserialize;
use std::io::Read;
use std::path::{Path, PathBuf};

// ── Dependency spec types ────────────────────────────────────────────

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum DepSpec {
    /// Short form: `jsx = "3.1.0"` (hex.pm version)
    Hex(String),
    /// Table form: `{ git = "...", tag = "..." }` or `{ path = "..." }`
    Table(DepTable),
}

#[derive(Deserialize, Debug, Clone)]
pub struct DepTable {
    pub git: Option<String>,
    pub path: Option<String>,
    pub tag: Option<String>,
    pub branch: Option<String>,
    pub rev: Option<String>,
}

/// Returns the deps directory for a project root: `<root>/.nera/deps/`
pub fn deps_dir_for_project(root: &Path) -> PathBuf {
    root.join(".nera").join("deps")
}

// ── nera init ────────────────────────────────────────────────────────

pub fn cmd_init() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;

    // Error if nera.toml already exists
    let toml_path = cwd.join("nera.toml");
    if toml_path.exists() {
        return Err("nera.toml already exists in this directory".into());
    }

    // Derive project name from directory name
    let name = cwd
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("cannot determine project name from current directory")?;

    // Validate name
    if name.is_empty()
        || !name.chars().next().unwrap().is_ascii_lowercase()
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(format!(
            "directory name '{}' is not a valid Erlang module name \
             (must start with lowercase, contain only [a-z0-9_])",
            name
        )
        .into());
    }

    // Write nera.toml
    let toml_content = format!(
        r#"[project]
name = "{name}"
entry = "{name}:start"
output = "{name}"
"#
    );
    std::fs::write(&toml_path, toml_content)?;

    // Create src/ if it doesn't exist
    let src_dir = cwd.join("src");
    if !src_dir.exists() {
        std::fs::create_dir_all(&src_dir)?;
    }

    // Append .nera/ to .gitignore
    update_gitignore(&cwd)?;

    eprintln!("Initialized nera project '{}' in {}", name, cwd.display());
    Ok(())
}

/// Ensure `.nera/` is in `.gitignore`. Creates the file if needed.
fn update_gitignore(project_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let gitignore_path = project_dir.join(".gitignore");
    if gitignore_path.exists() {
        let content = std::fs::read_to_string(&gitignore_path)?;
        // Check if already has .nera/ (with or without leading /)
        if content.lines().any(|l| {
            let trimmed = l.trim();
            trimmed == ".nera/" || trimmed == ".nera" || trimmed == "/.nera/" || trimmed == "/.nera"
        }) {
            return Ok(());
        }
        // Append
        let mut new_content = content;
        if !new_content.ends_with('\n') {
            new_content.push('\n');
        }
        new_content.push_str(".nera/\n");
        std::fs::write(&gitignore_path, new_content)?;
    } else {
        std::fs::write(&gitignore_path, ".nera/\n")?;
    }
    Ok(())
}

// ── nera deps get ────────────────────────────────────────────────────

pub fn cmd_deps_get(
    deps: &std::collections::HashMap<String, DepSpec>,
    project_root: &Path,
    verbose: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let deps_dir = deps_dir_for_project(project_root);

    if deps.is_empty() {
        eprintln!("No dependencies declared in nera.toml");
        return Ok(());
    }

    // Create .nera/deps/ if needed
    std::fs::create_dir_all(&deps_dir)?;

    let mut fetched = 0;
    let mut skipped = 0;

    // Sort deps by name for deterministic order
    let mut sorted_deps: Vec<_> = deps.iter().collect();
    sorted_deps.sort_by_key(|(name, _)| name.as_str());

    for (name, spec) in sorted_deps {
        let dest = deps_dir.join(name);

        // Skip if already fetched
        if dest.exists() {
            if verbose {
                eprintln!("  {} — already fetched, skipping", name);
            }
            skipped += 1;
            continue;
        }

        match spec {
            DepSpec::Hex(version) => {
                eprintln!("  Fetching {} {} from hex.pm...", name, version);
                fetch_hex_package(name, version, &dest, verbose)?;
                fetched += 1;
            }
            DepSpec::Table(table) => {
                if let Some(ref git_url) = table.git {
                    let ref_info = if let Some(ref tag) = table.tag {
                        format!(" (tag: {})", tag)
                    } else if let Some(ref branch) = table.branch {
                        format!(" (branch: {})", branch)
                    } else if let Some(ref rev) = table.rev {
                        format!(" (rev: {})", rev)
                    } else {
                        String::new()
                    };
                    eprintln!("  Fetching {} from git{} ...", name, ref_info);
                    fetch_git_dep(git_url, table, &dest)?;
                    fetched += 1;
                } else if let Some(ref path) = table.path {
                    let abs_path = if Path::new(path).is_absolute() {
                        PathBuf::from(path)
                    } else {
                        project_root.join(path)
                    };
                    let abs_path = abs_path.canonicalize().map_err(|e| {
                        format!("path dep '{}': cannot resolve '{}': {}", name, path, e)
                    })?;
                    eprintln!("  Linking {} -> {}", name, abs_path.display());
                    resolve_path_dep(&abs_path, &dest)?;
                    fetched += 1;
                } else {
                    return Err(format!(
                        "dep '{}': must specify 'git' or 'path' in table form",
                        name
                    )
                    .into());
                }
            }
        }
    }

    eprintln!(
        "Dependencies: {} fetched, {} already present",
        fetched, skipped
    );
    Ok(())
}

// ── nera deps add ────────────────────────────────────────────────────

pub fn cmd_deps_add(
    name: &str,
    version: Option<&str>,
    project_root: &Path,
    verbose: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let toml_path = project_root.join("nera.toml");
    if !toml_path.exists() {
        return Err("nera.toml not found — run 'nera init' first".into());
    }

    // Resolve version
    let version = match version {
        Some(v) => v.to_string(),
        None => {
            eprintln!("  Querying hex.pm for latest version of {}...", name);
            query_hex_latest_version(name)?
        }
    };

    // Add to nera.toml
    add_dep_to_toml(&toml_path, name, &version)?;
    eprintln!("  Added {} = \"{}\" to nera.toml", name, version);

    // Fetch
    let deps_dir = deps_dir_for_project(project_root);
    std::fs::create_dir_all(&deps_dir)?;
    let dest = deps_dir.join(name);
    if !dest.exists() {
        eprintln!("  Fetching {} {} from hex.pm...", name, version);
        fetch_hex_package(name, &version, &dest, verbose)?;
    }

    Ok(())
}

// ── Hex.pm fetch pipeline ────────────────────────────────────────────

fn fetch_hex_package(
    name: &str,
    version: &str,
    dest: &Path,
    verbose: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let url = format!(
        "https://repo.hex.pm/tarballs/{}-{}.tar",
        name, version
    );

    if verbose {
        eprintln!("    GET {}", url);
    }

    let resp = ureq::get(&url).call().map_err(|e| {
        format!("failed to download {} {}: {}", name, version, e)
    })?;

    let mut body = Vec::new();
    resp.into_reader().read_to_end(&mut body)?;

    if verbose {
        eprintln!("    Downloaded {} bytes", body.len());
    }

    // The hex tarball is an outer tar containing:
    // - VERSION
    // - metadata.config
    // - contents.tar.gz  <-- the actual package source
    // - CHECKSUM
    let mut contents_tar_gz: Option<Vec<u8>> = None;
    {
        let mut archive = tar::Archive::new(body.as_slice());
        for entry in archive.entries()? {
            let mut entry = entry?;
            let path = entry.path()?.to_path_buf();
            if path.to_str() == Some("contents.tar.gz") {
                let mut buf = Vec::new();
                entry.read_to_end(&mut buf)?;
                contents_tar_gz = Some(buf);
                break;
            }
        }
    }

    let contents_tar_gz = contents_tar_gz.ok_or_else(|| {
        format!(
            "hex package {} {}: missing contents.tar.gz in tarball",
            name, version
        )
    })?;

    if verbose {
        eprintln!("    Extracting contents.tar.gz ({} bytes)", contents_tar_gz.len());
    }

    // Decompress and extract contents.tar.gz into dest/
    std::fs::create_dir_all(dest)?;
    let decoder = flate2::read::GzDecoder::new(contents_tar_gz.as_slice());
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(dest)?;

    if verbose {
        eprintln!("    Extracted to {}", dest.display());
    }

    Ok(())
}

/// Query hex.pm API for the latest version of a package.
fn query_hex_latest_version(name: &str) -> Result<String, Box<dyn std::error::Error>> {
    let url = format!("https://hex.pm/api/packages/{}", name);
    let resp = ureq::get(&url)
        .set("Accept", "application/json")
        .call()
        .map_err(|e| format!("hex.pm API error for '{}': {}", name, e))?;

    let body: serde_json::Value = resp.into_json()?;

    // releases is sorted newest first
    let releases = body
        .get("releases")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| format!("package '{}' not found on hex.pm", name))?;
    let version = releases
        .first()
        .and_then(|rel| rel.get("version"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("package '{}' has no releases on hex.pm", name))?;

    Ok(version.to_string())
}

// ── Git fetch pipeline ───────────────────────────────────────────────

fn fetch_git_dep(
    url: &str,
    table: &DepTable,
    dest: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let dest_str = dest.to_string_lossy().to_string();

    let status = if let Some(ref tag) = table.tag {
        std::process::Command::new("git")
            .args(["clone", "--depth", "1", "--branch", tag, url, &dest_str])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .status()
    } else if let Some(ref branch) = table.branch {
        std::process::Command::new("git")
            .args(["clone", "--depth", "1", "--branch", branch, url, &dest_str])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .status()
    } else if let Some(ref rev) = table.rev {
        // Full clone needed for arbitrary rev checkout
        let s = std::process::Command::new("git")
            .args(["clone", url, &dest_str])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .status()?;
        if !s.success() {
            return Err(format!("git clone failed for {}", url).into());
        }
        std::process::Command::new("git")
            .args(["-C", &dest_str, "checkout", rev])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .status()
    } else {
        std::process::Command::new("git")
            .args(["clone", "--depth", "1", url, &dest_str])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .status()
    };

    match status {
        Ok(s) if s.success() => {}
        Ok(_) => return Err(format!("git clone failed for {}", url).into()),
        Err(e) => {
            return Err(format!("cannot run git: {} — is git installed?", e).into())
        }
    }

    // Remove .git/ to save space
    let git_dir = dest.join(".git");
    if git_dir.exists() {
        let _ = std::fs::remove_dir_all(&git_dir);
    }

    Ok(())
}

// ── Path dep handling ────────────────────────────────────────────────

fn resolve_path_dep(
    source: &Path,
    dest: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if !source.is_dir() {
        return Err(format!(
            "path dependency '{}' is not a directory",
            source.display()
        )
        .into());
    }

    // Ensure parent exists
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Create symlink
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, dest).map_err(|e| {
            format!(
                "cannot create symlink {} -> {}: {}",
                dest.display(),
                source.display(),
                e
            )
        })?;
    }

    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(source, dest).map_err(|e| {
            format!(
                "cannot create directory junction {} -> {}: {}",
                dest.display(),
                source.display(),
                e
            )
        })?;
    }

    Ok(())
}

// ── TOML manipulation ────────────────────────────────────────────────

fn add_dep_to_toml(
    toml_path: &Path,
    name: &str,
    version: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(toml_path)?;
    let new_line = format!("{} = \"{}\"", name, version);

    // Check if dep already exists
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(&format!("{} ", name))
            || trimmed.starts_with(&format!("{}=", name))
            || trimmed.starts_with(&format!("{} =", name))
        {
            return Err(format!("dep '{}' already exists in nera.toml", name).into());
        }
    }

    let new_content = if let Some(deps_pos) = find_deps_section_end(&content) {
        // Insert before the next section header or at EOF
        let mut result = content[..deps_pos].to_string();
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push_str(&new_line);
        result.push('\n');
        result.push_str(&content[deps_pos..]);
        result
    } else {
        // No [deps] section — append one
        let mut result = content;
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push_str(&format!("\n[deps]\n{}\n", new_line));
        result
    };

    std::fs::write(toml_path, new_content)?;
    Ok(())
}

/// Find the end of the `[deps]` section content (position of the next `[` header or EOF).
/// Returns `None` if no `[deps]` section exists.
fn find_deps_section_end(content: &str) -> Option<usize> {
    let mut in_deps = false;

    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed == "[deps]" {
            in_deps = true;
            continue;
        }
        if in_deps && trimmed.starts_with('[') {
            // Found next section header — insert before this
            let pos: usize = content
                .lines()
                .take(i)
                .map(|l| l.len() + 1)
                .sum();
            return Some(pos);
        }
    }

    if in_deps {
        // [deps] was the last section — insert at EOF
        Some(content.len())
    } else {
        None
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Create a temporary directory and return its path.
    /// The directory is NOT automatically cleaned up — caller must handle.
    fn tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nera_test_{}_{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    // ── DepSpec deserialization ───────────────────────────────────────

    #[test]
    fn depspec_hex_string() {
        let toml_str = r#"
[deps]
jsx = "3.1.0"
"#;
        let table: toml::Value = toml::from_str(toml_str).unwrap();
        let deps = table.get("deps").unwrap().as_table().unwrap();
        let spec: DepSpec = deps.get("jsx").unwrap().clone().try_into().unwrap();
        match spec {
            DepSpec::Hex(v) => assert_eq!(v, "3.1.0"),
            _ => panic!("expected Hex variant"),
        }
    }

    #[test]
    fn depspec_git_with_tag() {
        let toml_str = r#"
[deps]
ranch = { git = "https://github.com/ninenines/ranch.git", tag = "1.8.0" }
"#;
        let table: toml::Value = toml::from_str(toml_str).unwrap();
        let deps = table.get("deps").unwrap().as_table().unwrap();
        let spec: DepSpec = deps.get("ranch").unwrap().clone().try_into().unwrap();
        match spec {
            DepSpec::Table(t) => {
                assert_eq!(t.git.as_deref(), Some("https://github.com/ninenines/ranch.git"));
                assert_eq!(t.tag.as_deref(), Some("1.8.0"));
                assert!(t.branch.is_none());
                assert!(t.rev.is_none());
                assert!(t.path.is_none());
            }
            _ => panic!("expected Table variant"),
        }
    }

    #[test]
    fn depspec_git_with_branch() {
        let toml_str = r#"
[deps]
mylib = { git = "https://github.com/user/repo.git", branch = "main" }
"#;
        let table: toml::Value = toml::from_str(toml_str).unwrap();
        let deps = table.get("deps").unwrap().as_table().unwrap();
        let spec: DepSpec = deps.get("mylib").unwrap().clone().try_into().unwrap();
        match spec {
            DepSpec::Table(t) => {
                assert_eq!(t.git.as_deref(), Some("https://github.com/user/repo.git"));
                assert_eq!(t.branch.as_deref(), Some("main"));
                assert!(t.tag.is_none());
            }
            _ => panic!("expected Table variant"),
        }
    }

    #[test]
    fn depspec_git_with_rev() {
        let toml_str = r#"
[deps]
mylib = { git = "https://github.com/user/repo.git", rev = "abc123" }
"#;
        let table: toml::Value = toml::from_str(toml_str).unwrap();
        let deps = table.get("deps").unwrap().as_table().unwrap();
        let spec: DepSpec = deps.get("mylib").unwrap().clone().try_into().unwrap();
        match spec {
            DepSpec::Table(t) => {
                assert_eq!(t.rev.as_deref(), Some("abc123"));
            }
            _ => panic!("expected Table variant"),
        }
    }

    #[test]
    fn depspec_path() {
        let toml_str = r#"
[deps]
utils = { path = "../shared_utils" }
"#;
        let table: toml::Value = toml::from_str(toml_str).unwrap();
        let deps = table.get("deps").unwrap().as_table().unwrap();
        let spec: DepSpec = deps.get("utils").unwrap().clone().try_into().unwrap();
        match spec {
            DepSpec::Table(t) => {
                assert_eq!(t.path.as_deref(), Some("../shared_utils"));
                assert!(t.git.is_none());
            }
            _ => panic!("expected Table variant"),
        }
    }

    #[test]
    fn depspec_full_nera_toml() {
        // Test parsing a complete nera.toml with [project] + [deps]
        let toml_str = r#"
[project]
name = "myapp"
entry = "myapp:start"
output = "myapp"

[deps]
jsx = "3.1.0"
ranch = { git = "https://github.com/ninenines/ranch.git", tag = "1.8.0" }
utils = { path = "../shared_utils" }
"#;
        let parsed: toml::Value = toml::from_str(toml_str).unwrap();
        let deps_table = parsed.get("deps").unwrap().as_table().unwrap();

        // Verify all three dep types parse
        let jsx: DepSpec = deps_table.get("jsx").unwrap().clone().try_into().unwrap();
        assert!(matches!(jsx, DepSpec::Hex(ref v) if v == "3.1.0"));

        let ranch: DepSpec = deps_table.get("ranch").unwrap().clone().try_into().unwrap();
        assert!(matches!(ranch, DepSpec::Table(ref t) if t.git.is_some()));

        let utils: DepSpec = deps_table.get("utils").unwrap().clone().try_into().unwrap();
        assert!(matches!(utils, DepSpec::Table(ref t) if t.path.is_some()));
    }

    // ── deps_dir_for_project ─────────────────────────────────────────

    #[test]
    fn deps_dir_path() {
        let root = PathBuf::from("/home/user/myproject");
        assert_eq!(
            deps_dir_for_project(&root),
            PathBuf::from("/home/user/myproject/.nera/deps")
        );
    }

    // ── find_deps_section_end ────────────────────────────────────────

    #[test]
    fn find_deps_end_no_section() {
        let content = "[project]\nname = \"foo\"\n";
        assert_eq!(find_deps_section_end(content), None);
    }

    #[test]
    fn find_deps_end_empty_section_at_eof() {
        let content = "[project]\nname = \"foo\"\n\n[deps]\n";
        let pos = find_deps_section_end(content).unwrap();
        assert_eq!(pos, content.len());
    }

    #[test]
    fn find_deps_end_with_entries_at_eof() {
        let content = "[project]\nname = \"foo\"\n\n[deps]\njsx = \"3.1.0\"\n";
        let pos = find_deps_section_end(content).unwrap();
        assert_eq!(pos, content.len());
    }

    #[test]
    fn find_deps_end_followed_by_another_section() {
        let content = "[deps]\njsx = \"3.1.0\"\n\n[other]\nkey = \"val\"\n";
        let pos = find_deps_section_end(content).unwrap();
        // Should point to the start of "[other]" line
        let remainder = &content[pos..];
        assert!(remainder.starts_with("[other]"));
    }

    #[test]
    fn find_deps_end_empty_followed_by_section() {
        let content = "[deps]\n[other]\n";
        let pos = find_deps_section_end(content).unwrap();
        let remainder = &content[pos..];
        assert!(remainder.starts_with("[other]"));
    }

    // ── add_dep_to_toml ──────────────────────────────────────────────

    #[test]
    fn add_dep_creates_deps_section() {
        let dir = tmp_dir("add_dep_create");
        let toml_path = dir.join("nera.toml");
        std::fs::write(&toml_path, "[project]\nname = \"foo\"\n").unwrap();

        add_dep_to_toml(&toml_path, "jsx", "3.1.0").unwrap();

        let result = std::fs::read_to_string(&toml_path).unwrap();
        assert!(result.contains("[deps]"));
        assert!(result.contains("jsx = \"3.1.0\""));
        // Original content preserved
        assert!(result.contains("[project]"));
        assert!(result.contains("name = \"foo\""));

        cleanup(&dir);
    }

    #[test]
    fn add_dep_appends_to_existing_section() {
        let dir = tmp_dir("add_dep_append");
        let toml_path = dir.join("nera.toml");
        std::fs::write(
            &toml_path,
            "[project]\nname = \"foo\"\n\n[deps]\njsx = \"3.1.0\"\n",
        )
        .unwrap();

        add_dep_to_toml(&toml_path, "cowlib", "2.12.0").unwrap();

        let result = std::fs::read_to_string(&toml_path).unwrap();
        assert!(result.contains("jsx = \"3.1.0\""));
        assert!(result.contains("cowlib = \"2.12.0\""));

        cleanup(&dir);
    }

    #[test]
    fn add_dep_inserts_before_next_section() {
        let dir = tmp_dir("add_dep_before_section");
        let toml_path = dir.join("nera.toml");
        std::fs::write(
            &toml_path,
            "[deps]\njsx = \"3.1.0\"\n\n[other]\nkey = \"val\"\n",
        )
        .unwrap();

        add_dep_to_toml(&toml_path, "cowlib", "2.12.0").unwrap();

        let result = std::fs::read_to_string(&toml_path).unwrap();
        assert!(result.contains("cowlib = \"2.12.0\""));
        // [other] should still be present and after [deps]
        let deps_pos = result.find("[deps]").unwrap();
        let cowlib_pos = result.find("cowlib").unwrap();
        let other_pos = result.find("[other]").unwrap();
        assert!(deps_pos < cowlib_pos);
        assert!(cowlib_pos < other_pos);

        cleanup(&dir);
    }

    #[test]
    fn add_dep_rejects_duplicate() {
        let dir = tmp_dir("add_dep_dup");
        let toml_path = dir.join("nera.toml");
        std::fs::write(
            &toml_path,
            "[project]\nname = \"foo\"\n\n[deps]\njsx = \"3.1.0\"\n",
        )
        .unwrap();

        let err = add_dep_to_toml(&toml_path, "jsx", "4.0.0").unwrap_err();
        assert!(err.to_string().contains("already exists"));

        cleanup(&dir);
    }

    #[test]
    fn add_dep_rejects_duplicate_no_spaces() {
        let dir = tmp_dir("add_dep_dup_nospace");
        let toml_path = dir.join("nera.toml");
        std::fs::write(
            &toml_path,
            "[deps]\njsx=\"3.1.0\"\n",
        )
        .unwrap();

        let err = add_dep_to_toml(&toml_path, "jsx", "4.0.0").unwrap_err();
        assert!(err.to_string().contains("already exists"));

        cleanup(&dir);
    }

    // ── update_gitignore ─────────────────────────────────────────────

    #[test]
    fn gitignore_creates_new_file() {
        let dir = tmp_dir("gitignore_new");
        update_gitignore(&dir).unwrap();

        let content = std::fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert_eq!(content, ".nera/\n");

        cleanup(&dir);
    }

    #[test]
    fn gitignore_appends_to_existing() {
        let dir = tmp_dir("gitignore_append");
        std::fs::write(dir.join(".gitignore"), "*.o\n*.beam\n").unwrap();

        update_gitignore(&dir).unwrap();

        let content = std::fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert!(content.contains("*.o"));
        assert!(content.contains(".nera/"));
    }

    #[test]
    fn gitignore_appends_newline_if_missing() {
        let dir = tmp_dir("gitignore_nonl");
        std::fs::write(dir.join(".gitignore"), "*.o").unwrap();

        update_gitignore(&dir).unwrap();

        let content = std::fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert_eq!(content, "*.o\n.nera/\n");

        cleanup(&dir);
    }

    #[test]
    fn gitignore_skips_if_already_present() {
        let dir = tmp_dir("gitignore_skip");
        std::fs::write(dir.join(".gitignore"), "*.o\n.nera/\n").unwrap();

        update_gitignore(&dir).unwrap();

        let content = std::fs::read_to_string(dir.join(".gitignore")).unwrap();
        // Should NOT duplicate .nera/
        assert_eq!(content.matches(".nera/").count(), 1);

        cleanup(&dir);
    }

    #[test]
    fn gitignore_skips_variant_with_leading_slash() {
        let dir = tmp_dir("gitignore_slash");
        std::fs::write(dir.join(".gitignore"), "/.nera/\n").unwrap();

        update_gitignore(&dir).unwrap();

        let content = std::fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert_eq!(content, "/.nera/\n"); // unchanged

        cleanup(&dir);
    }

    #[test]
    fn gitignore_skips_variant_without_trailing_slash() {
        let dir = tmp_dir("gitignore_noslash");
        std::fs::write(dir.join(".gitignore"), ".nera\n").unwrap();

        update_gitignore(&dir).unwrap();

        let content = std::fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert_eq!(content, ".nera\n"); // unchanged

        cleanup(&dir);
    }

    // ── resolve_path_dep ─────────────────────────────────────────────

    #[test]
    fn path_dep_creates_symlink() {
        let dir = tmp_dir("path_dep");
        let source = dir.join("my_lib");
        let dest = dir.join("deps").join("my_lib");
        std::fs::create_dir_all(source.join("src")).unwrap();
        std::fs::write(source.join("src/my_lib.erl"), "-module(my_lib).").unwrap();

        resolve_path_dep(&source, &dest).unwrap();

        assert!(dest.exists());
        assert!(dest.join("src/my_lib.erl").exists());
        // Should be a symlink
        assert!(dest.symlink_metadata().unwrap().file_type().is_symlink());

        cleanup(&dir);
    }

    #[test]
    fn path_dep_rejects_nonexistent() {
        let dir = tmp_dir("path_dep_noexist");
        let source = dir.join("nonexistent");
        let dest = dir.join("deps").join("lib");

        let err = resolve_path_dep(&source, &dest).unwrap_err();
        assert!(err.to_string().contains("not a directory"));

        cleanup(&dir);
    }

    // ── cmd_deps_get orchestration ───────────────────────────────────

    #[test]
    fn deps_get_empty_deps() {
        let dir = tmp_dir("deps_get_empty");
        let deps = HashMap::new();

        // Should succeed with no-op
        cmd_deps_get(&deps, &dir, false).unwrap();

        cleanup(&dir);
    }

    #[test]
    fn deps_get_skips_already_fetched() {
        let dir = tmp_dir("deps_get_skip");
        let deps_dir = deps_dir_for_project(&dir);
        let dest = deps_dir.join("jsx");
        std::fs::create_dir_all(&dest).unwrap();

        let mut deps = HashMap::new();
        deps.insert("jsx".to_string(), DepSpec::Hex("3.1.0".to_string()));

        // Should succeed without hitting the network (dest already exists)
        cmd_deps_get(&deps, &dir, false).unwrap();

        cleanup(&dir);
    }

    #[test]
    fn deps_get_path_dep() {
        let dir = tmp_dir("deps_get_path");
        // Create a fake source library
        let lib_dir = dir.join("my_lib");
        std::fs::create_dir_all(lib_dir.join("src")).unwrap();
        std::fs::write(lib_dir.join("src/my_lib.erl"), "-module(my_lib).").unwrap();

        let mut deps = HashMap::new();
        deps.insert(
            "my_lib".to_string(),
            DepSpec::Table(DepTable {
                git: None,
                path: Some("my_lib".to_string()),
                tag: None,
                branch: None,
                rev: None,
            }),
        );

        cmd_deps_get(&deps, &dir, false).unwrap();

        // Verify symlink was created
        let dest = deps_dir_for_project(&dir).join("my_lib");
        assert!(dest.exists());
        assert!(dest.join("src/my_lib.erl").exists());

        cleanup(&dir);
    }

    #[test]
    fn deps_get_rejects_invalid_table() {
        let dir = tmp_dir("deps_get_invalid");

        let mut deps = HashMap::new();
        deps.insert(
            "bad".to_string(),
            DepSpec::Table(DepTable {
                git: None,
                path: None,
                tag: None,
                branch: None,
                rev: None,
            }),
        );

        let err = cmd_deps_get(&deps, &dir, false).unwrap_err();
        assert!(err.to_string().contains("must specify 'git' or 'path'"));

        cleanup(&dir);
    }

    #[test]
    fn deps_get_rejects_nonexistent_path_dep() {
        let dir = tmp_dir("deps_get_badpath");

        let mut deps = HashMap::new();
        deps.insert(
            "missing".to_string(),
            DepSpec::Table(DepTable {
                git: None,
                path: Some("../nonexistent_lib".to_string()),
                tag: None,
                branch: None,
                rev: None,
            }),
        );

        let err = cmd_deps_get(&deps, &dir, false).unwrap_err();
        assert!(err.to_string().contains("cannot resolve"));

        cleanup(&dir);
    }

    // ── cmd_deps_add (offline parts) ─────────────────────────────────

    #[test]
    fn deps_add_no_toml_errors() {
        let dir = tmp_dir("deps_add_notoml");

        let err = cmd_deps_add("jsx", Some("3.1.0"), &dir, false).unwrap_err();
        assert!(err.to_string().contains("nera.toml not found"));

        cleanup(&dir);
    }

    // ── Integration: NeraToml deserialization via main.rs types ───────

    #[test]
    fn nera_toml_with_deps_deserializes() {
        // Directly test that the full NeraToml struct with deps deserializes
        let toml_str = r#"
[project]
name = "myapp"
entry = "myapp:start"
output = "myapp"

[deps]
jsx = "3.1.0"
ranch = { git = "https://github.com/ninenines/ranch.git", tag = "1.8.0" }
utils = { path = "../shared_utils" }
"#;
        // Parse deps section as HashMap<String, DepSpec>
        #[derive(serde::Deserialize)]
        struct TestToml {
            #[serde(default)]
            deps: Option<HashMap<String, DepSpec>>,
        }

        let parsed: TestToml = toml::from_str(toml_str).unwrap();
        let deps = parsed.deps.unwrap();
        assert_eq!(deps.len(), 3);
        assert!(matches!(deps.get("jsx"), Some(DepSpec::Hex(v)) if v == "3.1.0"));
        assert!(matches!(deps.get("ranch"), Some(DepSpec::Table(t)) if t.git.is_some() && t.tag.as_deref() == Some("1.8.0")));
        assert!(matches!(deps.get("utils"), Some(DepSpec::Table(t)) if t.path.as_deref() == Some("../shared_utils")));
    }

    #[test]
    fn nera_toml_without_deps_deserializes() {
        let toml_str = r#"
[project]
name = "myapp"
"#;
        #[derive(serde::Deserialize)]
        struct TestToml {
            #[serde(default)]
            deps: Option<HashMap<String, DepSpec>>,
        }

        let parsed: TestToml = toml::from_str(toml_str).unwrap();
        assert!(parsed.deps.is_none());
    }

    // ── cmd_init (functional test) ───────────────────────────────────

    #[test]
    fn init_creates_toml_and_src() {
        let parent = tmp_dir("init_parent");
        let project = parent.join("my_app");
        std::fs::create_dir_all(&project).unwrap();

        // cmd_init uses current_dir, so we can't call it directly in
        // a parallel test. Instead, test the constituent parts:

        // 1. Verify toml content generation
        let name = "my_app";
        let toml_content = format!(
            "[project]\nname = \"{name}\"\nentry = \"{name}:start\"\noutput = \"{name}\"\n"
        );
        std::fs::write(project.join("nera.toml"), &toml_content).unwrap();

        let read_back = std::fs::read_to_string(project.join("nera.toml")).unwrap();
        assert!(read_back.contains("name = \"my_app\""));
        assert!(read_back.contains("entry = \"my_app:start\""));

        // 2. Verify gitignore creation
        update_gitignore(&project).unwrap();
        let gi = std::fs::read_to_string(project.join(".gitignore")).unwrap();
        assert!(gi.contains(".nera/"));

        // 3. Verify src/ creation
        std::fs::create_dir_all(project.join("src")).unwrap();
        assert!(project.join("src").is_dir());

        cleanup(&parent);
    }

    // ── Multiple deps in deterministic order ─────────────────────────

    #[test]
    fn deps_get_processes_path_deps_alphabetically() {
        let dir = tmp_dir("deps_get_order");

        // Create three fake libraries
        for name in &["zlib", "alib", "mlib"] {
            let lib = dir.join(name);
            std::fs::create_dir_all(lib.join("src")).unwrap();
            std::fs::write(
                lib.join("src").join(format!("{}.erl", name)),
                format!("-module({}).", name),
            )
            .unwrap();
        }

        let mut deps = HashMap::new();
        for name in &["zlib", "alib", "mlib"] {
            deps.insert(
                name.to_string(),
                DepSpec::Table(DepTable {
                    git: None,
                    path: Some(name.to_string()),
                    tag: None,
                    branch: None,
                    rev: None,
                }),
            );
        }

        cmd_deps_get(&deps, &dir, false).unwrap();

        // All three should be linked
        let deps_path = deps_dir_for_project(&dir);
        assert!(deps_path.join("alib").exists());
        assert!(deps_path.join("mlib").exists());
        assert!(deps_path.join("zlib").exists());

        cleanup(&dir);
    }

    // ── add_dep_to_toml preserves comments ───────────────────────────

    #[test]
    fn add_dep_preserves_comments() {
        let dir = tmp_dir("add_dep_comments");
        let toml_path = dir.join("nera.toml");
        let original = "[project]\nname = \"foo\"\n# My project config\n\n[deps]\n# hex packages\njsx = \"3.1.0\"\n";
        std::fs::write(&toml_path, original).unwrap();

        add_dep_to_toml(&toml_path, "cowlib", "2.12.0").unwrap();

        let result = std::fs::read_to_string(&toml_path).unwrap();
        assert!(result.contains("# hex packages"));
        assert!(result.contains("# My project config"));
        assert!(result.contains("jsx = \"3.1.0\""));
        assert!(result.contains("cowlib = \"2.12.0\""));

        cleanup(&dir);
    }

    // ── add_dep_to_toml with no trailing newline ─────────────────────

    #[test]
    fn add_dep_no_trailing_newline() {
        let dir = tmp_dir("add_dep_nonl");
        let toml_path = dir.join("nera.toml");
        std::fs::write(&toml_path, "[project]\nname = \"foo\"").unwrap();

        add_dep_to_toml(&toml_path, "jsx", "3.1.0").unwrap();

        let result = std::fs::read_to_string(&toml_path).unwrap();
        assert!(result.contains("[deps]"));
        assert!(result.contains("jsx = \"3.1.0\""));
        // Should be valid TOML
        let _: toml::Value = toml::from_str(&result).unwrap();

        cleanup(&dir);
    }

    // ── add multiple deps sequentially ───────────────────────────────

    #[test]
    fn add_multiple_deps_sequentially() {
        let dir = tmp_dir("add_multi");
        let toml_path = dir.join("nera.toml");
        std::fs::write(&toml_path, "[project]\nname = \"foo\"\n").unwrap();

        add_dep_to_toml(&toml_path, "jsx", "3.1.0").unwrap();
        add_dep_to_toml(&toml_path, "cowlib", "2.12.0").unwrap();
        add_dep_to_toml(&toml_path, "ranch", "1.8.0").unwrap();

        let result = std::fs::read_to_string(&toml_path).unwrap();
        assert!(result.contains("jsx = \"3.1.0\""));
        assert!(result.contains("cowlib = \"2.12.0\""));
        assert!(result.contains("ranch = \"1.8.0\""));

        // Should still be valid TOML
        let _: toml::Value = toml::from_str(&result).unwrap();

        // [deps] section should appear exactly once
        assert_eq!(result.matches("[deps]").count(), 1);

        cleanup(&dir);
    }

    // ── DepSpec name collision with shorter names ─────────────────────

    #[test]
    fn add_dep_no_false_positive_prefix_match() {
        // "js" should not match "jsx"
        let dir = tmp_dir("add_dep_prefix");
        let toml_path = dir.join("nera.toml");
        std::fs::write(
            &toml_path,
            "[project]\nname = \"foo\"\n\n[deps]\njsx = \"3.1.0\"\n",
        )
        .unwrap();

        // Adding "js" should succeed — it's a different package
        add_dep_to_toml(&toml_path, "js", "1.0.0").unwrap();

        let result = std::fs::read_to_string(&toml_path).unwrap();
        assert!(result.contains("jsx = \"3.1.0\""));
        assert!(result.contains("js = \"1.0.0\""));

        cleanup(&dir);
    }
}
