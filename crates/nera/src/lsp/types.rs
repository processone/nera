use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use tower_lsp::lsp_types;
use tower_lsp::lsp_types::Url;

use nera_syntax::ast;
use nera_syntax::preproc::MacroLocation;
use nera_syntax::source_map::SourceMap;
use nera_syntax::MultiSourceMap;

// ── Go-to-definition index ──────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct DefinitionLocation {
    pub(crate) file_path: PathBuf,
    pub(crate) range: lsp_types::Range,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct FunKey {
    pub(crate) module: String,
    pub(crate) function: String,
    pub(crate) arity: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct TypeKey {
    pub(crate) module: String,
    pub(crate) type_name: String,
    pub(crate) arity: u32,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ProjectIndex {
    pub(crate) functions: HashMap<FunKey, DefinitionLocation>,
    pub(crate) records: HashMap<String, Vec<DefinitionLocation>>,
    pub(crate) modules: HashMap<String, PathBuf>,
    pub(crate) types: HashMap<TypeKey, DefinitionLocation>,
}

/// Cached parsed module for cross-file operations (find references, completion).
#[derive(Debug, Clone)]
pub(crate) struct ParsedModuleCache {
    pub(crate) module_name: String,
    pub(crate) file_path: PathBuf,
    pub(crate) module: ast::Module,
    pub(crate) source: String,
    pub(crate) source_map: SourceMap,
}

/// Cached parse result for a single file, used by semantic tokens and go-to-definition.
pub(crate) struct DocumentCache {
    pub(crate) module: nera_syntax::ast::Module,
    /// The preprocessed (combined) source — AST spans refer to this.
    pub(crate) source: String,
    /// The original (pre-preprocessing) source — used to detect `?MACRO` tokens
    /// which are expanded away in the combined source.
    pub(crate) original_source: String,
    /// SourceMap built from the combined source (for span → LSP range conversion
    /// and semantic token encoding).
    pub(crate) source_map: SourceMap,
    /// SourceMap built from the original source (for LSP position → byte offset).
    pub(crate) original_source_map: SourceMap,
    /// Maps original-source byte ranges to combined-source byte ranges.
    /// Each entry: (original_start, original_end, combined_start).
    /// For non-included content, original bytes [orig_start..orig_end] map to
    /// combined bytes [combined_start..combined_start+(orig_end-orig_start)].
    pub(crate) offset_map: Vec<(usize, usize, usize)>,
    /// File segments from the preprocessor, used to identify which file a
    /// combined-source span belongs to (main file vs included header).
    pub(crate) file_segments: Vec<nera_syntax::preproc::FileSegment>,
    /// The project root, used for resolving relative include paths to URIs.
    pub(crate) project_root: Option<PathBuf>,
    /// Macro definition locations from the preprocessor, for go-to-definition on `?MACRO`.
    pub(crate) macro_locations: Vec<nera_syntax::preproc::MacroLocation>,
    /// True when the cached module is a partial AST (parse had errors).
    /// Used by the semantic tokens provider to decide whether to use
    /// lexer-based fallback tokens for regions not covered by the AST.
    pub(crate) has_parse_errors: bool,
}

/// Cached parse result for a single module, used by incremental checking.
///
/// When the source content (after preprocessing) hasn't changed, we can
/// skip re-parsing and reuse the cached AST and source map.  Atom IDs in
/// the cached AST remain valid because the LSP server uses a persistent
/// string interner that never forgets atoms.
#[derive(Clone)]
pub(crate) struct ParseCacheEntry {
    /// Hash of the combined source (after preprocessing).
    pub(crate) source_hash: u64,
    /// The display path used when this entry was parsed.
    pub(crate) display_path: String,
    /// Whether this is an embedded stdlib module.
    pub(crate) is_embedded: bool,
    /// Parsed AST.
    pub(crate) module: ast::Module,
    /// The combined source and source map from preprocessing.
    pub(crate) multi_source_map: MultiSourceMap,
    /// Parse errors from this module, replayed on cache hit.
    pub(crate) parse_errors: Vec<lsp_types::Diagnostic>,
    /// Macro definition locations from the preprocessor.
    pub(crate) macro_locations: Vec<MacroLocation>,
    /// Names of macros that were referenced during preprocessing.
    pub(crate) used_macros: HashSet<String>,
}

// ── Cursor hit-test ─────────────────────────────────────────────────

#[derive(Debug)]
pub(crate) enum CursorTarget {
    LocalCall { name: String, arity: u32 },
    RemoteCall { module: String, function: String, arity: u32 },
    Variable { name: String },
    RecordRef { name: String },
    LocalFunRef { name: String, arity: u32 },
    RemoteFunRef { module: String, function: String, arity: u32 },
}

/// A collected LSP diagnostic with its associated file URI.
pub(crate) struct FileDiagnostic {
    pub(crate) uri: Url,
    pub(crate) diagnostic: lsp_types::Diagnostic,
}
