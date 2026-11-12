use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tower_lsp::lsp_types::{self, DiagnosticSeverity, Range, Url};

use nera_syntax::error::Diagnostic as NeraDiagnostic;
use nera_syntax::semantic_tokens as sem;
use nera_syntax::lexer::Lexer;
use nera_syntax::parser::Parser;
use nera_syntax::preproc::Preprocessor;
use nera_syntax::MultiSourceMap;
use nera_syntax::ast::AtomId;

use nera_ir::lower::IrBuilder;

use string_interner::{DefaultStringInterner, Symbol};

use crate::{
    BuildOpts, SourceInput, discover_files, find_stdlib_dir,
    glob_erl_files, load_project_toml, EMBEDDED_STDLIB,
};

use super::types::{DefinitionLocation, FunKey, TypeKey, ProjectIndex, ParsedModuleCache, ParseCacheEntry};
use super::util::span_text;
use super::source_map::span_to_lsp_range;
use super::diagnostics::convert_diagnostic;

/// Compute a fast hash of a source string for cache comparison.
fn hash_source(source: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

type CheckResult = (
    HashMap<Url, Vec<lsp_types::Diagnostic>>,
    Vec<Url>,
    ProjectIndex,
    Vec<ParsedModuleCache>,
    Option<crate::app_registry::AppRegistry>,
);

/// Run the nera check pipeline and collect diagnostics as LSP types.
///
/// Unlike the CLI `check()`, this function:
/// - Collects diagnostics instead of emitting to stderr
/// - Continues past errors (doesn't short-circuit on the first failing module)
/// - Returns all checked file URIs so stale diagnostics can be cleared
pub(crate) fn check_for_lsp(
    root_path: &Path,
    parse_cache: &Mutex<HashMap<PathBuf, ParseCacheEntry>>,
    persistent_interner: &Mutex<DefaultStringInterner>,
) -> CheckResult {
    let mut diagnostics: HashMap<Url, Vec<lsp_types::Diagnostic>> = HashMap::new();
    let mut checked_uris: Vec<Url> = Vec::new();
    let mut project_index = ProjectIndex::default();
    let mut parsed_module_caches: Vec<ParsedModuleCache> = Vec::new();

    // Resolve build options with defaults, passing root_path so nera.toml
    // lookup does not depend on CWD (avoids race with concurrent checks).
    let opts = match BuildOpts::resolve(
        vec![], vec![], vec![], vec![],
        None, None, false, false, false, false,
        None, None, false, Some(root_path),
    ) {
        Ok(opts) => opts,
        Err(e) => {
            eprintln!("LSP: BuildOpts::resolve failed: {e}");
            return (diagnostics, checked_uris, project_index, parsed_module_caches, None);
        }
    };

    // Discover source files
    let (erl_files, include_paths, app_registry) = match discover_files(&opts) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("LSP: discover_files failed: {e}");
            return (diagnostics, checked_uris, project_index, parsed_module_caches, None);
        }
    };
    let include_lib_resolver = std::sync::Arc::new(app_registry.make_resolver());
    eprintln!("LSP: discovered {} source files, {} include paths, {} apps", erl_files.len(), include_paths.len(), app_registry.len());

    // Convert to SourceInput
    let mut sources: Vec<SourceInput> = erl_files.into_iter()
        .map(SourceInput::File)
        .collect();

    let mut using_embedded_stdlib = false;

    // Auto-include stdlib
    if !opts.no_stdlib {
        let user_modules: std::collections::HashSet<String> = sources.iter()
            .map(|s| s.module_name())
            .collect();

        if let Some(stdlib_dir) = find_stdlib_dir() {
            let stdlib_files = glob_erl_files(&stdlib_dir).unwrap_or_default();
            let mut to_prepend = Vec::new();
            for sf in stdlib_files {
                let mod_name = sf.file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                if !user_modules.contains(&mod_name) {
                    to_prepend.push(SourceInput::File(sf));
                }
            }
            to_prepend.extend(sources);
            sources = to_prepend;
        } else {
            using_embedded_stdlib = true;
            let mut to_prepend = Vec::new();
            for &(name, source) in EMBEDDED_STDLIB {
                if !user_modules.contains(name) {
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

    // Track all file URIs we check
    for input in &sources {
        if let Some(p) = input.as_path() {
            let abs = if p.is_absolute() {
                p.to_path_buf()
            } else {
                root_path.join(p)
            };
            if let Ok(uri) = Url::from_file_path(&abs) {
                checked_uris.push(uri);
            }
        }
    }

    // Phase 1: Parse all modules (with incremental cache)
    //
    // Use a persistent interner so that AtomId values in cached ASTs remain
    // valid across checks.  For each source file we read + preprocess to get
    // the combined source, hash it, and check the parse cache.  On a cache
    // hit we skip parsing entirely.
    //
    // The interner is cloned out of its Mutex here and written back after the
    // loop (at the end of Phase 1).  This is intentional: Parser::with_tokens
    // takes ownership of the interner, so we cannot hold the mutex during
    // parsing.  The cost of the clone is modest for typical projects (a few
    // hundred interned atoms) and avoids blocking atom-name reads during the
    // parse loop.
    let mut shared_interner = persistent_interner.lock().unwrap().clone();
    let mut parsed_modules: Vec<(String, bool, nera_syntax::ast::Module, MultiSourceMap)> = Vec::new();
    let mut module_macro_data: Vec<(Vec<nera_syntax::preproc::MacroLocation>, std::collections::HashSet<String>)> = Vec::new();
    // Snapshot the parse cache so the mutex is not held during I/O + parsing.
    // We clone the cache, work on the local copy, then merge back.
    let mut local_cache: HashMap<PathBuf, ParseCacheEntry> = {
        let guard = parse_cache.lock().unwrap();
        guard.clone()
    };
    let mut cache_hits = 0usize;
    let mut cache_misses = 0usize;
    // Track which file paths are still present so we can prune stale entries.
    let mut seen_paths: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();

    for input in &sources {
        let abs_path = input.as_path().map(|p| {
            if p.is_absolute() { p.to_path_buf() } else { root_path.join(p) }
        });

        let source = match input.read_source() {
            Ok(s) => s,
            Err(e) => {
                // Report file read errors as diagnostics
                if let Some(ref abs) = abs_path {
                    if let Ok(uri) = Url::from_file_path(abs) {
                        diagnostics.entry(uri).or_default().push(lsp_types::Diagnostic {
                            range: Range::default(),
                            severity: Some(DiagnosticSeverity::ERROR),
                            source: Some("nera".to_string()),
                            message: e.to_string(),
                            ..Default::default()
                        });
                    }
                }
                continue;
            }
        };

        // Lex
        let tokens = match Lexer::lex_all(&source) {
            Ok(tokens) => tokens,
            Err(errs) => {
                // Convert lex errors to diagnostics
                for err in &errs {
                    let diag = NeraDiagnostic::from_parse_error(err);
                    let single_sm = MultiSourceMap::new(
                        source.clone(),
                        vec![nera_syntax::FileSegment {
                            start: 0,
                            end: source.len(),
                            filename: input.display_path(),
                        }],
                    );
                    if let Some(fd) = convert_diagnostic(&diag, &single_sm, root_path) {
                        diagnostics.entry(fd.uri).or_default().push(fd.diagnostic);
                    }
                }
                continue;
            }
        };

        // Preprocess
        let preproc_path = match input.as_path() {
            Some(p) => p.to_path_buf(),
            None => PathBuf::from(input.display_path()),
        };
        let mut preproc = Preprocessor::new(&source, &preproc_path);
        if let Some(release) = app_registry.otp_release() { preproc.set_otp_release(release); }
        if !include_paths.is_empty() {
            preproc.set_include_paths(include_paths.clone());
        }
        {
            let resolver = include_lib_resolver.clone();
            preproc.set_include_lib_resolver(Box::new(move |path| resolver(path)));
        }
        let processed_tokens = match preproc.process(tokens) {
            Ok(tokens) => tokens,
            Err(errs) => {
                for err in &errs {
                    let diag = NeraDiagnostic::from_parse_error(err);
                    let single_sm = MultiSourceMap::new(
                        source.clone(),
                        vec![nera_syntax::FileSegment {
                            start: 0,
                            end: source.len(),
                            filename: input.display_path(),
                        }],
                    );
                    if let Some(fd) = convert_diagnostic(&diag, &single_sm, root_path) {
                        diagnostics.entry(fd.uri).or_default().push(fd.diagnostic);
                    }
                }
                continue;
            }
        };

        let (combined_source, segments, macro_locs, used_macros) = preproc.take_all();
        let source_hash = hash_source(&combined_source);

        // Cache key: absolute path (or synthetic path for embedded modules)
        let cache_key = abs_path.clone().unwrap_or_else(|| PathBuf::from(input.display_path()));
        seen_paths.insert(cache_key.clone());

        // Check parse cache
        if let Some(entry) = local_cache.get(&cache_key) {
            if entry.source_hash == source_hash {
                // Cache hit — reuse parsed AST and source map.
                cache_hits += 1;
                // Replay cached parse errors.
                if !entry.parse_errors.is_empty() {
                    if let Some(ref abs) = abs_path {
                        if let Ok(uri) = Url::from_file_path(abs) {
                            diagnostics.entry(uri).or_default().extend(entry.parse_errors.clone());
                        }
                    }
                }
                parsed_modules.push((
                    entry.display_path.clone(),
                    entry.is_embedded,
                    entry.module.clone(),
                    entry.multi_source_map.clone(),
                ));
                module_macro_data.push((
                    entry.macro_locations.clone(),
                    entry.used_macros.clone(),
                ));
                continue;
            }
        }

        // Cache miss — parse fresh.
        cache_misses += 1;
        let multi_source_map = MultiSourceMap::new(combined_source.clone(), segments);

        let mut parser = Parser::with_tokens(&combined_source, processed_tokens, shared_interner);
        let parse_result = parser.parse_module();

        // Convert and report parse errors as diagnostics
        let mut parse_error_diags = Vec::new();
        for err in &parse_result.errors {
            let diag = NeraDiagnostic::from_parse_error(err);
            if let Some(fd) = convert_diagnostic(&diag, &multi_source_map, root_path) {
                parse_error_diags.push(fd.diagnostic.clone());
                diagnostics.entry(fd.uri).or_default().push(fd.diagnostic);
            }
        }

        shared_interner = parser.into_interner();

        // Store in cache
        local_cache.insert(cache_key, ParseCacheEntry {
            source_hash,
            display_path: input.display_path(),
            is_embedded: input.is_embedded(),
            module: parse_result.module.clone(),
            multi_source_map: multi_source_map.clone(),
            parse_errors: parse_error_diags,
            macro_locations: macro_locs.clone(),
            used_macros: used_macros.clone(),
        });

        parsed_modules.push((input.display_path(), input.is_embedded(), parse_result.module, multi_source_map));
        module_macro_data.push((macro_locs, used_macros));
    }

    // Prune stale entries from the local snapshot, then merge back.
    // Checks are serialized by check_lock in lsp/mod.rs, so no risk of lost
    // updates.  The snapshot-merge pattern minimises lock duration in case
    // serialization is relaxed in the future.
    local_cache.retain(|path, _| seen_paths.contains(path));
    {
        let mut cache_guard = parse_cache.lock().unwrap();
        *cache_guard = local_cache;
    }

    eprintln!("LSP: parse cache: {} hits, {} misses", cache_hits, cache_misses);

    // Persist the interner for future checks.
    {
        let mut interner_guard = persistent_interner.lock().unwrap();
        *interner_guard = shared_interner.clone();
    }

    // Build atom table
    let mut atom_table: HashMap<u32, String> = HashMap::new();
    let mut true_atom_id = None;
    let mut false_atom_id = None;
    let mut self_atom_id = None;
    for (sym, s) in shared_interner.into_iter() {
        let id = sym.to_usize() as u32;
        if s == "true" { true_atom_id = Some(id); }
        else if s == "false" { false_atom_id = Some(id); }
        else if s == "self" { self_atom_id = Some(id); }
        atom_table.entry(id).or_insert_with(|| s.to_string());
    }

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

    // Ensure well-known module atoms
    {
        let mut next = atom_table.keys().max().map(|m| m + 1).unwrap_or(0);
        for required in &["erlang", "io", "maps", "gen_tcp", "lists"] {
            if !atom_table.values().any(|v| v == required) {
                atom_table.insert(next, required.to_string());
                next += 1;
            }
        }
    }

    let mut atom_names: HashMap<AtomId, String> = atom_table
        .iter()
        .map(|(id, name)| (AtomId(*id), name.clone()))
        .collect();

    let stdlib_dir_canonical = if !opts.no_stdlib && !using_embedded_stdlib {
        find_stdlib_dir().and_then(|d| d.canonicalize().ok())
    } else {
        None
    };

    // Phase 2: Lower to IR + collect warnings
    //
    // Cross-module unused_record detection: records defined in .hrl files may
    // appear unused in one module but used in another that includes the same
    // header.  We defer such warnings and only emit them if no module uses the
    // record.
    use crate::cross_module_warnings as cmw;

    let mut ir_modules = Vec::new();
    let globally_used_records = cmw::collect_global_record_usage(
        &parsed_modules, &atom_names, stdlib_dir_canonical.as_deref(),
    );
    let mut all_deferred: Vec<cmw::DeferredRecordWarning> = Vec::new();

    // Cross-module unused_macro detection: same pattern as records.
    let globally_used_macros = {
        let macro_usage_data: Vec<(std::collections::HashSet<String>, bool, &str)> = parsed_modules
            .iter()
            .zip(module_macro_data.iter())
            .map(|((dp, emb, _, _), (_, used))| (used.clone(), *emb, dp.as_str()))
            .collect();
        cmw::collect_global_macro_usage(&macro_usage_data, stdlib_dir_canonical.as_deref())
    };
    let mut all_deferred_macros: Vec<cmw::DeferredMacroWarning> = Vec::new();

    for (mod_idx, (display_path, is_embedded, module, multi_sm)) in parsed_modules.iter().enumerate() {
        let is_stdlib = cmw::is_stdlib_module(
            display_path, *is_embedded, stdlib_dir_canonical.as_deref(),
        );

        if !is_stdlib {
            let combined_source = multi_sm.combined_source();
            let result = cmw::analyze_module_warnings(
                module, &atom_names, display_path, multi_sm, mod_idx,
                Some(combined_source),
            );
            for w in &result.immediate {
                if let Some(fd) = convert_diagnostic(w, multi_sm, root_path) {
                    diagnostics.entry(fd.uri).or_default().push(fd.diagnostic);
                }
            }
            all_deferred.extend(result.deferred);

            // Unused macro analysis.
            let (macro_locs, used_macros) = &module_macro_data[mod_idx];
            let (macro_immediate, macro_deferred) = cmw::analyze_module_macro_warnings(
                macro_locs, used_macros, multi_sm, mod_idx,
            );
            for w in &macro_immediate {
                if let Some(fd) = convert_diagnostic(w, multi_sm, root_path) {
                    diagnostics.entry(fd.uri).or_default().push(fd.diagnostic);
                }
            }
            all_deferred_macros.extend(macro_deferred);
        }

        // Lower to IR
        let combined_sm = multi_sm.to_source_map(display_path);
        match IrBuilder::lower_module(
            module,
            AtomId(true_id),
            AtomId(false_id),
            AtomId(self_id),
            atom_names.clone(),
            Some(combined_sm),
        ) {
            Ok((ir_module, updated_atoms)) => {
                for (id, name) in &updated_atoms {
                    atom_table.entry(id.0).or_insert_with(|| name.clone());
                    atom_names.entry(*id).or_insert_with(|| name.clone());
                }
                ir_modules.push(ir_module);
            }
            Err(e) => {
                let diag = match e.span() {
                    Some(span) => NeraDiagnostic::error_at(e.to_string(), span),
                    None => NeraDiagnostic::error(format!("{}: {}", display_path, e)),
                };
                if let Some(fd) = convert_diagnostic(&diag, multi_sm, root_path) {
                    diagnostics.entry(fd.uri).or_default().push(fd.diagnostic);
                }
                continue;
            }
        }
    }

    // Emit deferred .hrl unused_record warnings only for records that are
    // not used in ANY module across the project.
    for deferred in cmw::resolve_deferred_warnings(&all_deferred, &globally_used_records) {
        let (_, _, _, multi_sm) = &parsed_modules[deferred.module_idx];
        if let Some(fd) = convert_diagnostic(&deferred.warning, multi_sm, root_path) {
            diagnostics.entry(fd.uri).or_default().push(fd.diagnostic);
        }
    }
    // Emit deferred .hrl unused_macro warnings for macros not used in any module.
    for deferred in cmw::resolve_deferred_macro_warnings(&all_deferred_macros, &globally_used_macros) {
        let (_, _, _, multi_sm) = &parsed_modules[deferred.module_idx];
        if let Some(fd) = convert_diagnostic(&deferred.warning, multi_sm, root_path) {
            diagnostics.entry(fd.uri).or_default().push(fd.diagnostic);
        }
    }

    // Phase 3: Validate — check for unresolved remote calls
    {
        let unresolved = nera_ir::validate::check_unresolved_calls(&ir_modules, &atom_names);
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
                        format!("function `{}:{}` is not exported", u.target_module, u.target_function)
                    } else {
                        format!("function `{}:{}/{}` is not exported", u.target_module, u.target_function, u.arity)
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
                        format!("unresolved function `{}:{}`", u.target_module, u.target_function)
                    } else {
                        format!("unresolved function `{}:{}/{}`", u.target_module, u.target_function, u.arity)
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
                    let diag = NeraDiagnostic::error_at(&msg, span).with_label(label);
                    if let Some(fd) = convert_diagnostic(&diag, multi_sm, root_path) {
                        diagnostics.entry(fd.uri).or_default().push(fd.diagnostic);
                    }
                }
            }
        }
    }

    // Build project index from parsed modules
    for (display_path, _is_embedded, module, multi_sm) in &parsed_modules {
        let module_name = atom_names.get(&module.name).cloned().unwrap_or_default();

        // Resolve file path
        let file_path = if Path::new(display_path).is_absolute() {
            PathBuf::from(display_path)
        } else {
            root_path.join(display_path)
        };

        project_index.modules.insert(module_name.clone(), file_path.clone());

        // Build a SourceMap for span→range conversion
        let sm = multi_sm.to_source_map(display_path);

        // Index functions
        for func in &module.functions {
            let func_name = atom_names.get(&AtomId(func.name.0)).cloned()
                .unwrap_or_else(|| {
                    let combined = multi_sm.combined_source();
                    span_text(combined, func.name_span).to_string()
                });
            let key = FunKey {
                module: module_name.clone(),
                function: func_name,
                arity: func.arity,
            };
            let loc = DefinitionLocation {
                file_path: file_path.clone(),
                range: span_to_lsp_range(func.name_span, &sm),
            };
            project_index.functions.entry(key).or_insert(loc);
        }

        // Index records
        for rec in &module.records {
            let rec_name = atom_names.get(&AtomId(rec.name.0)).cloned()
                .unwrap_or_else(|| {
                    // Fallback: extract from source text at the span.
                    // rec.span starts at the record name (e.g. "demo_state, {fields})")
                    let combined = multi_sm.combined_source();
                    let text = span_text(combined, rec.span);
                    let end = text.find([',', ' ', '{', '(', ')']).unwrap_or(text.len());
                    text[..end].to_string()
                });
            if !rec_name.is_empty() {
                let loc = DefinitionLocation {
                    file_path: file_path.clone(),
                    range: span_to_lsp_range(rec.span, &sm),
                };
                project_index.records.entry(rec_name).or_default().push(loc);
            }
        }

        // Index type definitions
        for td in &module.type_defs {
            let type_name = atom_names.get(&AtomId(td.name.0)).cloned()
                .unwrap_or_else(|| {
                    let combined = multi_sm.combined_source();
                    span_text(combined, td.name_span).to_string()
                });
            let key = TypeKey {
                module: module_name.clone(),
                type_name,
                arity: td.arity,
            };
            let loc = DefinitionLocation {
                file_path: file_path.clone(),
                range: span_to_lsp_range(td.name_span, &sm),
            };
            project_index.types.entry(key).or_insert(loc);
        }

        // Check for undefined type references (skip stdlib modules)
        let is_stdlib_mod = *_is_embedded || stdlib_dir_canonical
            .as_ref()
            .map(|sd| {
                PathBuf::from(display_path).canonicalize()
                    .ok()
                    .map(|cp| cp.starts_with(sd))
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if !is_stdlib_mod {
            let combined = multi_sm.combined_source();
            let type_refs = sem::collect_type_references(combined);
            for type_ref in &type_refs {
                let resolved = match &type_ref.module {
                    Some(mod_name) => {
                        // Remote: check project_index.types for any arity
                        (0..=20).any(|a| project_index.types.contains_key(&TypeKey {
                            module: mod_name.clone(),
                            type_name: type_ref.name.clone(),
                            arity: a,
                        }))
                    }
                    None => {
                        // Local: check built-in types or module type_defs
                        super::util::BUILTIN_TYPES.contains(&&*type_ref.name)
                        || module.type_defs.iter().any(|td| {
                            let td_name = atom_names.get(&nera_syntax::ast::AtomId(td.name.0))
                                .map(|s| s.as_str())
                                .unwrap_or_else(|| span_text(combined, td.name_span));
                            td_name == type_ref.name
                        })
                    }
                };
                if !resolved {
                    let msg = match &type_ref.module {
                        Some(mod_name) => format!("undefined type `{}:{}`", mod_name, type_ref.name),
                        None => format!("undefined type `{}`", type_ref.name),
                    };
                    let diag = NeraDiagnostic::warning_at(&msg, type_ref.span)
                        .with_code("undefined_type");
                    if let Some(fd) = convert_diagnostic(&diag, multi_sm, root_path) {
                        diagnostics.entry(fd.uri).or_default().push(fd.diagnostic);
                    }
                }
            }
        }

        // Cache parsed module for cross-file operations
        parsed_module_caches.push(ParsedModuleCache {
            module_name: module_name.clone(),
            file_path: file_path.clone(),
            module: module.clone(),
            source: multi_sm.combined_source().to_string(),
            source_map: sm,
        });
    }

    (diagnostics, checked_uris, project_index, parsed_module_caches, Some(app_registry))
}

/// Check a single .erl file standalone (lex + parse + warnings only).
///
/// Used as a fallback when no nera.toml project is found.
pub(crate) fn check_single_file(
    file_path: &Path,
) -> (HashMap<Url, Vec<lsp_types::Diagnostic>>, Vec<Url>) {
    let mut diagnostics: HashMap<Url, Vec<lsp_types::Diagnostic>> = HashMap::new();
    let mut checked_uris: Vec<Url> = Vec::new();

    let abs_path = if file_path.is_absolute() {
        file_path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_default()
            .join(file_path)
    };

    let uri = match Url::from_file_path(&abs_path) {
        Ok(u) => u,
        Err(_) => return (diagnostics, checked_uris),
    };
    checked_uris.push(uri.clone());

    let source = match std::fs::read_to_string(&abs_path) {
        Ok(s) => s,
        Err(e) => {
            diagnostics.entry(uri).or_default().push(lsp_types::Diagnostic {
                range: Range::default(),
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("nera".to_string()),
                message: format!("cannot read file: {e}"),
                ..Default::default()
            });
            return (diagnostics, checked_uris);
        }
    };

    let display_path = abs_path.to_string_lossy().to_string();
    let project_root = abs_path.parent().unwrap_or(Path::new("/"));

    let single_sm = || {
        MultiSourceMap::new(
            source.clone(),
            vec![nera_syntax::FileSegment {
                start: 0,
                end: source.len(),
                filename: display_path.clone(),
            }],
        )
    };

    // Lex
    let tokens = match Lexer::lex_all(&source) {
        Ok(tokens) => tokens,
        Err(errs) => {
            let sm = single_sm();
            for err in &errs {
                let diag = NeraDiagnostic::from_parse_error(err);
                if let Some(fd) = convert_diagnostic(&diag, &sm, project_root) {
                    diagnostics.entry(fd.uri).or_default().push(fd.diagnostic);
                }
            }
            return (diagnostics, checked_uris);
        }
    };

    // Preprocess
    let mut preproc = Preprocessor::new(&source, &abs_path);
    let processed_tokens = match preproc.process(tokens) {
        Ok(tokens) => tokens,
        Err(errs) => {
            let sm = single_sm();
            for err in &errs {
                let diag = NeraDiagnostic::from_parse_error(err);
                if let Some(fd) = convert_diagnostic(&diag, &sm, project_root) {
                    diagnostics.entry(fd.uri).or_default().push(fd.diagnostic);
                }
            }
            return (diagnostics, checked_uris);
        }
    };

    let (combined_source, segments) = preproc.take_combined_source_and_segments();
    let multi_source_map = MultiSourceMap::new(combined_source.clone(), segments);

    // Parse — always produces a (possibly partial) module
    let shared_interner = DefaultStringInterner::default();
    let mut parser = Parser::with_tokens(&combined_source, processed_tokens, shared_interner);
    let parse_result = parser.parse_module();

    // Report parse errors as diagnostics
    for err in &parse_result.errors {
        let diag = NeraDiagnostic::from_parse_error(err);
        if let Some(fd) = convert_diagnostic(&diag, &multi_source_map, project_root) {
            diagnostics.entry(fd.uri).or_default().push(fd.diagnostic);
        }
    }

    let module = parse_result.module;

    // Warnings
    let interner = parser.into_interner();
    let atom_names: HashMap<AtomId, String> = interner
        .into_iter()
        .map(|(sym, s)| (AtomId(sym.to_usize() as u32), s.to_string()))
        .collect();

    let warnings = nera_ir::warnings::analyze_module_with_source(
        &module, &atom_names, Some(&display_path), Some(multi_source_map.combined_source()),
    );
    for w in &warnings {
        // In single-file mode we have no cross-module context, so suppress
        // unused_record warnings for records from included files — they may
        // be used by other modules that include the same header.
        if w.code.as_deref() == Some("unused_record") {
            if let Some(ref span) = w.span {
                let file_id = multi_source_map.file_id_for_offset(span.start);
                if file_id != 0 {
                    continue;
                }
            }
        }
        if let Some(fd) = convert_diagnostic(w, &multi_source_map, project_root) {
            diagnostics.entry(fd.uri).or_default().push(fd.diagnostic);
        }
    }

    (diagnostics, checked_uris)
}

/// Fast syntax check from an in-memory source buffer (lex + preprocess + parse).
///
/// Used by `did_change` for real-time feedback. Skips warnings and IR lowering
/// to stay fast. Returns only syntax errors.
pub(crate) fn check_single_file_from_source(
    uri: &Url,
    source: &str,
) -> Vec<lsp_types::Diagnostic> {
    let mut diagnostics = Vec::new();

    let file_path = uri
        .to_file_path()
        .unwrap_or_else(|_| PathBuf::from(uri.path()));
    let display_path = file_path.to_string_lossy().to_string();

    // Find the real project root (where nera.toml lives) so include paths
    // resolve correctly.  Falls back to the file's parent directory.
    let project_root = file_path
        .parent()
        .and_then(find_project_root_from)
        .unwrap_or_else(|| file_path.parent().unwrap_or(Path::new("/")).to_path_buf());

    let single_sm = || {
        MultiSourceMap::new(
            source.to_string(),
            vec![nera_syntax::FileSegment {
                start: 0,
                end: source.len(),
                filename: display_path.clone(),
            }],
        )
    };

    // Lex
    let tokens = match Lexer::lex_all(source) {
        Ok(tokens) => tokens,
        Err(errs) => {
            let sm = single_sm();
            for err in &errs {
                let diag = NeraDiagnostic::from_parse_error(err);
                if let Some(fd) = convert_diagnostic(&diag, &sm, &project_root) {
                    diagnostics.push(fd.diagnostic);
                }
            }
            return diagnostics;
        }
    };

    // Preprocess — resolve include paths from nera.toml so includes work.
    let mut preproc = Preprocessor::new(source, &file_path);
    {
        let mut include_paths = Vec::new();
        let toml_path = project_root.join("nera.toml");
        if toml_path.is_file() {
            if let Ok(toml) = load_project_toml(&toml_path) {
                if let Some(ref inc_dirs) = toml.project.include {
                    for dir in inc_dirs {
                        let abs = project_root.join(dir);
                        if abs.is_dir() {
                            include_paths.push(abs);
                        }
                    }
                }
            }
        }
        // Fallback: check for conventional include/ directory
        if include_paths.is_empty() {
            let inc_dir = project_root.join("include");
            if inc_dir.is_dir() {
                include_paths.push(inc_dir);
            }
        }
        if !include_paths.is_empty() {
            preproc.set_include_paths(include_paths);
        }
    }
    let processed_tokens = match preproc.process(tokens) {
        Ok(tokens) => tokens,
        Err(errs) => {
            let sm = single_sm();
            for err in &errs {
                let diag = NeraDiagnostic::from_parse_error(err);
                if let Some(fd) = convert_diagnostic(&diag, &sm, &project_root) {
                    diagnostics.push(fd.diagnostic);
                }
            }
            return diagnostics;
        }
    };

    let (combined_source, segments) = preproc.take_combined_source_and_segments();
    let multi_source_map = MultiSourceMap::new(combined_source.clone(), segments);

    // Term files (.app, .app.src) contain a single Erlang term, not a module.
    // Parse them with parse_term() to avoid false "expected attribute" errors.
    let is_term_file = {
        let path = uri.as_str();
        path.ends_with(".app") || path.ends_with(".app.src")
    };

    let shared_interner = DefaultStringInterner::default();
    let mut parser = Parser::with_tokens(&combined_source, processed_tokens, shared_interner);

    if is_term_file {
        if let Err(err) = parser.parse_term() {
            let diag = NeraDiagnostic::from_parse_error(&err);
            if let Some(fd) = convert_diagnostic(&diag, &multi_source_map, &project_root) {
                diagnostics.push(fd.diagnostic);
            }
        }
    } else {
        let parse_result = parser.parse_module();
        for err in &parse_result.errors {
            let diag = NeraDiagnostic::from_parse_error(err);
            if let Some(fd) = convert_diagnostic(&diag, &multi_source_map, &project_root) {
                diagnostics.push(fd.diagnostic);
            }
        }
    }

    diagnostics
}

/// Walk up from `start_dir` looking for a project root.
///
/// 1. First pass: look for `nera.toml` (explicit project configuration).
/// 2. Second pass: if the file sits inside a `src/` directory, treat its
///    parent as the project root (standard Erlang/OTP layout convention).
pub(crate) fn find_project_root_from(start_dir: &Path) -> Option<PathBuf> {
    // Pass 1: walk up looking for nera.toml
    let mut dir = start_dir.to_path_buf();
    loop {
        if dir.join("nera.toml").is_file() {
            return Some(dir);
        }
        if !dir.pop() {
            break;
        }
    }

    // Pass 2: convention — if file is inside a "src/" or "include/"
    // directory, treat its parent as the project root (standard Erlang
    // layout: project/src/*.erl, project/include/*.hrl)
    let mut dir = start_dir.to_path_buf();
    loop {
        if let Some(name) = dir.file_name().and_then(|n| n.to_str()) {
            if name == "src" || name == "include" {
                if let Some(parent) = dir.parent() {
                    return Some(parent.to_path_buf());
                }
            }
        }
        if !dir.pop() {
            break;
        }
    }

    // Pass 3: if start_dir itself contains a src/ subdirectory, treat it
    // as the project root.  This handles the workspace root case where the
    // IDE opens the project directory directly.
    if start_dir.join("src").is_dir() {
        return Some(start_dir.to_path_buf());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_file_produces_no_diagnostics() {
        let uri = Url::parse("file:///tmp/myapp.app").unwrap();
        let source = r#"{application, myapp, [{vsn, "1.0"}, {modules, []}]}."#;
        let diags = check_single_file_from_source(&uri, source);
        assert!(
            diags.is_empty(),
            "expected no diagnostics for valid .app file, got: {:?}",
            diags
        );
    }

    #[test]
    fn app_src_file_produces_no_diagnostics() {
        let uri = Url::parse("file:///tmp/myapp.app.src").unwrap();
        let source = "{application, myapp, [{description, \"test\"}, {vsn, \"0.1.0\"}]}.";
        let diags = check_single_file_from_source(&uri, source);
        assert!(
            diags.is_empty(),
            "expected no diagnostics for valid .app.src file, got: {:?}",
            diags
        );
    }

    #[test]
    fn app_file_with_syntax_error_produces_diagnostic() {
        let uri = Url::parse("file:///tmp/broken.app").unwrap();
        let source = "{application, myapp, [}";
        let diags = check_single_file_from_source(&uri, source);
        assert!(
            !diags.is_empty(),
            "expected diagnostic for broken .app file"
        );
    }

    #[test]
    fn erl_file_still_uses_module_parsing() {
        let uri = Url::parse("file:///tmp/test.erl").unwrap();
        // A bare term is not a valid .erl file — should produce diagnostics
        let source = "{application, myapp, []}.";
        let diags = check_single_file_from_source(&uri, source);
        assert!(
            !diags.is_empty(),
            "expected diagnostics when parsing a term as .erl"
        );
    }

    /// Cross-module: a record in a .hrl used by one .erl but not another
    /// should NOT produce an unused_record warning.
    #[test]
    fn hrl_record_used_in_another_module_no_warning() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        let src_dir = root.join("src");
        std::fs::create_dir_all(&src_dir).unwrap();

        // Add nera.toml so paths resolve absolutely via project root
        std::fs::write(root.join("nera.toml"), r#"
[project]
name = "test_cross_module"
src = ["src"]
"#).unwrap();

        // Header defines a record
        std::fs::write(src_dir.join("defs.hrl"), r#"
-record(my_rec, {field1, field2}).
"#).unwrap();

        // Module A includes the header but does NOT use the record
        std::fs::write(src_dir.join("mod_a.erl"), r#"
-module(mod_a).
-include("defs.hrl").
-export([hello/0]).
hello() -> ok.
"#).unwrap();

        // Module B includes the header and USES the record
        std::fs::write(src_dir.join("mod_b.erl"), r#"
-module(mod_b).
-include("defs.hrl").
-export([make/0]).
make() -> #my_rec{field1 = a, field2 = b}.
"#).unwrap();

        let cache = std::sync::Mutex::new(HashMap::new());
        let interner = std::sync::Mutex::new(string_interner::DefaultStringInterner::default());
        let (diagnostics, _checked, _index, _modules, _) =
            check_for_lsp(&root, &cache, &interner);

        // There should be no unused_record warnings anywhere
        for (uri, diags) in &diagnostics {
            for d in diags {
                assert!(
                    !d.message.contains("record") || !d.message.contains("unused"),
                    "unexpected unused_record warning in {}: {}",
                    uri,
                    d.message
                );
            }
        }
    }

    /// Cross-module: a record in a .hrl NOT used by ANY module should still
    /// produce a warning.
    #[test]
    fn hrl_record_unused_in_all_modules_warns() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        let src_dir = root.join("src");
        std::fs::create_dir_all(&src_dir).unwrap();

        // Add nera.toml so paths resolve absolutely via project root
        std::fs::write(root.join("nera.toml"), r#"
[project]
name = "test_dead_rec"
src = ["src"]
"#).unwrap();

        // Header defines a record
        std::fs::write(src_dir.join("defs.hrl"), r#"
-record(dead_rec, {x}).
"#).unwrap();

        // Module A includes the header but does NOT use the record
        std::fs::write(src_dir.join("mod_a.erl"), r#"
-module(mod_a).
-include("defs.hrl").
-export([hello/0]).
hello() -> ok.
"#).unwrap();

        let cache = std::sync::Mutex::new(HashMap::new());
        let interner = std::sync::Mutex::new(string_interner::DefaultStringInterner::default());
        let (diagnostics, _checked, _index, _modules, _) =
            check_for_lsp(&root, &cache, &interner);

        // There should be an unused_record warning for dead_rec
        let has_unused_rec_warning = diagnostics.values().any(|diags| {
            diags.iter().any(|d| d.message.contains("'dead_rec'") && d.message.contains("unused"))
        });
        assert!(
            has_unused_rec_warning,
            "expected unused_record warning for 'dead_rec' not used in any module, diagnostics: {:?}",
            diagnostics.values().flat_map(|d| d.iter().map(|d| &d.message)).collect::<Vec<_>>()
        );
    }
}
