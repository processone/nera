use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::LanguageServer;

use nera_syntax::source_map::SourceMap;
use nera_syntax::span::Span;

use nera_syntax::semantic_tokens as sem;

use super::NeraBackend;
use super::{TOKEN_TYPES, TOKEN_MODIFIERS};
use super::types::*;
use super::util::*;
use super::cursor::*;
use super::references::*;
use super::source_map::*;
use super::check::*;
use super::semantic_tokens::encode_semantic_tokens;

#[tower_lsp::async_trait]
impl LanguageServer for NeraBackend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Store the workspace root — try root_uri first, then workspace_folders
        let root = params
            .root_uri
            .as_ref()
            .and_then(|u| u.to_file_path().ok())
            .or_else(|| {
                params
                    .workspace_folders
                    .as_ref()
                    .and_then(|folders| folders.first())
                    .and_then(|f| f.uri.to_file_path().ok())
            });
        if let Some(path) = root {
            let mut guard = self.root_path.lock().unwrap();
            *guard = Some(path);
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        save: Some(TextDocumentSyncSaveOptions::SaveOptions(
                            SaveOptions {
                                include_text: Some(false),
                            },
                        )),
                        ..Default::default()
                    },
                )),
                diagnostic_provider: Some(DiagnosticServerCapabilities::Options(
                    DiagnosticOptions {
                        inter_file_dependencies: true,
                        workspace_diagnostics: true,
                        ..Default::default()
                    },
                )),
                definition_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                references_provider: Some(OneOf::Left(true)),
                document_highlight_provider: Some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![":".into(), "#".into(), "?".into()]),
                    ..Default::default()
                }),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".into(), ",".into()]),
                    retrigger_characters: None,
                    work_done_progress_options: Default::default(),
                }),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                code_lens_provider: Some(CodeLensOptions {
                    resolve_provider: Some(false),
                }),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                inlay_hint_provider: Some(OneOf::Left(true)),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: Default::default(),
                })),
                document_formatting_provider: Some(OneOf::Left(true)),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: SemanticTokensLegend {
                                token_types: TOKEN_TYPES.to_vec(),
                                token_modifiers: TOKEN_MODIFIERS.to_vec(),
                            },
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            range: None,
                            ..Default::default()
                        },
                    ),
                ),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "nera".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "nera LSP server initialized")
            .await;
        self.run_check(None).await;
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        {
            let mut raw = self.raw_text.lock().unwrap();
            raw.insert(params.text_document.uri.clone(), params.text_document.text.clone());
        }
        self.cache_document(&params.text_document.uri, &params.text_document.text);
        self.run_check(Some(params.text_document.uri)).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        // With FULL sync, the last content change contains the full text.
        if let Some(change) = params.content_changes.last() {
            {
                let mut raw = self.raw_text.lock().unwrap();
                raw.insert(params.text_document.uri.clone(), change.text.clone());
            }
            self.cache_document(&params.text_document.uri, &change.text);

            // Bump generation and spawn a debounced fast syntax check.
            let gen = self.change_gen.fetch_add(1, Ordering::SeqCst) + 1;
            let gen_ref = self.change_gen.clone();
            let client = self.client.clone();
            let uri = params.text_document.uri.clone();
            let source = change.text.clone();
            let diag_cache = self.diag_cache.clone();

            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(300)).await;
                // If generation changed during sleep, a newer change supersedes us.
                if gen_ref.load(Ordering::SeqCst) != gen {
                    return;
                }
                let diags = check_single_file_from_source(&uri, &source);
                // Update the pull-diagnostics cache.
                {
                    let mut cache = diag_cache.lock().unwrap();
                    if diags.is_empty() {
                        cache.remove(&uri);
                    } else {
                        cache.insert(uri.clone(), diags.clone());
                    }
                }
                client.publish_diagnostics(uri, diags, None).await;
            });

            // Spawn a debounced full project check (1.5s) for warnings,
            // cross-file validation, and IR lowering — the same pipeline
            // that runs on save.
            let gen_ref2 = self.change_gen.clone();
            let last_gen = self.last_full_check_gen.clone();
            let client2 = self.client.clone();
            let uri2 = params.text_document.uri.clone();
            let diag_cache2 = self.diag_cache.clone();
            let symbol_index = self.symbol_index.clone();
            let project_modules = self.project_modules.clone();
            let root_path = self.root_path.lock().unwrap().clone();
            let parse_cache = self.parse_cache.clone();
            let persistent_interner = self.persistent_interner.clone();
            let check_lock = self.check_lock.clone();

            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(1500)).await;
                // Superseded by a newer change.
                if gen_ref2.load(Ordering::SeqCst) != gen {
                    return;
                }
                // Skip if nothing changed since the last full check.
                if last_gen.load(Ordering::SeqCst) == gen {
                    return;
                }

                let file_path = uri2.to_file_path().ok();
                let project_root = file_path
                    .as_ref()
                    .and_then(|fp| fp.parent())
                    .and_then(find_project_root_from)
                    .or_else(|| {
                        root_path.as_ref().and_then(|p| find_project_root_from(p))
                    });

                if let Some(root) = project_root {
                    // Serialize with other full project checks to prevent
                    // concurrent interner/parse-cache corruption.
                    let _guard = check_lock.lock().await;

                    // Re-check generation after acquiring the lock — a newer
                    // check may have completed while we were waiting.
                    if gen_ref2.load(Ordering::SeqCst) != gen {
                        return;
                    }

                    let (diagnostics, checked_uris, new_index, new_modules, _new_registry) =
                        tokio::task::spawn_blocking(move || {
                            super::check::check_for_lsp(&root, &parse_cache, &persistent_interner)
                        })
                            .await
                            .unwrap_or_else(|_| Default::default());

                    // A newer change arrived while we were checking — discard results.
                    if gen_ref2.load(Ordering::SeqCst) != gen {
                        return;
                    }

                    // Update caches.
                    {
                        let mut idx = symbol_index.lock().unwrap();
                        *idx = new_index;
                    }
                    {
                        let mut modules = project_modules.lock().unwrap();
                        *modules = new_modules;
                    }
                    {
                        let mut cache = diag_cache2.lock().unwrap();
                        for uri in &checked_uris {
                            if !diagnostics.contains_key(uri) {
                                cache.remove(uri);
                            }
                        }
                        for (uri, diags) in &diagnostics {
                            cache.insert(uri.clone(), diags.clone());
                        }
                    }

                    // Publish diagnostics.
                    for uri in &checked_uris {
                        client2.publish_diagnostics(uri.clone(), vec![], None).await;
                    }
                    for (uri, diags) in &diagnostics {
                        client2
                            .publish_diagnostics(uri.clone(), diags.clone(), None)
                            .await;
                    }

                    last_gen.store(gen, Ordering::SeqCst);
                }
            });
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.run_check(Some(params.text_document.uri)).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        // Clear diagnostics and cache for the closed file.
        {
            let mut cache = self.doc_cache.lock().unwrap();
            cache.remove(&params.text_document.uri);
        }
        {
            let mut raw = self.raw_text.lock().unwrap();
            raw.remove(&params.text_document.uri);
        }
        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri;

        // Get the latest raw source (always up-to-date, even when parsing fails).
        let raw_source = {
            let raw = self.raw_text.lock().unwrap();
            raw.get(&uri).cloned()
        };

        let result = {
            let cache = self.doc_cache.lock().unwrap();
            let doc = cache.get(&uri);

            // Use the full AST-based path only when the cache is fresh
            // (i.e. its original source matches the current raw text)
            // AND the parse had no errors (partial ASTs miss broken forms).
            let cache_is_fresh = match (&doc, &raw_source) {
                (Some(d), Some(raw)) => d.original_source == *raw,
                (Some(_), None) => true, // no raw_text yet, cache is fine
                _ => false,
            };
            let cache_has_errors = doc.as_ref().is_some_and(|d| d.has_parse_errors);

            if cache_is_fresh && !cache_has_errors {
                let doc = doc.unwrap();
                let mut sem_tokens = sem::collect_semantic_tokens_with_bif_check(
                    &doc.module,
                    &doc.source,
                    &nera_ir::bif_table::is_auto_import,
                );
                let mut macro_tokens = sem::collect_macro_tokens(&doc.original_source);
                sem_tokens.append(&mut macro_tokens);
                let mut type_tokens = sem::collect_type_tokens(&doc.original_source);
                sem_tokens.append(&mut type_tokens);
                sem_tokens.sort_by_key(|t| t.span.start);
                encode_semantic_tokens(&sem_tokens, &doc.source_map)
            } else if let Some(raw) = &raw_source {
                // Fallback: parsing failed or cache is stale — use lexer-based tokens
                // computed from the current source so highlights stay aligned.
                let mut sem_tokens = sem::collect_fallback_tokens(raw);
                let mut macro_tokens = sem::collect_macro_tokens(raw);
                sem_tokens.append(&mut macro_tokens);
                let mut type_tokens = sem::collect_type_tokens(raw);
                sem_tokens.append(&mut type_tokens);
                sem_tokens.sort_by_key(|t| t.span.start);
                let sm = nera_syntax::source_map::SourceMap::new(raw);
                encode_semantic_tokens(&sem_tokens, &sm)
            } else {
                return Ok(None);
            }
        };

        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data: result,
        })))
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let mut actions = Vec::new();
        let file_uri = params.text_document.uri;

        for diag in &params.context.diagnostics {
            if diag.code == Some(NumberOrString::String("unused_variable".to_string())) {
                if let Some(var_name) = extract_variable_name(&diag.message) {
                    let new_name = format!("_{}", var_name);
                    let edit = TextEdit::new(diag.range, new_name);

                    let doc_edit = TextDocumentEdit {
                        text_document: OptionalVersionedTextDocumentIdentifier {
                            uri: file_uri.clone(),
                            version: None,
                        },
                        edits: vec![OneOf::Left(edit)],
                    };

                    actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                        title: format!("Rename to _{}", var_name),
                        kind: Some(CodeActionKind::QUICKFIX),
                        diagnostics: Some(vec![diag.clone()]),
                        edit: Some(WorkspaceEdit {
                            document_changes: Some(DocumentChanges::Edits(vec![doc_edit])),
                            ..Default::default()
                        }),
                        is_preferred: Some(true),
                        ..Default::default()
                    }));
                }
            }

            if diag.code == Some(NumberOrString::String("unused_function".to_string())) {
                if let Some((fname, arity)) = extract_function_name_arity(&diag.message) {
                    // Fetch raw text to find where to insert the export attribute.
                    let raw = {
                        let rt = self.raw_text.lock().unwrap();
                        rt.get(&file_uri).cloned()
                    };
                    if let Some(raw_text) = raw {
                        let insert_line = find_line_after_module_attr(&raw_text);
                        let insert_pos = Position { line: insert_line, character: 0 };
                        let insert_range = Range::new(insert_pos, insert_pos);
                        let text = format!("-export([{}/{}]).\n", fname, arity);
                        let edit = TextEdit::new(insert_range, text);
                        let doc_edit = TextDocumentEdit {
                            text_document: OptionalVersionedTextDocumentIdentifier {
                                uri: file_uri.clone(),
                                version: None,
                            },
                            edits: vec![OneOf::Left(edit)],
                        };
                        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                            title: format!("Add {}/{} to -export list", fname, arity),
                            kind: Some(CodeActionKind::QUICKFIX),
                            diagnostics: Some(vec![diag.clone()]),
                            edit: Some(WorkspaceEdit {
                                document_changes: Some(DocumentChanges::Edits(vec![doc_edit])),
                                ..Default::default()
                            }),
                            is_preferred: Some(true),
                            ..Default::default()
                        }));
                    }
                }
            }

            if diag.code == Some(NumberOrString::String("missing_behaviour_callback".to_string())) {
                if let Some((cb_name, cb_arity)) = extract_function_name_arity(&diag.message) {
                    let raw = {
                        let rt = self.raw_text.lock().unwrap();
                        rt.get(&file_uri).cloned()
                    };
                    if let Some(raw_text) = raw {
                        // Build parameter list: _Arg1, _Arg2, ...
                        let params: Vec<String> = (1..=cb_arity)
                            .map(|i| format!("_Arg{}", i))
                            .collect();
                        let params_str = params.join(", ");
                        let stub = format!(
                            "\n{}({}) ->\n    erlang:error(not_implemented).\n",
                            cb_name, params_str,
                        );
                        // Insert at the end of the file.
                        let line_count = raw_text.lines().count() as u32;
                        let insert_pos = Position { line: line_count, character: 0 };
                        let insert_range = Range::new(insert_pos, insert_pos);
                        let edit = TextEdit::new(insert_range, stub);
                        let doc_edit = TextDocumentEdit {
                            text_document: OptionalVersionedTextDocumentIdentifier {
                                uri: file_uri.clone(),
                                version: None,
                            },
                            edits: vec![OneOf::Left(edit)],
                        };
                        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                            title: format!("Generate stub for {}/{}", cb_name, cb_arity),
                            kind: Some(CodeActionKind::QUICKFIX),
                            diagnostics: Some(vec![diag.clone()]),
                            edit: Some(WorkspaceEdit {
                                document_changes: Some(DocumentChanges::Edits(vec![doc_edit])),
                                ..Default::default()
                            }),
                            is_preferred: Some(true),
                            ..Default::default()
                        }));
                    }
                }
            }

            if diag.code == Some(NumberOrString::String("module_name_mismatch".to_string())) {
                if let Some((module_name, file_stem)) = extract_mismatch_names(&diag.message) {
                    // Quick fix 1: Change the module name to match the filename
                    let edit = TextEdit::new(diag.range, file_stem.to_string());
                    let doc_edit = TextDocumentEdit {
                        text_document: OptionalVersionedTextDocumentIdentifier {
                            uri: file_uri.clone(),
                            version: None,
                        },
                        edits: vec![OneOf::Left(edit)],
                    };
                    actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                        title: format!("Change -module({}) to -module({})", module_name, file_stem),
                        kind: Some(CodeActionKind::QUICKFIX),
                        diagnostics: Some(vec![diag.clone()]),
                        edit: Some(WorkspaceEdit {
                            document_changes: Some(DocumentChanges::Edits(vec![doc_edit])),
                            ..Default::default()
                        }),
                        is_preferred: Some(true),
                        ..Default::default()
                    }));

                    // Quick fix 2: Rename the file to match the module name
                    if let Ok(old_path) = file_uri.to_file_path() {
                        let new_path = old_path.with_file_name(format!("{}.erl", module_name));
                        if let Ok(new_uri) = Url::from_file_path(&new_path) {
                            let rename_op = DocumentChangeOperation::Op(ResourceOp::Rename(
                                RenameFile {
                                    old_uri: file_uri.clone(),
                                    new_uri,
                                    options: None,
                                    annotation_id: None,
                                },
                            ));
                            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                                title: format!("Rename file to {}.erl", module_name),
                                kind: Some(CodeActionKind::QUICKFIX),
                                diagnostics: Some(vec![diag.clone()]),
                                edit: Some(WorkspaceEdit {
                                    document_changes: Some(DocumentChanges::Operations(vec![rename_op])),
                                    ..Default::default()
                                }),
                                is_preferred: Some(false),
                                ..Default::default()
                            }));
                        }
                    }
                }
            }
        }

        // --- Code action: Generate -spec ---
        // If the cursor is inside a function that has no spec, offer to generate one.
        if let Some((module, source, offset_map)) = {
            let cache = self.doc_cache.lock().unwrap();
            cache.get(&file_uri).map(|doc| (
                doc.module.clone(),
                doc.source.clone(),
                doc.offset_map.clone(),
            ))
        } {
            // Use the selection range (cursor position) to find the function.
            let cursor_line = params.range.start.line;
            let sm = SourceMap::new(&source);
            for func in &module.functions {
                let func_range = span_to_original_lsp_range(
                    func.span,
                    &sm,
                    &sm,
                    &offset_map,
                );
                if cursor_line >= func_range.start.line
                    && cursor_line <= func_range.end.line
                {
                    let fname = span_text(&source, func.name_span);
                    // Check if a spec already exists.
                    let has_spec = module.specs.iter().any(|s| {
                        span_text(&source, s.name_span) == fname && s.arity == func.arity
                    });
                    if !has_spec {
                        let (param_types, return_type) =
                            super::type_infer::infer_function_spec(func, &source);
                        let spec_text =
                            super::type_infer::format_spec(fname, &param_types, &return_type);

                        // Insert at the beginning of the function's first line.
                        let insert_pos = Position {
                            line: func_range.start.line,
                            character: 0,
                        };
                        let insert_range = Range::new(insert_pos, insert_pos);
                        let edit = TextEdit::new(insert_range, format!("{}\n", spec_text));

                        let doc_edit = TextDocumentEdit {
                            text_document: OptionalVersionedTextDocumentIdentifier {
                                uri: file_uri.clone(),
                                version: None,
                            },
                            edits: vec![OneOf::Left(edit)],
                        };

                        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                            title: format!("Generate -spec for {}/{}", fname, func.arity),
                            kind: Some(CodeActionKind::SOURCE),
                            diagnostics: None,
                            edit: Some(WorkspaceEdit {
                                document_changes: Some(DocumentChanges::Edits(vec![doc_edit])),
                                ..Default::default()
                            }),
                            is_preferred: Some(false),
                            ..Default::default()
                        }));
                    }
                    break;
                }
            }
        }

        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;
        // Clone what we need from the cache then release the lock immediately,
        // so we don't block other handlers (e.g. run_check) while building the
        // response.
        let cached = {
            let cache = self.doc_cache.lock().unwrap();
            cache.get(&uri).map(|doc| (
                doc.module.clone(),
                doc.source.clone(),
                doc.source_map.clone(),
                doc.file_segments.clone(),
                doc.macro_locations.clone(),
            ))
        };

        let Some((module, source, sm, file_segments, macro_locations)) = cached else {
            return Ok(Some(DocumentSymbolResponse::Nested(vec![])));
        };

        // Determine the main file name from the URI so we can filter out
        // symbols that come from included header files.
        let main_file = uri.to_file_path().ok().map(|p| p.display().to_string());
        let is_from_include = |span: Span| -> bool {
            if file_segments.is_empty() {
                return false; // no includes, everything is main file
            }
            for seg in &file_segments {
                if span.start >= seg.start && span.start < seg.end {
                    // Check if this segment's filename matches the main file
                    if let Some(ref mf) = main_file {
                        return !mf.ends_with(&seg.filename) && !seg.filename.ends_with(mf.rsplit('/').next().unwrap_or(""));
                    }
                    return false;
                }
            }
            false
        };

        #[allow(deprecated)]
        let mut symbols: Vec<DocumentSymbol> = Vec::new();

        for func in &module.functions {
            if is_from_include(func.span) { continue; }
            let func_name = span_text(&source, func.name_span);
            let label = format!("{}/{}", func_name, func.arity);
            let is_exported = module.exports.iter().any(|fa| {
                fa.name == func.name && fa.arity == func.arity
            });
            let is_recursive = nera_ir::warnings::is_recursive(func);
            let detail = Some(match (is_exported, is_recursive) {
                (true, true) => "exported, recursive".to_string(),
                (true, false) => "exported".to_string(),
                (false, true) => "local, recursive".to_string(),
                (false, false) => "local".to_string(),
            });

            // Build children for function clauses (show parameters for all functions)
            #[allow(deprecated)]
            let children: Vec<DocumentSymbol> = func.clauses.iter().enumerate().map(|(i, clause)| {
                // Build clause label from patterns source text
                let params = clause.patterns.iter()
                    .map(|p| span_text(&source, pattern_span(p)))
                    .collect::<Vec<_>>()
                    .join(", ");
                let clause_label = format!("{}({})  [{}]", func_name, params, i + 1);

                DocumentSymbol {
                    name: clause_label,
                    detail: None,
                    kind: SymbolKind::FUNCTION,
                    tags: None,
                    deprecated: Some(false),
                    range: span_to_lsp_range(clause.span, &sm),
                    selection_range: span_to_lsp_range(clause.name_span, &sm),
                    children: None,
                }
            }).collect();

            #[allow(deprecated)]
            symbols.push(DocumentSymbol {
                name: label,
                detail,
                kind: SymbolKind::FUNCTION,
                tags: None,
                deprecated: Some(false),
                range: span_to_lsp_range(func.span, &sm),
                selection_range: span_to_lsp_range(func.name_span, &sm),
                children: Some(children),
            });
        }

        for rec in &module.records {
            if is_from_include(rec.span) { continue; }
            let rec_text = span_text(&source, rec.span);
            let end = rec_text
                .find([',', ' ', '{', '(', ')'])
                .unwrap_or(rec_text.len());
            let rec_name = &rec_text[..end];

            // Build children for record fields
            #[allow(deprecated)]
            let children: Vec<DocumentSymbol> = rec.fields.iter().map(|f| {
                let field_text = span_text(&source, f.span);
                let name_end = field_text.find(['=', ' ']).unwrap_or(field_text.len());
                let field_name = field_text[..name_end].trim().to_string();

                DocumentSymbol {
                    name: field_name,
                    detail: None,
                    kind: SymbolKind::FIELD,
                    tags: None,
                    deprecated: Some(false),
                    range: span_to_lsp_range(f.span, &sm),
                    selection_range: span_to_lsp_range(f.name_span, &sm),
                    children: None,
                }
            }).collect();

            #[allow(deprecated)]
            symbols.push(DocumentSymbol {
                name: format!("#{}", rec_name),
                detail: None,
                kind: SymbolKind::STRUCT,
                tags: None,
                deprecated: Some(false),
                range: span_to_lsp_range(rec.span, &sm),
                selection_range: span_to_lsp_range(rec.span, &sm),
                children: Some(children),
            });
        }

        for mac in &macro_locations {
            if is_from_include(mac.combined_span) { continue; }
            let name = match mac.arity {
                Some(n) => format!("?{}/{}", mac.name, n),
                None => format!("?{}", mac.name),
            };

            #[allow(deprecated)]
            symbols.push(DocumentSymbol {
                name,
                detail: Some("macro".to_string()),
                kind: SymbolKind::CONSTANT,
                tags: None,
                deprecated: Some(false),
                range: span_to_lsp_range(mac.combined_span, &sm),
                selection_range: span_to_lsp_range(mac.combined_span, &sm),
                children: None,
            });
        }

        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        let (module, source, original_source, _source_map, original_source_map, offset_map, _file_segments, _project_root, macro_locations) = {
            let cache = self.doc_cache.lock().unwrap();
            let Some(doc) = cache.get(&uri) else {
                return Ok(None);
            };
            (
                doc.module.clone(),
                doc.source.clone(),
                doc.original_source.clone(),
                doc.source_map.clone(),
                doc.original_source_map.clone(),
                doc.offset_map.clone(),
                doc.file_segments.clone(),
                doc.project_root.clone(),
                doc.macro_locations.clone(),
            )
        };

        let orig_offset = original_source_map.position_to_offset(pos.line, pos.character);

        // Check for macro hover
        if let Some(macro_name) = find_macro_at_offset(&original_source, orig_offset) {
            // Handle predefined macros first
            match macro_name.as_str() {
                "MODULE" => {
                    let mod_name = module.name_span;
                    let resolved = if mod_name.start < mod_name.end && mod_name.end <= source.len() {
                        &source[mod_name.start..mod_name.end]
                    } else {
                        "unknown"
                    };
                    let value = format!(
                        "```erlang\n?MODULE\n```\nPredefined macro — expands to the current module name `{}`",
                        resolved
                    );
                    return Ok(Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value,
                        }),
                        range: None,
                    }));
                }
                "MODULE_STRING" => {
                    let mod_name = module.name_span;
                    let resolved = if mod_name.start < mod_name.end && mod_name.end <= source.len() {
                        &source[mod_name.start..mod_name.end]
                    } else {
                        "unknown"
                    };
                    let value = format!(
                        "```erlang\n?MODULE_STRING\n```\nPredefined macro — expands to the module name as a string `\"{}\"`",
                        resolved
                    );
                    return Ok(Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value,
                        }),
                        range: None,
                    }));
                }
                "LINE" => {
                    // pos.line is 0-indexed, Erlang lines are 1-indexed
                    let line_number = pos.line + 1;
                    let value = format!(
                        "```erlang\n?LINE\n```\nPredefined macro — expands to the current line number (`{}`)",
                        line_number
                    );
                    return Ok(Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value,
                        }),
                        range: None,
                    }));
                }
                "FILE" => {
                    let filename = uri.to_file_path()
                        .ok()
                        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
                        .unwrap_or_else(|| "unknown".to_string());
                    let value = format!(
                        "```erlang\n?FILE\n```\nPredefined macro — expands to the current filename (`\"{}\"`)",
                        filename
                    );
                    return Ok(Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value,
                        }),
                        range: None,
                    }));
                }
                "FUNCTION_NAME" => {
                    // Find the enclosing function
                    let offset = map_original_to_combined(orig_offset, &offset_map);
                    let mut func_info = None;
                    for func in &module.functions {
                        if in_span(offset, func.span) {
                            let fname = span_text(&source, func.name_span);
                            func_info = Some(fname.to_string());
                            break;
                        }
                    }
                    let value = match func_info {
                        Some(name) => format!(
                            "```erlang\n?FUNCTION_NAME\n```\nPredefined macro — expands to the current function name (`{}`)",
                            name
                        ),
                        None => "```erlang\n?FUNCTION_NAME\n```\nPredefined macro — expands to the current function name".to_string(),
                    };
                    return Ok(Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value,
                        }),
                        range: None,
                    }));
                }
                "FUNCTION_ARITY" => {
                    let offset = map_original_to_combined(orig_offset, &offset_map);
                    let mut arity_info = None;
                    for func in &module.functions {
                        if in_span(offset, func.span) {
                            arity_info = Some(func.arity);
                            break;
                        }
                    }
                    let value = match arity_info {
                        Some(arity) => format!(
                            "```erlang\n?FUNCTION_ARITY\n```\nPredefined macro — expands to the current function arity (`{}`)",
                            arity
                        ),
                        None => "```erlang\n?FUNCTION_ARITY\n```\nPredefined macro — expands to the current function arity".to_string(),
                    };
                    return Ok(Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value,
                        }),
                        range: None,
                    }));
                }
                _ => {}
            }
            // User-defined macros
            if let Some(loc) = macro_locations.iter().find(|m| m.name == macro_name) {
                let value = format!("```erlang\n?{}\n```\nMacro defined at offset {}", macro_name, loc.combined_span.start);
                return Ok(Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value,
                    }),
                    range: None,
                }));
            }
        }

        // Check for type reference in type attributes
        if let Some(type_ref) = find_type_ref_at_offset(&original_source, orig_offset) {
            // Look up type definition
            let hover = if let Some(ref mod_name) = type_ref.module {
                // Remote type: look up in project index
                let idx = self.symbol_index.lock().unwrap();
                let mut found = None;
                for arity in 0..=10 {
                    let key = TypeKey {
                        module: mod_name.clone(),
                        type_name: type_ref.name.clone(),
                        arity,
                    };
                    if let Some(loc) = idx.types.get(&key) {
                        // Read the type definition from the file
                        if let Ok(src) = std::fs::read_to_string(&loc.file_path) {
                            let line_idx = loc.range.start.line as usize;
                            if let Some(line) = src.lines().nth(line_idx) {
                                // Find the full attribute line (may be on one line)
                                found = Some(format!("```erlang\n{}\n```\n{}:{}/{}",
                                    line.trim(), mod_name, type_ref.name, arity));
                            }
                        }
                        if found.is_none() {
                            found = Some(format!("```erlang\n{}:{}/{}\n```", mod_name, type_ref.name, arity));
                        }
                        break;
                    }
                }
                found
            } else {
                // Local type: find in module.type_defs
                let mut found = None;
                for td in &module.type_defs {
                    let td_name = span_text(&source, td.name_span);
                    if td_name == type_ref.name {
                        // Extract the full definition from source
                        let def_text = span_text(&source, td.span);
                        let kind = if td.is_opaque { "opaque" } else { "type" };
                        found = Some(format!("```erlang\n-{} {}\n```", kind, def_text));
                        break;
                    }
                }
                if found.is_none() && BUILTIN_TYPES.contains(&type_ref.name.as_str()) {
                    found = Some(format!("```erlang\n{}\n```\nBuilt-in type", type_ref.name));
                }
                found
            };
            if let Some(value) = hover {
                return Ok(Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value,
                    }),
                    range: None,
                }));
            }
        }

        let offset = map_original_to_combined(orig_offset, &offset_map);
        let target = match find_cursor_target(&module, offset, &source) {
            Some(t) => t,
            None => return Ok(None),
        };

        let hover_text = match target {
            CursorTarget::LocalCall { ref name, arity } | CursorTarget::LocalFunRef { ref name, arity } => {
                // Find the function in the current module and show its clauses
                let mut clauses_text = Vec::new();
                for func in &module.functions {
                    let func_name = span_text(&source, func.name_span);
                    if func_name == name.as_str() && func.arity == arity {
                        for clause in &func.clauses {
                            let params: Vec<String> = clause
                                .patterns
                                .iter()
                                .map(|p| span_text(&source, pattern_span(p)).to_string())
                                .collect();
                            clauses_text.push(format!("{}({})", name, params.join(", ")));
                        }
                        break;
                    }
                }
                let is_exported = module.functions.iter().any(|f| {
                    span_text(&source, f.name_span) == name.as_str()
                        && f.arity == arity
                        && module.exports.iter().any(|e| e.arity == arity && e.name == f.name)
                });
                // Look up matching -spec
                let spec_line = module.specs.iter().find_map(|s| {
                    let sname = span_text(&source, s.name_span);
                    if sname == name.as_str() && s.arity == arity && !s.is_callback {
                        Some(span_text(&source, s.span).to_string())
                    } else {
                        None
                    }
                });
                if clauses_text.is_empty() {
                    // Not defined locally — check BIF table
                    let bif = nera_ir::bif_table::BIF_TABLE.iter().find(|b| {
                        nera_ir::bif_table::is_auto_imported_entry(b)
                            && b.function == name.as_str()
                            && b.arity == arity
                    });
                    if let Some(b) = bif {
                        let bif_spec = b.spec.map(|s| format!("{}\n\n", s)).unwrap_or_default();
                        format!("```erlang\n{}{}:{}/{}\n```\nBuilt-in function (auto-imported)", bif_spec, b.module, b.function, b.arity)
                    } else {
                        format!("```erlang\n{}/{}\n```\n (local)", name, arity)
                    }
                } else {
                    let export_tag = if is_exported { " (exported)" } else { " (local)" };
                    let spec_section = spec_line.map(|s| format!("{}\n\n", s)).unwrap_or_default();
                    format!("```erlang\n{}{}\n```\n{}", spec_section, clauses_text.join("\n"), export_tag)
                }
            }
            CursorTarget::RemoteCall { ref module, ref function, arity }
            | CursorTarget::RemoteFunRef { ref module, ref function, arity } => {
                // Check BIF table first
                let bif = nera_ir::bif_table::BIF_TABLE.iter().find(|b| {
                    b.module == module.as_str()
                        && b.function == function.as_str()
                        && b.arity == arity
                });
                if let Some(b) = bif {
                    let auto = if nera_ir::bif_table::is_auto_imported_entry(b) { " (auto-imported)" } else { "" };
                    let bif_spec = b.spec.map(|s| format!("{}\n\n", s)).unwrap_or_default();
                    format!("```erlang\n{}{}:{}/{}\n```\nBuilt-in function{}", bif_spec, b.module, b.function, b.arity, auto)
                } else {
                    // Look up spec from the target module in project cache
                    let remote_spec = {
                        let project_modules = self.project_modules.lock().unwrap();
                        project_modules.iter().find_map(|cached| {
                            if cached.module_name == module.as_str() {
                                cached.module.specs.iter().find_map(|s| {
                                    let sname = span_text(&cached.source, s.name_span);
                                    if sname == function.as_str() && s.arity == arity && !s.is_callback {
                                        Some(span_text(&cached.source, s.span).to_string())
                                    } else {
                                        None
                                    }
                                })
                            } else {
                                None
                            }
                        })
                    };
                    let spec_section = remote_spec.map(|s| format!("{}\n\n", s)).unwrap_or_default();
                    format!("```erlang\n{}{}:{}/{}\n```", spec_section, module, function, arity)
                }
            }
            CursorTarget::Variable { ref name } => {
                format!("```erlang\n{}\n```\nVariable", name)
            }
            CursorTarget::RecordRef { ref name } => {
                // Show record fields
                let mut fields_text = String::new();
                // Search current module first
                for rec in &module.records {
                    let rec_text = span_text(&source, rec.span);
                    let end = rec_text
                        .find([',', ' ', '{', '(', ')'])
                        .unwrap_or(rec_text.len());
                    let rec_name = &rec_text[..end];
                    if rec_name == name.as_str() {
                        let field_names: Vec<String> = rec.fields.iter().map(|f| {
                            let ft = span_text(&source, f.span);
                            let e = ft.find(['=', ' ']).unwrap_or(ft.len());
                            ft[..e].trim().to_string()
                        }).collect();
                        fields_text = format!("{{{}}}", field_names.join(", "));
                        break;
                    }
                }
                if fields_text.is_empty() {
                    format!("```erlang\n#{}\n```\nRecord", name)
                } else {
                    format!("```erlang\n#{}{}  \n```\nRecord", name, fields_text)
                }
            }
        };

        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: hover_text,
            }),
            range: None,
        }))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;

        let (module, source, original_source_map, offset_map, source_map) = {
            let cache = self.doc_cache.lock().unwrap();
            let Some(doc) = cache.get(&uri) else {
                return Ok(None);
            };
            (
                doc.module.clone(),
                doc.source.clone(),
                doc.original_source_map.clone(),
                doc.offset_map.clone(),
                doc.source_map.clone(),
            )
        };

        let orig_offset = original_source_map.position_to_offset(pos.line, pos.character);
        let offset = map_original_to_combined(orig_offset, &offset_map);

        // Get the current module name for cross-file reference searches.
        let cur_mod_name = {
            let text = span_text(&source, module.span);
            let end = text.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(text.len());
            text[..end].to_string()
        };

        // First check if the cursor is on a function definition name (name_span).
        // find_cursor_target only looks inside clauses, so it misses the definition site.
        let target = if let Some(def_func) = module.functions.iter().find(|f| in_span(offset, f.name_span)) {
            let name = span_text(&source, def_func.name_span).to_string();
            Some(CursorTarget::LocalCall { name, arity: def_func.arity })
        } else {
            find_cursor_target(&module, offset, &source)
        };

        let target = match target {
            Some(t) => t,
            None => return Ok(None),
        };

        let mut locations = Vec::new();

        match target {
            CursorTarget::LocalCall { ref name, arity } | CursorTarget::LocalFunRef { ref name, arity } => {
                // Find all local call sites in the current module
                let spans = find_local_call_spans(&module, &source, name, arity);
                for span in spans {
                    locations.push(Location {
                        uri: uri.clone(),
                        range: span_to_lsp_range(span, &source_map),
                    });
                }
                // Also search all project modules for remote calls to this function
                // (e.g. other modules calling greeter:message/1)
                {
                    let project_modules = self.project_modules.lock().unwrap();
                    for cached in project_modules.iter() {
                        let spans = find_remote_call_spans(
                            &cached.module, &cached.source, &cur_mod_name, name, arity,
                        );
                        for span in spans {
                            if let Ok(file_uri) = Url::from_file_path(&cached.file_path) {
                                locations.push(Location {
                                    uri: file_uri,
                                    range: span_to_lsp_range(span, &cached.source_map),
                                });
                            }
                        }
                    }
                }
                // Also include the definition itself if requested
                if params.context.include_declaration {
                    for func in &module.functions {
                        let func_name = span_text(&source, func.name_span);
                        if func_name == name.as_str() && func.arity == arity {
                            locations.push(Location {
                                uri: uri.clone(),
                                range: span_to_lsp_range(func.name_span, &source_map),
                            });
                        }
                    }
                }
            }
            CursorTarget::RemoteCall { module: ref target_mod, ref function, arity }
            | CursorTarget::RemoteFunRef { module: ref target_mod, ref function, arity } => {
                // Search all cached modules for references to this remote call
                let project_modules = self.project_modules.lock().unwrap();
                for cached in project_modules.iter() {
                    let spans = find_remote_call_spans(&cached.module, &cached.source, target_mod, function, arity);
                    for span in spans {
                        if let Ok(file_uri) = Url::from_file_path(&cached.file_path) {
                            locations.push(Location {
                                uri: file_uri,
                                range: span_to_lsp_range(span, &cached.source_map),
                            });
                        }
                    }
                }
                // Also search the current file for local calls if it's the target module
                if cur_mod_name == *target_mod {
                    let local_spans = find_local_call_spans(&module, &source, function, arity);
                    for span in local_spans {
                        locations.push(Location {
                            uri: uri.clone(),
                            range: span_to_lsp_range(span, &source_map),
                        });
                    }
                }
            }
            CursorTarget::Variable { ref name } => {
                // Find all variable occurrences in the enclosing clause
                for func in &module.functions {
                    if !in_span(offset, func.span) { continue; }
                    for clause in &func.clauses {
                        if !in_span(offset, clause.span) { continue; }
                        let spans = find_variable_spans_in_clause(clause, &source, name);
                        for span in spans {
                            locations.push(Location {
                                uri: uri.clone(),
                                range: span_to_lsp_range(span, &source_map),
                            });
                        }
                    }
                }
            }
            CursorTarget::RecordRef { ref name } => {
                // Search all cached modules for record definitions and usages
                let project_modules = self.project_modules.lock().unwrap();
                for cached in project_modules.iter() {
                    // Record definitions
                    for rec in &cached.module.records {
                        let rec_text = span_text(&cached.source, rec.span);
                        let end = rec_text.find([',', ' ', '{', '(', ')']).unwrap_or(rec_text.len());
                        let rec_name = &rec_text[..end];
                        if rec_name == name.as_str() {
                            if let Ok(file_uri) = Url::from_file_path(&cached.file_path) {
                                locations.push(Location {
                                    uri: file_uri,
                                    range: span_to_lsp_range(rec.span, &cached.source_map),
                                });
                            }
                        }
                    }
                    // Record usages in expressions and patterns
                    let spans = find_record_ref_spans(&cached.module, &cached.source, name);
                    for span in spans {
                        if let Ok(file_uri) = Url::from_file_path(&cached.file_path) {
                            locations.push(Location {
                                uri: file_uri,
                                range: span_to_lsp_range(span, &cached.source_map),
                            });
                        }
                    }
                }
            }
        }

        if locations.is_empty() {
            Ok(None)
        } else {
            Ok(Some(locations))
        }
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        let (module, source, original_source_map, offset_map, source_map) = {
            let cache = self.doc_cache.lock().unwrap();
            let Some(doc) = cache.get(&uri) else {
                return Ok(None);
            };
            (
                doc.module.clone(),
                doc.source.clone(),
                doc.original_source_map.clone(),
                doc.offset_map.clone(),
                doc.source_map.clone(),
            )
        };

        let orig_offset = original_source_map.position_to_offset(pos.line, pos.character);
        let offset = map_original_to_combined(orig_offset, &offset_map);

        let cur_mod_name = {
            let text = span_text(&source, module.span);
            let end = text.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(text.len());
            text[..end].to_string()
        };

        // First check if the cursor is on a function definition name.
        let target = if let Some(def_func) = module.functions.iter().find(|f| in_span(offset, f.name_span)) {
            let name = span_text(&source, def_func.name_span).to_string();
            Some(CursorTarget::LocalCall { name, arity: def_func.arity })
        } else {
            find_cursor_target(&module, offset, &source)
        };

        let target = match target {
            Some(t) => t,
            None => return Ok(None),
        };

        let mut highlights: Vec<DocumentHighlight> = Vec::new();

        match target {
            CursorTarget::LocalCall { ref name, arity } | CursorTarget::LocalFunRef { ref name, arity } => {
                // Definition sites (each clause's name_span) → Write.
                for func in &module.functions {
                    let func_name = span_text(&source, func.name_span);
                    if func_name == name.as_str() && func.arity == arity {
                        for clause in &func.clauses {
                            highlights.push(DocumentHighlight {
                                range: span_to_lsp_range(clause.name_span, &source_map),
                                kind: Some(DocumentHighlightKind::WRITE),
                            });
                        }
                    }
                }
                // Local call sites / fun refs in this file → Read.
                for span in find_local_call_spans(&module, &source, name, arity) {
                    highlights.push(DocumentHighlight {
                        range: span_to_lsp_range(span, &source_map),
                        kind: Some(DocumentHighlightKind::READ),
                    });
                }
                // Remote refs to this module within the current file → Read.
                for span in find_remote_call_spans(&module, &source, &cur_mod_name, name, arity) {
                    highlights.push(DocumentHighlight {
                        range: span_to_lsp_range(span, &source_map),
                        kind: Some(DocumentHighlightKind::READ),
                    });
                }
            }
            CursorTarget::RemoteCall { module: ref target_mod, ref function, arity }
            | CursorTarget::RemoteFunRef { module: ref target_mod, ref function, arity } => {
                // Remote refs in the current file → Read.
                for span in find_remote_call_spans(&module, &source, target_mod, function, arity) {
                    highlights.push(DocumentHighlight {
                        range: span_to_lsp_range(span, &source_map),
                        kind: Some(DocumentHighlightKind::READ),
                    });
                }
                // If the target is this module, include local refs + definitions.
                if cur_mod_name == *target_mod {
                    for func in &module.functions {
                        let func_name = span_text(&source, func.name_span);
                        if func_name == function.as_str() && func.arity == arity {
                            for clause in &func.clauses {
                                highlights.push(DocumentHighlight {
                                    range: span_to_lsp_range(clause.name_span, &source_map),
                                    kind: Some(DocumentHighlightKind::WRITE),
                                });
                            }
                        }
                    }
                    for span in find_local_call_spans(&module, &source, function, arity) {
                        highlights.push(DocumentHighlight {
                            range: span_to_lsp_range(span, &source_map),
                            kind: Some(DocumentHighlightKind::READ),
                        });
                    }
                }
            }
            CursorTarget::Variable { ref name } => {
                // Find the enclosing clause and classify pattern vs expression occurrences.
                for func in &module.functions {
                    if !in_span(offset, func.span) { continue; }
                    for clause in &func.clauses {
                        if !in_span(offset, clause.span) { continue; }
                        for (span, kind) in find_variable_highlights_in_clause(clause, &source, name) {
                            highlights.push(DocumentHighlight {
                                range: span_to_lsp_range(span, &source_map),
                                kind: Some(kind),
                            });
                        }
                    }
                }
            }
            CursorTarget::RecordRef { .. } => {
                // Record highlights: out of scope for this size.
            }
        }

        if highlights.is_empty() {
            Ok(None)
        } else {
            Ok(Some(highlights))
        }
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let uri = params.text_document.uri;
        let pos = params.position;

        let (module, source, original_source_map, offset_map, source_map) = {
            let cache = self.doc_cache.lock().unwrap();
            let Some(doc) = cache.get(&uri) else {
                return Ok(None);
            };
            (
                doc.module.clone(),
                doc.source.clone(),
                doc.original_source_map.clone(),
                doc.offset_map.clone(),
                doc.source_map.clone(),
            )
        };

        let orig_offset = original_source_map.position_to_offset(pos.line, pos.character);
        let offset = map_original_to_combined(orig_offset, &offset_map);

        // Variable rename: cursor is on a variable occurrence inside a clause.
        if let Some(CursorTarget::Variable { ref name }) = find_cursor_target(&module, offset, &source) {
            for func in &module.functions {
                if !in_span(offset, func.span) { continue; }
                for clause in &func.clauses {
                    if !in_span(offset, clause.span) { continue; }
                    let spans = find_variable_spans_in_clause(clause, &source, name);
                    for span in spans {
                        if in_span(offset, span) {
                            let range = span_to_lsp_range(span, &source_map);
                            return Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
                                range,
                                placeholder: name.clone(),
                            }));
                        }
                    }
                }
            }
            return Ok(None);
        }

        // Function rename: identify the name span at the cursor.
        if let Some((span, name)) = function_rename_span_at(&module, &source, offset) {
            return Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
                range: span_to_lsp_range(span, &source_map),
                placeholder: name,
            }));
        }

        Ok(None)
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let new_name = params.new_name;

        let (module, source, original_source_map, offset_map, source_map) = {
            let cache = self.doc_cache.lock().unwrap();
            let Some(doc) = cache.get(&uri) else {
                return Ok(None);
            };
            (
                doc.module.clone(),
                doc.source.clone(),
                doc.original_source_map.clone(),
                doc.offset_map.clone(),
                doc.source_map.clone(),
            )
        };

        let orig_offset = original_source_map.position_to_offset(pos.line, pos.character);
        let offset = map_original_to_combined(orig_offset, &offset_map);

        // ── Variable rename ─────────────────────────────────────────────
        if let Some(CursorTarget::Variable { ref name }) =
            find_cursor_target(&module, offset, &source)
        {
            if !is_valid_variable_name(&new_name) {
                return Err(tower_lsp::jsonrpc::Error {
                    code: tower_lsp::jsonrpc::ErrorCode::InvalidParams,
                    message: format!(
                        "Invalid variable name '{}': must start with an uppercase letter or underscore",
                        new_name
                    ).into(),
                    data: None,
                });
            }
            for func in &module.functions {
                if !in_span(offset, func.span) { continue; }
                for clause in &func.clauses {
                    if !in_span(offset, clause.span) { continue; }
                    let spans = find_variable_spans_in_clause(clause, &source, name);
                    let edits: Vec<TextEdit> = spans
                        .into_iter()
                        .map(|span| TextEdit::new(
                            span_to_lsp_range(span, &source_map),
                            new_name.clone(),
                        ))
                        .collect();

                    if edits.is_empty() {
                        return Ok(None);
                    }

                    let doc_edit = TextDocumentEdit {
                        text_document: OptionalVersionedTextDocumentIdentifier {
                            uri: uri.clone(),
                            version: None,
                        },
                        edits: edits.into_iter().map(OneOf::Left).collect(),
                    };

                    return Ok(Some(WorkspaceEdit {
                        document_changes: Some(DocumentChanges::Edits(vec![doc_edit])),
                        ..Default::default()
                    }));
                }
            }
            return Ok(None);
        }

        // ── Function rename ─────────────────────────────────────────────
        let Some(target) = function_rename_target_at(&module, &source, offset) else {
            return Err(tower_lsp::jsonrpc::Error {
                code: tower_lsp::jsonrpc::ErrorCode::InvalidParams,
                message: "Rename is only supported for variables and functions".into(),
                data: None,
            });
        };

        if !is_valid_unquoted_atom(&new_name) {
            return Err(tower_lsp::jsonrpc::Error {
                code: tower_lsp::jsonrpc::ErrorCode::InvalidParams,
                message: format!(
                    "Invalid function name '{}': must be a lowercase atom",
                    new_name
                ).into(),
                data: None,
            });
        }

        let cur_mod_name = module_name_of(&module, &source);
        let FunctionRenameTarget { owner_module, name: func_name, arity } = target;

        let mut doc_edits: Vec<TextDocumentEdit> = Vec::new();
        let cur_file_path = uri.to_file_path().ok();

        // Edits in the defining module's file.
        // If the defining module is the currently-open file, edit its fresh
        // source. Otherwise find it in project_modules.
        if owner_module == cur_mod_name {
            let spans = collect_defining_module_spans(&module, &source, &func_name, arity);
            if !spans.is_empty() {
                let edits = spans
                    .into_iter()
                    .map(|s| TextEdit::new(span_to_lsp_range(s, &source_map), new_name.clone()))
                    .map(OneOf::Left)
                    .collect();
                doc_edits.push(TextDocumentEdit {
                    text_document: OptionalVersionedTextDocumentIdentifier {
                        uri: uri.clone(),
                        version: None,
                    },
                    edits,
                });
            }
        } else {
            let project_modules = self.project_modules.lock().unwrap();
            for cached in project_modules.iter() {
                let cached_mod_name = module_name_of(&cached.module, &cached.source);
                if cached_mod_name != owner_module { continue; }
                // Skip if this cached entry points at the currently-open file
                // — its AST may be stale.
                if let Some(ref cfp) = cur_file_path {
                    if &cached.file_path == cfp { continue; }
                }
                let Ok(file_uri) = Url::from_file_path(&cached.file_path) else { continue; };
                let spans = collect_defining_module_spans(
                    &cached.module, &cached.source, &func_name, arity,
                );
                if spans.is_empty() { continue; }
                let edits = spans
                    .into_iter()
                    .map(|s| TextEdit::new(span_to_lsp_range(s, &cached.source_map), new_name.clone()))
                    .map(OneOf::Left)
                    .collect();
                doc_edits.push(TextDocumentEdit {
                    text_document: OptionalVersionedTextDocumentIdentifier {
                        uri: file_uri,
                        version: None,
                    },
                    edits,
                });
                break;
            }
        }

        // Remote references in all other project modules.
        {
            let project_modules = self.project_modules.lock().unwrap();
            for cached in project_modules.iter() {
                // Skip the currently-open file (edited from fresh source).
                if let Some(ref cfp) = cur_file_path {
                    if &cached.file_path == cfp { continue; }
                }
                // Skip the defining module (already handled above).
                let cached_mod_name = module_name_of(&cached.module, &cached.source);
                if cached_mod_name == owner_module { continue; }
                let spans = find_remote_call_name_spans(
                    &cached.module, &cached.source, &owner_module, &func_name, arity,
                );
                if spans.is_empty() { continue; }
                let Ok(file_uri) = Url::from_file_path(&cached.file_path) else { continue; };
                let edits = spans
                    .into_iter()
                    .map(|s| TextEdit::new(span_to_lsp_range(s, &cached.source_map), new_name.clone()))
                    .map(OneOf::Left)
                    .collect();
                doc_edits.push(TextDocumentEdit {
                    text_document: OptionalVersionedTextDocumentIdentifier {
                        uri: file_uri,
                        version: None,
                    },
                    edits,
                });
            }
        }

        // Remote references in the currently-open file, when the target
        // function is defined elsewhere.
        if owner_module != cur_mod_name {
            let spans = find_remote_call_name_spans(
                &module, &source, &owner_module, &func_name, arity,
            );
            if !spans.is_empty() {
                let edits = spans
                    .into_iter()
                    .map(|s| TextEdit::new(span_to_lsp_range(s, &source_map), new_name.clone()))
                    .map(OneOf::Left)
                    .collect();
                doc_edits.push(TextDocumentEdit {
                    text_document: OptionalVersionedTextDocumentIdentifier {
                        uri: uri.clone(),
                        version: None,
                    },
                    edits,
                });
            }
        }

        if doc_edits.is_empty() {
            return Ok(None);
        }

        Ok(Some(WorkspaceEdit {
            document_changes: Some(DocumentChanges::Edits(doc_edits)),
            ..Default::default()
        }))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;

        // Use the latest raw text (always up-to-date, even when parsing fails)
        // for determining the completion context (line text, trigger character).
        let current_text = {
            let raw = self.raw_text.lock().unwrap();
            raw.get(&uri).cloned()
        };
        let current_text = match current_text {
            Some(t) => t,
            None => return Ok(None),
        };

        // Use a fresh SourceMap on the raw text for accurate position→offset.
        let raw_sm = SourceMap::new(&current_text);
        let raw_offset = raw_sm.position_to_offset(pos.line, pos.character);
        let raw_offset = raw_offset.min(current_text.len());

        // Read the cached parsed module (may be slightly stale, but useful for
        // variable scope and local function info).
        let cached = {
            let cache = self.doc_cache.lock().unwrap();
            cache.get(&uri).map(|doc| (
                doc.module.clone(),
                doc.source.clone(),
                doc.offset_map.clone(),
            ))
        };

        let mut items = Vec::new();

        // Determine completion context by looking at text before cursor
        let line_start = current_text[..raw_offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let line_text = &current_text[line_start..raw_offset];

        // Check if we're inside a type attribute for type completions
        let in_type_context = {
            // Scan backward from cursor to find attribute start
            let search_start = raw_offset.saturating_sub(2000);
            let before = &current_text[search_start..raw_offset];
            let attr_keywords = ["-spec", "-type", "-opaque", "-callback"];
            let mut found = false;
            for kw in &attr_keywords {
                if let Some(pos) = before.rfind(kw) {
                    let abs_pos = search_start + pos;
                    let is_line_start = abs_pos == 0 || current_text.as_bytes()[abs_pos - 1] == b'\n';
                    if is_line_start {
                        // Check we haven't passed the terminating dot
                        let after = &current_text[abs_pos..raw_offset];
                        // Simple heuristic: no dot-at-depth-0 between attr start and cursor
                        let mut depth: i32 = 0;
                        let mut past_dot = false;
                        for ch in after.chars() {
                            match ch {
                                '(' | '{' | '[' => depth += 1,
                                ')' | '}' | ']' => depth -= 1,
                                '.' if depth <= 0 => { past_dot = true; break; }
                                _ => {}
                            }
                        }
                        if !past_dot {
                            found = true;
                            break;
                        }
                    }
                }
            }
            found
        };
        // --- Completion: -spec / -callback function names with inferred types ---
        let in_spec_name_context = {
            let trimmed = line_text.trim_start();
            if (trimmed.starts_with("-spec ") || trimmed.starts_with("-callback "))
                && !trimmed.contains("->")
                && !trimmed.contains('(')
            {
                let kw_len = if trimmed.starts_with("-spec ") { 6 } else { 10 };
                Some(trimmed[kw_len..].to_string())
            } else {
                None
            }
        };

        if let Some(prefix) = in_spec_name_context {
            if let Some((ref module, ref source, _)) = cached {
                for func in &module.functions {
                    let fname = span_text(source, func.name_span);
                    if !fname.starts_with(&prefix) {
                        continue;
                    }
                    // Skip functions that already have a spec.
                    let has_spec = module.specs.iter().any(|s| {
                        span_text(source, s.name_span) == fname && s.arity == func.arity
                    });
                    if has_spec {
                        continue;
                    }

                    let (param_types, return_type) =
                        super::type_infer::infer_function_spec(func, source);
                    let snippet =
                        super::type_infer::format_spec_snippet(fname, &param_types, &return_type);

                    items.push(CompletionItem {
                        label: format!("{}/{}", fname, func.arity),
                        kind: Some(CompletionItemKind::FUNCTION),
                        detail: Some("generate spec".into()),
                        insert_text: Some(snippet),
                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                        ..Default::default()
                    });
                }
            }

            // Short-circuit: don't offer module/variable/BIF completions in spec context.
            // Deduplicate
            let mut seen = std::collections::HashSet::new();
            items.retain(|item| seen.insert(item.label.clone()));
            return Ok(Some(CompletionResponse::Array(items)));
        }

        // Check for module:function completion (after ':')
        if let Some(colon_pos) = line_text.rfind(':') {
            let before_colon = line_text[..colon_pos].trim_start();
            // Extract module name (last word before colon)
            let mod_name = before_colon.split(|c: char| !c.is_alphanumeric() && c != '_')
                .next_back()
                .unwrap_or("");
            let after_colon = &line_text[colon_pos + 1..];

            if !mod_name.is_empty() {
                // Complete functions from that module via ProjectIndex
                let idx = self.symbol_index.lock().unwrap();
                for key in idx.functions.keys() {
                    if key.module == mod_name && key.function.starts_with(after_colon) {
                        items.push(CompletionItem {
                            label: format!("{}/{}", key.function, key.arity),
                            kind: Some(CompletionItemKind::FUNCTION),
                            detail: Some(format!("{}:{}/{}", key.module, key.function, key.arity)),
                            insert_text: Some(key.function.clone()),
                            ..Default::default()
                        });
                    }
                }
                // Also check BIF table for that module
                for bif in nera_ir::bif_table::BIF_TABLE {
                    if bif.module == mod_name && bif.function.starts_with(after_colon) {
                        items.push(CompletionItem {
                            label: format!("{}/{}", bif.function, bif.arity),
                            kind: Some(CompletionItemKind::FUNCTION),
                            detail: Some(format!("{}:{}/{} (built-in)", bif.module, bif.function, bif.arity)),
                            insert_text: Some(bif.function.to_string()),
                            ..Default::default()
                        });
                    }
                }
                // Complete remote types when in type context
                if in_type_context {
                    for key in idx.types.keys() {
                        if key.module == mod_name && key.type_name.starts_with(after_colon) {
                            items.push(CompletionItem {
                                label: format!("{}/{}", key.type_name, key.arity),
                                kind: Some(CompletionItemKind::TYPE_PARAMETER),
                                detail: Some(format!("{}:{}/{}", key.module, key.type_name, key.arity)),
                                insert_text: Some(key.type_name.clone()),
                                ..Default::default()
                            });
                        }
                    }
                }
            }
        }
        // Check for record completion (after '#')
        else if let Some(hash_pos) = line_text.rfind('#') {
            let after_hash = &line_text[hash_pos + 1..];
            // Check if we're inside record field completion (after '{')
            if let Some(brace_pos) = after_hash.find('{') {
                let rec_name = &after_hash[..brace_pos];
                let after_brace = &after_hash[brace_pos + 1..];
                let prefix = after_brace.split(',').next_back().unwrap_or("").trim();
                // Find record fields from cached module (if available)
                if let Some((ref module, ref source, _)) = cached {
                    for rec in &module.records {
                        let rt = span_text(source, rec.span);
                        let end = rt.find([',', ' ', '{', '(', ')']).unwrap_or(rt.len());
                        if &rt[..end] == rec_name {
                            for field in &rec.fields {
                                let ft = span_text(source, field.span);
                                let fe = ft.find(['=', ' ']).unwrap_or(ft.len());
                                let fname = ft[..fe].trim();
                                if fname.starts_with(prefix) {
                                    items.push(CompletionItem {
                                        label: fname.to_string(),
                                        kind: Some(CompletionItemKind::FIELD),
                                        detail: Some(format!("#{}.", rec_name)),
                                        insert_text: Some(format!("{} = ", fname)),
                                        ..Default::default()
                                    });
                                }
                            }
                        }
                    }
                }
            } else {
                // Complete record names
                let prefix = after_hash;
                if let Some((ref module, ref source, _)) = cached {
                    for rec in &module.records {
                        let rt = span_text(source, rec.span);
                        let end = rt.find([',', ' ', '{', '(', ')']).unwrap_or(rt.len());
                        let rn = &rt[..end];
                        if rn.starts_with(prefix) {
                            items.push(CompletionItem {
                                label: rn.to_string(),
                                kind: Some(CompletionItemKind::STRUCT),
                                detail: Some("record".to_string()),
                                ..Default::default()
                            });
                        }
                    }
                }
                // Also from ProjectIndex
                let idx = self.symbol_index.lock().unwrap();
                for rec_name in idx.records.keys() {
                    if rec_name.starts_with(after_hash) {
                        items.push(CompletionItem {
                            label: rec_name.clone(),
                            kind: Some(CompletionItemKind::STRUCT),
                            detail: Some("record".to_string()),
                            ..Default::default()
                        });
                    }
                }
            }
        }
        // Macro completion (after '?')
        else if line_text.ends_with('?') || (line_text.contains('?') && !line_text.ends_with(',') && !line_text.ends_with(' ')) {
            let prefix = if let Some(q_pos) = line_text.rfind('?') {
                &line_text[q_pos + 1..]
            } else {
                ""
            };
            // Predefined macros
            let predefined = [
                ("MODULE", "Expands to the current module name"),
                ("MODULE_STRING", "Expands to the module name as a string"),
                ("LINE", "Expands to the current line number"),
                ("FILE", "Expands to the current filename"),
                ("FUNCTION_NAME", "Expands to the current function name"),
                ("FUNCTION_ARITY", "Expands to the current function arity"),
                ("OTP_RELEASE", "Expands to the OTP release number"),
                ("MACHINE", "Expands to the machine name (e.g. 'BEAM')"),
            ];
            for (name, detail) in predefined {
                if name.starts_with(prefix) {
                    items.push(CompletionItem {
                        label: name.to_string(),
                        kind: Some(CompletionItemKind::CONSTANT),
                        detail: Some(detail.to_string()),
                        ..Default::default()
                    });
                }
            }
            // User-defined macros from the preprocessor
            {
                let cache = self.doc_cache.lock().unwrap();
                if let Some(doc) = cache.get(&uri) {
                    for loc in &doc.macro_locations {
                        if loc.name.starts_with(prefix) {
                            items.push(CompletionItem {
                                label: loc.name.clone(),
                                kind: Some(CompletionItemKind::CONSTANT),
                                detail: Some("user-defined macro".to_string()),
                                ..Default::default()
                            });
                        }
                    }
                }
            }
        }
        // General completion (bare identifier)
        else {
            // Extract the prefix being typed
            let word_start = line_text.rfind(|c: char| !c.is_alphanumeric() && c != '_')
                .map(|i| i + 1)
                .unwrap_or(0);
            let prefix = &line_text[word_start..];
            let is_variable = prefix.starts_with(|c: char| c.is_uppercase() || c == '_');

            if is_variable {
                // Complete variables in scope (requires a successful parse)
                if let Some((ref module, ref source, ref offset_map)) = cached {
                    let offset = map_original_to_combined(raw_offset, offset_map);
                    for func in &module.functions {
                        if !in_span(offset, func.span) { continue; }
                        for clause in &func.clauses {
                            if !in_span(offset, clause.span) { continue; }
                            let mut bindings = Vec::new();
                            for pat in &clause.patterns {
                                collect_pattern_bindings(pat, source, &mut bindings);
                            }
                            collect_body_bindings(&clause.body, source, &mut bindings);
                            // Deduplicate and filter by prefix
                            let mut seen = std::collections::HashSet::new();
                            for (name, _span) in &bindings {
                                if name.starts_with(prefix) && seen.insert(name.clone()) {
                                    items.push(CompletionItem {
                                        label: name.clone(),
                                        kind: Some(CompletionItemKind::VARIABLE),
                                        ..Default::default()
                                    });
                                }
                            }
                        }
                    }
                }
            } else {
                // Complete local functions (from cached parse if available)
                if let Some((ref module, ref source, _)) = cached {
                    for func in &module.functions {
                        let fname = span_text(source, func.name_span);
                        if fname.starts_with(prefix) {
                            items.push(CompletionItem {
                                label: format!("{}/{}", fname, func.arity),
                                kind: Some(CompletionItemKind::FUNCTION),
                                insert_text: Some(fname.to_string()),
                                detail: if module.exports.iter().any(|e| e.name == func.name && e.arity == func.arity) {
                                    Some("exported".to_string())
                                } else {
                                    Some("local".to_string())
                                },
                                ..Default::default()
                            });
                        }
                    }
                }
                // Complete auto-imported BIFs
                for bif in nera_ir::bif_table::BIF_TABLE {
                    if nera_ir::bif_table::is_auto_imported_entry(bif) && bif.function.starts_with(prefix) {
                        items.push(CompletionItem {
                            label: format!("{}/{}", bif.function, bif.arity),
                            kind: Some(CompletionItemKind::FUNCTION),
                            insert_text: Some(bif.function.to_string()),
                            detail: Some(format!("{}:{} (built-in)", bif.module, bif.function)),
                            ..Default::default()
                        });
                    }
                }
                // Complete module names
                let idx = self.symbol_index.lock().unwrap();
                for mod_name in idx.modules.keys() {
                    if mod_name.starts_with(prefix) {
                        items.push(CompletionItem {
                            label: mod_name.clone(),
                            kind: Some(CompletionItemKind::MODULE),
                            detail: Some("module".to_string()),
                            ..Default::default()
                        });
                    }
                }

                // Complete type names when inside a type attribute
                if in_type_context {
                    // Built-in types
                    for bt in BUILTIN_TYPES {
                        if bt.starts_with(prefix) {
                            items.push(CompletionItem {
                                label: format!("{}()", bt),
                                kind: Some(CompletionItemKind::TYPE_PARAMETER),
                                insert_text: Some(bt.to_string()),
                                detail: Some("built-in type".to_string()),
                                ..Default::default()
                            });
                        }
                    }
                    // Current module's type definitions
                    if let Some((ref module, ref source, _)) = cached {
                        for td in &module.type_defs {
                            let tname = span_text(source, td.name_span);
                            if tname.starts_with(prefix) {
                                let kind = if td.is_opaque { "opaque" } else { "type" };
                                items.push(CompletionItem {
                                    label: format!("{}/{}", tname, td.arity),
                                    kind: Some(CompletionItemKind::TYPE_PARAMETER),
                                    insert_text: Some(tname.to_string()),
                                    detail: Some(format!("{} ({})", tname, kind)),
                                    ..Default::default()
                                });
                            }
                        }
                    }
                    // Types from project index
                    for key in idx.types.keys() {
                        if key.type_name.starts_with(prefix) {
                            items.push(CompletionItem {
                                label: format!("{}:{}/{}", key.module, key.type_name, key.arity),
                                kind: Some(CompletionItemKind::TYPE_PARAMETER),
                                insert_text: Some(format!("{}:{}", key.module, key.type_name)),
                                detail: Some(format!("type from {}", key.module)),
                                ..Default::default()
                            });
                        }
                    }
                }
            }
        }

        // Deduplicate by label
        let mut seen = std::collections::HashSet::new();
        items.retain(|item| seen.insert(item.label.clone()));

        if items.is_empty() {
            Ok(None)
        } else {
            Ok(Some(CompletionResponse::Array(items)))
        }
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        // Use raw_text for accurate position mapping (same reason as completion:
        // the parsed cache may be stale when the file has syntax errors).
        let current_text = {
            let raw = self.raw_text.lock().unwrap();
            raw.get(&uri).cloned()
        };
        let current_text = match current_text {
            Some(t) => t,
            None => return Ok(None),
        };

        let raw_sm = SourceMap::new(&current_text);
        let raw_offset = raw_sm.position_to_offset(pos.line, pos.character)
            .min(current_text.len());

        // Read cached module for local function lookups
        let cached = {
            let cache = self.doc_cache.lock().unwrap();
            cache.get(&uri).map(|doc| (
                doc.module.clone(),
                doc.source.clone(),
            ))
        };

        // Scan backwards from cursor to find the enclosing function call
        // Count commas to determine active parameter
        let text_before = &current_text[..raw_offset];
        let mut paren_depth = 0i32;
        let mut comma_count = 0u32;
        let mut call_start = None;

        for (i, ch) in text_before.char_indices().rev() {
            match ch {
                ')' => paren_depth += 1,
                '(' => {
                    if paren_depth == 0 {
                        call_start = Some(i);
                        break;
                    }
                    paren_depth -= 1;
                }
                ',' if paren_depth == 0 => comma_count += 1,
                _ => {}
            }
        }

        let call_start = match call_start {
            Some(s) => s,
            None => return Ok(None),
        };

        // Extract the function name/module before the opening paren
        let before_paren = &current_text[..call_start];
        let name_end = call_start;
        let name_start = before_paren
            .rfind(|c: char| !c.is_alphanumeric() && c != '_' && c != ':')
            .map(|i| i + 1)
            .unwrap_or(0);
        let call_text = &current_text[name_start..name_end];

        let mut signatures = Vec::new();

        if let Some(colon_pos) = call_text.rfind(':') {
            // Remote call: module:function
            let mod_name = &call_text[..colon_pos];
            let fun_name = &call_text[colon_pos + 1..];

            // Check BIF table
            for bif in nera_ir::bif_table::BIF_TABLE {
                if bif.module == mod_name && bif.function == fun_name {
                    let params: Vec<ParameterInformation> = (0..bif.arity)
                        .map(|i| ParameterInformation {
                            label: ParameterLabel::Simple(format!("Arg{}", i + 1)),
                            documentation: None,
                        })
                        .collect();
                    signatures.push(SignatureInformation {
                        label: format!("{}:{}/{}", bif.module, bif.function, bif.arity),
                        documentation: Some(Documentation::String("Built-in function".to_string())),
                        parameters: Some(params),
                        active_parameter: None,
                    });
                }
            }

            // Check project modules
            let project_modules = self.project_modules.lock().unwrap();
            for cached in project_modules.iter() {
                if cached.module_name == mod_name {
                    for func in &cached.module.functions {
                        let fname = span_text(&cached.source, func.name_span);
                        if fname == fun_name {
                            for clause in &func.clauses {
                                let params: Vec<ParameterInformation> = clause
                                    .patterns
                                    .iter()
                                    .map(|p| ParameterInformation {
                                        label: ParameterLabel::Simple(
                                            span_text(&cached.source, pattern_span(p)).to_string(),
                                        ),
                                        documentation: None,
                                    })
                                    .collect();
                                let clause_label: Vec<String> = clause
                                    .patterns
                                    .iter()
                                    .map(|p| span_text(&cached.source, pattern_span(p)).to_string())
                                    .collect();
                                signatures.push(SignatureInformation {
                                    label: format!("{}:{}({})", mod_name, fun_name, clause_label.join(", ")),
                                    documentation: None,
                                    parameters: Some(params),
                                    active_parameter: None,
                                });
                            }
                        }
                    }
                }
            }
        } else {
            // Local call
            let fun_name = call_text;

            // Check local functions (from cached parse if available)
            if let Some((ref module, ref source)) = cached {
                for func in &module.functions {
                    let fname = span_text(source, func.name_span);
                    if fname == fun_name {
                        for clause in &func.clauses {
                            let params: Vec<ParameterInformation> = clause
                                .patterns
                                .iter()
                                .map(|p| ParameterInformation {
                                    label: ParameterLabel::Simple(
                                        span_text(source, pattern_span(p)).to_string(),
                                    ),
                                    documentation: None,
                                })
                                .collect();
                            let clause_label: Vec<String> = clause
                                .patterns
                                .iter()
                                .map(|p| span_text(source, pattern_span(p)).to_string())
                                .collect();
                            signatures.push(SignatureInformation {
                                label: format!("{}({})", fun_name, clause_label.join(", ")),
                                documentation: None,
                                parameters: Some(params),
                                active_parameter: None,
                            });
                        }
                    }
                }
            }

            // Check auto-imported BIFs
            for bif in nera_ir::bif_table::BIF_TABLE {
                if nera_ir::bif_table::is_auto_imported_entry(bif) && bif.function == fun_name {
                    let params: Vec<ParameterInformation> = (0..bif.arity)
                        .map(|i| ParameterInformation {
                            label: ParameterLabel::Simple(format!("Arg{}", i + 1)),
                            documentation: None,
                        })
                        .collect();
                    signatures.push(SignatureInformation {
                        label: format!("{}/{}", bif.function, bif.arity),
                        documentation: Some(Documentation::String("Built-in function".to_string())),
                        parameters: Some(params),
                        active_parameter: None,
                    });
                }
            }
        }

        if signatures.is_empty() {
            Ok(None)
        } else {
            Ok(Some(SignatureHelp {
                signatures,
                active_signature: Some(0),
                active_parameter: Some(comma_count),
            }))
        }
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        let query = params.query.to_lowercase();
        let idx = self.symbol_index.lock().unwrap();

        #[allow(deprecated)]
        let mut symbols: Vec<SymbolInformation> = Vec::new();

        for (key, loc) in &idx.functions {
            let label = format!("{}:{}/{}", key.module, key.function, key.arity);
            if query.is_empty() || label.to_lowercase().contains(&query) {
                if let Ok(file_uri) = Url::from_file_path(&loc.file_path) {
                    #[allow(deprecated)]
                    symbols.push(SymbolInformation {
                        name: format!("{}/{}", key.function, key.arity),
                        kind: SymbolKind::FUNCTION,
                        tags: None,
                        deprecated: Some(false),
                        location: Location {
                            uri: file_uri,
                            range: loc.range,
                        },
                        container_name: Some(key.module.clone()),
                    });
                }
            }
        }

        for (rec_name, locs) in &idx.records {
            if query.is_empty() || rec_name.to_lowercase().contains(&query) {
                if let Some(loc) = locs.first() {
                    if let Ok(file_uri) = Url::from_file_path(&loc.file_path) {
                        #[allow(deprecated)]
                        symbols.push(SymbolInformation {
                            name: format!("#{}", rec_name),
                            kind: SymbolKind::STRUCT,
                            tags: None,
                            deprecated: Some(false),
                            location: Location {
                                uri: file_uri,
                                range: loc.range,
                            },
                            container_name: None,
                        });
                    }
                }
            }
        }

        // Macros from open documents
        {
            let cache = self.doc_cache.lock().unwrap();
            for (uri, doc) in cache.iter() {
                let sm = &doc.source_map;
                for mac in &doc.macro_locations {
                    let name = match mac.arity {
                        Some(n) => format!("?{}/{}", mac.name, n),
                        None => format!("?{}", mac.name),
                    };
                    if query.is_empty() || name.to_lowercase().contains(&query) {
                        #[allow(deprecated)]
                        symbols.push(SymbolInformation {
                            name,
                            kind: SymbolKind::CONSTANT,
                            tags: None,
                            deprecated: Some(false),
                            location: Location {
                                uri: uri.clone(),
                                range: span_to_lsp_range(mac.combined_span, sm),
                            },
                            container_name: None,
                        });
                    }
                }
            }
        }

        if symbols.is_empty() {
            Ok(None)
        } else {
            Ok(Some(symbols))
        }
    }

    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        let uri = params.text_document.uri;

        let cached = {
            let cache = self.doc_cache.lock().unwrap();
            cache.get(&uri).map(|doc| (
                doc.module.clone(),
                doc.source.clone(),
                doc.source_map.clone(),
                doc.file_segments.clone(),
            ))
        };

        let Some((module, source, sm, file_segments)) = cached else {
            return Ok(None);
        };

        // Get the main file name for filtering includes
        let main_file = uri.to_file_path().ok().map(|p| p.display().to_string());
        let is_from_include = |span: Span| -> bool {
            if file_segments.is_empty() { return false; }
            for seg in &file_segments {
                if span.start >= seg.start && span.start < seg.end {
                    if let Some(ref mf) = main_file {
                        return !mf.ends_with(&seg.filename)
                            && !seg.filename.ends_with(mf.rsplit('/').next().unwrap_or(""));
                    }
                    return false;
                }
            }
            false
        };

        // Get current module name for cross-file search
        let cur_mod_name = {
            let text = span_text(&source, module.span);
            let end = text.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(text.len());
            text[..end].to_string()
        };

        let mut lenses = Vec::new();

        for func in &module.functions {
            if is_from_include(func.span) { continue; }

            let func_name = span_text(&source, func.name_span).to_string();

            // Count local references in this module
            let local_refs = find_local_call_spans(&module, &source, &func_name, func.arity);
            let mut ref_count = local_refs.len();

            // Count remote references across all project modules
            {
                let project_modules = self.project_modules.lock().unwrap();
                for cached_mod in project_modules.iter() {
                    let remote_refs = find_remote_call_spans(
                        &cached_mod.module, &cached_mod.source,
                        &cur_mod_name, &func_name, func.arity,
                    );
                    ref_count += remote_refs.len();
                }
            }

            let range = span_to_lsp_range(func.name_span, &sm);
            let title = if ref_count == 0 {
                "no references".to_string()
            } else if ref_count == 1 {
                "1 reference".to_string()
            } else {
                format!("{} references", ref_count)
            };

            lenses.push(CodeLens {
                range,
                command: Some(Command {
                    title,
                    command: "nera.findReferences".to_string(),
                    arguments: Some(vec![
                        serde_json::to_value(uri.to_string()).unwrap(),
                        serde_json::to_value(range.start.line).unwrap(),
                        serde_json::to_value(range.start.character).unwrap(),
                    ]),
                }),
                data: None,
            });
        }

        if lenses.is_empty() {
            Ok(None)
        } else {
            Ok(Some(lenses))
        }
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let uri = params.text_document.uri;

        let cached = {
            let cache = self.doc_cache.lock().unwrap();
            cache.get(&uri).map(|doc| (
                doc.module.clone(),
                doc.source.clone(),
                doc.source_map.clone(),
            ))
        };

        let Some((module, source, sm)) = cached else {
            return Ok(None);
        };

        let mut hints = Vec::new();

        // Walk all function bodies looking for call expressions
        for func in &module.functions {
            for clause in &func.clauses {
                collect_inlay_hints_from_exprs(
                    &clause.body, &module, &source, &sm, &self.project_modules,
                    clause.span, &mut hints,
                );
            }
        }

        if hints.is_empty() {
            Ok(None)
        } else {
            Ok(Some(hints))
        }
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        let (module, source, original_source, source_map, original_source_map, offset_map, file_segments, project_root, macro_locations) = {
            let cache = self.doc_cache.lock().unwrap();
            let Some(doc) = cache.get(&uri) else {
                return Ok(None);
            };
            (
                doc.module.clone(),
                doc.source.clone(),
                doc.original_source.clone(),
                doc.source_map.clone(),
                doc.original_source_map.clone(),
                doc.offset_map.clone(),
                doc.file_segments.clone(),
                doc.project_root.clone(),
                doc.macro_locations.clone(),
            )
        };

        // Convert LSP position (in original file) to byte offset in the combined source.
        let orig_offset = original_source_map.position_to_offset(pos.line, pos.character);

        // Check if cursor is on an -include("...") or -include_lib("...") directive.
        if let Some(include_path) = find_include_at_offset(&original_source, orig_offset) {
            // Try to resolve the include path to an actual file.
            // First check file_segments — the preprocessor already resolved it.
            if let Some(seg) = file_segments.iter().find(|s| s.filename.ends_with(&include_path) || include_path.ends_with(&s.filename)) {
                let resolved = if Path::new(&seg.filename).is_absolute() {
                    PathBuf::from(&seg.filename)
                } else if let Some(ref root) = project_root {
                    let candidate = root.join(&seg.filename);
                    if candidate.exists() { candidate } else { root.join(&include_path) }
                } else {
                    PathBuf::from(&seg.filename)
                };
                if let Ok(inc_uri) = Url::from_file_path(&resolved) {
                    return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                        uri: inc_uri,
                        range: Range::default(), // jump to top of file
                    })));
                }
            }
            // Fallback: resolve relative to project root
            if let Some(ref root) = project_root {
                // Try common locations: include/, src/, project root
                for base in &[root.join("include"), root.join("src"), root.clone()] {
                    let candidate = base.join(&include_path);
                    if candidate.exists() {
                        if let Ok(inc_uri) = Url::from_file_path(&candidate) {
                            return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                                uri: inc_uri,
                                range: Range::default(),
                            })));
                        }
                    }
                }
            }
        }

        // Check for macro reference in original source (macros are expanded away in AST).
        if let Some(macro_name) = find_macro_at_offset(&original_source, orig_offset) {
            if let Some(loc) = macro_locations.iter().find(|m| m.name == macro_name) {
                if let Some((def_uri, range)) = resolve_span_to_location(
                    loc.combined_span, &uri, &file_segments, &source_map,
                    &original_source_map, &offset_map, project_root.as_deref(),
                ) {
                    return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                        uri: def_uri,
                        range,
                    })));
                }
            }
        }

        // Check if cursor is on a function name/arity in an -export([...]) attribute.
        // These are not part of the AST functions, so find_cursor_target won't find them.
        if let Some((fun_name, arity)) = find_export_entry_at_offset(&original_source, orig_offset) {
            for func in &module.functions {
                let fname = span_text(&source, func.name_span);
                if fname == fun_name && func.arity == arity {
                    if let Some((def_uri, range)) = resolve_span_to_location(
                        func.name_span, &uri, &file_segments, &source_map,
                        &original_source_map, &offset_map, project_root.as_deref(),
                    ) {
                        return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                            uri: def_uri,
                            range,
                        })));
                    }
                }
            }
        }

        // Check for type reference in type attributes
        if let Some(type_ref) = find_type_ref_at_offset(&original_source, orig_offset) {
            if let Some(ref mod_name) = type_ref.module {
                // Remote type: look up in project index
                let idx = self.symbol_index.lock().unwrap();
                for arity in 0..=10 {
                    let key = TypeKey {
                        module: mod_name.clone(),
                        type_name: type_ref.name.clone(),
                        arity,
                    };
                    if let Some(loc) = idx.types.get(&key) {
                        if let Ok(def_uri) = Url::from_file_path(&loc.file_path) {
                            return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                                uri: def_uri,
                                range: loc.range,
                            })));
                        }
                    }
                }
            } else {
                // Local type: find in module.type_defs
                for td in &module.type_defs {
                    let td_name = span_text(&source, td.name_span);
                    if td_name == type_ref.name {
                        if let Some((def_uri, range)) = resolve_span_to_location(
                            td.name_span, &uri, &file_segments, &source_map,
                            &original_source_map, &offset_map, project_root.as_deref(),
                        ) {
                            return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                                uri: def_uri,
                                range,
                            })));
                        }
                    }
                }
                // Check spec function names: if cursor is on the function name in a spec,
                // go to the function definition
                for spec in &module.specs {
                    let spec_name = span_text(&source, spec.name_span);
                    if spec_name == type_ref.name {
                        // Find the matching function
                        for func in &module.functions {
                            let fname = span_text(&source, func.name_span);
                            if fname == type_ref.name && func.arity == spec.arity {
                                if let Some((def_uri, range)) = resolve_span_to_location(
                                    func.name_span, &uri, &file_segments, &source_map,
                                    &original_source_map, &offset_map, project_root.as_deref(),
                                ) {
                                    return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                                        uri: def_uri,
                                        range,
                                    })));
                                }
                            }
                        }
                    }
                }
            }
        }

        let offset = map_original_to_combined(orig_offset, &offset_map);

        // Check if the cursor is on a function definition name (name_span at function
        // or clause level). find_cursor_target only looks inside clauses, so it misses
        // the definition site.
        let target = if let Some(def_func) = module.functions.iter().find(|f| {
            in_span(offset, f.name_span)
                || f.clauses.iter().any(|c| in_span(offset, c.name_span))
        }) {
            let name = span_text(&source, def_func.name_span).to_string();
            Some(CursorTarget::LocalCall { name, arity: def_func.arity })
        } else {
            find_cursor_target(&module, offset, &source)
        };

        let target = match target {
            Some(t) => t,
            None => return Ok(None),
        };

        match target {
            CursorTarget::LocalCall { name, arity } | CursorTarget::LocalFunRef { name, arity } => {
                // Search same module's functions
                for func in &module.functions {
                    let func_name = span_text(&source, func.name_span);
                    if func_name == name && func.arity == arity {
                        if let Some((def_uri, range)) = resolve_span_to_location(
                            func.name_span, &uri, &file_segments, &source_map,
                            &original_source_map, &offset_map, project_root.as_deref(),
                        ) {
                            return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                                uri: def_uri,
                                range,
                            })));
                        }
                    }
                }
                // Not found locally — not a same-module function
                Ok(None)
            }

            CursorTarget::RemoteCall { module: mod_name, function, arity }
            | CursorTarget::RemoteFunRef { module: mod_name, function, arity } => {
                let key = FunKey {
                    module: mod_name,
                    function,
                    arity,
                };
                let idx = self.symbol_index.lock().unwrap();
                if let Some(loc) = idx.functions.get(&key) {
                    if let Ok(def_uri) = Url::from_file_path(&loc.file_path) {
                        return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                            uri: def_uri,
                            range: loc.range,
                        })));
                    }
                }
                return Ok(None);
            }

            CursorTarget::Variable { name } => {
                // Find the first binding of this variable in the enclosing function clause
                for func in &module.functions {
                    if !in_span(offset, func.span) {
                        continue;
                    }
                    for clause in &func.clauses {
                        if !in_span(offset, clause.span) {
                            continue;
                        }
                        // Collect all bindings in patterns
                        let mut bindings = Vec::new();
                        for pat in &clause.patterns {
                            collect_pattern_bindings(pat, &source, &mut bindings);
                        }
                        // Also collect bindings from case/match expressions in body
                        collect_body_bindings(&clause.body, &source, &mut bindings);
                        // Find the first binding with matching name
                        for (bind_name, bind_span) in &bindings {
                            if *bind_name == name {
                                if let Some((def_uri, range)) = resolve_span_to_location(
                                    *bind_span, &uri, &file_segments, &source_map,
                                    &original_source_map, &offset_map, project_root.as_deref(),
                                ) {
                                    return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                                        uri: def_uri,
                                        range,
                                    })));
                                }
                            }
                        }
                        return Ok(None);
                    }
                }
                Ok(None)
            }

            CursorTarget::RecordRef { name } => {
                // Check same module's records first (may be in an included header)
                for rec in &module.records {
                    let rec_text = span_text(&source, rec.span);
                    // rec.span starts at the record name (e.g. "demo_state, {name, age})")
                    // Extract the name: text up to first comma, space, brace, or paren.
                    let end = rec_text.find([',', ' ', '{', '(', ')']).unwrap_or(rec_text.len());
                    let rec_name = &rec_text[..end];
                    if rec_name == name {
                        if let Some((def_uri, range)) = resolve_span_to_location(
                            rec.span, &uri, &file_segments, &source_map,
                            &original_source_map, &offset_map, project_root.as_deref(),
                        ) {
                            return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                                uri: def_uri,
                                range,
                            })));
                        }
                    }
                }
                // Fall back to project index
                let idx = self.symbol_index.lock().unwrap();
                if let Some(locs) = idx.records.get(&name) {
                    if let Some(loc) = locs.first() {
                        if let Ok(def_uri) = Url::from_file_path(&loc.file_path) {
                            return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                                uri: def_uri,
                                range: loc.range,
                            })));
                        }
                    }
                }
                Ok(None)
            }

        }
    }

    async fn diagnostic(
        &self,
        params: DocumentDiagnosticParams,
    ) -> Result<DocumentDiagnosticReportResult> {
        let uri = params.text_document.uri;
        let items = {
            let cache = self.diag_cache.lock().unwrap();
            cache.get(&uri).cloned().unwrap_or_default()
        };
        Ok(DocumentDiagnosticReportResult::Report(
            DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                related_documents: None,
                full_document_diagnostic_report: FullDocumentDiagnosticReport {
                    result_id: None,
                    items,
                },
            }),
        ))
    }

    async fn workspace_diagnostic(
        &self,
        params: WorkspaceDiagnosticParams,
    ) -> Result<WorkspaceDiagnosticReportResult> {
        let _ = params;
        let cache = self.diag_cache.lock().unwrap();
        let items: Vec<WorkspaceDocumentDiagnosticReport> = cache
            .iter()
            .map(|(uri, diags)| {
                WorkspaceDocumentDiagnosticReport::Full(
                    WorkspaceFullDocumentDiagnosticReport {
                        uri: uri.clone(),
                        version: None,
                        full_document_diagnostic_report: FullDocumentDiagnosticReport {
                            result_id: None,
                            items: diags.clone(),
                        },
                    },
                )
            })
            .collect();
        Ok(WorkspaceDiagnosticReportResult::Report(
            WorkspaceDiagnosticReport { items },
        ))
    }

    async fn folding_range(
        &self,
        params: FoldingRangeParams,
    ) -> Result<Option<Vec<FoldingRange>>> {
        let uri = params.text_document.uri;
        let cached = {
            let cache = self.doc_cache.lock().unwrap();
            cache.get(&uri).map(|doc| (
                doc.module.clone(),
                doc.source_map.clone(),
                doc.original_source.clone(),
                doc.original_source_map.clone(),
            ))
        };

        let Some((module, sm, original_source, original_sm)) = cached else {
            return Ok(None);
        };

        let mut ranges = Vec::new();

        // Fold multi-line attribute directives (-export, -export_type, -import, -type, -spec, etc.)
        collect_attribute_folds(&original_source, &original_sm, &mut ranges);

        for func in &module.functions {
            let r = span_to_lsp_range(func.span, &sm);
            if r.start.line < r.end.line {
                ranges.push(FoldingRange {
                    start_line: r.start.line,
                    start_character: None,
                    end_line: r.end.line,
                    end_character: None,
                    kind: None,
                    collapsed_text: None,
                });
            }
            // Walk clause bodies for expression-level folds
            for clause in &func.clauses {
                for expr in &clause.body {
                    collect_expr_folds(expr, &sm, &mut ranges);
                }
            }
        }

        for rec in &module.records {
            let r = span_to_lsp_range(rec.span, &sm);
            if r.start.line < r.end.line {
                ranges.push(FoldingRange {
                    start_line: r.start.line,
                    start_character: None,
                    end_line: r.end.line,
                    end_character: None,
                    kind: None,
                    collapsed_text: None,
                });
            }
        }

        if ranges.is_empty() {
            Ok(None)
        } else {
            Ok(Some(ranges))
        }
    }

    async fn formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        self.client
            .log_message(MessageType::INFO, format!("formatting requested for {}", uri))
            .await;

        let source = {
            let raw = self.raw_text.lock().unwrap();
            raw.get(&uri).cloned()
        };
        let source = match source {
            Some(s) => s,
            None => {
                self.client
                    .log_message(
                        MessageType::WARNING,
                        format!("formatting: no cached source for {}", uri),
                    )
                    .await;
                return Ok(None);
            }
        };

        let indent_size = params.options.tab_size as usize;
        let config = nera_fmt::config::FormatConfig {
            max_width: 100,
            indent_size,
        };

        let is_term_file = {
            let path = uri.as_str();
            path.ends_with(".app") || path.ends_with(".app.src")
        };

        let formatted = match tokio::task::spawn_blocking({
            let source = source.clone();
            move || {
                if is_term_file {
                    nera_fmt::format_term_source(&source, &config)
                } else {
                    nera_fmt::format_source(&source, &config)
                }
            }
        })
        .await
        {
            Ok(Ok(f)) => f,
            Ok(Err(fmt_err)) => {
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!("format_source error for {}: {}", uri, fmt_err),
                    )
                    .await;
                return Err(tower_lsp::jsonrpc::Error {
                    code: tower_lsp::jsonrpc::ErrorCode::InternalError,
                    message: format!("Formatting failed: {}", fmt_err).into(),
                    data: None,
                });
            }
            Err(join_err) => {
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!("formatting task panicked for {}: {}", uri, join_err),
                    )
                    .await;
                return Err(tower_lsp::jsonrpc::Error {
                    code: tower_lsp::jsonrpc::ErrorCode::InternalError,
                    message: "Formatting task panicked".into(),
                    data: None,
                });
            }
        };

        if formatted == source {
            self.client
                .log_message(MessageType::INFO, format!("already formatted: {}", uri))
                .await;
            return Ok(None);
        }

        // Whole-document replacement — compute correct end position.
        // `str::lines()` omits a trailing empty line for strings ending with '\n',
        // so we must account for that to cover the full document range.
        let end_pos = if source.ends_with('\n') {
            Position::new(source.lines().count() as u32, 0)
        } else {
            let last_line = source.lines().count().saturating_sub(1) as u32;
            let last_line_len = source.lines().last().map_or(0, |l| l.len()) as u32;
            Position::new(last_line, last_line_len)
        };

        self.client
            .log_message(
                MessageType::INFO,
                format!(
                    "formatting {}: {} -> {} bytes",
                    uri,
                    source.len(),
                    formatted.len()
                ),
            )
            .await;

        Ok(Some(vec![TextEdit {
            range: Range {
                start: Position::new(0, 0),
                end: end_pos,
            },
            new_text: formatted,
        }]))
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

/// Recursively walk an expression tree and collect folding ranges for
/// block-like constructs (case, if, receive, try, begin, fun, maybe).
pub(crate) fn collect_expr_folds(
    expr: &nera_syntax::ast::Expr,
    sm: &SourceMap,
    ranges: &mut Vec<FoldingRange>,
) {
    use nera_syntax::ast::Expr;

    // Check if this expression itself is foldable
    let foldable = matches!(
        expr,
        Expr::Case(..) | Expr::If(..) | Expr::Receive(..)
        | Expr::Try { .. } | Expr::Block(..) | Expr::Fun(..)
        | Expr::Maybe { .. }
    );

    if foldable {
        let span = expr_span(expr);
        let r = span_to_lsp_range(span, sm);
        if r.start.line < r.end.line {
            ranges.push(FoldingRange {
                start_line: r.start.line,
                start_character: None,
                end_line: r.end.line.saturating_sub(1),
                end_character: None,
                kind: None,
                collapsed_text: None,
            });
        }
    }

    // Recurse into sub-expressions
    match expr {
        Expr::Case(scrutinee, clauses, _) => {
            collect_expr_folds(scrutinee, sm, ranges);
            for clause in clauses {
                for e in &clause.body {
                    collect_expr_folds(e, sm, ranges);
                }
            }
        }
        Expr::If(clauses, _) => {
            for clause in clauses {
                for e in &clause.body {
                    collect_expr_folds(e, sm, ranges);
                }
            }
        }
        Expr::Receive(clauses, after, _) => {
            for clause in clauses {
                for e in &clause.body {
                    collect_expr_folds(e, sm, ranges);
                }
            }
            if let Some(after_clause) = after {
                for e in &after_clause.body {
                    collect_expr_folds(e, sm, ranges);
                }
            }
        }
        Expr::Fun(_, clauses, _) => {
            for clause in clauses {
                for e in &clause.body {
                    collect_expr_folds(e, sm, ranges);
                }
            }
        }
        Expr::Block(exprs, _) => {
            for e in exprs {
                collect_expr_folds(e, sm, ranges);
            }
        }
        Expr::Try { body, of_clauses, catch_clauses, after_body, .. } => {
            for e in body {
                collect_expr_folds(e, sm, ranges);
            }
            for clause in of_clauses {
                for e in &clause.body {
                    collect_expr_folds(e, sm, ranges);
                }
            }
            for clause in catch_clauses {
                for e in &clause.body {
                    collect_expr_folds(e, sm, ranges);
                }
            }
            if let Some(ab) = after_body {
                for e in ab {
                    collect_expr_folds(e, sm, ranges);
                }
            }
        }
        Expr::Maybe { body, else_clauses, .. } => {
            for item in body {
                match item {
                    nera_syntax::ast::MaybeExpr::Expr(e) => collect_expr_folds(e, sm, ranges),
                    nera_syntax::ast::MaybeExpr::CondMatch(_, e, _) => collect_expr_folds(e, sm, ranges),
                }
            }
            for clause in else_clauses {
                for e in &clause.body {
                    collect_expr_folds(e, sm, ranges);
                }
            }
        }
        // For other compound expressions, recurse into their sub-parts
        Expr::Match(_, rhs, _) => collect_expr_folds(rhs, sm, ranges),
        Expr::UnaryOp(_, e, _) | Expr::Throw(e, _) | Expr::Catch(e, _) => {
            collect_expr_folds(e, sm, ranges);
        }
        Expr::BinOp(_, lhs, rhs, _) | Expr::Send(lhs, rhs, _) => {
            collect_expr_folds(lhs, sm, ranges);
            collect_expr_folds(rhs, sm, ranges);
        }
        Expr::Call(callee, args, _) => {
            // Recurse into remote call target expressions
            if let nera_syntax::ast::CallTarget::Remote(m, f) = callee {
                collect_expr_folds(m, sm, ranges);
                collect_expr_folds(f, sm, ranges);
            }
            for a in args {
                collect_expr_folds(a, sm, ranges);
            }
        }
        Expr::Tuple(exprs, _) => {
            for e in exprs {
                collect_expr_folds(e, sm, ranges);
            }
        }
        Expr::Cons(head, tail, _) => {
            collect_expr_folds(head, sm, ranges);
            collect_expr_folds(tail, sm, ranges);
        }
        Expr::ListComp(e, quals, _) | Expr::BinComp(e, quals, _) => {
            collect_expr_folds(e, sm, ranges);
            for qual in quals {
                match qual {
                    nera_syntax::ast::LcQualifier::Generator(_, e, _)
                    | nera_syntax::ast::LcQualifier::BinGenerator(_, e, _) => {
                        collect_expr_folds(e, sm, ranges);
                    }
                    nera_syntax::ast::LcQualifier::Filter(e, _) => {
                        collect_expr_folds(e, sm, ranges);
                    }
                }
            }
        }
        Expr::MapUpdate(base, fields, _) => {
            collect_expr_folds(base, sm, ranges);
            for f in fields {
                collect_expr_folds(&f.value, sm, ranges);
            }
        }
        Expr::Map(fields, _) => {
            for f in fields {
                collect_expr_folds(&f.value, sm, ranges);
            }
        }
        Expr::RecordCreate { fields, .. } => {
            for f in fields {
                collect_expr_folds(&f.value, sm, ranges);
            }
        }
        Expr::RecordUpdate { base, fields, .. } => {
            collect_expr_folds(base, sm, ranges);
            for f in fields {
                collect_expr_folds(&f.value, sm, ranges);
            }
        }
        Expr::Binary(segments, _) => {
            for seg in segments {
                collect_expr_folds(&seg.value, sm, ranges);
            }
        }
        Expr::RecordAccess { base, .. } => {
            collect_expr_folds(base, sm, ranges);
        }
        // Leaf expressions — no sub-expressions to recurse into
        _ => {}
    }
}

/// Scan the original source text for multi-line attribute directives and
/// emit folding ranges for them.
///
/// Matches `-export(`, `-export_type(`, `-import(`, `-type `, `-opaque `,
/// `-spec `, `-callback `, `-record(`, `-define(`, `-include`, `-ifdef`,
/// `-ifndef`, `-behaviour`, `-behavior` — any top-level attribute that spans
/// multiple lines.
pub(crate) fn collect_attribute_folds(
    source: &str,
    sm: &SourceMap,
    ranges: &mut Vec<FoldingRange>,
) {
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Attributes start with `-` at the beginning of a line
        if bytes[i] == b'-' && (i == 0 || bytes[i - 1] == b'\n') {
            // Find the end of this attribute (the closing `.\n` or `.<EOF>`)
            if let Some(dot_pos) = find_attribute_end(source, i) {
                let start_pos = sm.offset_to_position(i);
                let end_pos = sm.offset_to_position(dot_pos);
                let start_line = (start_pos.line - 1) as u32;
                let end_line = (end_pos.line - 1) as u32;
                if start_line < end_line {
                    ranges.push(FoldingRange {
                        start_line,
                        start_character: None,
                        end_line,
                        end_character: None,
                        kind: None,
                        collapsed_text: None,
                    });
                }
                i = dot_pos + 1;
                continue;
            }
        }
        i += 1;
    }
}

/// Find the end of an Erlang attribute starting at `start`.
/// Returns the byte offset of the terminating `.` character.
///
/// Handles nested parentheses, brackets, braces, strings, and quoted atoms
/// so that dots inside those constructs are not mistaken for the terminator.
fn find_attribute_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut i = start;
    let mut depth = 0i32; // tracks (), [], {}

    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'"' => {
                // Skip string literal
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' { i += 1; } // skip escaped char
                    else if bytes[i] == b'"' { break; }
                    i += 1;
                }
            }
            b'\'' => {
                // Skip quoted atom
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' { i += 1; }
                    else if bytes[i] == b'\'' { break; }
                    i += 1;
                }
            }
            b'%' => {
                // Skip line comment
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                // Don't advance past newline — the loop increment handles it
                continue;
            }
            b'.' if depth <= 0 => {
                // Check that the dot is followed by whitespace, newline, EOF, or %
                let next = bytes.get(i + 1).copied();
                if next.is_none()
                    || next == Some(b'\n')
                    || next == Some(b'\r')
                    || next == Some(b' ')
                    || next == Some(b'\t')
                    || next == Some(b'%')
                {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}
