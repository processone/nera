//! LSP server for real-time nera diagnostics.
//!
//! Provides structured diagnostics via the Language Server Protocol,
//! replacing the regex-based problem matcher workflow. Runs a full
//! project check on save and publishes diagnostics per file.

mod types;
mod util;
mod cursor;
mod references;
mod source_map;
mod diagnostics;
mod check;
mod handlers;
mod semantic_tokens;
mod type_infer;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LspService, Server};

use nera_syntax::lexer::Lexer;
use nera_syntax::parser::Parser;
use nera_syntax::preproc::Preprocessor;
use nera_syntax::source_map::SourceMap;

use string_interner::DefaultStringInterner;

use crate::load_project_toml;

use types::*;
use check::find_project_root_from;
use source_map::build_offset_map;

// ── Token type constants ────────────────────────────────────────────

/// The semantic token legend.  Must match the order declared in `initialize()`.
const TOKEN_TYPES: &[SemanticTokenType] = &[
    SemanticTokenType::FUNCTION,
    SemanticTokenType::VARIABLE,
    SemanticTokenType::NAMESPACE,
    SemanticTokenType::TYPE,
    SemanticTokenType::PROPERTY,
    SemanticTokenType::MACRO,
];

const TOKEN_MODIFIERS: &[SemanticTokenModifier] = &[
    SemanticTokenModifier::DECLARATION,
    SemanticTokenModifier::DEFINITION,
    SemanticTokenModifier::new("unused"),
    SemanticTokenModifier::new("exported"),
    SemanticTokenModifier::DEPRECATED,
    SemanticTokenModifier::DEFAULT_LIBRARY,
];

// ── LSP Backend ──────────────────────────────────────────────────────

struct NeraBackend {
    client: Client,
    root_path: Mutex<Option<std::path::PathBuf>>,
    /// Per-file cache of parsed modules for semantic tokens.
    doc_cache: Mutex<HashMap<Url, DocumentCache>>,
    /// Cache of last-published diagnostics per file, used by pull diagnostics.
    diag_cache: Arc<Mutex<HashMap<Url, Vec<Diagnostic>>>>,
    /// Generation counter for debouncing did_change fast checks.
    change_gen: Arc<AtomicU64>,
    /// Generation at which the last full project check completed.
    /// Used to skip redundant debounced full checks when nothing changed.
    last_full_check_gen: Arc<AtomicU64>,
    /// Cross-file symbol index for go-to-definition.
    symbol_index: Arc<Mutex<ProjectIndex>>,
    /// Cached parsed modules for cross-file operations (find references, completion).
    project_modules: Arc<Mutex<Vec<ParsedModuleCache>>>,
    /// Latest raw source text per file, updated on every did_change even when
    /// parsing fails.  Used by completion to read the current line text without
    /// depending on a successful parse.
    raw_text: Mutex<HashMap<Url, String>>,
    /// Per-module parse cache for incremental checking.
    /// Keyed by absolute file path, stores the last parsed AST and source hash
    /// so unchanged modules can skip re-parsing.
    parse_cache: Arc<Mutex<HashMap<std::path::PathBuf, ParseCacheEntry>>>,
    /// Persistent string interner that survives across checks.
    /// Atom IDs assigned by this interner are stable, so cached ASTs remain
    /// valid without remapping.
    persistent_interner: Arc<Mutex<DefaultStringInterner>>,
    /// Serializes full project checks (`check_for_lsp`) to prevent concurrent
    /// mutations of the persistent interner and parse cache.
    check_lock: Arc<tokio::sync::Mutex<()>>,
    /// Cached `-include_lib` resolver built from the application registry.
    /// Updated when a full project check runs (which discovers deps and OTP).
    include_lib_resolver: Arc<Mutex<Option<Arc<Box<crate::app_registry::IncludeLibResolver>>>>>,
    otp_release: Arc<Mutex<Option<u32>>>,
}

impl NeraBackend {
    fn new(client: Client) -> Self {
        Self {
            client,
            root_path: Mutex::new(None),
            doc_cache: Mutex::new(HashMap::new()),
            diag_cache: Arc::new(Mutex::new(HashMap::new())),
            change_gen: Arc::new(AtomicU64::new(0)),
            last_full_check_gen: Arc::new(AtomicU64::new(0)),
            symbol_index: Arc::new(Mutex::new(ProjectIndex::default())),
            project_modules: Arc::new(Mutex::new(Vec::new())),
            raw_text: Mutex::new(HashMap::new()),
            parse_cache: Arc::new(Mutex::new(HashMap::new())),
            persistent_interner: Arc::new(Mutex::new(DefaultStringInterner::default())),
            check_lock: Arc::new(tokio::sync::Mutex::new(())),
            include_lib_resolver: Arc::new(Mutex::new(None)),
            otp_release: Arc::new(Mutex::new(None)),
        }
    }

    /// Resolve include paths for preprocessing a single file.
    ///
    /// Uses the project root (from nera.toml lookup or workspace root) to find
    /// standard include directories (include/, src/).
    fn resolve_include_paths(&self, file_path: &Path) -> Vec<std::path::PathBuf> {
        let mut paths = Vec::new();

        // Try to find project root from the file's location
        let project_root = file_path
            .parent()
            .and_then(find_project_root_from)
            .or_else(|| {
                let guard = self.root_path.lock().unwrap();
                guard.clone()
            });

        if let Some(ref root) = project_root {
            // Try to read include dirs from nera.toml
            let toml_path = root.join("nera.toml");
            let mut found_toml_includes = false;
            if toml_path.is_file() {
                if let Ok(toml) = load_project_toml(&toml_path) {
                    if let Some(ref include_dirs) = toml.project.include {
                        for dir in include_dirs {
                            let abs = root.join(dir);
                            if abs.is_dir() {
                                paths.push(abs);
                                found_toml_includes = true;
                            }
                        }
                    }
                }
            }

            // Fallback: check for conventional directories
            if !found_toml_includes {
                let inc = root.join("include");
                if inc.is_dir() {
                    paths.push(inc);
                }
            }

            // Also add src/ for -include from same project
            let src = root.join("src");
            if src.is_dir() {
                paths.push(src);
            }

            // Also add tests/ so test files can resolve includes
            let tests = root.join("tests");
            if tests.is_dir() {
                paths.push(tests);
            }

            // Add the project root itself
            paths.push(root.clone());
        }

        // Always add the file's own directory
        if let Some(parent) = file_path.parent() {
            paths.push(parent.to_path_buf());
        }

        paths
    }

    /// Parse a single file and cache the result for semantic tokens and go-to-definition.
    ///
    /// Tries preprocessing first (resolves includes/macros). If that fails,
    /// falls back to parsing the raw source so go-to-definition still works
    /// for the parts of the file that don't depend on macros.
    fn cache_document(&self, uri: &Url, source: &str) {
        // Term files (.app, .app.src) contain a single Erlang term, not a
        // module.  They have no functions/exports/records to cache.
        let path = uri.as_str();
        if path.ends_with(".app") || path.ends_with(".app.src") {
            return;
        }

        let tokens = match Lexer::lex_all(source) {
            Ok(t) => t,
            Err(_) => return,
        };

        let file_path = uri
            .to_file_path()
            .unwrap_or_else(|_| std::path::PathBuf::from(uri.path()));

        // Resolve include paths from the project root (if available).
        let include_paths = self.resolve_include_paths(&file_path);

        let mut preproc = Preprocessor::new(source, &file_path);
        if let Some(release) = *self.otp_release.lock().unwrap() { preproc.set_otp_release(release); }
        if !include_paths.is_empty() {
            preproc.set_include_paths(include_paths);
        }
        if let Some(ref resolver) = *self.include_lib_resolver.lock().unwrap() {
            let resolver = resolver.clone();
            preproc.set_include_lib_resolver(Box::new(move |path| resolver(path)));
        }

        let project_root = file_path
            .parent()
            .and_then(find_project_root_from)
            .or_else(|| {
                let guard = self.root_path.lock().unwrap();
                guard.clone()
            });

        let preprocess_ok = match preproc.process(tokens) {
            Ok(processed) => {
                let original_sm = SourceMap::new(source);
                let (combined, segments, macro_locs, _used_macros) = preproc.take_all();
                let sm = SourceMap::new(&combined);
                let display_path = file_path.to_string_lossy().to_string();
                let offset_map = build_offset_map(source, &combined, &segments, &display_path);
                let mut parser = Parser::with_tokens(
                    &combined,
                    processed,
                    DefaultStringInterner::default(),
                );
                {
                    let result = parser.parse_module();
                    let no_errors = !result.has_errors();
                    // Always cache the (possibly partial) module so IDE
                    // features work on the valid portions of the file.
                    let mut cache = self.doc_cache.lock().unwrap();
                    cache.insert(uri.clone(), DocumentCache {
                        module: result.module,
                        source: combined,
                        original_source: source.to_string(),
                        source_map: sm,
                        original_source_map: original_sm,
                        offset_map,
                        file_segments: segments,
                        project_root: project_root.clone(),
                        macro_locations: macro_locs,
                        has_parse_errors: !no_errors,
                    });
                    no_errors
                }
            }
            Err(_) => false,
        };
        if preprocess_ok {
            return;
        }

        // Fallback: parse the raw source directly (without preprocessing).
        // This won't resolve macros or includes, but gives us an AST for
        // local navigation (variables, local calls).
        let fallback_tokens = match Lexer::lex_all(source) {
            Ok(t) => t,
            Err(_) => return,
        };
        let mut parser = Parser::with_tokens(
            source,
            fallback_tokens,
            DefaultStringInterner::default(),
        );
        let result = parser.parse_module();
        let has_errors = result.has_errors();

        let sm = SourceMap::new(source);
        let mut cache = self.doc_cache.lock().unwrap();
        cache.insert(uri.clone(), DocumentCache {
            module: result.module,
            source: source.to_string(),
            original_source: source.to_string(),
            source_map: sm.clone(),
            original_source_map: sm,
            offset_map: vec![(0, source.len(), 0)],
            file_segments: vec![],
            project_root,
            macro_locations: vec![],
            has_parse_errors: has_errors,
        });
    }

    /// Run check and publish diagnostics.
    ///
    /// If `file_uri` is provided, resolve the project root from the file's
    /// location (walking up to find `nera.toml`). If no project is found,
    /// fall back to a single-file check.
    async fn run_check(&self, file_uri: Option<Url>) {
        // Determine the project root:
        // 1. Walk up from the saved file looking for nera.toml
        // 2. Fall back to workspace root_path
        // 3. If neither works, do a single-file check
        let file_path = file_uri
            .as_ref()
            .and_then(|u| u.to_file_path().ok());

        let project_root = file_path
            .as_ref()
            .and_then(|fp| fp.parent())
            .and_then(find_project_root_from)
            .or_else(|| {
                let guard = self.root_path.lock().unwrap();
                guard.as_ref().and_then(|p| find_project_root_from(p))
            });

        match project_root {
            Some(root) => {
                self.client
                    .log_message(
                        MessageType::INFO,
                        format!("run_check: project root={}", root.display()),
                    )
                    .await;

                // Serialize full project checks to prevent concurrent mutations
                // of the persistent interner and parse cache.
                let _guard = self.check_lock.lock().await;

                let cache = self.parse_cache.clone();
                let interner = self.persistent_interner.clone();
                let (diagnostics, checked_uris, new_index, new_modules, new_registry) =
                    tokio::task::spawn_blocking(move || {
                        check::check_for_lsp(&root, &cache, &interner)
                    })
                        .await
                        .unwrap_or_else(|_| Default::default());

                // Update the symbol index and project module cache
                {
                    let mut idx = self.symbol_index.lock().unwrap();
                    *idx = new_index;
                }
                {
                    let mut modules = self.project_modules.lock().unwrap();
                    *modules = new_modules;
                }
                // Update the cached include_lib resolver
                if let Some(registry) = new_registry {
                    *self.otp_release.lock().unwrap() = registry.otp_release();
                    let mut resolver = self.include_lib_resolver.lock().unwrap();
                    *resolver = Some(Arc::new(registry.make_resolver()));
                }

                self.publish_results(&diagnostics, &checked_uris).await;

                // Mark this generation as fully checked so debounced
                // did_change checks can skip redundant work.
                let current_gen = self.change_gen.load(std::sync::atomic::Ordering::SeqCst);
                self.last_full_check_gen
                    .store(current_gen, std::sync::atomic::Ordering::SeqCst);
            }
            None => {
                // No project found — single-file fallback
                if let Some(fp) = file_path {
                    self.client
                        .log_message(
                            MessageType::INFO,
                            format!(
                                "run_check: no project root found, single-file check for {}",
                                fp.display()
                            ),
                        )
                        .await;

                    let (diagnostics, checked_uris) =
                        tokio::task::spawn_blocking(move || check::check_single_file(&fp))
                            .await
                            .unwrap_or_default();

                    self.publish_results(&diagnostics, &checked_uris).await;
                } else {
                    self.client
                        .log_message(
                            MessageType::INFO,
                            "run_check: no project root and no file URI, skipping",
                        )
                        .await;
                }
            }
        }
    }

    /// Publish diagnostics results to the client.
    async fn publish_results(
        &self,
        diagnostics: &HashMap<Url, Vec<Diagnostic>>,
        checked_uris: &[Url],
    ) {
        self.client
            .log_message(
                MessageType::INFO,
                format!(
                    "run_check: {} files checked, {} files with diagnostics",
                    checked_uris.len(),
                    diagnostics.len()
                ),
            )
            .await;

        // Update the pull-diagnostics cache.
        {
            let mut cache = self.diag_cache.lock().unwrap();
            // Clear checked files that have no diagnostics.
            for uri in checked_uris {
                if !diagnostics.contains_key(uri) {
                    cache.remove(uri);
                }
            }
            // Store files that do have diagnostics.
            for (uri, diags) in diagnostics {
                cache.insert(uri.clone(), diags.clone());
            }
        }

        // Clear diagnostics for all checked files first (push path)
        for uri in checked_uris {
            self.client
                .publish_diagnostics(uri.clone(), vec![], None)
                .await;
        }

        // Publish new diagnostics per file (push path)
        for (uri, diags) in diagnostics {
            self.client
                .log_message(
                    MessageType::INFO,
                    format!("publishing {} diagnostics for {}", diags.len(), uri),
                )
                .await;
            self.client
                .publish_diagnostics(uri.clone(), diags.clone(), None)
                .await;
        }
    }
}

// ── Entry point ──────────────────────────────────────────────────────

pub fn run_lsp() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();

        let (service, socket) = LspService::new(NeraBackend::new);
        Server::new(stdin, stdout, socket).serve(service).await;
    });
    Ok(())
}
