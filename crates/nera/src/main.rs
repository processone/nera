//! nera — Erlang-to-native compiler
//!
//! Compiles Erlang source files to standalone native binaries.
//! Uses Cranelift for code generation and embeds a lightweight runtime
//! with Erlang-style processes, message passing, and per-process GC.

use clap::{Parser as ClapParser, Subcommand};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use nera_syntax::lexer::Lexer;
use nera_syntax::parser::Parser;
use nera_syntax::preproc::Preprocessor;
use nera_syntax::source_map::SourceMap;
use nera_syntax::MultiSourceMap;
use nera_syntax::error::{emit_parse_errors, emit_parse_errors_multi, Diagnostic};
use nera_ir::lower::IrBuilder;
use nera_codegen::compiler::ModuleCompiler;
use nera_codegen::linker::Linker;
use target_lexicon::Triple;

mod app_registry;
mod cross_module_warnings;
mod deps;
mod lsp;
mod otp;
mod test_runner;
mod eunit_runner;
mod module_info;
mod runtime_cache;

// ── Embedded stdlib ──────────────────────────────────────────────────

include!(concat!(env!("OUT_DIR"), "/embedded_stdlib.rs"));

// ── Embedded runtime library ────────────────────────────────────────

include!(concat!(env!("OUT_DIR"), "/embedded_runtime.rs"));

// ── Source input (file or embedded) ──────────────────────────────────

pub(crate) enum SourceInput {
    File(PathBuf),
    Embedded { name: String, source: &'static str },
}

impl SourceInput {
    pub(crate) fn display_path(&self) -> String {
        match self {
            SourceInput::File(p) => p.display().to_string(),
            SourceInput::Embedded { name, .. } => format!("<stdlib>/{}.erl", name),
        }
    }

    pub(crate) fn module_name(&self) -> String {
        match self {
            SourceInput::File(p) => p.file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default(),
            SourceInput::Embedded { name, .. } => name.clone(),
        }
    }

    pub(crate) fn read_source(&self) -> Result<String, Box<dyn std::error::Error>> {
        match self {
            SourceInput::File(p) => std::fs::read_to_string(p)
                .map_err(|e| format!("cannot read {}: {}", p.display(), e).into()),
            SourceInput::Embedded { source, .. } => Ok(source.to_string()),
        }
    }

    pub(crate) fn as_path(&self) -> Option<&Path> {
        match self {
            SourceInput::File(p) => Some(p.as_path()),
            SourceInput::Embedded { .. } => None,
        }
    }

    pub(crate) fn is_embedded(&self) -> bool {
        matches!(self, SourceInput::Embedded { .. })
    }
}

// ── CLI ──────────────────────────────────────────────────────────────

#[derive(ClapParser)]
#[command(
    name = "nera",
    version = env!("CARGO_PKG_VERSION"),
    author = "ProcessOne <contact@process-one.net>",
    about = "Erlang-to-native compiler with lightweight runtime",
    after_help = "Website: https://process-one.net"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new Erlang project
    New {
        /// Project name (used as directory name and module name)
        name: String,
    },

    /// Initialize a nera project in the current directory
    Init,

    /// Manage dependencies
    Deps {
        #[command(subcommand)]
        action: DepsAction,
    },

    /// Compile the current project or given source files
    Build {
        /// Input .erl files
        files: Vec<PathBuf>,

        /// Source directory — recursively discover .erl files (repeatable)
        #[arg(long = "src")]
        src_dirs: Vec<PathBuf>,

        /// Include search path for -include() directives (repeatable)
        #[arg(short = 'I', long = "include")]
        include_paths: Vec<PathBuf>,

        /// Dependency directory — scan each subdirectory as an OTP app (repeatable)
        #[arg(long = "deps-dir")]
        deps_dirs: Vec<PathBuf>,

        /// Output binary name
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Entry point (module:function)
        #[arg(short, long)]
        entry: Option<String>,

        /// Emit IR for debugging (do not compile)
        #[arg(long)]
        emit_ir: bool,

        /// Emit object files only (do not link)
        #[arg(long)]
        emit_obj: bool,

        /// Do not include the built-in stdlib modules
        #[arg(long)]
        no_stdlib: bool,

        /// Suppress all compiler warnings
        #[arg(long)]
        no_warnings: bool,

        /// Target triple for cross-compilation (e.g., x86_64-unknown-linux-gnu)
        #[arg(long)]
        target: Option<String>,

        /// Override the linker command (default: $CC, then 'cc')
        #[arg(long)]
        linker: Option<String>,

        /// Verbose output
        #[arg(short, long)]
        verbose: bool,
    },

    /// Build and run the current project
    Run {
        /// Verbose output
        #[arg(short, long)]
        verbose: bool,

        /// Do not include the built-in stdlib modules
        #[arg(long)]
        no_stdlib: bool,

        /// Suppress all compiler warnings
        #[arg(long)]
        no_warnings: bool,

        /// Arguments passed to the compiled binary (after --)
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// Type-check source files without compiling
    Check {
        /// Input .erl files
        files: Vec<PathBuf>,

        /// Source directory — recursively discover .erl files (repeatable)
        #[arg(long = "src")]
        src_dirs: Vec<PathBuf>,

        /// Include search path for -include() directives (repeatable)
        #[arg(short = 'I', long = "include")]
        include_paths: Vec<PathBuf>,

        /// Dependency directory — scan each subdirectory as an OTP app (repeatable)
        #[arg(long = "deps-dir")]
        deps_dirs: Vec<PathBuf>,

        /// Do not include the built-in stdlib modules
        #[arg(long)]
        no_stdlib: bool,

        /// Suppress all compiler warnings
        #[arg(long)]
        no_warnings: bool,

        /// Verbose output
        #[arg(short, long)]
        verbose: bool,
    },

    /// Start the Language Server Protocol server for editor integration
    Lsp {
        /// Transport mechanism (currently only "stdio" is supported)
        #[arg(long, default_value = "stdio")]
        transport: String,
    },

    /// Run project tests (test/0 or configured EUnit)
    Test {
        /// Filter tests by name pattern (e.g., guard_*, arith_*)
        #[arg(value_name = "PATTERN")]
        patterns: Vec<String>,

        /// List available tests without running them
        #[arg(long)]
        list: bool,

        /// Verbose output (show compile errors in detail)
        #[arg(short, long)]
        verbose: bool,

        /// Do not include the built-in stdlib modules
        #[arg(long)]
        no_stdlib: bool,

        /// Suppress all compiler warnings
        #[arg(long)]
        no_warnings: bool,

        /// Timeout per legacy test or entire EUnit suite in seconds
        #[arg(long, default_value = "10")]
        timeout: u64,

        /// Number of parallel legacy test threads (EUnit uses its own scheduling)
        #[arg(short = 'j', long, default_value_t = default_jobs())]
        jobs: usize,
    },

    /// Format Erlang source files
    Fmt {
        /// Input .erl/.app/.app.src files (if empty, discovers files in src/)
        files: Vec<PathBuf>,

        /// Check formatting without modifying files (exit 1 if changes needed)
        #[arg(long)]
        check: bool,

        /// Maximum line width
        #[arg(long, default_value = "100")]
        width: usize,

        /// Indentation size in spaces
        #[arg(long, default_value = "4")]
        indent: usize,

        /// Print formatted output to stdout instead of modifying files
        #[arg(long)]
        stdout: bool,
    },

    /// Show the Nera compiler version and build information
    Version,
}

fn default_jobs() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

#[derive(Subcommand)]
enum DepsAction {
    /// Fetch all dependencies declared in nera.toml
    Get {
        /// Verbose output
        #[arg(short, long)]
        verbose: bool,
    },
    /// Add a hex.pm dependency to nera.toml and fetch it
    Add {
        /// Package name on hex.pm
        name: String,
        /// Version (queries hex.pm for latest if omitted)
        version: Option<String>,
        /// Verbose output
        #[arg(short, long)]
        verbose: bool,
    },
}

// ── Project config (nera.toml) ───────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct NeraToml {
    pub(crate) project: ProjectConfig,
    #[serde(default)]
    pub(crate) test: Option<eunit_runner::TestConfig>,
    #[serde(default)]
    deps: Option<HashMap<String, deps::DepSpec>>,
    #[serde(default)]
    pub(crate) otp: Option<OtpConfig>,
}

/// Optional `[otp]` section in `nera.toml` for configuring OTP paths.
#[derive(Deserialize, Default)]
pub(crate) struct OtpConfig {
    /// Explicit OTP root directory (auto-detected if omitted).
    pub(crate) root: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct ProjectConfig {
    #[allow(dead_code)]
    name: String,
    entry: Option<String>,
    output: Option<String>,
    pub(crate) src: Option<Vec<String>>,
    pub(crate) include: Option<Vec<String>>,
    /// Report unavailable remote targets as runtime undef instead of build errors.
    #[serde(default)]
    defer_undefined: bool,
}

/// Walk up from `start_dir` (or CWD if `None`) to find `nera.toml`.
pub(crate) fn find_project_toml(start_dir: Option<&Path>) -> Option<PathBuf> {
    let mut dir = match start_dir {
        Some(d) => d.to_path_buf(),
        None => std::env::current_dir().ok()?,
    };
    loop {
        let candidate = dir.join("nera.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Read and parse `nera.toml` from `path`.
pub(crate) fn load_project_toml(path: &Path) -> Result<NeraToml, Box<dyn std::error::Error>> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
    let config: NeraToml = toml::from_str(&contents)
        .map_err(|e| format!("invalid {}: {}", path.display(), e))?;
    Ok(config)
}

// ── Build options (unified from CLI flags + nera.toml) ───────────────

pub(crate) struct BuildOpts {
    files: Vec<PathBuf>,
    src_dirs: Vec<PathBuf>,
    include_paths: Vec<PathBuf>,
    deps_dirs: Vec<PathBuf>,
    /// Conventional `tests/` folder compiled with the sources when it exists.
    /// `None` when the command line names the sources without `--src`.
    tests_dir: Option<PathBuf>,
    output: PathBuf,
    entry: String,
    emit_ir: bool,
    emit_obj: bool,
    no_stdlib: bool,
    no_warnings: bool,
    defer_undefined: bool,
    target: Option<Triple>,
    linker: Option<String>,
    verbose: bool,
    /// Explicit OTP root directory (from `nera.toml` `[otp] root`).
    otp_root: Option<PathBuf>,
}

impl BuildOpts {
    /// Resolve build options from CLI flags, falling back to `nera.toml` defaults.
    ///
    /// When `project_root` is provided (e.g. from the LSP), `nera.toml` is
    /// searched from that directory instead of relying on CWD, and its
    /// `tests/` folder joins the build.
    #[expect(clippy::too_many_arguments, reason = "Mirrors independently configurable CLI build options")]
    pub(crate) fn resolve(
        files: Vec<PathBuf>,
        src_dirs: Vec<PathBuf>,
        include_paths: Vec<PathBuf>,
        deps_dirs: Vec<PathBuf>,
        output: Option<PathBuf>,
        entry: Option<String>,
        emit_ir: bool,
        emit_obj: bool,
        no_stdlib: bool,
        no_warnings: bool,
        target: Option<String>,
        linker: Option<String>,
        verbose: bool,
        project_root: Option<&Path>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Try loading nera.toml for defaults
        let toml_cfg = find_project_toml(project_root).and_then(|p| {
            let root = p.parent().unwrap().to_path_buf();
            load_project_toml(&p).ok().map(|cfg| (root, cfg))
        });

        let has_explicit_sources = !files.is_empty() || !src_dirs.is_empty() || !deps_dirs.is_empty();

        // Resolve src_dirs: CLI > nera.toml > convention ("src" if it exists)
        let src_dirs = if !src_dirs.is_empty() {
            src_dirs
        } else if !has_explicit_sources {
            if let Some((ref root, ref cfg)) = toml_cfg {
                if let Some(ref dirs) = cfg.project.src {
                    dirs.iter().map(|d| root.join(d)).collect()
                } else {
                    let src = root.join("src");
                    if src.is_dir() { vec![src] } else { vec![] }
                }
            } else {
                let src = PathBuf::from("src");
                if src.is_dir() { vec![src] } else { vec![] }
            }
        } else {
            vec![]
        };

        // Resolve the tests/ convention: beside the caller's project root,
        // else beside the first source directory, else in CWD. Files or
        // --deps-dir given without --src are the whole build: CWD's tests/
        // stays out of it.
        let tests_dir = if project_root.is_none() && src_dirs.is_empty() && has_explicit_sources {
            None
        } else {
            let root = project_root
                .map(Path::to_path_buf)
                .or_else(|| src_dirs.first().and_then(|d| d.parent()).map(Path::to_path_buf))
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
            Some(root.join("tests"))
        };

        // Resolve include_paths: CLI > nera.toml > empty
        // Also auto-discover "include/" directory relative to project root.
        let mut include_paths = if !include_paths.is_empty() {
            include_paths
        } else if let Some((ref root, ref cfg)) = toml_cfg {
            if let Some(ref dirs) = cfg.project.include {
                dirs.iter().map(|d| root.join(d)).collect()
            } else {
                // Convention: add "include/" if it exists
                let inc = root.join("include");
                if inc.is_dir() { vec![inc] } else { vec![] }
            }
        } else {
            // No nera.toml — try CWD-relative convention
            let inc = PathBuf::from("include");
            if inc.is_dir() { vec![inc] } else { vec![] }
        };

        // Always ensure src dirs are in include paths — in standard Erlang
        // structure, .hrl files co-located with .erl files in src/ must always
        // be resolvable via -include("file.hrl").
        for dir in &src_dirs {
            if !include_paths.contains(dir) {
                include_paths.push(dir.clone());
            }
        }

        // Resolve output: CLI > nera.toml > "a.out"
        let output = output.unwrap_or_else(|| {
            if let Some((_, ref cfg)) = toml_cfg {
                if let Some(ref o) = cfg.project.output {
                    return PathBuf::from(o);
                }
            }
            PathBuf::from("a.out")
        });

        // Resolve entry: CLI > nera.toml > "main:start"
        let entry = entry.unwrap_or_else(|| {
            if let Some((_, ref cfg)) = toml_cfg {
                if let Some(ref e) = cfg.project.entry {
                    return e.clone();
                }
            }
            "main:start".to_string()
        });

        // Parse --target triple
        let target = target.map(|s| {
            s.parse::<Triple>()
                .map_err(|e| format!("invalid target triple '{}': {}", s, e))
        }).transpose()?;

        // Append .exe for Windows targets if not already present
        let output = if let Some(ref t) = target {
            if t.operating_system == target_lexicon::OperatingSystem::Windows
                && !output.to_string_lossy().ends_with(".exe")
            {
                output.with_extension("exe")
            } else {
                output
            }
        } else {
            output
        };

        // Auto-discover .nera/deps/ from nera.toml project root
        let mut deps_dirs = deps_dirs;
        if let Some((ref root, _)) = toml_cfg {
            let nera_deps = deps::deps_dir_for_project(root);
            if nera_deps.is_dir() && !deps_dirs.contains(&nera_deps) {
                deps_dirs.push(nera_deps);
            }
        }

        // Resolve OTP root from nera.toml [otp] section
        let otp_root = toml_cfg.as_ref().and_then(|(root, cfg)| {
            cfg.otp.as_ref()?.root.as_ref().map(|r| root.join(r))
        });

        Ok(BuildOpts {
            files,
            src_dirs,
            include_paths,
            deps_dirs,
            tests_dir,
            output,
            entry,
            emit_ir,
            emit_obj,
            no_stdlib,
            no_warnings,
            target,
            linker,
            verbose,
            otp_root,
            defer_undefined: toml_cfg.as_ref().is_some_and(|(_, cfg)| cfg.project.defer_undefined),
        })
    }
}

// ── Entry point ──────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Command::New { name } => cmd_new(&name),
        Command::Init => deps::cmd_init(),
        Command::Deps { action } => cmd_deps(action),
        Command::Build {
            files, src_dirs, include_paths, deps_dirs,
            output, entry, emit_ir, emit_obj, no_stdlib, no_warnings,
            target, linker, verbose,
        } => {
            let opts = BuildOpts::resolve(
                files, src_dirs, include_paths, deps_dirs,
                output, entry, emit_ir, emit_obj, no_stdlib, no_warnings,
                target, linker, verbose, None,
            );
            match opts {
                Ok(opts) => cmd_build(&opts),
                Err(e) => Err(e),
            }
        }
        Command::Check {
            files, src_dirs, include_paths, deps_dirs,
            no_stdlib, no_warnings, verbose,
        } => {
            cmd_check(files, src_dirs, include_paths, deps_dirs, no_stdlib, no_warnings, verbose)
        }
        Command::Fmt { files, check, width, indent, stdout } => {
            cmd_fmt(files, check, width, indent, stdout)
        }
        Command::Lsp { transport } => cmd_lsp(&transport),
        Command::Run { verbose, no_stdlib, no_warnings, args } => cmd_run(verbose, no_stdlib, no_warnings, &args),
        Command::Test { patterns, list, verbose, no_stdlib, no_warnings, timeout, jobs } => {
            cmd_test(patterns, list, verbose, no_stdlib, no_warnings, timeout, jobs)
        }
        Command::Version => cmd_version(),
    };

    if let Err(e) = result {
        let msg = e.to_string();
        if !msg.is_empty() {
            eprintln!("nera: error: {}", msg);
        }
        std::process::exit(1);
    }
}

// ── nera version ─────────────────────────────────────────────────────

fn cmd_version() -> Result<(), Box<dyn std::error::Error>> {
    println!("nera {} (built {})", env!("CARGO_PKG_VERSION"), env!("NERA_BUILD_DATE"));
    Ok(())
}

/// Return the current UTC date as "YYYY-MM-DD" without external crates.
fn format_utc_date() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    // Civil date from Unix timestamp (Howard Hinnant's algorithm)
    let days = secs.div_euclid(86400) + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", y, m, d)
}

// ── nera new ─────────────────────────────────────────────────────────

fn cmd_new(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Validate project name (must be a valid Erlang module name)
    if name.is_empty() {
        return Err("project name cannot be empty".into());
    }
    if !name.chars().next().unwrap().is_ascii_lowercase() {
        return Err("project name must start with a lowercase letter (Erlang module convention)".into());
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err("project name must contain only letters, digits, and underscores".into());
    }

    let project_dir = PathBuf::from(name);
    if project_dir.exists() {
        return Err(format!("directory '{}' already exists", name).into());
    }

    // Create directory structure
    let src_dir = project_dir.join("src");
    std::fs::create_dir_all(&src_dir)
        .map_err(|e| format!("cannot create {}: {}", src_dir.display(), e))?;

    // Write nera.toml
    let toml_content = format!(
        r#"[project]
name = "{name}"
entry = "{name}:start"
output = "{name}"
"#
    );
    std::fs::write(project_dir.join("nera.toml"), toml_content)?;

    // Write src/<name>.erl
    let erl_content = format!(
        r#"-module({name}).
-export([start/0]).

start() ->
    io:format("Hello from {name}!~n").
"#
    );
    std::fs::write(src_dir.join(format!("{}.erl", name)), erl_content)?;

    // Write .gitignore
    let gitignore = format!(
        "/{name}
*.o
*.beam
.nera/
"
    );
    std::fs::write(project_dir.join(".gitignore"), gitignore)?;

    // Initialize git repository
    let git_status = std::process::Command::new("git")
        .args(["init", name])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    match git_status {
        Ok(status) if status.success() => {
            eprintln!("Created project '{}' with git repository", name);
        }
        Ok(_) => {
            eprintln!("Created project '{}' (git init failed — is git installed?)", name);
            return Err("git init failed".into());
        }
        Err(_) => {
            eprintln!("Created project '{}' (git not found — install git to enable version control)", name);
            return Err("git is not installed".into());
        }
    }

    eprintln!();
    eprintln!("  cd {}", name);
    eprintln!("  nera build");
    eprintln!("  nera run");

    Ok(())
}

// ── nera deps ─────────────────────────────────────────────────────────

fn cmd_deps(action: DepsAction) -> Result<(), Box<dyn std::error::Error>> {
    let toml_path = find_project_toml(None)
        .ok_or("nera.toml not found — run 'nera init' or 'nera new' first")?;
    let project_root = toml_path.parent().unwrap().to_path_buf();
    let config = load_project_toml(&toml_path)?;

    match action {
        DepsAction::Get { verbose } => {
            let deps = config.deps.unwrap_or_default();
            deps::cmd_deps_get(&deps, &project_root, verbose)
        }
        DepsAction::Add { name, version, verbose } => {
            deps::cmd_deps_add(&name, version.as_deref(), &project_root, verbose)
        }
    }
}

// ── nera lsp ────────────────────────────────────────────────────────

fn cmd_lsp(transport: &str) -> Result<(), Box<dyn std::error::Error>> {
    if transport != "stdio" {
        return Err(format!("unsupported transport '{}'; only 'stdio' is supported", transport).into());
    }
    lsp::run_lsp()
}

// ── nera check ──────────────────────────────────────────────────────

fn cmd_check(
    files: Vec<PathBuf>,
    src_dirs: Vec<PathBuf>,
    include_paths: Vec<PathBuf>,
    deps_dirs: Vec<PathBuf>,
    no_stdlib: bool,
    no_warnings: bool,
    verbose: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let opts = BuildOpts::resolve(
        files, src_dirs, include_paths, deps_dirs,
        None, None, false, false, no_stdlib, no_warnings,
        None, None, verbose, None,
    )?;
    let result = check(&opts)?;
    if result.warning_count > 0 {
        eprintln!("All checks passed ({}).", make_summary(0, result.warning_count));
    } else {
        eprintln!("All checks passed.");
    }
    Ok(())
}

// ── nera fmt ────────────────────────────────────────────────────────

fn cmd_fmt(
    files: Vec<PathBuf>,
    check: bool,
    width: usize,
    indent: usize,
    stdout: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = nera_fmt::config::FormatConfig {
        max_width: width,
        indent_size: indent,
    };

    // Discover files if none given
    let file_list = if files.is_empty() {
        discover_erl_files()?
    } else {
        files
    };

    if file_list.is_empty() {
        eprintln!("nera fmt: no formattable files found");
        return Ok(());
    }

    let mut any_changed = false;

    for path in &file_list {
        let source = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;

        let formatted = if is_app_file(path) {
            nera_fmt::format_term_source(&source, &config)
        } else {
            nera_fmt::format_source(&source, &config)
        }
        .map_err(|e| format!("{}: {}", path.display(), e))?;

        if formatted != source {
            any_changed = true;
            if check {
                eprintln!("{}: needs formatting", path.display());
            } else if stdout {
                print!("{}", formatted);
            } else {
                std::fs::write(path, &formatted)
                    .map_err(|e| format!("cannot write {}: {}", path.display(), e))?;
                eprintln!("formatted {}", path.display());
            }
        }
    }

    if check && any_changed {
        std::process::exit(1);
    }

    Ok(())
}

/// Discover .erl files in the project src/ directory.
fn discover_erl_files() -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut files = Vec::new();

    // Try project root from nera.toml
    let src_dir = if let Some(toml_path) = find_project_toml(None) {
        let project_root = toml_path.parent().unwrap().to_path_buf();
        let toml = load_project_toml(&toml_path)?;
        if let Some(src_dirs) = &toml.project.src {
            for dir in src_dirs {
                let dir_path = project_root.join(dir);
                if dir_path.is_dir() {
                    collect_erl_files(&dir_path, &mut files);
                }
            }
            return Ok(files);
        }
        project_root.join("src")
    } else {
        PathBuf::from("src")
    };

    if src_dir.is_dir() {
        collect_erl_files(&src_dir, &mut files);
    }
    Ok(files)
}

fn collect_erl_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_erl_files(&path, out);
            } else if is_formattable_file(&path) {
                out.push(path);
            }
        }
    }
}

/// Check if a file is a formattable source file (.erl, .app, .app.src).
fn is_formattable_file(path: &Path) -> bool {
    if path.extension().is_some_and(|e| e == "erl" || e == "app") {
        return true;
    }
    is_app_src_file(path)
}

/// Check if a file is an `.app` or `.app.src` file (term file).
fn is_app_file(path: &Path) -> bool {
    if path.extension().is_some_and(|e| e == "app") {
        return true;
    }
    is_app_src_file(path)
}

/// Check if a file has the `.app.src` double extension.
fn is_app_src_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with(".app.src"))
}

// ── nera build ───────────────────────────────────────────────────────

fn cmd_build(opts: &BuildOpts) -> Result<(), Box<dyn std::error::Error>> {
    compile(opts)
}

// ── nera run ─────────────────────────────────────────────────────────

fn cmd_run(verbose: bool, no_stdlib: bool, no_warnings: bool, args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let opts = BuildOpts::resolve(
        vec![], vec![], vec![], vec![],
        None, None, false, false, no_stdlib, no_warnings,
        None, None, verbose, None,
    )?;

    compile(&opts)?;

    // Execute the compiled binary
    let binary = &opts.output;
    if !binary.exists() {
        return Err(format!("binary '{}' not found after build", binary.display()).into());
    }

    // Make path absolute for execution
    let binary_path = if binary.is_absolute() {
        binary.clone()
    } else {
        std::env::current_dir()?.join(binary)
    };

    if opts.verbose {
        eprintln!("Running {}...", binary_path.display());
    }

    let status = std::process::Command::new(&binary_path)
        .args(args)
        .status()
        .map_err(|e| format!("cannot execute {}: {}", binary_path.display(), e))?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}

// ── nera test ────────────────────────────────────────────────────────

fn cmd_test(
    patterns: Vec<String>,
    list: bool,
    verbose: bool,
    no_stdlib: bool,
    no_warnings: bool,
    timeout: u64,
    jobs: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    test_runner::run_tests(&test_runner::TestOpts {
        patterns,
        list,
        verbose,
        no_stdlib,
        no_warnings,
        timeout,
        jobs,
    })
}

// ── Compilation pipeline ─────────────────────────────────────────────

/// Recursively collect all `.erl` files under `dir`.
pub(crate) fn glob_erl_files(dir: &Path) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut result = Vec::new();
    if !dir.is_dir() {
        return Err(format!("not a directory: {}", dir.display()).into());
    }
    glob_erl_recursive(dir, &mut result)?;
    result.sort();
    Ok(result)
}

fn glob_erl_recursive(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    for entry in std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read directory {}: {}", dir.display(), e))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            glob_erl_recursive(&path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("erl") {
            out.push(path);
        }
    }
    Ok(())
}

/// Retain disposable private storage through the link, including error paths.
struct ExtractedRuntime {
    path: PathBuf,
    _cache: runtime_cache::RuntimeCache,
}

/// Extract the embedded compressed runtime into private storage.
///
/// Keys the cache by the embedded gzip bytes, not the compiler version or
/// build date: same-day builds can contain different runtimes. Returns `None`
/// if no runtime was embedded or extraction fails.
fn extract_embedded_runtime() -> Option<ExtractedRuntime> {
    use flate2::read::GzDecoder;
    use sha2::{Digest, Sha256};

    let compressed = EMBEDDED_RUNTIME?;
    let filename = format!("libnera_runtime-{:x}.a", Sha256::digest(compressed));
    let base = std::env::temp_dir();
    let mut cache = match runtime_cache::RuntimeCache::new(&base) {
        Ok(cache) => cache,
        Err(err) => {
            eprintln!("Warning: failed to create private embedded runtime cache: {}", err);
            return None;
        }
    };
    let mut path = cache.path.join(&filename);

    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if runtime_cache::reusable_archive(&metadata) => {
            return Some(ExtractedRuntime { path, _cache: cache });
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        _ => {
            // Never repair or follow an unexpected entry. Use independent
            // private storage so an obstructed cache cannot deny the build.
            cache = match cache.temporary() {
                Ok(cache) => cache,
                Err(err) => {
                    eprintln!("Warning: failed to create private embedded runtime cache: {}", err);
                    return None;
                }
            };
            path = cache.path.join(filename);
        }
    }

    // Each concurrent compiler gets its own owner-only staging file, on the
    // same filesystem as the cache. Never expose a partially written archive.
    let mut staging = match tempfile::Builder::new().prefix("nera-runtime-").tempfile_in(&cache.path) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("Warning: failed to create embedded runtime temporary file: {}", err);
            return None;
        }
    };
    let mut decoder = GzDecoder::new(compressed);
    if let Err(err) = std::io::copy(&mut decoder, &mut staging) {
        eprintln!("Warning: failed to extract embedded runtime library: {}", err);
        return None;
    }
    let path = publish_embedded_runtime(staging, path)?;
    Some(ExtractedRuntime { path, _cache: cache })
}

fn publish_embedded_runtime(staging: tempfile::NamedTempFile, path: PathBuf) -> Option<PathBuf> {
    if let Err(err) = staging.persist(&path) {
        // A peer may have published this same content while we extracted it.
        // Windows can reject replacing that file while a linker has it open.
        if std::fs::symlink_metadata(&path)
            .is_ok_and(|metadata| runtime_cache::reusable_archive(&metadata))
        {
            return Some(path);
        }
        eprintln!("Warning: failed to publish embedded runtime to {}: {}", path.display(), err);
        return None;
    }

    Some(path)
}

/// Find the stdlib directory containing built-in Erlang modules (e.g. lists.erl).
///
/// Search strategy (mirrors `Linker::find_runtime_lib()`):
/// 1. `<exe_dir>/../lib/nera/erlang/stdlib/` — installed distribution layout
/// 2. `<exe_dir>/../../erlang/stdlib/` — workspace layout (`target/release/nera` → `erlang/stdlib/`)
/// 3. `erlang/stdlib/` relative to CWD — development fallback
pub(crate) fn find_stdlib_dir() -> Option<PathBuf> {
    // Strategy 1 & 2: relative to the executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            // Installed layout: <prefix>/bin/nera → <prefix>/lib/nera/erlang/stdlib/
            let installed = exe_dir.join("../lib/nera/erlang/stdlib");
            if installed.is_dir() {
                return Some(installed);
            }
            // Workspace layout: target/release/nera → ../../erlang/stdlib/
            let workspace = exe_dir.join("../../erlang/stdlib");
            if workspace.is_dir() {
                return Some(workspace);
            }
        }
    }

    // Strategy 3: CWD fallback (for running from workspace root)
    let cwd = PathBuf::from("erlang/stdlib");
    if cwd.is_dir() {
        return Some(cwd);
    }

    None
}

type DiscoveredFiles = (Vec<PathBuf>, Vec<PathBuf>, app_registry::AppRegistry);

/// Discover all `.erl` source files, include paths, and build the application
/// registry for `-include_lib` resolution.
///
/// Returns `(erl_files, include_paths, app_registry)`.
pub(crate) fn discover_files(opts: &BuildOpts) -> Result<DiscoveredFiles, Box<dyn std::error::Error>> {
    let mut erl_files: Vec<PathBuf> = opts.files.clone();
    let mut include_paths: Vec<PathBuf> = opts.include_paths.clone();

    // --src dirs: recursively find .erl files and add them as include paths
    for dir in &opts.src_dirs {
        let found = glob_erl_files(dir)?;
        if opts.verbose {
            eprintln!("--src {}: found {} .erl files", dir.display(), found.len());
        }
        erl_files.extend(found);
        // Add source directories to include paths so that .hrl files
        // co-located with .erl files are found by -include("file.hrl").
        if !include_paths.contains(dir) {
            include_paths.push(dir.clone());
        }
    }

    // Conventional tests/ directory (see BuildOpts::resolve): include its
    // .erl files and add it to include paths.
    if let Some(tests_dir) = opts.tests_dir.as_ref().filter(|d| d.is_dir()) {
        let found = glob_erl_files(tests_dir)?;
        if opts.verbose {
            eprintln!("tests {}: found {} .erl files", tests_dir.display(), found.len());
        }
        erl_files.extend(found);
        if !include_paths.contains(tests_dir) {
            include_paths.push(tests_dir.clone());
        }
    }

    // --deps-dir: scan each subdirectory as an OTP application
    for deps_dir in &opts.deps_dirs {
        if !deps_dir.is_dir() {
            return Err(format!("deps directory does not exist: {}", deps_dir.display()).into());
        }
        let mut entries: Vec<_> = std::fs::read_dir(deps_dir)
            .map_err(|e| format!("cannot read deps directory {}: {}", deps_dir.display(), e))?
            .filter_map(|e| e.ok())
            .collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let app_dir = entry.path();
            if !app_dir.is_dir() {
                continue;
            }
            let app_src = app_dir.join("src");
            let app_include = app_dir.join("include");

            if app_src.is_dir() {
                let found = glob_erl_files(&app_src)?;
                if opts.verbose {
                    eprintln!(
                        "  dep {}: found {} .erl files",
                        app_dir.file_name().unwrap_or_default().to_string_lossy(),
                        found.len()
                    );
                }
                erl_files.extend(found);
            }
            if app_include.is_dir() {
                include_paths.push(app_include);
            }
        }
    }

    // Deduplicate files (same canonical path)
    erl_files.sort();
    erl_files.dedup();

    if erl_files.is_empty() {
        return Err("no .erl source files found (use positional args, --src, or --deps-dir)".into());
    }

    // Build the application registry for -include_lib resolution.
    // Detect OTP root from config, env, or probing.
    let otp_root = otp::find_otp_root(opts.otp_root.as_deref());
    if opts.verbose {
        match &otp_root {
            Some(root) => eprintln!("OTP root: {}", root.display()),
            None => eprintln!("OTP root: not found (include_lib for OTP apps will fail)"),
        }
    }
    let registry = app_registry::AppRegistry::build(
        otp_root.as_deref(),
        &opts.deps_dirs,
        &opts.src_dirs,
    );
    if opts.verbose {
        eprintln!("Application registry: {} apps", registry.len());
    }

    Ok((erl_files, include_paths, registry))
}

/// Format a summary line like "2 error(s), 3 warning(s) emitted".
fn make_summary(errors: usize, warnings: usize) -> String {
    let mut parts = Vec::new();
    if errors > 0 {
        parts.push(format!("{} error{}", errors, if errors == 1 { "" } else { "s" }));
    }
    if warnings > 0 {
        parts.push(format!("{} warning{}", warnings, if warnings == 1 { "" } else { "s" }));
    }
    if parts.is_empty() {
        return String::new();
    }
    format!("{} emitted", parts.join(", "))
}

/// Result of the check (parse + lower + validate) pipeline, used by compile() to continue.
struct CheckResult {
    ir_modules: Vec<nera_ir::ir::IrModule>,
    atom_table: HashMap<u32, String>,
    #[allow(dead_code)]
    atom_names: HashMap<nera_syntax::ast::AtomId, String>,
    #[allow(dead_code)]
    parsed_modules: Vec<(String, bool, nera_syntax::ast::Module, MultiSourceMap)>,
    warning_count: usize,
}

fn check(opts: &BuildOpts) -> Result<CheckResult, Box<dyn std::error::Error>> {
    use string_interner::{DefaultStringInterner, Symbol};
    use nera_syntax::ast::AtomId;

    let mut total_warnings: usize = 0;

    // Discover source files, include paths, and build app registry
    let (erl_files, include_paths, app_registry) = discover_files(opts)?;
    let include_lib_resolver = std::sync::Arc::new(app_registry.make_resolver());
    let otp_release = app_registry.otp_release();

    // Convert to SourceInput
    let mut sources: Vec<SourceInput> = erl_files.into_iter()
        .map(SourceInput::File)
        .collect();

    // Track whether we're using embedded stdlib (for warning suppression)
    let mut using_embedded_stdlib = false;

    // Auto-include stdlib modules unless --no-stdlib is set.
    // Stdlib files are prepended so they are compiled first and available
    // for cross-module direct linking from user modules.
    if !opts.no_stdlib {
        // Collect module names already provided by user to avoid conflicts
        let user_modules: std::collections::HashSet<String> = sources.iter()
            .map(|s| s.module_name())
            .collect();

        if let Some(stdlib_dir) = find_stdlib_dir() {
            // Filesystem stdlib takes priority (allows overrides)
            let stdlib_files = glob_erl_files(&stdlib_dir).unwrap_or_default();
            if opts.verbose {
                eprintln!("stdlib {}: found {} modules", stdlib_dir.display(), stdlib_files.len());
            }
            let mut to_prepend = Vec::new();
            for sf in stdlib_files {
                let mod_name = sf.file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                if user_modules.contains(&mod_name) {
                    if opts.verbose {
                        eprintln!("  stdlib {} shadowed by user module", mod_name);
                    }
                } else {
                    to_prepend.push(SourceInput::File(sf));
                }
            }
            to_prepend.extend(sources);
            sources = to_prepend;
        } else {
            // Fallback: use embedded stdlib sources
            if opts.verbose {
                eprintln!("stdlib: using {} embedded modules", EMBEDDED_STDLIB.len());
            }
            using_embedded_stdlib = true;
            let mut to_prepend = Vec::new();
            for &(name, source) in EMBEDDED_STDLIB {
                if user_modules.contains(name) {
                    if opts.verbose {
                        eprintln!("  stdlib {} shadowed by user module", name);
                    }
                } else {
                    to_prepend.push(SourceInput::Embedded {
                        name: name.to_string(),
                        source,
                    });
                }
            }
            to_prepend.extend(sources);
            sources = to_prepend;
        }
    }

    if opts.verbose {
        eprintln!("Checking {} source files", sources.len());
        if !include_paths.is_empty() {
            eprintln!("Include paths: {:?}", include_paths);
        }
    }

    let mut ir_modules = Vec::new();
    let mut atom_table: HashMap<u32, String> = HashMap::new();

    // Phase 1: Parse all input files with a SHARED interner so that
    // atom IDs are globally unique across all modules.
    let mut shared_interner = DefaultStringInterner::default();
    let mut parsed_modules = Vec::new();
    let mut module_macro_data: Vec<(Vec<nera_syntax::preproc::MacroLocation>, std::collections::HashSet<String>)> = Vec::new();

    for input in &sources {
        if opts.verbose {
            eprintln!("Parsing {}...", input.display_path());
        }

        let source = input.read_source()?;

        let source_map = SourceMap::with_filename(&source, input.display_path());

        // Lex
        let tokens = Lexer::lex_all(&source).map_err(|errs| {
            let n = emit_parse_errors(&errs, &source_map);
            make_summary(n, total_warnings)
        })?;

        // Preprocess (expand macros, conditionals, includes)
        let preproc_path = match input.as_path() {
            Some(p) => p.to_path_buf(),
            None => PathBuf::from(input.display_path()),
        };
        let mut preproc = Preprocessor::new(&source, &preproc_path);
        if let Some(release) = otp_release {
            preproc.set_otp_release(release);
        }
        if !include_paths.is_empty() {
            preproc.set_include_paths(include_paths.clone());
        }
        {
            let resolver = include_lib_resolver.clone();
            preproc.set_include_lib_resolver(Box::new(move |path| resolver(path)));
        }
        let processed_tokens = preproc.process(tokens).map_err(|errs| {
            let n = emit_parse_errors(&errs, &source_map);
            make_summary(n, total_warnings)
        })?;

        // Use the combined source (main + included files) so that all token
        // spans are valid — included tokens have their spans offset to index
        // into this combined buffer.  Build a MultiSourceMap for accurate
        // error reporting across included files.
        let (combined_source, segments, macro_locs, used_macros) = preproc.take_all();
        let multi_source_map = MultiSourceMap::new(combined_source.clone(), segments);

        if opts.verbose {
            eprintln!("  Preprocessed ({} tokens)", processed_tokens.len());
        }

        // Parse (with pre-lexed, preprocessed tokens)
        let mut parser = Parser::with_tokens(&combined_source, processed_tokens, shared_interner);
        let parse_result = parser.parse_module();
        if parse_result.has_errors() {
            let n = emit_parse_errors_multi(&parse_result.errors, &multi_source_map);
            return Err(make_summary(n, total_warnings).into());
        }
        let mut module = parse_result.module;

        if opts.verbose {
            eprintln!(
                "  Parsed module with {} functions",
                module.functions.len()
            );
        }

        // Take back the interner for the next parser
        shared_interner = parser.into_interner();
        module_info::add_exports_info(&mut module, &mut shared_interner)?;
        parsed_modules.push((input.display_path(), input.is_embedded(), module, multi_source_map));
        module_macro_data.push((macro_locs, used_macros));
    }

    // Build the atom table from the shared interner
    let mut true_atom_id = None;
    let mut false_atom_id = None;
    let mut self_atom_id = None;
    for (sym, s) in shared_interner.into_iter() {
        let id = sym.to_usize() as u32;
        if s == "true" {
            true_atom_id = Some(id);
        } else if s == "false" {
            false_atom_id = Some(id);
        } else if s == "self" {
            self_atom_id = Some(id);
        }
        atom_table.entry(id).or_insert_with(|| s.to_string());
    }

    // Ensure true, false, and self have known atom IDs.
    let next_id = atom_table.keys().max().map(|m| m + 1).unwrap_or(0);
    let true_id = true_atom_id.unwrap_or_else(|| {
        atom_table.insert(next_id, "true".to_string());
        next_id
    });
    let false_id = false_atom_id.unwrap_or_else(|| {
        let id = if true_atom_id.is_none() { next_id + 1 } else { next_id };
        atom_table.insert(id, "false".to_string());
        id
    });
    let self_id = self_atom_id.unwrap_or_else(|| {
        let id = atom_table.keys().max().map(|m| m + 1).unwrap_or(0);
        atom_table.insert(id, "self".to_string());
        id
    });

    // Ensure well-known module atoms are in the atom table (needed for auto-import)
    {
        let mut next = atom_table.keys().max().map(|m| m + 1).unwrap_or(0);
        for required in &["erlang", "io", "maps", "gen_tcp", "lists"] {
            if !atom_table.values().any(|v| v == required) {
                atom_table.insert(next, required.to_string());
                next += 1;
            }
        }
    }

    // Build atom name map for IR lowering (AtomId -> String)
    let mut atom_names: std::collections::HashMap<AtomId, String> = atom_table
        .iter()
        .map(|(id, name)| (AtomId(*id), name.clone()))
        .collect();

    // Resolve stdlib directory path for skipping warnings on stdlib modules
    let stdlib_dir_canonical = if !opts.no_stdlib && !using_embedded_stdlib {
        find_stdlib_dir().and_then(|d| d.canonicalize().ok())
    } else {
        None
    };

    // Phase 2: Lower all modules to IR
    //
    // Cross-module unused_record detection: records defined in .hrl files may
    // appear unused in one module but used in another that includes the same
    // header.  Collect record usage across all modules and defer .hrl warnings.
    use cross_module_warnings as cmw;

    let globally_used_records = if opts.no_warnings {
        std::collections::HashSet::new()
    } else {
        cmw::collect_global_record_usage(
            &parsed_modules, &atom_names, stdlib_dir_canonical.as_deref(),
        )
    };
    let mut all_deferred: Vec<cmw::DeferredRecordWarning> = Vec::new();

    // Cross-module unused_macro detection: same pattern as records.
    let globally_used_macros = if opts.no_warnings {
        std::collections::HashSet::new()
    } else {
        let macro_usage_data: Vec<(std::collections::HashSet<String>, bool, &str)> = parsed_modules
            .iter()
            .zip(module_macro_data.iter())
            .map(|((dp, emb, _, _), (_, used))| (used.clone(), *emb, dp.as_str()))
            .collect();
        cmw::collect_global_macro_usage(&macro_usage_data, stdlib_dir_canonical.as_deref())
    };
    let mut all_deferred_macros: Vec<cmw::DeferredMacroWarning> = Vec::new();

    for (mod_idx, (display_path, is_embedded, module, multi_sm)) in parsed_modules.iter().enumerate() {
        // Semantic errors share the AST diagnostics pass with warnings.
        // --no-warnings must suppress warnings only, never permit invalid code.
        let is_stdlib = cmw::is_stdlib_module(
            display_path, *is_embedded, stdlib_dir_canonical.as_deref(),
        );
        if !is_stdlib {
            let result = cmw::analyze_module_warnings(
                module, &atom_names, display_path, multi_sm, mod_idx, None,
            );
            for diagnostic in &result.immediate {
                if !opts.no_warnings || diagnostic.severity == nera_syntax::error::Severity::Error {
                    diagnostic.emit_multi(multi_sm);
                }
            }
            if !opts.no_warnings {
                total_warnings += result.warning_count;
            }
            if result.error_count > 0 {
                return Err(make_summary(result.error_count, total_warnings).into());
            }
            if !opts.no_warnings {
                all_deferred.extend(result.deferred);
                // Unused macro analysis.
                let (macro_locs, used_macros) = &module_macro_data[mod_idx];
                let (macro_immediate, macro_deferred) = cmw::analyze_module_macro_warnings(
                    macro_locs, used_macros, multi_sm, mod_idx,
                );
                for w in &macro_immediate {
                    w.emit_multi(multi_sm);
                    total_warnings += 1;
                }
                all_deferred_macros.extend(macro_deferred);
            }
        }

        // Build a simple SourceMap from the combined source for IR lowering
        // (SetLine instructions only need line numbers, not per-file tracking).
        let combined_sm = multi_sm.to_source_map(display_path);
        let (ir_module, updated_atoms) = IrBuilder::lower_module(module, AtomId(true_id), AtomId(false_id), AtomId(self_id), atom_names.clone(), Some(combined_sm))
            .map_err(|e| {
                let diag = match e.span() {
                    Some(span) => Diagnostic::error_at(e.to_string(), span),
                    None => Diagnostic::error(format!("{}: {}", display_path, e)),
                };
                diag.emit_multi(multi_sm);
                make_summary(1, total_warnings)
            })?;

        // Merge any new atoms created during lowering (e.g. case_clause, if_clause)
        for (id, name) in &updated_atoms {
            atom_table.entry(id.0).or_insert_with(|| name.clone());
            atom_names.entry(*id).or_insert_with(|| name.clone());
        }

        if opts.verbose {
            eprintln!(
                "  Lowered to {} IR functions",
                ir_module.functions.len()
            );
        }

        ir_modules.push(ir_module);
    }

    // Emit deferred .hrl unused_record warnings for records not used in any module.
    if !opts.no_warnings {
        for deferred in cmw::resolve_deferred_warnings(&all_deferred, &globally_used_records) {
            let (_, _, _, multi_sm) = &parsed_modules[deferred.module_idx];
            deferred.warning.emit_multi(multi_sm);
            total_warnings += 1;
        }
        // Emit deferred .hrl unused_macro warnings for macros not used in any module.
        for deferred in cmw::resolve_deferred_macro_warnings(&all_deferred_macros, &globally_used_macros) {
            let (_, _, _, multi_sm) = &parsed_modules[deferred.module_idx];
            deferred.warning.emit_multi(multi_sm);
            total_warnings += 1;
        }
    }

    // Phase 3: Validate — check for unresolved remote calls across all modules.
    // This must happen after all modules are lowered so we have the full picture.
    {
        let mut unresolved = nera_ir::validate::check_unresolved_calls(&ir_modules, &atom_names);
        if opts.defer_undefined {
            // Undefined remote targets already dispatch through runtime apply.
            // Keep private functions rejected: the runtime registry is not an export table.
            unresolved.retain(|call| call.kind != nera_ir::validate::UnresolvedKind::NotDefined);
        }
        if !unresolved.is_empty() {
            // Build module name → (AST, MultiSourceMap) lookup for rich diagnostics.
            let module_sources: HashMap<String, (&nera_syntax::ast::Module, &MultiSourceMap)> =
                parsed_modules
                    .iter()
                    .filter_map(|(_, _, module, sm)| {
                        atom_names
                            .get(&module.name)
                            .map(|name| (name.clone(), (module, sm)))
                    })
                    .collect();

            for u in &unresolved {
                use nera_ir::validate::UnresolvedKind;

                let (msg, label) = match u.kind {
                    UnresolvedKind::NotExported => {
                        let msg = if u.arity == u32::MAX {
                            format!(
                                "function `{}:{}` is not exported",
                                u.target_module, u.target_function,
                            )
                        } else {
                            format!(
                                "function `{}:{}/{}` is not exported",
                                u.target_module, u.target_function, u.arity,
                            )
                        };
                        let label = if u.arity == u32::MAX {
                            format!("add `{}` to -export([...]) in module `{}`", u.target_function, u.target_module)
                        } else {
                            format!("add `{}/{}` to -export([...]) in module `{}`", u.target_function, u.arity, u.target_module)
                        };
                        (msg, label)
                    }
                    UnresolvedKind::NotDefined => {
                        let msg = if u.arity == u32::MAX {
                            format!(
                                "unresolved function `{}:{}`",
                                u.target_module, u.target_function,
                            )
                        } else {
                            format!(
                                "unresolved function `{}:{}/{}`",
                                u.target_module, u.target_function, u.arity,
                            )
                        };
                        let label = format!("module `{}` is not available", u.target_module);
                        (msg, label)
                    }
                };

                if let Some((ast_mod, multi_sm)) = module_sources.get(&u.caller_module) {
                    if let Some(span) = nera_ir::validate::find_call_site(
                        ast_mod,
                        &atom_names,
                        &u.target_module,
                        &u.target_function,
                        u.arity,
                    ) {
                        Diagnostic::error_at(&msg, span)
                            .with_label(label)
                            .emit_multi(multi_sm);
                        continue;
                    }
                }
                // Fallback: no source context available
                eprintln!("error: {} (in module {})", msg, u.caller_module);
            }
            return Err(make_summary(unresolved.len(), total_warnings).into());
        }
    }

    Ok(CheckResult {
        ir_modules,
        atom_table,
        atom_names,
        parsed_modules,
        warning_count: total_warnings,
    })
}

fn compile(opts: &BuildOpts) -> Result<(), Box<dyn std::error::Error>> {
    let CheckResult { ir_modules, atom_table, warning_count, .. } = check(opts)?;

    // Emit IR if requested
    if opts.emit_ir {
        for ir_module in &ir_modules {
            println!("{}", ir_module);
        }
        return Ok(());
    }

    // Phase 4: Compile to native code
    let target_triple = opts.target.clone().unwrap_or_else(Triple::host);
    let is_cross = opts.target.is_some() && target_triple != Triple::host();
    if is_cross && opts.verbose {
        eprintln!("Cross-compiling for {}", target_triple);
    }
    let mut compiler = ModuleCompiler::for_target(target_triple.clone())?;
    compiler.set_atom_table(atom_table.clone());

    // Parse --entry flag (module:function)
    let entry_parts: Vec<&str> = opts.entry.split(':').collect();
    let entry_mod_fun = if entry_parts.len() == 2 {
        Some((entry_parts[0], entry_parts[1]))
    } else {
        None
    };

    let mut object_files = Vec::new();
    let total_modules = ir_modules.len();
    let all_modules_refs: Vec<&nera_ir::ir::IrModule> = ir_modules.iter().collect();

    for (idx, ir_module) in ir_modules.iter().enumerate() {
        // Use module name string for the object file, falling back to index
        let mod_name = atom_table.get(&ir_module.name.0)
            .cloned()
            .unwrap_or_else(|| format!("module_{}", idx));
        let obj_path = PathBuf::from(format!("{}.o", mod_name));
        // Generate main() in the last (or only) module
        let generate_main = idx == total_modules - 1;

        if opts.verbose {
            eprintln!("Compiling module {} to {}...", ir_module.name.0, obj_path.display());
        }

        compiler.compile_to_file(ir_module, &obj_path, generate_main, &all_modules_refs, entry_mod_fun)?;
        object_files.push(obj_path);
    }

    // Generate metadata object with compiler version and compilation date
    let meta_path = PathBuf::from("__nera_metadata.o");
    let compile_date = format_utc_date();
    let meta_bytes = compiler.generate_metadata_object(
        env!("CARGO_PKG_VERSION"),
        &compile_date,
    )?;
    std::fs::write(&meta_path, meta_bytes)?;
    object_files.push(meta_path);

    if opts.emit_obj {
        eprintln!("Object files written:");
        for obj in &object_files {
            eprintln!("  {}", obj.display());
        }
        return Ok(());
    }

    // Phase 4: Link
    if opts.verbose {
        eprintln!("Linking to {}...", opts.output.display());
    }

    let mut linker = Linker::new(opts.output.clone(), target_triple.clone());
    if let Some(ref linker_cmd) = opts.linker {
        linker.set_linker_cmd(linker_cmd.clone());
    }
    for obj in &object_files {
        linker.add_object(obj.clone());
    }

    let embedded_runtime;
    if let Some(runtime_lib) = Linker::find_runtime_lib_for_target(&target_triple) {
        linker.set_runtime_lib(runtime_lib);
    } else if is_cross {
        return Err(format!(
            "runtime library not found for target '{}'.\n\
             Build it with: cargo build -p nera-runtime --target {} --release",
            target_triple, target_triple
        ).into());
    } else if let Some(embedded) = extract_embedded_runtime() {
        embedded_runtime = embedded;
        if opts.verbose {
            eprintln!("Using embedded runtime library: {}", embedded_runtime.path.display());
        }
        linker.set_runtime_lib(embedded_runtime.path.clone());
    } else {
        eprintln!("Warning: runtime library not found. Build with 'cargo build -p nera-runtime' first.");
    }

    linker.link()?;

    // Clean up object files
    for obj in &object_files {
        let _ = std::fs::remove_file(obj);
    }

    if opts.verbose {
        eprintln!("Done! Binary written to {}", opts.output.display());
    }

    if warning_count > 0 {
        eprintln!("warning: {}", make_summary(0, warning_count));
    }

    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// NERA-60 F1: a failed publish must still use a peer's completed archive.
    #[test]
    fn runtime_publish_reuses_completed_file_after_persist_error() {
        let dir = tempfile::tempdir().unwrap();
        let staging = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        let path = dir.path().join("runtime.a");
        // Model the completed publication with a real rename. The original
        // staging name is now absent, so persist deterministically fails.
        fs::rename(staging.path(), &path).unwrap();
        assert!(!staging.path().exists());
        assert!(path.is_file());
        assert_eq!(publish_embedded_runtime(staging, path.clone()), Some(path));
    }

    /// NERA-60 F1: an absent archive or a directory is not a winning publisher.
    #[test]
    fn runtime_publish_error_without_completed_file_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        for is_directory in [false, true] {
            let staging = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
            fs::remove_file(staging.path()).unwrap();
            let path = dir.path().join(if is_directory { "directory.a" } else { "missing.a" });
            if is_directory {
                fs::create_dir(&path).unwrap();
            }
            assert!(publish_embedded_runtime(staging, path.clone()).is_none());
            assert!(!path.is_file());
        }
    }

    /// NERA-61: publication failure must not reuse a group/other-writable archive.
    /// NERA-116 N3: the common guard test owns live-symlink rejection.
    #[cfg(unix)]
    #[test]
    fn runtime_publish_error_rejects_writable_archive() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let staging = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        let path = dir.path().join("writable.a");
        fs::rename(staging.path(), &path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();
        assert!(publish_embedded_runtime(staging, path).is_none());
    }

    /// Helper: create a temp directory with an optional nera.toml and directory layout.
    fn setup_project(dir: &Path, toml_content: Option<&str>, subdirs: &[&str]) {
        fs::create_dir_all(dir).unwrap();
        if let Some(content) = toml_content {
            fs::write(dir.join("nera.toml"), content).unwrap();
        }
        for sub in subdirs {
            fs::create_dir_all(dir.join(sub)).unwrap();
        }
    }

    #[test]
    fn test_config_rejects_unknown_framework() {
        assert!(toml::from_str::<NeraToml>(r#"[project]
name = "demo"
[test]
framework = "typo"
modules = ["demo_tests"]
"#).is_err());
    }

    #[test]
    fn deferred_calls_require_explicit_project_option() {
        let strict: NeraToml = toml::from_str("[project]\nname = \"test\"\n").unwrap();
        assert!(!strict.project.defer_undefined);
        let deferred: NeraToml = toml::from_str(
            "[project]\nname = \"test\"\ndefer_undefined = true\n"
        ).unwrap();
        assert!(deferred.project.defer_undefined);
    }

    #[test]
    fn find_project_toml_from_explicit_path() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("myproject");
        setup_project(&root, Some("[project]\nname = \"test\"\n"), &[]);

        let found = find_project_toml(Some(&root));
        assert_eq!(found, Some(root.join("nera.toml")));
    }

    #[test]
    fn find_project_toml_walks_up() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("myproject");
        let nested = root.join("src").join("deep");
        setup_project(&root, Some("[project]\nname = \"test\"\n"), &["src/deep"]);

        let found = find_project_toml(Some(&nested));
        assert_eq!(found, Some(root.join("nera.toml")));
    }

    #[test]
    fn find_project_toml_returns_none_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let found = find_project_toml(Some(tmp.path()));
        assert!(found.is_none());
    }

    #[test]
    fn resolve_reads_include_from_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        let toml = r#"
[project]
name = "test"
src = ["src"]
include = ["inc", "extra/headers"]
"#;
        setup_project(&root, Some(toml), &["src", "inc", "extra/headers"]);
        // Write a dummy .erl file so discover_files doesn't fail
        fs::write(root.join("src/foo.erl"), "-module(foo).").unwrap();

        let opts = BuildOpts::resolve(
            vec![], vec![], vec![], vec![],
            None, None, false, false, true, false,
            None, None, false, Some(&root),
        ).unwrap();

        assert!(opts.include_paths.contains(&root.join("inc")));
        assert!(opts.include_paths.contains(&root.join("extra/headers")));
    }

    #[test]
    fn resolve_auto_discovers_include_dir_without_toml_field() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        let toml = r#"
[project]
name = "test"
src = ["src"]
"#;
        setup_project(&root, Some(toml), &["src", "include"]);
        fs::write(root.join("src/bar.erl"), "-module(bar).").unwrap();

        let opts = BuildOpts::resolve(
            vec![], vec![], vec![], vec![],
            None, None, false, false, true, false,
            None, None, false, Some(&root),
        ).unwrap();

        assert!(opts.include_paths.contains(&root.join("include")));
    }

    #[test]
    fn resolve_cli_include_takes_precedence() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        let toml = r#"
[project]
name = "test"
src = ["src"]
include = ["inc"]
"#;
        setup_project(&root, Some(toml), &["src", "inc"]);
        fs::write(root.join("src/baz.erl"), "-module(baz).").unwrap();

        let cli_inc = root.join("my_custom_inc");
        fs::create_dir_all(&cli_inc).unwrap();

        let opts = BuildOpts::resolve(
            vec![], vec![], vec![cli_inc.clone()], vec![],
            None, None, false, false, true, false,
            None, None, false, Some(&root),
        ).unwrap();

        // CLI include should be present, toml include should NOT
        assert!(opts.include_paths.contains(&cli_inc));
        assert!(!opts.include_paths.contains(&root.join("inc")));
        // src/ should always be present regardless of CLI override
        assert!(opts.include_paths.contains(&root.join("src")),
            "src/ should always be in include_paths even when CLI overrides include");
    }

    #[test]
    fn resolve_src_always_in_include_paths_with_toml_override() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        let toml = r#"
[project]
name = "test"
src = ["src"]
include = ["inc"]
"#;
        setup_project(&root, Some(toml), &["src", "inc"]);
        fs::write(root.join("src/foo.erl"), "-module(foo).").unwrap();

        let opts = BuildOpts::resolve(
            vec![], vec![], vec![], vec![],
            None, None, false, false, true, false,
            None, None, false, Some(&root),
        ).unwrap();

        // Both the explicit toml include AND src/ should be present
        assert!(opts.include_paths.contains(&root.join("inc")),
            "toml-specified include dir should be present");
        assert!(opts.include_paths.contains(&root.join("src")),
            "src/ should always be in include_paths even when toml overrides include");
    }

    #[test]
    fn resolve_src_in_include_paths_without_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        setup_project(&root, None, &["src"]);
        fs::write(root.join("src/bar.erl"), "-module(bar).").unwrap();

        // Without nera.toml, convention-based src detection uses CWD
        let original_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&root).unwrap();
        let opts = BuildOpts::resolve(
            vec![], vec![], vec![], vec![],
            None, None, false, false, true, false,
            None, None, false, Some(&root),
        ).unwrap();
        std::env::set_current_dir(&original_cwd).unwrap();

        // src/ should be in include_paths (added from src_dirs)
        assert!(opts.include_paths.iter().any(|p| p.ends_with("src")),
            "src/ should be in include_paths by convention, got: {:?}", opts.include_paths);
    }

    // ── File type detection tests ──────────────────────────────────

    #[test]
    fn is_app_file_detects_app_extension() {
        assert!(is_app_file(Path::new("myapp.app")));
        assert!(is_app_file(Path::new("/path/to/myapp.app")));
    }

    #[test]
    fn is_app_file_detects_app_src_extension() {
        assert!(is_app_file(Path::new("myapp.app.src")));
        assert!(is_app_file(Path::new("/path/to/myapp.app.src")));
    }

    #[test]
    fn is_app_file_rejects_erl() {
        assert!(!is_app_file(Path::new("module.erl")));
        assert!(!is_app_file(Path::new("module.hrl")));
    }

    #[test]
    fn is_formattable_file_accepts_all_types() {
        assert!(is_formattable_file(Path::new("module.erl")));
        assert!(is_formattable_file(Path::new("myapp.app")));
        assert!(is_formattable_file(Path::new("myapp.app.src")));
    }

    #[test]
    fn is_formattable_file_rejects_non_erlang() {
        assert!(!is_formattable_file(Path::new("readme.md")));
        assert!(!is_formattable_file(Path::new("config.toml")));
        assert!(!is_formattable_file(Path::new("module.beam")));
    }

    #[test]
    fn collect_discovers_app_files() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("mymod.erl"), "-module(mymod).").unwrap();
        fs::write(src.join("myapp.app.src"), "{application, myapp, []}.").unwrap();
        fs::write(src.join("other.app"), "{application, other, []}.").unwrap();
        fs::write(src.join("readme.md"), "# readme").unwrap();

        let mut files = Vec::new();
        collect_erl_files(&src, &mut files);
        let names: Vec<String> = files.iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(names.contains(&"mymod.erl".to_string()));
        assert!(names.contains(&"myapp.app.src".to_string()));
        assert!(names.contains(&"other.app".to_string()));
        assert!(!names.contains(&"readme.md".to_string()));
        assert_eq!(files.len(), 3);
    }

    // ── discover_files tests/ auto-discovery ────────────────────

    #[test]
    fn check_uses_selected_otp_release() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        setup_project(root, Some("[project]\nname = \"otp_probe\"\nsrc = [\"src\"]\n[otp]\nroot = \"otp\"\n"), &["src", "otp/lib", "otp/releases"]);
        fs::write(root.join("otp/releases/start_erl.data"), "15.0 27\n").unwrap();
        fs::write(root.join("src/otp_probe.erl"), "-module(otp_probe). -export([test/0]). -if(?OTP_RELEASE =:= 27). test() -> ok. -else. test() -> ?WRONG_OTP. -endif.").unwrap();
        let opts = BuildOpts::resolve(vec![], vec![], vec![], vec![], None, None,
            false, false, true, true, None, None, false, Some(root)).unwrap();
        assert!(check(&opts).is_ok());
    }

    #[test]
    fn discover_files_includes_tests_dir_by_convention() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        setup_project(&root, None, &["src", "tests"]);
        fs::write(root.join("src/main.erl"), "-module(main).").unwrap();
        fs::write(root.join("tests/test_one.erl"), "-module(test_one).").unwrap();
        fs::write(root.join("tests/test_two.erl"), "-module(test_two).").unwrap();

        let opts = BuildOpts::resolve(
            vec![], vec![root.join("src")], vec![], vec![],
            None, None, false, false, true, false,
            None, None, false, Some(&root),
        ).unwrap();
        let (erl_files, include_paths, _) = discover_files(&opts).unwrap();

        let names: Vec<String> = erl_files.iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(names.contains(&"test_one.erl".to_string()),
            "should discover test files, got: {:?}", names);
        assert!(names.contains(&"test_two.erl".to_string()),
            "should discover test files, got: {:?}", names);
        assert!(names.contains(&"main.erl".to_string()),
            "should still have src files, got: {:?}", names);
        assert!(include_paths.contains(&root.join("tests")),
            "tests/ should be in include_paths, got: {:?}", include_paths);
    }

    #[test]
    fn discover_files_includes_tests_with_toml_src() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        let toml = r#"
[project]
name = "myapp"
src = ["src"]
include = ["include"]
"#;
        setup_project(&root, Some(toml), &["src", "include", "tests"]);
        fs::write(root.join("src/app.erl"), "-module(app).").unwrap();
        fs::write(root.join("tests/app_test.erl"), "-module(app_test).").unwrap();
        fs::write(root.join("include/app.hrl"), "-define(VER, 1).").unwrap();

        let opts = BuildOpts::resolve(
            vec![], vec![], vec![], vec![],
            None, None, false, false, true, false,
            None, None, false, Some(&root),
        ).unwrap();
        let (erl_files, include_paths, _) = discover_files(&opts).unwrap();

        let names: Vec<String> = erl_files.iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(names.contains(&"app_test.erl".to_string()),
            "should discover test files when nera.toml is present, got: {:?}", names);
        assert!(include_paths.contains(&root.join("include")),
            "toml include/ should be in include_paths, got: {:?}", include_paths);
        assert!(include_paths.contains(&root.join("tests")),
            "tests/ should be in include_paths, got: {:?}", include_paths);
    }

    #[test]
    fn discover_files_no_tests_dir_is_fine() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        setup_project(&root, None, &["src"]);
        fs::write(root.join("src/main.erl"), "-module(main).").unwrap();

        let opts = BuildOpts::resolve(
            vec![], vec![root.join("src")], vec![], vec![],
            None, None, false, false, true, false,
            None, None, false, Some(&root),
        ).unwrap();
        let (erl_files, include_paths, _) = discover_files(&opts).unwrap();

        assert_eq!(erl_files.len(), 1);
        assert!(!include_paths.iter().any(|p| p.ends_with("tests")),
            "no tests/ dir means none in include_paths, got: {:?}", include_paths);
    }
}
