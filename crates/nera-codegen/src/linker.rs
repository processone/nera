//! Links compiled object files with the runtime to produce executables.

use std::path::PathBuf;
use std::process::Command;
use target_lexicon::{OperatingSystem, Triple};

#[derive(Debug, thiserror::Error)]
pub enum LinkerError {
    #[error("linking failed with status: {0}")]
    LinkFailed(std::process::ExitStatus),

    #[error("linker not found: {0}")]
    LinkerNotFound(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct Linker {
    /// Object files to link
    pub object_files: Vec<PathBuf>,
    /// Path to the runtime static library (libnera_runtime.a)
    pub runtime_lib: Option<PathBuf>,
    /// Output binary path
    pub output: PathBuf,
    /// Target triple (determines platform-specific linker flags)
    pub target: Triple,
    /// Override the linker command (default: $CC, then "cc")
    pub linker_cmd: Option<String>,
}

impl Linker {
    pub fn new(output: PathBuf, target: Triple) -> Self {
        Self {
            object_files: Vec::new(),
            runtime_lib: None,
            output,
            target,
            linker_cmd: None,
        }
    }

    pub fn add_object(&mut self, path: PathBuf) {
        self.object_files.push(path);
    }

    pub fn set_runtime_lib(&mut self, path: PathBuf) {
        self.runtime_lib = Some(path);
    }

    pub fn set_linker_cmd(&mut self, cmd: String) {
        self.linker_cmd = Some(cmd);
    }

    /// Resolve the linker command: explicit override > $CC > "cc"
    fn resolve_linker(&self) -> String {
        if let Some(ref cmd) = self.linker_cmd {
            return cmd.clone();
        }
        if let Ok(cc) = std::env::var("CC") {
            return cc;
        }
        "cc".to_string()
    }

    /// Build the linker command without running it.
    fn build_command(&self) -> Command {
        let linker = self.resolve_linker();
        let mut cmd = Command::new(&linker);

        cmd.arg("-o").arg(&self.output);

        // Add object files
        for obj in &self.object_files {
            cmd.arg(obj);
        }

        // Link with runtime library by passing the full path directly.
        // We must NOT use `-L<dir> -lnera_runtime` because the extracted
        // runtime file has a content-keyed name (libnera_runtime-<sha256>.a)
        // that doesn't match the `-l` search pattern (libnera_runtime.a).
        if let Some(ref runtime_lib) = self.runtime_lib {
            let canonical = runtime_lib.canonicalize().unwrap_or_else(|_| runtime_lib.clone());
            eprintln!("Linking with runtime: {}", canonical.display());
            cmd.arg(&canonical);
        }

        // Platform-specific libraries and dead code elimination
        // Based on target triple (not host), enabling cross-compilation.
        match self.target.operating_system {
            OperatingSystem::Linux => {
                cmd.arg("-lpthread");
                cmd.arg("-lm");
                cmd.arg("-ldl");
                cmd.arg("-Wl,--gc-sections");
                // Force the linker to keep jemalloc's compile-time config symbol.
                cmd.arg("-Wl,--undefined=_rjem_malloc_conf");
            }
            OperatingSystem::Darwin(_) | OperatingSystem::MacOSX { .. } => {
                cmd.arg("-lpthread");
                cmd.arg("-lm");
                cmd.arg("-Wl,-dead_strip");
                cmd.arg("-Wl,-no_warn_duplicate_libraries");
                // Force the linker to keep jemalloc's compile-time config symbol.
                // Mach-O adds an underscore to the C symbol _rjem_malloc_conf.
                cmd.arg("-Wl,-u,__rjem_malloc_conf");
            }
            OperatingSystem::Windows => {
                cmd.arg("-lws2_32");
                cmd.arg("-lbcrypt");
                cmd.arg("-luserenv");
            }
            _ => {
                // Best-effort: link with common libraries
                cmd.arg("-lpthread");
                cmd.arg("-lm");
            }
        }

        cmd
    }

    /// Link all object files with the runtime to produce an executable.
    pub fn link(&self) -> Result<(), LinkerError> {
        let mut cmd = self.build_command();

        let output = cmd.output()?;

        if !output.status.success() {
            eprintln!(
                "Linker stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            return Err(LinkerError::LinkFailed(output.status));
        }

        Ok(())
    }

    /// Find the runtime library for the host platform.
    /// Uses a robust strategy:
    /// 1. Check relative to the running executable (handles `cargo run`)
    /// 2. Check relative to the current working directory
    ///
    /// Prefers release builds over debug builds.
    pub fn find_runtime_lib() -> Option<PathBuf> {
        // Strategy 1: Look relative to the current executable.
        // When running via `cargo run --release`, the executable is at
        // `target/release/nera`, so its parent's parent is the workspace root.
        if let Ok(exe) = std::env::current_exe() {
            if let Some(exe_dir) = exe.parent() {
                for name in ["libnera_runtime.a", "nera_runtime.lib"] {
                    let candidate = exe_dir.join(name);
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                    // Installed layout: <prefix>/bin/nera → <prefix>/lib/<runtime>
                    let installed = exe_dir.join("../lib").join(name);
                    if installed.is_file() {
                        return Some(installed);
                    }
                }
            }
        }

        // Strategy 2: Look relative to the current working directory (workspace root).
        let candidates = [
            PathBuf::from("target/release/libnera_runtime.a"),
            PathBuf::from("target/debug/libnera_runtime.a"),
            PathBuf::from("../nera-runtime/target/release/libnera_runtime.a"),
        ];

        for path in &candidates {
            if path.exists() {
                return Some(path.clone());
            }
        }

        None
    }

    /// Return the linker command string without running it. For testing.
    #[cfg(test)]
    fn resolved_linker(&self) -> String {
        self.resolve_linker()
    }

    /// Find the runtime library for a specific target triple.
    /// For cross-compilation, looks in `target/<triple>/release/`.
    /// For host compilation, falls back to `find_runtime_lib()`.
    pub fn find_runtime_lib_for_target(target: &Triple) -> Option<PathBuf> {
        if *target != Triple::host() {
            let target_str = target.to_string();

            // Check cargo target directory (workspace-relative)
            let candidates = [
                PathBuf::from(format!("target/{}/release/libnera_runtime.a", target_str)),
                PathBuf::from(format!("target/{}/debug/libnera_runtime.a", target_str)),
            ];
            for path in &candidates {
                if path.exists() {
                    return Some(path.clone());
                }
            }

            // Check relative to executable
            if let Ok(exe) = std::env::current_exe() {
                if let Some(exe_dir) = exe.parent() {
                    let candidate = exe_dir.join(format!("../../target/{}/release/libnera_runtime.a", target_str));
                    if candidate.exists() {
                        return Some(candidate);
                    }
                }
            }

            return None;
        }

        // Host target: use existing search logic
        Self::find_runtime_lib()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use target_lexicon::Triple;

    fn host_linker() -> Linker {
        Linker::new(PathBuf::from("/tmp/test_output"), Triple::host())
    }

    // ---------------------------------------------------------------
    // Construction and configuration
    // ---------------------------------------------------------------

    #[test]
    fn new_linker_has_empty_objects() {
        let linker = host_linker();
        assert!(linker.object_files.is_empty());
        assert!(linker.runtime_lib.is_none());
        assert!(linker.linker_cmd.is_none());
        assert_eq!(linker.output, PathBuf::from("/tmp/test_output"));
    }

    #[test]
    fn add_object_files() {
        let mut linker = host_linker();
        linker.add_object(PathBuf::from("a.o"));
        linker.add_object(PathBuf::from("b.o"));
        assert_eq!(linker.object_files.len(), 2);
        assert_eq!(linker.object_files[0], PathBuf::from("a.o"));
        assert_eq!(linker.object_files[1], PathBuf::from("b.o"));
    }

    #[test]
    fn set_runtime_lib_path() {
        let mut linker = host_linker();
        linker.set_runtime_lib(PathBuf::from("/usr/lib/libnera_runtime.a"));
        assert_eq!(
            linker.runtime_lib,
            Some(PathBuf::from("/usr/lib/libnera_runtime.a"))
        );
    }

    #[test]
    fn set_linker_cmd_override() {
        let mut linker = host_linker();
        linker.set_linker_cmd("zig cc".to_string());
        assert_eq!(linker.linker_cmd, Some("zig cc".to_string()));
    }

    // ---------------------------------------------------------------
    // resolve_linker precedence
    // ---------------------------------------------------------------

    #[test]
    fn resolve_linker_explicit_override() {
        let mut linker = host_linker();
        linker.set_linker_cmd("custom-ld".to_string());
        assert_eq!(linker.resolved_linker(), "custom-ld");
    }

    #[test]
    fn resolve_linker_default_fallback() {
        let linker = host_linker();
        // Clear CC env var for this test
        let saved = std::env::var("CC").ok();
        std::env::remove_var("CC");

        let result = linker.resolved_linker();
        assert_eq!(result, "cc");

        // Restore CC if it was set
        if let Some(cc) = saved {
            std::env::set_var("CC", cc);
        }
    }

    // ---------------------------------------------------------------
    // Target triple preserved
    // ---------------------------------------------------------------

    // The native allocator-profile regression requires the strong prefixed
    // configuration symbol to survive archive extraction and dead stripping.
    #[test]
    fn linker_retains_prefixed_allocator_configuration() {
        for (triple, expected) in [
            ("x86_64-unknown-linux-gnu", "-Wl,--undefined=_rjem_malloc_conf"),
            ("aarch64-unknown-linux-gnu", "-Wl,--undefined=_rjem_malloc_conf"),
            ("aarch64-apple-darwin", "-Wl,-u,__rjem_malloc_conf"),
        ] {
            let linker = Linker::new(PathBuf::from("out"), triple.parse().unwrap());
            let command = linker.build_command();
            assert!(command.get_args().any(|arg| arg == expected), "{triple}");
        }
    }

    #[test]
    fn linker_preserves_target() {
        let triple: Triple = "x86_64-unknown-linux-gnu".parse().unwrap();
        let linker = Linker::new(PathBuf::from("out"), triple.clone());
        assert_eq!(linker.target, triple);
    }

    // ---------------------------------------------------------------
    // LinkerError display
    // ---------------------------------------------------------------

    #[test]
    fn linker_error_not_found_display() {
        let e = LinkerError::LinkerNotFound("custom-ld".to_string());
        let msg = e.to_string();
        assert!(msg.contains("custom-ld"));
    }

    /// Regression test: the runtime library must be passed as a full path,
    /// not via `-lnera_runtime`, because the extracted file has a
    /// content-keyed name that `-l` won't find.
    #[test]
    fn runtime_lib_linked_by_full_path_not_dash_l() {
        let mut linker = host_linker();
        let runtime_path = PathBuf::from(
            "/tmp/libnera_runtime-1a851819aa825f9de364649b9b78da44d376c7d691c68fc634ca4b4d9014fd81.a",
        );
        linker.set_runtime_lib(runtime_path.clone());

        let cmd = linker.build_command();
        let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect();

        // Must contain the full path to the runtime library
        assert!(
            args.iter().any(|a| a.contains("libnera_runtime")),
            "expected runtime library path in args: {:?}",
            args
        );
        // Must NOT use -lnera_runtime (which would fail for content-keyed filenames)
        assert!(
            !args.iter().any(|a| a == "-lnera_runtime"),
            "must not use -lnera_runtime flag: {:?}",
            args
        );
    }
}
