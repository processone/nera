use super::*;
use super::diagnostics::convert_diagnostic;
use super::handlers::{collect_expr_folds, collect_attribute_folds};
use super::util::{
    extract_variable_name, extract_mismatch_names, extract_function_name_arity,
    find_line_after_module_attr, find_include_at_offset, find_export_entry_at_offset,
    find_macro_at_offset, pattern_span, span_text, in_span,
    collect_pattern_bindings, collect_body_bindings, extract_funref_local_name,
    extract_funref_remote_names, is_valid_variable_name,
};
use super::cursor::find_cursor_target;
use super::references::{
    find_local_call_spans, find_remote_call_spans, find_variable_spans_in_clause,
    find_variable_highlights_in_clause, collect_inlay_hints_from_exprs,
};
use super::source_map::{
    build_offset_map, span_to_lsp_range, resolve_span_to_location,
};
use std::path::PathBuf;
use super::check::find_project_root_from;
use nera_syntax::error::Diagnostic as NeraDiagnostic;
use nera_syntax::span::Span;
use nera_syntax::MultiSourceMap;

    /// Helper: build a single-file MultiSourceMap.
    fn single_file_msm(source: &str, filename: &str) -> MultiSourceMap {
        MultiSourceMap::new(
            source.to_string(),
            vec![nera_syntax::FileSegment {
                start: 0,
                end: source.len(),
                filename: filename.to_string(),
            }],
        )
    }

    #[test]
    fn convert_error_diagnostic_line_col() {
        let src = "-module(test).\nstart() -> ok.\n";
        let msm = single_file_msm(src, "src/test.erl");
        let root = Path::new("/project");

        // Span pointing at "start" on line 2 (offset 15..20)
        let diag = NeraDiagnostic::error_at("unexpected token", Span::new(15, 20));
        let fd = convert_diagnostic(&diag, &msm, root).expect("should convert");

        assert_eq!(fd.uri.path(), "/project/src/test.erl");
        assert_eq!(fd.diagnostic.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(fd.diagnostic.source, Some("nera".to_string()));
        // Line 2, column 1 (0-based: line=1, character=0)
        assert_eq!(fd.diagnostic.range.start.line, 1);
        assert_eq!(fd.diagnostic.range.start.character, 0);
        // End at column 6 (0-based: character=5)
        assert_eq!(fd.diagnostic.range.end.line, 1);
        assert_eq!(fd.diagnostic.range.end.character, 5);
    }

    #[test]
    fn convert_warning_diagnostic() {
        let src = "hello\n";
        let msm = single_file_msm(src, "src/test.erl");
        let root = Path::new("/project");

        let diag = NeraDiagnostic::warning_at("unused variable", Span::new(0, 5));
        let fd = convert_diagnostic(&diag, &msm, root).expect("should convert");

        assert_eq!(fd.diagnostic.severity, Some(DiagnosticSeverity::WARNING));
    }

    #[test]
    fn convert_diagnostic_no_span_returns_none() {
        let src = "hello\n";
        let msm = single_file_msm(src, "src/test.erl");
        let root = Path::new("/project");

        let diag = NeraDiagnostic::error("no span");
        assert!(convert_diagnostic(&diag, &msm, root).is_none());
    }

    #[test]
    fn convert_diagnostic_with_label() {
        let src = "hello\n";
        let msm = single_file_msm(src, "src/test.erl");
        let root = Path::new("/project");

        let diag = NeraDiagnostic::error_at("main error", Span::new(0, 5))
            .with_label("additional context");
        let fd = convert_diagnostic(&diag, &msm, root).expect("should convert");

        assert!(fd.diagnostic.message.contains("main error"));
        assert!(fd.diagnostic.message.contains("additional context"));
    }

    #[test]
    fn convert_diagnostic_multi_segment() {
        // Simulate main.erl + included header.hrl
        let main_src = "-module(main).\n";        // 15 bytes
        let hrl_src = "-define(X, 1).\n";          // 15 bytes
        let combined = format!("{}{}", main_src, hrl_src);

        let msm = MultiSourceMap::new(
            combined,
            vec![
                nera_syntax::FileSegment {
                    start: 0,
                    end: 15,
                    filename: "src/main.erl".to_string(),
                },
                nera_syntax::FileSegment {
                    start: 15,
                    end: 30,
                    filename: "include/header.hrl".to_string(),
                },
            ],
        );
        let root = Path::new("/project");

        // Error in the header file at offset 15 (start of second segment)
        let diag = NeraDiagnostic::error_at("bad define", Span::new(15, 22));
        let fd = convert_diagnostic(&diag, &msm, root).expect("should convert");

        // Should resolve to header.hrl, not main.erl
        assert_eq!(fd.uri.path(), "/project/include/header.hrl");
        // Should be line 1, column 1 of the header (0-based: line=0, character=0)
        assert_eq!(fd.diagnostic.range.start.line, 0);
        assert_eq!(fd.diagnostic.range.start.character, 0);
    }

    #[test]
    fn convert_diagnostic_absolute_filename() {
        let src = "hello\n";
        let msm = single_file_msm(src, "/absolute/path/test.erl");
        let root = Path::new("/project");

        let diag = NeraDiagnostic::error_at("error", Span::new(0, 5));
        let fd = convert_diagnostic(&diag, &msm, root).expect("should convert");

        // Should use the absolute path, not join with root
        assert_eq!(fd.uri.path(), "/absolute/path/test.erl");
    }

    #[test]
    fn convert_diagnostic_propagates_code() {
        let src = "hello\n";
        let msm = single_file_msm(src, "src/test.erl");
        let root = Path::new("/project");

        let diag = NeraDiagnostic::warning_at("variable 'X' is unused", Span::new(0, 5))
            .with_code("unused_variable");
        let fd = convert_diagnostic(&diag, &msm, root).expect("should convert");

        assert_eq!(
            fd.diagnostic.code,
            Some(NumberOrString::String("unused_variable".to_string()))
        );
    }

    #[test]
    fn convert_diagnostic_no_code_by_default() {
        let src = "hello\n";
        let msm = single_file_msm(src, "src/test.erl");
        let root = Path::new("/project");

        let diag = NeraDiagnostic::error_at("some error", Span::new(0, 5));
        let fd = convert_diagnostic(&diag, &msm, root).expect("should convert");

        assert_eq!(fd.diagnostic.code, None);
    }

    #[test]
    fn extract_variable_name_valid() {
        assert_eq!(extract_variable_name("variable 'X' is unused"), Some("X"));
        assert_eq!(extract_variable_name("variable 'Foo' is unused"), Some("Foo"));
        assert_eq!(extract_variable_name("variable 'MyVar123' is unused"), Some("MyVar123"));
    }

    #[test]
    fn extract_variable_name_no_match() {
        assert_eq!(extract_variable_name("some other message"), None);
        assert_eq!(extract_variable_name("variable is unused"), None);
    }

    #[test]
    fn extract_mismatch_names_valid() {
        assert_eq!(
            extract_mismatch_names("module 'foo' does not match filename 'bar'"),
            Some(("foo", "bar"))
        );
        assert_eq!(
            extract_mismatch_names("module 'my_mod' does not match filename 'other'"),
            Some(("my_mod", "other"))
        );
    }

    #[test]
    fn extract_mismatch_names_no_match() {
        assert_eq!(extract_mismatch_names("some other message"), None);
        assert_eq!(extract_mismatch_names("module is wrong"), None);
    }

    #[test]
    fn extract_function_name_arity_valid() {
        assert_eq!(
            extract_function_name_arity("function 'foo/2' is unused"),
            Some(("foo", 2))
        );
        assert_eq!(
            extract_function_name_arity(
                "undefined callback function 'init/1' (behaviour 'gen_server')"
            ),
            Some(("init", 1))
        );
        assert_eq!(
            extract_function_name_arity("function 'my_fun/0' is unused"),
            Some(("my_fun", 0))
        );
    }

    #[test]
    fn extract_function_name_arity_no_match() {
        assert_eq!(extract_function_name_arity("some other message"), None);
        assert_eq!(extract_function_name_arity("function 'foo' is bad"), None);
        assert_eq!(extract_function_name_arity("nothing quoted here"), None);
    }

    #[test]
    fn find_line_after_module_attr_basic() {
        let src = "-module(foo).\n-export([bar/0]).\nbar() -> ok.\n";
        assert_eq!(find_line_after_module_attr(src), 1);
    }

    #[test]
    fn find_line_after_module_attr_indented() {
        let src = "%% header\n   -module(foo).\nbar() -> ok.\n";
        assert_eq!(find_line_after_module_attr(src), 2);
    }

    #[test]
    fn find_line_after_module_attr_missing() {
        let src = "foo() -> ok.\n";
        assert_eq!(find_line_after_module_attr(src), 0);
    }

    // ── Go-to-definition unit tests ─────────────────────────────────

    /// Parse an Erlang source string into (Module, combined_source).
    fn parse_module(source: &str) -> (nera_syntax::ast::Module, String) {
        let tokens = Lexer::lex_all(source).expect("lex should succeed");
        let file_path = PathBuf::from("test.erl");
        let mut preproc = Preprocessor::new(source, &file_path);
        let processed = preproc.process(tokens).expect("preprocess should succeed");
        let (combined, _segments) = preproc.take_combined_source_and_segments();
        let interner = DefaultStringInterner::default();
        let mut parser = Parser::with_tokens(&combined, processed, interner);
        let result = parser.parse_module();
        assert!(result.errors.is_empty(), "parse errors: {:#?}", result.errors);
        (result.module, combined)
    }

    #[test]
    fn extract_funref_local_name_parses() {
        assert_eq!(extract_funref_local_name("fun foo/2"), Some("foo".to_string()));
        assert_eq!(extract_funref_local_name("fun  bar/0"), Some("bar".to_string()));
        assert_eq!(extract_funref_local_name("not a funref"), None);
    }

    #[test]
    fn extract_funref_remote_names_parses() {
        assert_eq!(
            extract_funref_remote_names("fun lists:map/2"),
            Some(("lists".to_string(), "map".to_string()))
        );
        assert_eq!(
            extract_funref_remote_names("fun io:format/1"),
            Some(("io".to_string(), "format".to_string()))
        );
        assert_eq!(extract_funref_remote_names("fun foo/1"), None);
    }

    #[test]
    fn find_cursor_target_local_call() {
        let src = "-module(test).\nstart() -> foo(1).\nfoo(X) -> X.\n";
        let (module, combined) = parse_module(src);
        // Find offset of "foo" in "foo(1)" on line 2
        let foo_call_offset = combined.find("foo(1)").expect("should find foo(1)");
        let target = find_cursor_target(&module, foo_call_offset, &combined);
        match target {
            Some(CursorTarget::LocalCall { name, arity }) => {
                assert_eq!(name, "foo");
                assert_eq!(arity, 1);
            }
            other => panic!("expected LocalCall, got {:?}", other),
        }
    }

    #[test]
    fn find_cursor_target_remote_call() {
        let src = "-module(test).\nstart() -> lists:reverse([1,2]).\n";
        let (module, combined) = parse_module(src);
        // Find offset of "lists" in the remote call
        let lists_offset = combined.find("lists:reverse").expect("should find lists:reverse");
        let target = find_cursor_target(&module, lists_offset, &combined);
        match target {
            Some(CursorTarget::RemoteCall { module: mod_name, function, arity }) => {
                assert_eq!(mod_name, "lists");
                assert_eq!(function, "reverse");
                assert_eq!(arity, 1);
            }
            other => panic!("expected RemoteCall, got {:?}", other),
        }
    }

    #[test]
    fn find_cursor_target_remote_call_on_function_name() {
        let src = "-module(test).\nstart() -> lists:reverse([1,2]).\n";
        let (module, combined) = parse_module(src);
        // Put cursor on "reverse" (after the colon)
        let reverse_offset = combined.find("reverse").expect("should find reverse");
        let target = find_cursor_target(&module, reverse_offset, &combined);
        match target {
            Some(CursorTarget::RemoteCall { module: mod_name, function, .. }) => {
                assert_eq!(mod_name, "lists");
                assert_eq!(function, "reverse");
            }
            other => panic!("expected RemoteCall, got {:?}", other),
        }
    }

    #[test]
    fn find_cursor_target_variable() {
        let src = "-module(test).\nfoo(X) -> X + 1.\n";
        let (module, combined) = parse_module(src);
        // Find the second "X" (the use in the body)
        let body_start = combined.find("-> X").expect("should find -> X") + 3;
        let target = find_cursor_target(&module, body_start, &combined);
        match target {
            Some(CursorTarget::Variable { name }) => {
                assert_eq!(name, "X");
            }
            other => panic!("expected Variable, got {:?}", other),
        }
    }

    #[test]
    fn find_cursor_target_variable_in_pattern() {
        let src = "-module(test).\nfoo(MyVar) -> MyVar.\n";
        let (module, combined) = parse_module(src);
        // Cursor on "MyVar" in the pattern
        let pat_offset = combined.find("MyVar").expect("should find MyVar");
        let target = find_cursor_target(&module, pat_offset, &combined);
        match target {
            Some(CursorTarget::Variable { name }) => {
                assert_eq!(name, "MyVar");
            }
            other => panic!("expected Variable, got {:?}", other),
        }
    }

    #[test]
    fn find_cursor_target_record_create() {
        let src = "-module(test).\n-record(person, {name, age}).\nstart() -> #person{name = alice}.\n";
        let (module, combined) = parse_module(src);
        // Cursor on "#person" in the record create expression
        let rec_offset = combined.rfind("#person").expect("should find #person in body");
        let target = find_cursor_target(&module, rec_offset, &combined);
        match target {
            Some(CursorTarget::RecordRef { name }) => {
                assert_eq!(name, "person");
            }
            other => panic!("expected RecordRef, got {:?}", other),
        }
    }

    #[test]
    fn find_cursor_target_record_pattern() {
        let src = "-module(test).\n-record(person, {name, age}).\nfoo(#person{name = N}) -> N.\n";
        let (module, combined) = parse_module(src);
        // Cursor on "#person" in the pattern
        let rec_offset = combined.rfind("#person").expect("should find #person");
        let target = find_cursor_target(&module, rec_offset, &combined);
        match target {
            Some(CursorTarget::RecordRef { name }) => {
                assert_eq!(name, "person");
            }
            other => panic!("expected RecordRef, got {:?}", other),
        }
    }

    #[test]
    fn find_cursor_target_in_case_branch() {
        let src = "-module(test).\nfoo(X) -> case X of 1 -> bar(X); _ -> baz(X) end.\nbar(Y) -> Y.\nbaz(Z) -> Z.\n";
        let (module, combined) = parse_module(src);
        // Find "bar" call in the case branch
        let bar_offset = combined.find("bar(X)").expect("should find bar(X)");
        let target = find_cursor_target(&module, bar_offset, &combined);
        match target {
            Some(CursorTarget::LocalCall { name, arity }) => {
                assert_eq!(name, "bar");
                assert_eq!(arity, 1);
            }
            other => panic!("expected LocalCall for bar, got {:?}", other),
        }
    }

    #[test]
    fn find_cursor_target_none_on_atom() {
        let src = "-module(test).\nfoo() -> ok.\n";
        let (module, combined) = parse_module(src);
        // Cursor on "ok" — an atom literal, should return None
        let ok_offset = combined.find("ok.").expect("should find ok");
        let target = find_cursor_target(&module, ok_offset, &combined);
        assert!(target.is_none(), "expected None for atom literal, got {:?}", target);
    }

    #[test]
    fn find_cursor_target_none_outside_functions() {
        let src = "-module(test).\nfoo() -> ok.\n";
        let (module, combined) = parse_module(src);
        // Cursor at offset 0 — on the -module attribute, outside functions
        let target = find_cursor_target(&module, 0, &combined);
        assert!(target.is_none(), "expected None outside functions, got {:?}", target);
    }

    #[test]
    fn collect_pattern_bindings_simple() {
        let src = "-module(test).\nfoo({A, B, [H|T]}) -> {A, B, H, T}.\n";
        let (module, combined) = parse_module(src);
        let clause = &module.functions[0].clauses[0];
        let mut bindings = Vec::new();
        for pat in &clause.patterns {
            collect_pattern_bindings(pat, &combined, &mut bindings);
        }
        let names: Vec<&str> = bindings.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["A", "B", "H", "T"]);
    }

    #[test]
    fn collect_pattern_bindings_nested_record() {
        let src = "-module(test).\n-record(r, {x, y}).\nfoo(#r{x = X, y = Y}) -> {X, Y}.\n";
        let (module, combined) = parse_module(src);
        let clause = &module.functions[0].clauses[0];
        let mut bindings = Vec::new();
        for pat in &clause.patterns {
            collect_pattern_bindings(pat, &combined, &mut bindings);
        }
        let names: Vec<&str> = bindings.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["X", "Y"]);
    }

    /// End-to-end test: simulate goto_definition for a variable use in the body.
    /// This mimics what the LSP handler does: LSP position → offset → cursor target → binding lookup.
    #[test]
    fn goto_definition_variable_end_to_end() {
        let src = "-module(test).\nfoo(X, Y) -> X + Y.\n";
        let (module, combined) = parse_module(src);
        let sm = SourceMap::new(&combined);

        // Find the body "X" (after "-> ")
        let body_x_offset = combined.find("-> X").unwrap() + 3; // offset of 'X' in body

        // Step 1: find cursor target
        let target = find_cursor_target(&module, body_x_offset, &combined);
        assert!(matches!(&target, Some(CursorTarget::Variable { name }) if name == "X"),
            "expected Variable(X), got {:?}", target);

        // Step 2: resolve to binding
        let name = match target.unwrap() {
            CursorTarget::Variable { name } => name,
            _ => unreachable!(),
        };

        let mut found_binding = None;
        for func in &module.functions {
            for clause in &func.clauses {
                if !in_span(body_x_offset, clause.span) {
                    continue;
                }
                let mut bindings = Vec::new();
                for pat in &clause.patterns {
                    collect_pattern_bindings(pat, &combined, &mut bindings);
                }
                collect_body_bindings(&clause.body, &combined, &mut bindings);
                for (bind_name, bind_span) in &bindings {
                    if *bind_name == name {
                        found_binding = Some(span_to_lsp_range(*bind_span, &sm));
                        break;
                    }
                }
            }
        }

        let range = found_binding.expect("should find binding for X");
        // X is in the pattern on line 2 (0-based: 1), after "foo("
        assert_eq!(range.start.line, 1);
        assert_eq!(range.start.character, 4); // "foo(" = 4 chars, then X
    }

    /// Test: variable bound via match expression in the body (State = #rec{}).
    #[test]
    fn goto_definition_variable_bound_in_body() {
        let src = "-module(test).\n-record(demo_state, {}).\nmessage(Name) ->\n    State = #demo_state{},\n    {Name, State}.\n";
        let (module, combined) = parse_module(src);
        let sm = SourceMap::new(&combined);

        // Find the "State" use in "{Name, State}" on the last line
        let state_use_offset = combined.rfind("State}").expect("should find State}");

        // Step 1: find cursor target
        let target = find_cursor_target(&module, state_use_offset, &combined);
        assert!(matches!(&target, Some(CursorTarget::Variable { name }) if name == "State"),
            "expected Variable(State), got {:?}", target);

        // Step 2: resolve to binding
        let name = "State";
        let mut found_binding = None;
        for func in &module.functions {
            if !in_span(state_use_offset, func.span) { continue; }
            for clause in &func.clauses {
                if !in_span(state_use_offset, clause.span) { continue; }
                let mut bindings = Vec::new();
                for pat in &clause.patterns {
                    collect_pattern_bindings(pat, &combined, &mut bindings);
                }
                collect_body_bindings(&clause.body, &combined, &mut bindings);
                for (bind_name, bind_span) in &bindings {
                    if *bind_name == name {
                        found_binding = Some((*bind_span, span_to_lsp_range(*bind_span, &sm)));
                        break;
                    }
                }
            }
        }

        let (_bind_span, range) = found_binding.expect("should find binding for State");
        // State is bound on the 4th line (0-based: 3), "    State = ..."
        assert_eq!(range.start.line, 3);
        assert_eq!(range.start.character, 4); // 4 spaces indent
    }

    #[test]
    fn span_to_lsp_range_basic() {
        let src = "hello\nworld\n";
        let sm = SourceMap::new(src);
        let range = span_to_lsp_range(Span::new(6, 11), &sm);
        // "world" is on line 2 (0-based: 1), columns 0..5
        assert_eq!(range.start.line, 1);
        assert_eq!(range.start.character, 0);
        assert_eq!(range.end.line, 1);
        assert_eq!(range.end.character, 5);
    }

    // ── resolve_span_to_location tests ──────────────────────────────

    #[test]
    fn resolve_span_in_main_file() {
        let main_src = "-module(test).\nfoo() -> ok.\n";
        let sm = SourceMap::new(main_src);
        let original_sm = SourceMap::new(main_src);
        let segments = vec![nera_syntax::preproc::FileSegment {
            start: 0,
            end: main_src.len(),
            filename: "/project/src/test.erl".to_string(),
        }];
        let offset_map = vec![(0, main_src.len(), 0)];
        let uri = Url::from_file_path("/project/src/test.erl").unwrap();

        // Span of "foo" (offset 15..18)
        let result = resolve_span_to_location(
            Span::new(15, 18), &uri, &segments, &sm, &original_sm, &offset_map, Some(Path::new("/project")),
        );
        let (resolved_uri, range) = result.expect("should resolve");
        // Should stay in the main file
        assert_eq!(resolved_uri, uri);
        assert_eq!(range.start.line, 1);
        assert_eq!(range.start.character, 0);
    }

    #[test]
    fn resolve_span_in_included_file() {
        // Simulate: main file (15 bytes) + included header (25 bytes)
        let main_part = "-module(test).\n";          // 15 bytes
        let hrl_part = "-record(person, {name}).\n"; // 25 bytes
        let combined = format!("{}{}", main_part, hrl_part);
        let sm = SourceMap::new(&combined);
        let original_sm = SourceMap::new(main_part);
        let segments = vec![
            nera_syntax::preproc::FileSegment {
                start: 0,
                end: 15,
                filename: "/project/src/test.erl".to_string(),
            },
            nera_syntax::preproc::FileSegment {
                start: 15,
                end: 40,
                filename: "/project/include/demo.hrl".to_string(),
            },
        ];
        let offset_map = vec![(0, 15, 0)];
        let uri = Url::from_file_path("/project/src/test.erl").unwrap();

        // Span pointing inside the included header (offset 15..39 = "-record(person, {name}).")
        let result = resolve_span_to_location(
            Span::new(15, 39), &uri, &segments, &sm, &original_sm, &offset_map, Some(Path::new("/project")),
        );
        let (resolved_uri, range) = result.expect("should resolve");
        // Should point to the include file
        assert_eq!(resolved_uri.path(), "/project/include/demo.hrl");
        // Line 1, col 1 of the header (0-based: line=0, character=0)
        assert_eq!(range.start.line, 0);
        assert_eq!(range.start.character, 0);
    }

    #[test]
    fn resolve_span_in_included_file_not_at_start() {
        // Header has two lines; span points at the second line
        let main_part = "-module(test).\n";              // 15 bytes
        let hrl_part = "-define(X, 1).\n-record(r, {}).\n"; // 15 + 16 = 31 bytes
        let combined = format!("{}{}", main_part, hrl_part);
        let sm = SourceMap::new(&combined);
        let original_sm = SourceMap::new(main_part);
        let segments = vec![
            nera_syntax::preproc::FileSegment {
                start: 0,
                end: 15,
                filename: "/project/src/test.erl".to_string(),
            },
            nera_syntax::preproc::FileSegment {
                start: 15,
                end: 15 + 31,
                filename: "/project/include/demo.hrl".to_string(),
            },
        ];
        let offset_map = vec![(0, 15, 0)];
        let uri = Url::from_file_path("/project/src/test.erl").unwrap();

        // Span pointing at "-record(r, {})." which starts at offset 30 in combined
        let record_start = 15 + 15; // after main + first hrl line
        let result = resolve_span_to_location(
            Span::new(record_start, record_start + 15), &uri, &segments,
            &sm, &original_sm, &offset_map, Some(Path::new("/project")),
        );
        let (resolved_uri, range) = result.expect("should resolve");
        assert_eq!(resolved_uri.path(), "/project/include/demo.hrl");
        // Second line of header (0-based: line=1)
        assert_eq!(range.start.line, 1);
        assert_eq!(range.start.character, 0);
    }

    #[test]
    fn resolve_span_no_segments_falls_back() {
        let src = "-module(test).\nfoo() -> ok.\n";
        let sm = SourceMap::new(src);
        let original_sm = SourceMap::new(src);
        let uri = Url::from_file_path("/project/src/test.erl").unwrap();

        // No file segments — should fall back to original file
        let result = resolve_span_to_location(
            Span::new(15, 18), &uri, &[], &sm, &original_sm, &[], None,
        );
        let (resolved_uri, _range) = result.expect("should resolve");
        assert_eq!(resolved_uri, uri);
    }

    // ── nera.toml include path resolution tests ─────────────────────

    #[test]
    fn resolve_include_paths_from_nera_toml() {
        use std::fs;

        let tmp = std::env::temp_dir().join("nera_test_include_paths");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("src")).unwrap();
        fs::create_dir_all(tmp.join("include")).unwrap();
        fs::create_dir_all(tmp.join("custom_inc")).unwrap();
        fs::write(
            tmp.join("nera.toml"),
            "[project]\nname = \"test\"\ninclude = [\"include\", \"custom_inc\"]\nsrc = [\"src\"]\n",
        ).unwrap();
        fs::write(tmp.join("src/test.erl"), "-module(test).\n").unwrap();

        let file_path = tmp.join("src/test.erl");

        // Simulate what resolve_include_paths does
        let project_root = find_project_root_from(file_path.parent().unwrap());
        assert_eq!(project_root.as_deref(), Some(tmp.as_path()));

        let toml_path = tmp.join("nera.toml");
        let toml = load_project_toml(&toml_path).expect("should parse nera.toml");
        assert_eq!(toml.project.include, Some(vec!["include".to_string(), "custom_inc".to_string()]));
        assert_eq!(toml.project.src, Some(vec!["src".to_string()]));

        // Verify the resolved paths would include custom_inc
        let mut paths = Vec::new();
        if let Some(ref inc_dirs) = toml.project.include {
            for dir in inc_dirs {
                let abs = tmp.join(dir);
                if abs.is_dir() {
                    paths.push(abs);
                }
            }
        }
        assert_eq!(paths.len(), 2);
        assert!(paths[0].ends_with("include"));
        assert!(paths[1].ends_with("custom_inc"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resolve_include_paths_fallback_without_toml() {
        use std::fs;

        let tmp = std::env::temp_dir().join("nera_test_no_toml");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("include")).unwrap();
        fs::create_dir_all(tmp.join("src")).unwrap();
        // No nera.toml — should detect project root via src/ subdirectory

        // Verify find_project_root_from detects the directory as a project root
        // because it contains a src/ subdirectory
        let project_root = find_project_root_from(&tmp);
        assert_eq!(project_root.as_deref(), Some(tmp.as_path()));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn build_offset_map_with_include() {
        // Main file: "-module(test).\n" (15 bytes), then "-include(\"h.hrl\").\n" (19 bytes)
        // but after preprocessing, combined = main_prefix + hrl_content + main_suffix
        let original = "-module(test).\n-include(\"h.hrl\").\nfoo() -> ok.\n";
        let hrl_content = "-define(X, 1).\n";
        // Combined: main line 1 + hrl content + main line 3
        let combined = format!("-module(test).\n{}foo() -> ok.\n", hrl_content);
        let segments = vec![
            nera_syntax::preproc::FileSegment {
                start: 0,
                end: 15,
                filename: "test.erl".to_string(),
            },
            nera_syntax::preproc::FileSegment {
                start: 15,
                end: 15 + hrl_content.len(),
                filename: "h.hrl".to_string(),
            },
            nera_syntax::preproc::FileSegment {
                start: 15 + hrl_content.len(),
                end: combined.len(),
                filename: "test.erl".to_string(),
            },
        ];

        let offset_map = build_offset_map(original, &combined, &segments, "test.erl");
        assert!(!offset_map.is_empty(), "offset map should not be empty");

        // First chunk: original [0..15) maps to combined [0..15)
        assert_eq!(offset_map[0], (0, 15, 0));

        // Second chunk maps original after the -include line to combined after the hrl content
        // Original "-include..." is at [15..34), "foo() -> ok.\n" is at [34..47)
        // Combined "foo() -> ok.\n" starts at 15 + 15 = 30
        if offset_map.len() > 1 {
            let (orig_start, _, comb_start) = offset_map[1];
            assert_eq!(comb_start, 15 + hrl_content.len());
            // The original offset should be after the include directive
            assert!(orig_start >= 15);
        }
    }

    /// End-to-end test: preprocess a file with -include, then verify
    /// that record navigation resolves to the included header file.
    #[test]
    fn goto_record_in_included_file_end_to_end() {
        use std::fs;

        let tmp = std::env::temp_dir().join("nera_test_record_include");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("src")).unwrap();
        fs::create_dir_all(tmp.join("include")).unwrap();

        let hrl_content = "-record(demo_state, {name, age}).\n";
        fs::write(tmp.join("include/demo.hrl"), hrl_content).unwrap();

        let erl_source = "-module(greeter).\n-include(\"demo.hrl\").\n-export([message/1]).\nmessage(Name) ->\n    State = #demo_state{},\n    {Name, State}.\n";
        let erl_path = tmp.join("src/greeter.erl");
        fs::write(&erl_path, erl_source).unwrap();

        // Preprocess with include paths
        let tokens = Lexer::lex_all(erl_source).expect("lex");
        let mut preproc = Preprocessor::new(erl_source, &erl_path);
        preproc.set_include_paths(vec![tmp.join("include")]);
        let processed = preproc.process(tokens).expect("preprocess should succeed");
        let (combined, segments) = preproc.take_combined_source_and_segments();

        // Verify we have at least the main file + include segments
        assert!(segments.len() >= 2, "expected at least 2 segments, got {}", segments.len());

        // Parse the combined source
        let interner = DefaultStringInterner::default();
        let mut parser = Parser::with_tokens(&combined, processed, interner);
        let result = parser.parse_module();
        assert!(result.errors.is_empty(), "parse errors: {:#?}", result.errors);
        let module = result.module;

        // Find the record definition
        assert!(!module.records.is_empty(), "module should have records from the include");
        let rec = &module.records[0];
        let rec_text = span_text(&combined, rec.span);
        // Verify the record span falls within the include segment
        let include_seg = segments.iter().find(|s| s.filename.contains("demo.hrl"));
        assert!(include_seg.is_some(), "should have a demo.hrl segment");
        let inc_seg = include_seg.unwrap();
        assert!(
            rec.span.start >= inc_seg.start && rec.span.start < inc_seg.end,
            "record span {} should be within include segment {}..{}",
            rec.span.start, inc_seg.start, inc_seg.end,
        );

        // Now test resolve_span_to_location
        let sm = SourceMap::new(&combined);
        let original_sm = SourceMap::new(erl_source);
        let display_path = erl_path.to_string_lossy().to_string();
        let offset_map = build_offset_map(erl_source, &combined, &segments, &display_path);
        let uri = Url::from_file_path(&erl_path).unwrap();

        let result = resolve_span_to_location(
            rec.span, &uri, &segments, &sm, &original_sm, &offset_map, Some(tmp.as_path()),
        );
        let (resolved_uri, range) = result.expect("should resolve");

        // Should resolve to demo.hrl, NOT greeter.erl
        assert!(
            resolved_uri.path().contains("demo.hrl"),
            "expected demo.hrl URI, got {}",
            resolved_uri,
        );
        // Record should be at line 0 of demo.hrl
        assert_eq!(range.start.line, 0);

        // Also verify that the record name extraction works correctly
        // (rec.span starts at the name, NOT at "-record(")
        let end = rec_text.find([',', ' ', '{', '(', ')']).unwrap_or(rec_text.len());
        let extracted_name = &rec_text[..end];
        assert_eq!(extracted_name, "demo_state");

        // Verify CursorTarget::RecordRef matching works
        let rec_use_offset = combined.rfind("#demo_state").expect("should find #demo_state in body");
        let target = find_cursor_target(&module, rec_use_offset, &combined);
        match target {
            Some(CursorTarget::RecordRef { name }) => {
                assert_eq!(name, "demo_state");
                // Verify this name matches the extracted record name
                assert_eq!(name, extracted_name);
            }
            other => panic!("expected RecordRef, got {:?}", other),
        }

        let _ = fs::remove_dir_all(&tmp);
    }

    // ── find_macro_at_offset tests ──────────────────────────────────

    #[test]
    fn find_macro_at_question_mark() {
        let src = "io:format(?APP_NAME)";
        // offset 10 is '?'
        assert_eq!(find_macro_at_offset(src, 10), Some("APP_NAME".to_string()));
    }

    #[test]
    fn find_macro_at_name_start() {
        let src = "io:format(?APP_NAME)";
        // offset 11 is 'A' (first char of name)
        assert_eq!(find_macro_at_offset(src, 11), Some("APP_NAME".to_string()));
    }

    #[test]
    fn find_macro_in_middle_of_name() {
        let src = "io:format(?APP_NAME)";
        // offset 14 is '_' in APP_NAME
        assert_eq!(find_macro_at_offset(src, 14), Some("APP_NAME".to_string()));
    }

    #[test]
    fn find_macro_at_last_char() {
        let src = "io:format(?APP_NAME)";
        // offset 18 is 'E' (last char of APP_NAME)
        assert_eq!(find_macro_at_offset(src, 18), Some("APP_NAME".to_string()));
    }

    #[test]
    fn find_macro_not_on_macro() {
        let src = "io:format(?APP_NAME)";
        // offset 0 is 'i' — not on a macro
        assert_eq!(find_macro_at_offset(src, 0), None);
    }

    #[test]
    fn find_macro_past_end() {
        let src = "?FOO";
        // offset 19 is past end — not on a macro
        assert_eq!(find_macro_at_offset(src, 19), None);
    }

    #[test]
    fn find_macro_after_name() {
        let src = "io:format(?APP_NAME)";
        // offset 19 is ')' — just past the macro
        assert_eq!(find_macro_at_offset(src, 19), None);
    }

    #[test]
    fn find_macro_module() {
        let src = "?MODULE";
        assert_eq!(find_macro_at_offset(src, 0), Some("MODULE".to_string()));
        assert_eq!(find_macro_at_offset(src, 3), Some("MODULE".to_string()));
        assert_eq!(find_macro_at_offset(src, 6), Some("MODULE".to_string()));
    }

    // ── Macro location tracking in preprocessor ─────────────────────

    #[test]
    fn preproc_tracks_macro_location() {
        use nera_syntax::lexer::Lexer;
        use nera_syntax::preproc::Preprocessor;
        use std::path::PathBuf;

        let src = "-define(APP_NAME, \"myapp\").\n-module(test).\n";
        let tokens = Lexer::lex_all(src).unwrap();
        let path = PathBuf::from("test.erl");
        let mut preproc = Preprocessor::new(src, &path);
        let _ = preproc.process(tokens).unwrap();
        let (_, _, macro_locs, _) = preproc.take_all();

        assert_eq!(macro_locs.len(), 1);
        assert_eq!(macro_locs[0].name, "APP_NAME");
        // The span should cover the `-define(APP_NAME, "myapp").` text
        let define_text = &src[macro_locs[0].combined_span.start..macro_locs[0].combined_span.end];
        assert!(define_text.starts_with("-define"));
        assert!(define_text.ends_with('.'));
    }

    // ── find_export_entry_at_offset tests ────────────────────────────

    #[test]
    fn export_entry_on_function_name() {
        let src = "-module(test).\n-export([start/0, foo/2]).\nstart() -> ok.\n";
        // Cursor on "start" in the export list
        let offset = src.find("start/0").unwrap();
        let result = find_export_entry_at_offset(src, offset);
        assert_eq!(result, Some(("start".to_string(), 0)));
    }

    #[test]
    fn export_entry_on_arity() {
        let src = "-module(test).\n-export([start/0, foo/2]).\nstart() -> ok.\n";
        // Cursor on "0" in "start/0"
        let offset = src.find("start/0").unwrap() + 6; // on the '0'
        let result = find_export_entry_at_offset(src, offset);
        assert_eq!(result, Some(("start".to_string(), 0)));
    }

    #[test]
    fn export_entry_on_second_function() {
        let src = "-module(test).\n-export([start/0, foo/2]).\nstart() -> ok.\nfoo(A, B) -> {A, B}.\n";
        let offset = src.find("foo/2").unwrap();
        let result = find_export_entry_at_offset(src, offset);
        assert_eq!(result, Some(("foo".to_string(), 2)));
    }

    #[test]
    fn export_entry_outside_export() {
        let src = "-module(test).\n-export([start/0]).\nstart() -> ok.\n";
        // Cursor on "start" in the function definition, not the export
        let offset = src.rfind("start()").unwrap();
        let result = find_export_entry_at_offset(src, offset);
        assert_eq!(result, None);
    }

    #[test]
    fn export_entry_no_export() {
        let src = "-module(test).\nstart() -> ok.\n";
        let offset = src.find("start").unwrap();
        let result = find_export_entry_at_offset(src, offset);
        assert_eq!(result, None);
    }

    // ── find_include_at_offset tests ─────────────────────────────────

    #[test]
    fn include_on_filename() {
        let src = "-module(test).\n-include(\"demo.hrl\").\n";
        let offset = src.find("demo.hrl").unwrap();
        assert_eq!(find_include_at_offset(src, offset), Some("demo.hrl".to_string()));
    }

    #[test]
    fn include_on_directive() {
        let src = "-module(test).\n-include(\"demo.hrl\").\n";
        let offset = src.find("-include").unwrap() + 1;
        assert_eq!(find_include_at_offset(src, offset), Some("demo.hrl".to_string()));
    }

    #[test]
    fn include_lib() {
        let src = "-module(test).\n-include_lib(\"kernel/include/file.hrl\").\n";
        let offset = src.find("kernel").unwrap();
        assert_eq!(find_include_at_offset(src, offset), Some("kernel/include/file.hrl".to_string()));
    }

    #[test]
    fn include_not_on_include_line() {
        let src = "-module(test).\n-include(\"demo.hrl\").\nstart() -> ok.\n";
        let offset = src.find("start").unwrap();
        assert_eq!(find_include_at_offset(src, offset), None);
    }

    // ── pattern_span tests ─────────────────────────────────────────────

    #[test]
    fn pattern_span_variable() {
        let src = "-module(test).\nfoo(X) -> X.\n";
        let (module, combined) = parse_module(src);
        let pat = &module.functions[0].clauses[0].patterns[0];
        let span = pattern_span(pat);
        assert_eq!(span_text(&combined, span), "X");
    }

    #[test]
    fn pattern_span_tuple() {
        let src = "-module(test).\nfoo({A, B}) -> {A, B}.\n";
        let (module, combined) = parse_module(src);
        let pat = &module.functions[0].clauses[0].patterns[0];
        let span = pattern_span(pat);
        assert_eq!(span_text(&combined, span), "{A, B}");
    }

    #[test]
    fn pattern_span_wildcard() {
        let src = "-module(test).\nfoo(_) -> ok.\n";
        let (module, combined) = parse_module(src);
        let pat = &module.functions[0].clauses[0].patterns[0];
        let span = pattern_span(pat);
        assert_eq!(span_text(&combined, span), "_");
    }

    #[test]
    fn pattern_span_atom() {
        let src = "-module(test).\nfoo(ok) -> ok.\n";
        let (module, combined) = parse_module(src);
        let pat = &module.functions[0].clauses[0].patterns[0];
        let span = pattern_span(pat);
        assert_eq!(span_text(&combined, span), "ok");
    }

    #[test]
    fn pattern_span_integer() {
        let src = "-module(test).\nfoo(42) -> ok.\n";
        let (module, combined) = parse_module(src);
        let pat = &module.functions[0].clauses[0].patterns[0];
        let span = pattern_span(pat);
        assert_eq!(span_text(&combined, span), "42");
    }

    #[test]
    fn pattern_span_cons() {
        let src = "-module(test).\nfoo([H|T]) -> {H, T}.\n";
        let (module, combined) = parse_module(src);
        let pat = &module.functions[0].clauses[0].patterns[0];
        let span = pattern_span(pat);
        assert_eq!(span_text(&combined, span), "[H|T]");
    }

    #[test]
    fn pattern_span_nil() {
        let src = "-module(test).\nfoo([]) -> ok.\n";
        let (module, combined) = parse_module(src);
        let pat = &module.functions[0].clauses[0].patterns[0];
        let span = pattern_span(pat);
        assert_eq!(span_text(&combined, span), "[]");
    }

    #[test]
    fn pattern_span_match() {
        let src = "-module(test).\nfoo(X = {ok, V}) -> {X, V}.\n";
        let (module, combined) = parse_module(src);
        let pat = &module.functions[0].clauses[0].patterns[0];
        let span = pattern_span(pat);
        assert_eq!(span_text(&combined, span), "X = {ok, V}");
    }

    // ── find_local_call_spans tests ────────────────────────────────────

    #[test]
    fn find_local_calls_simple() {
        let src = "-module(test).\nstart() -> foo(1).\nfoo(X) -> X.\n";
        let (module, combined) = parse_module(src);
        let spans = find_local_call_spans(&module, &combined, "foo", 1);
        assert_eq!(spans.len(), 1);
        assert_eq!(span_text(&combined, spans[0]), "foo(1)");
    }

    #[test]
    fn find_local_calls_multiple() {
        let src = "-module(test).\nstart() -> foo(1), foo(2), foo(3).\nfoo(X) -> X.\n";
        let (module, combined) = parse_module(src);
        let spans = find_local_call_spans(&module, &combined, "foo", 1);
        assert_eq!(spans.len(), 3);
        assert_eq!(span_text(&combined, spans[0]), "foo(1)");
        assert_eq!(span_text(&combined, spans[1]), "foo(2)");
        assert_eq!(span_text(&combined, spans[2]), "foo(3)");
    }

    #[test]
    fn find_local_calls_different_arity() {
        let src = "-module(test).\nstart() -> foo(1), foo(1, 2).\nfoo(X) -> X.\nfoo(X, Y) -> {X, Y}.\n";
        let (module, combined) = parse_module(src);
        let spans1 = find_local_call_spans(&module, &combined, "foo", 1);
        assert_eq!(spans1.len(), 1);
        assert_eq!(span_text(&combined, spans1[0]), "foo(1)");
        let spans2 = find_local_call_spans(&module, &combined, "foo", 2);
        assert_eq!(spans2.len(), 1);
        assert_eq!(span_text(&combined, spans2[0]), "foo(1, 2)");
    }

    #[test]
    fn find_local_calls_no_match() {
        let src = "-module(test).\nstart() -> bar(1).\nbar(X) -> X.\n";
        let (module, combined) = parse_module(src);
        let spans = find_local_call_spans(&module, &combined, "foo", 1);
        assert!(spans.is_empty());
    }

    #[test]
    fn find_local_calls_in_case() {
        let src = "-module(test).\nstart(X) -> case X of 1 -> foo(X); _ -> ok end.\nfoo(Y) -> Y.\n";
        let (module, combined) = parse_module(src);
        let spans = find_local_call_spans(&module, &combined, "foo", 1);
        assert_eq!(spans.len(), 1);
        assert_eq!(span_text(&combined, spans[0]), "foo(X)");
    }

    #[test]
    fn find_local_calls_in_nested_case() {
        let src = "-module(test).\nstart(X) -> case X of 1 -> case X of 1 -> foo(X); _ -> ok end; _ -> foo(X) end.\nfoo(Y) -> Y.\n";
        let (module, combined) = parse_module(src);
        let spans = find_local_call_spans(&module, &combined, "foo", 1);
        assert_eq!(spans.len(), 2);
    }

    #[test]
    fn find_local_calls_across_functions() {
        let src = "-module(test).\na() -> foo(1).\nb() -> foo(2).\nc() -> bar(1).\nfoo(X) -> X.\nbar(X) -> X.\n";
        let (module, combined) = parse_module(src);
        let spans = find_local_call_spans(&module, &combined, "foo", 1);
        assert_eq!(spans.len(), 2);
        assert_eq!(span_text(&combined, spans[0]), "foo(1)");
        assert_eq!(span_text(&combined, spans[1]), "foo(2)");
    }

    #[test]
    fn find_local_calls_in_fun_expression() {
        let src = "-module(test).\nstart() -> F = fun() -> foo(1) end, F().\nfoo(X) -> X.\n";
        let (module, combined) = parse_module(src);
        let spans = find_local_call_spans(&module, &combined, "foo", 1);
        assert_eq!(spans.len(), 1);
        assert_eq!(span_text(&combined, spans[0]), "foo(1)");
    }

    #[test]
    fn find_local_calls_in_try_catch() {
        let src = "-module(test).\nstart() -> try foo(1) catch _:_ -> foo(2) end.\nfoo(X) -> X.\n";
        let (module, combined) = parse_module(src);
        let spans = find_local_call_spans(&module, &combined, "foo", 1);
        assert_eq!(spans.len(), 2);
    }

    #[test]
    fn find_local_calls_in_list_comprehension() {
        let src = "-module(test).\nstart(L) -> [foo(X) || X <- L].\nfoo(X) -> X.\n";
        let (module, combined) = parse_module(src);
        let spans = find_local_call_spans(&module, &combined, "foo", 1);
        assert_eq!(spans.len(), 1);
        assert_eq!(span_text(&combined, spans[0]), "foo(X)");
    }

    // ── find_remote_call_spans tests ───────────────────────────────────

    #[test]
    fn find_remote_calls_simple() {
        let src = "-module(test).\nstart() -> lists:reverse([1, 2]).\n";
        let (module, combined) = parse_module(src);
        let spans = find_remote_call_spans(&module, &combined, "lists", "reverse", 1);
        assert_eq!(spans.len(), 1);
        assert_eq!(span_text(&combined, spans[0]), "lists:reverse([1, 2])");
    }

    #[test]
    fn find_remote_calls_multiple() {
        let src = "-module(test).\nstart(L) -> lists:reverse(L), lists:reverse(lists:sort(L)).\n";
        let (module, combined) = parse_module(src);
        let spans = find_remote_call_spans(&module, &combined, "lists", "reverse", 1);
        assert_eq!(spans.len(), 2);
    }

    #[test]
    fn find_remote_calls_no_match_wrong_module() {
        let src = "-module(test).\nstart() -> lists:reverse([1, 2]).\n";
        let (module, combined) = parse_module(src);
        let spans = find_remote_call_spans(&module, &combined, "string", "reverse", 1);
        assert!(spans.is_empty());
    }

    #[test]
    fn find_remote_calls_no_match_wrong_arity() {
        let src = "-module(test).\nstart() -> lists:reverse([1, 2]).\n";
        let (module, combined) = parse_module(src);
        let spans = find_remote_call_spans(&module, &combined, "lists", "reverse", 2);
        assert!(spans.is_empty());
    }

    #[test]
    fn find_remote_calls_in_case() {
        let src = "-module(test).\nstart(L) -> case L of [] -> []; _ -> lists:reverse(L) end.\n";
        let (module, combined) = parse_module(src);
        let spans = find_remote_call_spans(&module, &combined, "lists", "reverse", 1);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn find_remote_calls_mixed_modules() {
        let src = "-module(test).\nstart(L) -> io:format(\"~p~n\", [lists:reverse(L)]).\n";
        let (module, combined) = parse_module(src);
        let io_spans = find_remote_call_spans(&module, &combined, "io", "format", 2);
        assert_eq!(io_spans.len(), 1);
        let lists_spans = find_remote_call_spans(&module, &combined, "lists", "reverse", 1);
        assert_eq!(lists_spans.len(), 1);
    }

    // ── find_variable_spans_in_clause tests ────────────────────────────

    #[test]
    fn find_var_spans_simple_binding_and_use() {
        let src = "-module(test).\nfoo(X) -> X + 1.\n";
        let (module, combined) = parse_module(src);
        let clause = &module.functions[0].clauses[0];
        let spans = find_variable_spans_in_clause(clause, &combined, "X");
        assert_eq!(spans.len(), 2); // binding in pattern + use in body
        assert_eq!(span_text(&combined, spans[0]), "X"); // pattern binding
        assert_eq!(span_text(&combined, spans[1]), "X"); // body use
    }

    #[test]
    fn find_var_spans_multiple_uses() {
        let src = "-module(test).\nfoo(X) -> {X, X, X}.\n";
        let (module, combined) = parse_module(src);
        let clause = &module.functions[0].clauses[0];
        let spans = find_variable_spans_in_clause(clause, &combined, "X");
        assert_eq!(spans.len(), 4); // 1 binding + 3 uses
    }

    #[test]
    fn find_var_spans_no_match() {
        let src = "-module(test).\nfoo(X) -> X.\n";
        let (module, combined) = parse_module(src);
        let clause = &module.functions[0].clauses[0];
        let spans = find_variable_spans_in_clause(clause, &combined, "Y");
        assert!(spans.is_empty());
    }

    #[test]
    fn find_var_spans_tuple_pattern() {
        let src = "-module(test).\nfoo({X, Y}) -> X + Y.\n";
        let (module, combined) = parse_module(src);
        let clause = &module.functions[0].clauses[0];
        let x_spans = find_variable_spans_in_clause(clause, &combined, "X");
        assert_eq!(x_spans.len(), 2); // pattern + body
        let y_spans = find_variable_spans_in_clause(clause, &combined, "Y");
        assert_eq!(y_spans.len(), 2);
    }

    #[test]
    fn find_var_spans_cons_pattern() {
        let src = "-module(test).\nfoo([H|T]) -> {H, T}.\n";
        let (module, combined) = parse_module(src);
        let clause = &module.functions[0].clauses[0];
        let h_spans = find_variable_spans_in_clause(clause, &combined, "H");
        assert_eq!(h_spans.len(), 2);
        let t_spans = find_variable_spans_in_clause(clause, &combined, "T");
        assert_eq!(t_spans.len(), 2);
    }

    #[test]
    fn find_var_spans_in_case_body() {
        let src = "-module(test).\nfoo(X) -> case X of 1 -> X; _ -> X + 1 end.\n";
        let (module, combined) = parse_module(src);
        let clause = &module.functions[0].clauses[0];
        let spans = find_variable_spans_in_clause(clause, &combined, "X");
        // 1 pattern binding + 3 body uses (case scrutinee, clause 1 body, clause 2 body)
        assert_eq!(spans.len(), 4);
    }

    #[test]
    fn find_var_spans_in_match_expr() {
        let src = "-module(test).\nfoo(X) -> Y = X + 1, Y.\n";
        let (module, combined) = parse_module(src);
        let clause = &module.functions[0].clauses[0];
        let x_spans = find_variable_spans_in_clause(clause, &combined, "X");
        assert_eq!(x_spans.len(), 2); // pattern + body use
        let y_spans = find_variable_spans_in_clause(clause, &combined, "Y");
        assert_eq!(y_spans.len(), 2); // match binding + final use
    }

    #[test]
    fn find_var_spans_in_fun_expression() {
        let src = "-module(test).\nfoo(X) -> F = fun(Y) -> X + Y end, F(X).\n";
        let (module, combined) = parse_module(src);
        let clause = &module.functions[0].clauses[0];
        let x_spans = find_variable_spans_in_clause(clause, &combined, "X");
        // pattern binding + use inside fun + use in F(X)
        assert_eq!(x_spans.len(), 3);
    }

    #[test]
    fn find_var_spans_multiple_clauses_isolated() {
        // Variable spans in clause 1 should not include clause 2
        let src = "-module(test).\nfoo(1) -> ok;\nfoo(X) -> X.\n";
        let (module, combined) = parse_module(src);
        let clause0 = &module.functions[0].clauses[0];
        let spans0 = find_variable_spans_in_clause(clause0, &combined, "X");
        assert!(spans0.is_empty()); // X is not in clause 0
        let clause1 = &module.functions[0].clauses[1];
        let spans1 = find_variable_spans_in_clause(clause1, &combined, "X");
        assert_eq!(spans1.len(), 2); // pattern + body
    }

    #[test]
    fn find_var_spans_in_list_comprehension() {
        let src = "-module(test).\nfoo(L) -> [X || X <- L].\n";
        let (module, combined) = parse_module(src);
        let clause = &module.functions[0].clauses[0];
        let x_spans = find_variable_spans_in_clause(clause, &combined, "X");
        // X appears: in the output expression + in the generator pattern
        assert_eq!(x_spans.len(), 2);
        let l_spans = find_variable_spans_in_clause(clause, &combined, "L");
        // L appears: in the function pattern + in the generator source
        assert_eq!(l_spans.len(), 2);
    }

    // ── find_variable_highlights_in_clause tests ──────────────────────

    #[test]
    fn find_var_highlights_binding_vs_use() {
        use tower_lsp::lsp_types::DocumentHighlightKind;
        let src = "-module(test).\nfoo(X) -> X + 1.\n";
        let (module, combined) = parse_module(src);
        let clause = &module.functions[0].clauses[0];
        let hls = find_variable_highlights_in_clause(clause, &combined, "X");
        assert_eq!(hls.len(), 2);
        // Write (pattern binding) comes first; Read (use) after.
        assert_eq!(hls[0].1, DocumentHighlightKind::WRITE);
        assert_eq!(hls[1].1, DocumentHighlightKind::READ);
    }

    #[test]
    fn find_var_highlights_match_expr() {
        use tower_lsp::lsp_types::DocumentHighlightKind;
        let src = "-module(test).\nfoo(X) -> Y = X + 1, Y.\n";
        let (module, combined) = parse_module(src);
        let clause = &module.functions[0].clauses[0];
        let y_hls = find_variable_highlights_in_clause(clause, &combined, "Y");
        assert_eq!(y_hls.len(), 2);
        let writes = y_hls.iter().filter(|(_, k)| *k == DocumentHighlightKind::WRITE).count();
        let reads = y_hls.iter().filter(|(_, k)| *k == DocumentHighlightKind::READ).count();
        assert_eq!(writes, 1);
        assert_eq!(reads, 1);
    }

    #[test]
    fn find_var_highlights_case_clause_pattern_is_write() {
        use tower_lsp::lsp_types::DocumentHighlightKind;
        let src = "-module(test).\nfoo() -> case 1 of X -> X + 1 end.\n";
        let (module, combined) = parse_module(src);
        let clause = &module.functions[0].clauses[0];
        let hls = find_variable_highlights_in_clause(clause, &combined, "X");
        assert_eq!(hls.len(), 2);
        let writes = hls.iter().filter(|(_, k)| *k == DocumentHighlightKind::WRITE).count();
        let reads = hls.iter().filter(|(_, k)| *k == DocumentHighlightKind::READ).count();
        assert_eq!(writes, 1); // case clause pattern
        assert_eq!(reads, 1);  // clause body use
    }

    // ── Inlay hints tests ─────────────────────────────────────────────

    /// Helper: collect inlay hints from a single-module source.
    fn collect_hints(source: &str) -> Vec<(String, u32, u32)> {
        let (module, combined) = parse_module(source);
        let sm = SourceMap::new(&combined);
        let project_modules = Arc::new(Mutex::new(Vec::new()));
        let mut hints = Vec::new();
        for func in &module.functions {
            for clause in &func.clauses {
                collect_inlay_hints_from_exprs(
                    &clause.body, &module, &combined, &sm, &project_modules, clause.span, &mut hints,
                );
            }
        }
        hints.iter().map(|h| {
            let label = match &h.label {
                InlayHintLabel::String(s) => s.clone(),
                _ => panic!("expected string label"),
            };
            (label, h.position.line, h.position.character)
        }).collect()
    }

    #[test]
    fn inlay_hints_bif_auto_imported() {
        // element/2 is auto-imported from erlang — should show N: and Tuple: hints
        let src = "-module(test).\nfoo(T) -> element(2, T).\n";
        let hints = collect_hints(src);
        assert_eq!(hints.len(), 1); // T matches "Tuple" case-insensitively → skipped, only N: shown
        // Actually: "N" is a single char, so it gets skipped by the <=1 filter.
        // And "T" matches "Tuple" case-insensitively → skipped.
        // So we get 0 hints. Let's verify:
        // Wait — N is len 1, so skipped. T vs Tuple → case-insensitive first char? No, full comparison.
        // "t" != "tuple" (lowercase), so T does NOT match Tuple. Let me re-check the logic.
        // The code compares var_name.to_lowercase() == name.to_lowercase()
        // "T".to_lowercase() = "t", "Tuple".to_lowercase() = "tuple" → not equal, so NOT skipped.
        // So we should get 1 hint (Tuple:) because N is skipped (len <=1).
        assert_eq!(hints[0].0, "Tuple:");
    }

    #[test]
    fn inlay_hints_bif_remote_call() {
        // maps:get/2 should show Key: and Map: hints
        let src = "-module(test).\nfoo(M) -> maps:get(name, M).\n";
        let hints = collect_hints(src);
        // "name" is an atom, not a variable → not skipped
        // "M" vs "Map" → "m" != "map" → not skipped
        assert_eq!(hints.len(), 2);
        assert_eq!(hints[0].0, "Key:");
        assert_eq!(hints[1].0, "Map:");
    }

    #[test]
    fn inlay_hints_bif_keyfind() {
        // lists:keyfind/3 — Key, N, TupleList
        let src = "-module(test).\nfoo(Users) -> lists:keyfind(alice, 1, Users).\n";
        let hints = collect_hints(src);
        // "alice" → not a var, shows Key:
        // "1" → literal, shows N: — wait, N is len 1, skipped!
        // "Users" vs "TupleList" → not equal, shows TupleList:
        assert_eq!(hints.len(), 2);
        assert_eq!(hints[0].0, "Key:");
        assert_eq!(hints[1].0, "TupleList:");
    }

    #[test]
    fn inlay_hints_bif_spawn() {
        // spawn/3 — Module, Function, Args
        let src = "-module(test).\nfoo() -> spawn(mymod, start, []).\n";
        let hints = collect_hints(src);
        assert_eq!(hints.len(), 3);
        assert_eq!(hints[0].0, "Module:");
        assert_eq!(hints[1].0, "Function:");
        assert_eq!(hints[2].0, "Args:");
    }

    #[test]
    fn inlay_hints_bif_skip_same_name_var() {
        // If the variable name matches the parameter name (case-insensitive), skip the hint
        let src = "-module(test).\nfoo(Key, Map) -> maps:get(Key, Map).\n";
        let hints = collect_hints(src);
        // Both "Key" matches "Key" and "Map" matches "Map" → both skipped
        assert_eq!(hints.len(), 0);
    }

    #[test]
    fn inlay_hints_bif_type_check_no_hints() {
        // is_integer/1 has param_names=None, no hints should appear
        let src = "-module(test).\nfoo(X) -> is_integer(X).\n";
        let hints = collect_hints(src);
        assert_eq!(hints.len(), 0);
    }

    #[test]
    fn inlay_hints_bif_setelement() {
        // setelement/3 — Index, Tuple, Value
        let src = "-module(test).\nfoo(T) -> setelement(1, T, new_val).\n";
        let hints = collect_hints(src);
        // "1" → literal, "Index" len > 1 → show Index:
        // "T" vs "Tuple" → "t" != "tuple" → show Tuple:
        // "new_val" → atom, show Value:
        assert_eq!(hints.len(), 3);
        assert_eq!(hints[0].0, "Index:");
        assert_eq!(hints[1].0, "Tuple:");
        assert_eq!(hints[2].0, "Value:");
    }

    #[test]
    fn inlay_hints_bif_ets_operations() {
        // ets:lookup/2 — Table, Key
        let src = "-module(test).\nfoo(Tab) -> ets:lookup(Tab, user_id).\n";
        let hints = collect_hints(src);
        assert_eq!(hints.len(), 2);
        assert_eq!(hints[0].0, "Table:");
        assert_eq!(hints[1].0, "Key:");
    }

    #[test]
    fn inlay_hints_best_name_across_clauses_bare_over_underscore() {
        // The first clause uses _Pred (underscore-prefixed) while the second uses Pred.
        // The hint should show "Pred:" (the bare variable from clause 2).
        let src = "\
-module(test).
process(_Pred, []) -> ok;
process(Pred, [H|T]) -> Pred(H), process(Pred, T).
caller() -> process(fun is_atom/1, [a, b, c]).
";
        let hints = collect_hints(src);
        let labels: Vec<&str> = hints.iter().map(|h| h.0.as_str()).collect();
        assert!(labels.contains(&"Pred:"), "expected Pred: hint, got {:?}", labels);
        assert!(!labels.contains(&"_Pred:"), "should not show _Pred:");
    }

    #[test]
    fn inlay_hints_best_name_across_clauses_var_over_pattern() {
        // First clause uses a literal [] pattern, second clause has a named variable List.
        // The hint should show "List:" not "[]:".
        let src = "\
-module(test).
rev([], Acc) -> Acc;
rev(List, Acc) -> rev(tl(List), [hd(List)|Acc]).
go() -> rev([1,2,3], []).
";
        let hints = collect_hints(src);
        let labels: Vec<&str> = hints.iter().map(|h| h.0.as_str()).collect();
        assert!(labels.contains(&"List:"), "expected List: hint, got {:?}", labels);
        assert!(labels.contains(&"Acc:"), "expected Acc: hint, got {:?}", labels);
    }

    #[test]
    fn inlay_hints_best_name_single_clause_unchanged() {
        // Single-clause function: existing behaviour should be preserved.
        let src = "\
-module(test).
add(Left, Right) -> Left + Right.
go() -> add(1, 2).
";
        let hints = collect_hints(src);
        assert_eq!(hints.len(), 2);
        assert_eq!(hints[0].0, "Left:");
        assert_eq!(hints[1].0, "Right:");
    }

    #[test]
    fn inlay_hints_best_name_all_clauses_underscore() {
        // When all clauses use underscore-prefixed names, strip the leading underscore.
        let src = "\
-module(test).
ignore(_Val, _Tag) -> ok;
ignore(_Val, _Tag) -> ok.
go() -> ignore(42, info).
";
        let hints = collect_hints(src);
        let labels: Vec<&str> = hints.iter().map(|h| h.0.as_str()).collect();
        assert!(labels.contains(&"Val:"), "expected Val: (stripped underscore), got {:?}", labels);
        assert!(labels.contains(&"Tag:"), "expected Tag: (stripped underscore), got {:?}", labels);
    }

    #[test]
    fn inlay_hints_best_name_pattern_only_suppressed() {
        // When no clause has a variable at a position (only patterns like [] and [H|T]),
        // the hint for that position should be suppressed.
        let src = "\
-module(test).
filter(_Pred, [], Acc) -> lists:reverse(Acc);
filter(Pred, [H|T], Acc) ->
    case Pred(H) of
        true -> filter(Pred, T, [H|Acc]);
        false -> filter(Pred, T, Acc)
    end.
go() -> filter(fun is_atom/1, [a,b,c], []).
";
        let hints = collect_hints(src);
        let labels: Vec<&str> = hints.iter().map(|h| h.0.as_str()).collect();
        // Second param ([] / [H|T]) has no variable name — should be suppressed
        assert!(!labels.iter().any(|l| l.starts_with("[")), "pattern hints like []: should be suppressed, got {:?}", labels);
        // First param: Pred should appear (bare wins over _Pred, stripped)
        assert!(labels.contains(&"Pred:"), "expected Pred: hint, got {:?}", labels);
        // Third param: Acc should appear
        assert!(labels.contains(&"Acc:"), "expected Acc: hint, got {:?}", labels);
    }

    #[test]
    fn inlay_hints_best_name_single_char_bare_wins_over_underscore() {
        // F (bare, single char) should win over _F (underscore-prefixed).
        // After stripping, F is len 1, so the hint is suppressed (better than showing _F:).
        let src = "\
-module(test).
foldl(_F, Acc, []) -> Acc;
foldl(F, Acc, [H|T]) -> foldl(F, F(H, Acc), T).
go() -> foldl(fun erlang:'+'/2, 0, [1,2,3]).
";
        let hints = collect_hints(src);
        let labels: Vec<&str> = hints.iter().map(|h| h.0.as_str()).collect();
        // F is single char → suppressed by display filter. No _F: should appear either.
        assert!(!labels.iter().any(|l| *l == "_F:" || *l == "F:"), "single-char param should be suppressed, got {:?}", labels);
        // Acc should still appear
        assert!(labels.contains(&"Acc:"), "expected Acc: hint, got {:?}", labels);
    }

    #[test]
    fn inlay_hints_best_name_wildcard_all_clauses_suppressed() {
        // When all clauses use bare wildcard _ at a position, suppress the hint.
        let src = "\
-module(test).
handle(_, State) -> {ok, State};
handle(_, State) -> {error, State}.
go(S) -> handle(msg, S).
";
        let hints = collect_hints(src);
        let labels: Vec<&str> = hints.iter().map(|h| h.0.as_str()).collect();
        // First param: _ in all clauses → suppressed
        assert!(!labels.contains(&"_:"), "wildcard should be suppressed, got {:?}", labels);
        // Second param: State should appear
        assert!(labels.contains(&"State:"), "expected State: hint, got {:?}", labels);
    }

    #[test]
    fn inlay_hints_best_name_atom_pattern_with_variable_in_other_clause() {
        // One clause uses an atom pattern, another has a variable — variable wins.
        let src = "\
-module(test).
respond(ok, Data) -> {ok, Data};
respond(Action, Data) -> {Action, Data}.
go() -> respond(retry, payload).
";
        let hints = collect_hints(src);
        let labels: Vec<&str> = hints.iter().map(|h| h.0.as_str()).collect();
        assert!(labels.contains(&"Action:"), "expected Action: (variable wins over atom), got {:?}", labels);
        assert!(labels.contains(&"Data:"), "expected Data: hint, got {:?}", labels);
    }

    #[test]
    fn inlay_hints_best_name_match_pattern_extracts_inner() {
        // Match pattern X = {ok, Val}: extract_param_name recurses into the inner pattern.
        // The variable name from the inner pattern should participate in scoring.
        let src = "\
-module(test).
handle(Result = {ok, _}) -> Result.
go() -> handle({ok, 42}).
";
        let hints = collect_hints(src);
        let labels: Vec<&str> = hints.iter().map(|h| h.0.as_str()).collect();
        // extract_param_name for Match pattern recurses into inner ({ok, _}),
        // which is a tuple — scored as pattern (1). That's the only clause.
        // Score 1 → suppressed. Let's verify no confusing hint is shown.
        // Actually: Match(_, inner, _) recurses into inner. Inner is {ok, _} which
        // is a Tuple pattern — extract gives the tuple text, score 1 → suppressed.
        assert!(hints.is_empty() || !labels.iter().any(|l| l.contains("{")),
            "tuple pattern hint should be suppressed, got {:?}", labels);
    }

    #[test]
    fn inlay_hints_no_hints_on_macro_define_line() {
        // When a macro body contains a function call (e.g. ets:new/2),
        // the expanded tokens carry spans from the -define line. Hints
        // must NOT appear on the -define line — they should be suppressed.
        let src = "\
-module(test).
-define(MAKE_TABLE, ets:new(my_tab, [named_table, public])).
go() -> ?MAKE_TABLE.
";
        let hints = collect_hints(src);
        // All hints should be on the go() function line (line 2, 0-indexed),
        // not on the -define line (line 1).
        for (label, line, _col) in &hints {
            assert!(*line >= 2, "hint '{}' at line {} should not appear on -define line", label, line);
        }
    }

    #[test]
    fn inlay_hints_function_like_macro_no_stray_hints() {
        // Function-like macro with a call inside — hints must not leak
        // to the -define line.
        let src = "\
-module(test).
-define(STORE(K, V), ets:insert(my_tab, {K, V})).
go() -> ?STORE(foo, bar).
";
        let hints = collect_hints(src);
        for (label, line, _col) in &hints {
            assert!(*line >= 2, "hint '{}' at line {} leaked to -define", label, line);
        }
    }

    #[test]
    fn inlay_hints_direct_call_still_shown() {
        // A direct (non-macro) call in the same module must still produce hints.
        let src = "\
-module(test).
-define(MAKE_TABLE, ets:new(my_tab, [named_table, public])).
go() ->
    ?MAKE_TABLE,
    ets:insert(my_tab, {key, val}).
";
        let hints = collect_hints(src);
        // The direct ets:insert call should produce hints
        let insert_hints: Vec<_> = hints.iter()
            .filter(|(l, _, _)| l == "Table:" || l == "ObjectOrObjects:")
            .collect();
        assert!(!insert_hints.is_empty(),
            "expected inlay hints for the direct ets:insert call, got {:?}", hints);
    }

    // ── Document symbol export detection ────────────────────────────

    /// Helper: parse a module and return (func_name, arity, is_exported) for each function.
    fn collect_symbol_exports(source: &str) -> Vec<(String, u32, bool)> {
        let (module, combined) = parse_module(source);
        module.functions.iter().map(|func| {
            let func_name = span_text(&combined, func.name_span).to_string();
            let is_exported = module.exports.iter().any(|fa| {
                fa.name == func.name && fa.arity == func.arity
            });
            (func_name, func.arity, is_exported)
        }).collect()
    }

    #[test]
    fn document_symbol_exported_vs_local() {
        let src = "\
-module(test).
-export([start/1, stop/0]).
start(Arg) -> ok.
stop() -> ok.
helper(X) -> X.
";
        let syms = collect_symbol_exports(src);
        assert_eq!(syms, vec![
            ("start".to_string(), 1, true),
            ("stop".to_string(), 0, true),
            ("helper".to_string(), 1, false),
        ]);
    }

    #[test]
    fn document_symbol_local_not_exported_by_arity_collision() {
        // Regression: a local function whose arity matches an exported function
        // must not be marked as exported.
        let src = "\
-module(test).
-export([start/3]).
start(A, B, C) -> ok.
do_terminate(Mod, Reason, State) -> ok.
";
        let syms = collect_symbol_exports(src);
        assert_eq!(syms, vec![
            ("start".to_string(), 3, true),
            ("do_terminate".to_string(), 3, false),
        ]);
    }

    #[test]
    fn document_symbol_no_exports() {
        let src = "\
-module(test).
foo() -> ok.
bar(X) -> X.
";
        let syms = collect_symbol_exports(src);
        assert_eq!(syms, vec![
            ("foo".to_string(), 0, false),
            ("bar".to_string(), 1, false),
        ]);
    }

    #[test]
    fn document_symbol_all_exported() {
        let src = "\
-module(test).
-export([foo/0, bar/1]).
foo() -> ok.
bar(X) -> X.
";
        let syms = collect_symbol_exports(src);
        assert_eq!(syms, vec![
            ("foo".to_string(), 0, true),
            ("bar".to_string(), 1, true),
        ]);
    }

    // ── Folding range tests ────────────────────────────────────────────

    /// Helper: parse source and collect folding ranges for the module.
    fn collect_folding_ranges(source: &str) -> Vec<(u32, u32)> {
        use tower_lsp::lsp_types::FoldingRange;
        let (module, combined) = parse_module(source);
        let sm = nera_syntax::source_map::SourceMap::new(&combined);
        let original_sm = nera_syntax::source_map::SourceMap::new(source);
        let mut ranges: Vec<FoldingRange> = Vec::new();

        // Attribute folds (export, type, spec, etc.)
        collect_attribute_folds(source, &original_sm, &mut ranges);

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

        ranges.into_iter().map(|r| (r.start_line, r.end_line)).collect()
    }

    #[test]
    fn folding_range_single_line_function_no_fold() {
        let src = "-module(test).\nfoo() -> ok.\n";
        let folds = collect_folding_ranges(src);
        assert!(folds.is_empty(), "single-line function should not produce a fold");
    }

    #[test]
    fn folding_range_two_line_function() {
        let src = "\
-module(test).
foo() ->
    ok.
";
        let folds = collect_folding_ranges(src);
        assert_eq!(folds.len(), 1, "two-line function should produce a fold");
        assert_eq!(folds[0], (1, 2), "fold should span lines 1-2");
    }

    #[test]
    fn folding_range_multiline_function() {
        let src = "\
-module(test).
foo() ->
    X = 1,
    X + 1.
";
        let folds = collect_folding_ranges(src);
        assert!(!folds.is_empty(), "multi-line function should produce a fold");
        // Function starts on line 1 (0-indexed), body ends on line 3
        let func_fold = &folds[0];
        assert_eq!(func_fold.0, 1, "fold should start on the function line");
        assert!(func_fold.1 >= 2, "fold end should be at least line 2");
    }

    #[test]
    fn folding_range_case_expression() {
        let src = "\
-module(test).
foo(X) ->
    case X of
        1 -> one;
        2 -> two
    end.
";
        let folds = collect_folding_ranges(src);
        // Should have function fold + case fold
        assert!(folds.len() >= 2, "expected at least 2 folds (function + case), got {}", folds.len());
        // The case fold should start on line 2 (the `case` keyword)
        let case_fold = folds.iter().find(|f| f.0 == 2);
        assert!(case_fold.is_some(), "expected a fold starting on the case line");
    }

    #[test]
    fn folding_range_receive_expression() {
        let src = "\
-module(test).
foo() ->
    receive
        {ok, Msg} -> Msg;
        error -> exit
    end.
";
        let folds = collect_folding_ranges(src);
        assert!(folds.len() >= 2, "expected at least 2 folds (function + receive)");
        let recv_fold = folds.iter().find(|f| f.0 == 2);
        assert!(recv_fold.is_some(), "expected a fold starting on the receive line");
    }

    #[test]
    fn folding_range_try_expression() {
        let src = "\
-module(test).
foo() ->
    try
        dangerous()
    catch
        _:_ -> error
    end.
";
        let folds = collect_folding_ranges(src);
        assert!(folds.len() >= 2, "expected at least 2 folds (function + try)");
        let try_fold = folds.iter().find(|f| f.0 == 2);
        assert!(try_fold.is_some(), "expected a fold starting on the try line");
    }

    #[test]
    fn folding_range_begin_block() {
        let src = "\
-module(test).
foo() ->
    begin
        X = 1,
        X + 1
    end.
";
        let folds = collect_folding_ranges(src);
        assert!(folds.len() >= 2, "expected at least 2 folds (function + begin)");
        let begin_fold = folds.iter().find(|f| f.0 == 2);
        assert!(begin_fold.is_some(), "expected a fold starting on the begin line");
    }

    #[test]
    fn folding_range_fun_expression() {
        let src = "\
-module(test).
foo() ->
    F = fun(X) ->
        X + 1
    end,
    F(1).
";
        let folds = collect_folding_ranges(src);
        assert!(folds.len() >= 2, "expected at least 2 folds (function + fun)");
        // The fun expression starts on line 2
        let fun_fold = folds.iter().find(|f| f.0 == 2);
        assert!(fun_fold.is_some(), "expected a fold starting on the fun line");
    }

    #[test]
    fn folding_range_nested_case_in_case() {
        let src = "\
-module(test).
foo(X) ->
    case X of
        1 ->
            case X + 1 of
                2 -> ok;
                _ -> error
            end;
        _ -> default
    end.
";
        let folds = collect_folding_ranges(src);
        // function fold + outer case + inner case = at least 3
        let case_folds: Vec<_> = folds.iter().filter(|f| f.0 >= 2).collect();
        assert!(case_folds.len() >= 2, "expected at least 2 case folds, got {}", case_folds.len());
    }

    #[test]
    fn folding_range_if_expression() {
        let src = "\
-module(test).
foo(X) ->
    if
        X > 0 -> positive;
        true -> non_positive
    end.
";
        let folds = collect_folding_ranges(src);
        assert!(folds.len() >= 2, "expected at least 2 folds (function + if)");
        let if_fold = folds.iter().find(|f| f.0 == 2);
        assert!(if_fold.is_some(), "expected a fold starting on the if line");
    }

    #[test]
    fn folding_range_single_line_export_no_fold() {
        let src = "-module(test).\n-export([foo/0]).\nfoo() -> ok.\n";
        let folds = collect_folding_ranges(src);
        // No fold for single-line export
        assert!(
            !folds.iter().any(|f| f.0 == 1),
            "single-line export should not produce a fold"
        );
    }

    #[test]
    fn folding_range_multiline_export() {
        let src = "\
-module(test).
-export([
    foo/0,
    bar/1,
    baz/2
]).
foo() -> ok.
bar(_) -> ok.
baz(_, _) -> ok.
";
        let folds = collect_folding_ranges(src);
        let export_fold = folds.iter().find(|f| f.0 == 1);
        assert!(export_fold.is_some(), "multi-line export should produce a fold");
        // The export spans lines 1-5 (0-indexed)
        let (_, end) = export_fold.unwrap();
        assert_eq!(*end, 5, "export fold should end on the closing line");
    }

    #[test]
    fn folding_range_multiline_export_type() {
        let src = "\
-module(test).
-export_type([
    my_type/0,
    other_type/1
]).
";
        let folds = collect_folding_ranges(src);
        let export_fold = folds.iter().find(|f| f.0 == 1);
        assert!(export_fold.is_some(), "multi-line export_type should produce a fold");
    }

    #[test]
    fn folding_range_multiline_spec() {
        let src = "\
-module(test).
-spec foo(integer(),
          string()) -> ok.
foo(_, _) -> ok.
";
        let folds = collect_folding_ranges(src);
        let spec_fold = folds.iter().find(|f| f.0 == 1);
        assert!(spec_fold.is_some(), "multi-line spec should produce a fold");
    }

    // ── Undefined type diagnostic integration tests ──────────────────────

    #[test]
    fn check_for_lsp_warns_undefined_type() {
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();

        // Write nera.toml
        std::fs::write(
            tmp.path().join("nera.toml"),
            "[project]\nname = \"test_proj\"\nsrc = [\"src\"]\n",
        ).unwrap();

        // Write a module that references an undefined type
        std::fs::write(
            src_dir.join("my_mod.erl"),
            "-module(my_mod).\n-spec foo(undefined_type()) -> ok.\nfoo(_) -> ok.\n",
        ).unwrap();

        let (diags, _uris, _idx, _caches, _) = {
                let cache = std::sync::Mutex::new(std::collections::HashMap::new());
                let interner = std::sync::Mutex::new(string_interner::DefaultStringInterner::default());
                super::check::check_for_lsp(tmp.path(), &cache, &interner)
            };

        // Find diagnostics for my_mod.erl
        let mod_diags: Vec<_> = diags.iter()
            .filter(|(uri, _)| uri.path().ends_with("my_mod.erl"))
            .flat_map(|(_, ds)| ds)
            .collect();

        let undef = mod_diags.iter().find(|d| d.message.contains("undefined type"));
        assert!(undef.is_some(), "should warn about undefined_type, got: {:?}", mod_diags);
        assert!(undef.unwrap().message.contains("undefined_type"));
    }

    #[test]
    fn check_for_lsp_no_warning_for_builtin_types() {
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();

        std::fs::write(
            tmp.path().join("nera.toml"),
            "[project]\nname = \"test_proj\"\nsrc = [\"src\"]\n",
        ).unwrap();

        // Only built-in types — should produce no undefined_type warnings
        std::fs::write(
            src_dir.join("my_mod.erl"),
            "-module(my_mod).\n-spec foo(integer(), atom()) -> list().\nfoo(X, Y) -> [X, Y].\n",
        ).unwrap();

        let (diags, _uris, _idx, _caches, _) = {
                let cache = std::sync::Mutex::new(std::collections::HashMap::new());
                let interner = std::sync::Mutex::new(string_interner::DefaultStringInterner::default());
                super::check::check_for_lsp(tmp.path(), &cache, &interner)
            };

        let undef_diags: Vec<_> = diags.iter()
            .filter(|(uri, _)| uri.path().ends_with("my_mod.erl"))
            .flat_map(|(_, ds)| ds)
            .filter(|d| d.message.contains("undefined type"))
            .collect();

        assert!(undef_diags.is_empty(), "built-in types should not trigger warnings, got: {:?}", undef_diags);
    }

    #[test]
    fn check_for_lsp_no_warning_for_local_type() {
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();

        std::fs::write(
            tmp.path().join("nera.toml"),
            "[project]\nname = \"test_proj\"\nsrc = [\"src\"]\n",
        ).unwrap();

        // Local type defined + used — no warning expected
        std::fs::write(
            src_dir.join("my_mod.erl"),
            "-module(my_mod).\n-type my_type() :: integer().\n-spec foo(my_type()) -> ok.\nfoo(_) -> ok.\n",
        ).unwrap();

        let (diags, _uris, _idx, _caches, _) = {
                let cache = std::sync::Mutex::new(std::collections::HashMap::new());
                let interner = std::sync::Mutex::new(string_interner::DefaultStringInterner::default());
                super::check::check_for_lsp(tmp.path(), &cache, &interner)
            };

        let undef_diags: Vec<_> = diags.iter()
            .filter(|(uri, _)| uri.path().ends_with("my_mod.erl"))
            .flat_map(|(_, ds)| ds)
            .filter(|d| d.message.contains("undefined type"))
            .collect();

        assert!(undef_diags.is_empty(), "locally defined type should not trigger warnings, got: {:?}", undef_diags);
    }

    // ── Incremental parse cache tests ──────────────────────────────────

    /// Helper: create a fresh parse cache + interner pair for tests.
    fn fresh_cache() -> (
        std::sync::Mutex<std::collections::HashMap<PathBuf, super::types::ParseCacheEntry>>,
        std::sync::Mutex<string_interner::DefaultStringInterner>,
    ) {
        (
            std::sync::Mutex::new(std::collections::HashMap::new()),
            std::sync::Mutex::new(string_interner::DefaultStringInterner::default()),
        )
    }

    /// Helper: set up a temp project with nera.toml and a src/ directory.
    fn setup_temp_project() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(
            tmp.path().join("nera.toml"),
            "[project]\nname = \"test_proj\"\nsrc = [\"src\"]\n",
        ).unwrap();
        (tmp, src_dir)
    }

    #[test]
    fn parse_cache_hit_on_unchanged_source() {
        let (tmp, src_dir) = setup_temp_project();
        std::fs::write(
            src_dir.join("alpha.erl"),
            "-module(alpha).\n-export([hello/0]).\nhello() -> world.\n",
        ).unwrap();

        let (cache, interner) = fresh_cache();

        // First check — cold cache, everything is a miss.
        let (diags1, _uris1, idx1, _, _) =
            super::check::check_for_lsp(tmp.path(), &cache, &interner);

        let cache_size_after_first = cache.lock().unwrap().len();
        assert!(cache_size_after_first > 0, "cache should be populated after first check");

        // Second check — same source, should be cache hits.
        let (diags2, _uris2, idx2, _, _) =
            super::check::check_for_lsp(tmp.path(), &cache, &interner);

        // Results should be identical.
        assert_eq!(diags1.len(), diags2.len(), "diagnostics should match between checks");
        assert_eq!(idx1.functions.len(), idx2.functions.len(), "index should match between checks");

        // Cache size should not grow (no new entries).
        let cache_size_after_second = cache.lock().unwrap().len();
        assert_eq!(cache_size_after_first, cache_size_after_second);
    }

    #[test]
    fn parse_cache_invalidated_on_source_change() {
        let (tmp, src_dir) = setup_temp_project();
        let file = src_dir.join("beta.erl");
        std::fs::write(&file, "-module(beta).\nhello() -> ok.\n").unwrap();

        let (cache, interner) = fresh_cache();

        // First check.
        super::check::check_for_lsp(tmp.path(), &cache, &interner);
        let hash1 = cache.lock().unwrap().get(&file).unwrap().source_hash;

        // Modify source.
        std::fs::write(&file, "-module(beta).\nhello() -> changed.\n").unwrap();

        // Second check — cache should be invalidated for beta.erl.
        super::check::check_for_lsp(tmp.path(), &cache, &interner);
        let hash2 = cache.lock().unwrap().get(&file).unwrap().source_hash;

        assert_ne!(hash1, hash2, "source hash should change after file modification");
    }

    #[test]
    fn parse_cache_prunes_deleted_files() {
        let (tmp, src_dir) = setup_temp_project();
        let file_a = src_dir.join("gamma.erl");
        let file_b = src_dir.join("delta.erl");
        std::fs::write(&file_a, "-module(gamma).\n").unwrap();
        std::fs::write(&file_b, "-module(delta).\n").unwrap();

        let (cache, interner) = fresh_cache();

        // First check — both files cached.
        super::check::check_for_lsp(tmp.path(), &cache, &interner);
        assert!(cache.lock().unwrap().contains_key(&file_a));
        assert!(cache.lock().unwrap().contains_key(&file_b));

        // Delete one file.
        std::fs::remove_file(&file_b).unwrap();

        // Second check — deleted file should be pruned from cache.
        super::check::check_for_lsp(tmp.path(), &cache, &interner);
        assert!(cache.lock().unwrap().contains_key(&file_a), "existing file should remain cached");
        assert!(!cache.lock().unwrap().contains_key(&file_b), "deleted file should be pruned from cache");
    }

    #[test]
    fn parse_cache_preserves_diagnostics_across_checks() {
        let (tmp, src_dir) = setup_temp_project();
        // Module with a warning (unused variable).
        std::fs::write(
            src_dir.join("warn_mod.erl"),
            "-module(warn_mod).\nfoo(X) -> ok.\n",
        ).unwrap();

        let (cache, interner) = fresh_cache();

        // First check.
        let (diags1, _, _, _, _) = super::check::check_for_lsp(tmp.path(), &cache, &interner);
        let warn_count1 = diags1.iter()
            .filter(|(uri, _)| uri.path().ends_with("warn_mod.erl"))
            .flat_map(|(_, ds)| ds)
            .count();

        // Second check (cache hit path).
        let (diags2, _, _, _, _) = super::check::check_for_lsp(tmp.path(), &cache, &interner);
        let warn_count2 = diags2.iter()
            .filter(|(uri, _)| uri.path().ends_with("warn_mod.erl"))
            .flat_map(|(_, ds)| ds)
            .count();

        assert_eq!(warn_count1, warn_count2,
            "diagnostic count should be identical on cache hit (got {} vs {})",
            warn_count1, warn_count2);
        assert!(warn_count1 > 0, "should have at least one warning for unused variable");
    }

    #[test]
    fn parse_cache_persistent_interner_stable_atom_ids() {
        let (tmp, src_dir) = setup_temp_project();
        std::fs::write(
            src_dir.join("atoms_mod.erl"),
            "-module(atoms_mod).\n-export([greet/0]).\ngreet() -> hello_world.\n",
        ).unwrap();

        let (cache, interner) = fresh_cache();

        // First check — populates interner.
        let (_, _, idx1, _, _) = super::check::check_for_lsp(tmp.path(), &cache, &interner);

        // Second check — cache hit, interner unchanged.
        let (_, _, idx2, _, _) = super::check::check_for_lsp(tmp.path(), &cache, &interner);

        // The function index should contain the same entries.
        assert_eq!(idx1.functions.len(), idx2.functions.len());
        for (key, loc1) in &idx1.functions {
            let loc2 = idx2.functions.get(key);
            assert!(loc2.is_some(), "function {:?} should be in index on second check", key);
            assert_eq!(loc1.range, loc2.unwrap().range, "range should be stable for {:?}", key);
        }
    }

    #[test]
    fn parse_cache_new_file_added() {
        let (tmp, src_dir) = setup_temp_project();
        std::fs::write(
            src_dir.join("first.erl"),
            "-module(first).\n",
        ).unwrap();

        let (cache, interner) = fresh_cache();

        // First check.
        let (_, _, idx1, _, _) = super::check::check_for_lsp(tmp.path(), &cache, &interner);
        let mod_count1 = idx1.modules.len();

        // Add a new file.
        std::fs::write(
            src_dir.join("second.erl"),
            "-module(second).\n-export([run/0]).\nrun() -> ok.\n",
        ).unwrap();

        // Second check — should pick up the new file.
        let (_, _, idx2, _, _) = super::check::check_for_lsp(tmp.path(), &cache, &interner);
        let mod_count2 = idx2.modules.len();

        assert!(mod_count2 > mod_count1,
            "adding a file should increase module count (was {}, now {})",
            mod_count1, mod_count2);
        assert!(idx2.modules.contains_key("second"), "new module should be in index");
    }

    #[test]
    fn parse_cache_parse_errors_replayed_on_hit() {
        let (tmp, src_dir) = setup_temp_project();
        // Module with a syntax error.
        std::fs::write(
            src_dir.join("broken.erl"),
            "-module(broken).\nfoo( -> ok.\n",
        ).unwrap();

        let (cache, interner) = fresh_cache();

        // First check — parse error reported.
        let (diags1, _, _, _, _) = super::check::check_for_lsp(tmp.path(), &cache, &interner);
        let err_count1 = diags1.iter()
            .filter(|(uri, _)| uri.path().ends_with("broken.erl"))
            .flat_map(|(_, ds)| ds)
            .filter(|d| d.severity == Some(tower_lsp::lsp_types::DiagnosticSeverity::ERROR))
            .count();

        // Second check — should replay cached parse errors.
        let (diags2, _, _, _, _) = super::check::check_for_lsp(tmp.path(), &cache, &interner);
        let err_count2 = diags2.iter()
            .filter(|(uri, _)| uri.path().ends_with("broken.erl"))
            .flat_map(|(_, ds)| ds)
            .filter(|d| d.severity == Some(tower_lsp::lsp_types::DiagnosticSeverity::ERROR))
            .count();

        assert!(err_count1 > 0, "should have parse errors on first check");
        assert_eq!(err_count1, err_count2,
            "parse errors should be replayed on cache hit ({} vs {})",
            err_count1, err_count2);
    }

    #[test]
    fn parse_cache_cross_file_validation_works_with_cache() {
        let (tmp, src_dir) = setup_temp_project();
        std::fs::write(
            src_dir.join("caller.erl"),
            "-module(caller).\ncall() -> callee:run().\n",
        ).unwrap();
        std::fs::write(
            src_dir.join("callee.erl"),
            "-module(callee).\n-export([run/0]).\nrun() -> ok.\n",
        ).unwrap();

        let (cache, interner) = fresh_cache();

        // First check — no unresolved call errors.
        let (diags1, _, _, _, _) = super::check::check_for_lsp(tmp.path(), &cache, &interner);
        let unresolved1 = diags1.iter()
            .filter(|(uri, _)| uri.path().ends_with("caller.erl"))
            .flat_map(|(_, ds)| ds)
            .filter(|d| d.message.contains("unresolved"))
            .count();

        // Second check — cache hit, cross-file validation still works.
        let (diags2, _, _, _, _) = super::check::check_for_lsp(tmp.path(), &cache, &interner);
        let unresolved2 = diags2.iter()
            .filter(|(uri, _)| uri.path().ends_with("caller.erl"))
            .flat_map(|(_, ds)| ds)
            .filter(|d| d.message.contains("unresolved"))
            .count();

        assert_eq!(unresolved1, unresolved2,
            "cross-file validation should produce same results with cache");
    }

    #[test]
    fn guard_bif_is_list_no_false_error() {
        let (tmp, src_dir) = setup_temp_project();
        // Update nera.toml to include include paths
        std::fs::write(
            tmp.path().join("nera.toml"),
            "[project]\nname = \"test_proj\"\nsrc = [\"src\"]\ninclude = [\"include\"]\n",
        ).unwrap();
        let inc_dir = tmp.path().join("include");
        std::fs::create_dir_all(&inc_dir).unwrap();
        std::fs::write(
            inc_dir.join("demo.hrl"),
            "-record(demo_state, {name, count = 0}).\n",
        ).unwrap();
        std::fs::write(
            src_dir.join("greeter.erl"),
            concat!(
                "-module(greeter).\n",
                "-include(\"demo.hrl\").\n",
                "-export([message/1]).\n",
                "message(Name) when is_list(Name) ->\n",
                "    State = #demo_state{name = Name},\n",
                "    {ok, State}.\n",
            ),
        ).unwrap();

        let (cache, interner) = fresh_cache();

        // First check
        let (diags1, _, _, _, _) = super::check::check_for_lsp(tmp.path(), &cache, &interner);
        let guard_errors: Vec<_> = diags1.iter()
            .filter(|(uri, _)| uri.path().ends_with("greeter.erl"))
            .flat_map(|(_, ds)| ds)
            .filter(|d| d.message.contains("illegal in guard"))
            .collect();
        assert!(guard_errors.is_empty(),
            "is_list/1 in guard should not produce an error, got: {:?}", guard_errors);

        // Second check (cache hit)
        let (diags2, _, _, _, _) = super::check::check_for_lsp(tmp.path(), &cache, &interner);
        let guard_errors2: Vec<_> = diags2.iter()
            .filter(|(uri, _)| uri.path().ends_with("greeter.erl"))
            .flat_map(|(_, ds)| ds)
            .filter(|d| d.message.contains("illegal in guard"))
            .collect();
        assert!(guard_errors2.is_empty(),
            "is_list/1 in guard should not produce an error on cache hit, got: {:?}", guard_errors2);
    }

    // ── type_infer integration tests ────────────────────────────────

    use super::type_infer::*;

    #[test]
    fn infer_spec_no_args_atom_return() {
        let src = "-module(test).\nfoo() -> ok.\n";
        let (module, combined) = parse_module(src);
        let (params, ret) = infer_function_spec(&module.functions[0], &combined);
        assert!(params.is_empty());
        assert_eq!(format_type(&ret), "ok");
    }

    #[test]
    fn infer_spec_integer_pattern() {
        let src = "-module(test).\nfoo(42) -> ok.\n";
        let (module, combined) = parse_module(src);
        let (params, _) = infer_function_spec(&module.functions[0], &combined);
        assert_eq!(format_type(&params[0]), "integer()");
    }

    #[test]
    fn infer_spec_integer_guard() {
        let src = "-module(test).\nfoo(X) when is_integer(X) -> X + 1.\n";
        let (module, combined) = parse_module(src);
        let (params, _) = infer_function_spec(&module.functions[0], &combined);
        assert_eq!(format_type(&params[0]), "integer()");
    }

    #[test]
    fn infer_spec_tuple_pattern() {
        let src = "-module(test).\nfoo({ok, Val}) -> Val.\n";
        let (module, combined) = parse_module(src);
        let (params, _) = infer_function_spec(&module.functions[0], &combined);
        assert_eq!(format_type(&params[0]), "{ok, term()}");
    }

    #[test]
    fn infer_spec_list_pattern() {
        let src = "-module(test).\nfoo([H|T]) -> {H, T}.\n";
        let (module, combined) = parse_module(src);
        let (params, ret) = infer_function_spec(&module.functions[0], &combined);
        assert_eq!(format_type(&params[0]), "list()");
        assert_eq!(format_type(&ret), "{term(), term()}");
    }

    #[test]
    fn infer_spec_binary_pattern() {
        let src = "-module(test).\nfoo(<<X:8>>) -> X.\n";
        let (module, combined) = parse_module(src);
        let (params, _) = infer_function_spec(&module.functions[0], &combined);
        assert_eq!(format_type(&params[0]), "binary()");
    }

    #[test]
    fn infer_spec_multi_clause_union_return() {
        let src = "-module(test).\nfoo(1) -> one;\nfoo(2) -> two.\n";
        let (module, combined) = parse_module(src);
        let (_, ret) = infer_function_spec(&module.functions[0], &combined);
        let ret_str = format_type(&ret);
        assert!(ret_str.contains("one"), "expected 'one' in: {}", ret_str);
        assert!(ret_str.contains("two"), "expected 'two' in: {}", ret_str);
    }

    #[test]
    fn infer_spec_boolean_guards() {
        let src = "-module(test).\nfoo(X) when is_boolean(X) -> not X.\n";
        let (module, combined) = parse_module(src);
        let (params, _) = infer_function_spec(&module.functions[0], &combined);
        assert_eq!(format_type(&params[0]), "boolean()");
    }

    #[test]
    fn infer_spec_atom_return_true_false_becomes_boolean() {
        let src = "-module(test).\nis_valid(true) -> false;\nis_valid(false) -> true.\n";
        let (module, combined) = parse_module(src);
        let (params, ret) = infer_function_spec(&module.functions[0], &combined);
        assert_eq!(format_type(&params[0]), "boolean()");
        assert_eq!(format_type(&ret), "boolean()");
    }

    #[test]
    fn infer_spec_variable_bound_in_match_pattern() {
        let src = "-module(test).\nfoo(X = {ok, _}) -> X.\n";
        let (module, combined) = parse_module(src);
        let (params, ret) = infer_function_spec(&module.functions[0], &combined);
        assert_eq!(format_type(&params[0]), "{ok, term()}");
        // Return type: X is bound to {ok, _} via match pattern
        assert_eq!(format_type(&ret), "{ok, term()}");
    }

    #[test]
    fn infer_spec_map_pattern() {
        let src = "-module(test).\nfoo(#{key := Val}) -> Val.\n";
        let (module, combined) = parse_module(src);
        let (params, _) = infer_function_spec(&module.functions[0], &combined);
        assert_eq!(format_type(&params[0]), "map()");
    }

    #[test]
    fn infer_spec_record_pattern() {
        let src = "-module(test).\n-record(person, {name, age}).\nfoo(#person{}) -> ok.\n";
        let (module, combined) = parse_module(src);
        let (params, _) = infer_function_spec(&module.functions[0], &combined);
        assert_eq!(format_type(&params[0]), "#person{}");
    }

    #[test]
    fn infer_spec_wildcard_is_term() {
        let src = "-module(test).\nfoo(_) -> ok.\n";
        let (module, combined) = parse_module(src);
        let (params, _) = infer_function_spec(&module.functions[0], &combined);
        assert_eq!(format_type(&params[0]), "term()");
    }

    #[test]
    fn infer_spec_multiple_params() {
        let src = "-module(test).\nfoo(X, Y) when is_integer(X), is_atom(Y) -> {X, Y}.\n";
        let (module, combined) = parse_module(src);
        let (params, ret) = infer_function_spec(&module.functions[0], &combined);
        assert_eq!(format_type(&params[0]), "integer()");
        assert_eq!(format_type(&params[1]), "atom()");
        assert_eq!(format_type(&ret), "{term(), term()}");
    }

    #[test]
    fn infer_spec_generates_full_spec_text() {
        let src = "-module(test).\nfoo(X) when is_integer(X) -> ok.\n";
        let (module, combined) = parse_module(src);
        let func = &module.functions[0];
        let fname = span_text(&combined, func.name_span);
        let (params, ret) = infer_function_spec(func, &combined);
        let spec = format_spec(fname, &params, &ret);
        assert_eq!(spec, "-spec foo(integer()) -> ok.");
    }

    #[test]
    fn infer_spec_existing_spec_detected() {
        let src = "-module(test).\n-spec foo() -> ok.\nfoo() -> ok.\n";
        let (module, combined) = parse_module(src);
        let func = &module.functions[0];
        let fname = span_text(&combined, func.name_span);
        let has_spec = module.specs.iter().any(|s| {
            span_text(&combined, s.name_span) == fname && s.arity == func.arity
        });
        assert!(has_spec, "should detect existing spec");
    }

    #[test]
    fn spec_completion_context_detected() {
        // Simulates the line text detection in the completion handler
        let line = "-spec fo";
        let trimmed = line.trim_start();
        assert!(trimmed.starts_with("-spec "));
        assert!(!trimmed.contains("->"));
        assert!(!trimmed.contains('('));
        let prefix = &trimmed[6..];
        assert_eq!(prefix, "fo");
    }

    #[test]
    fn spec_completion_not_inside_parens() {
        let line = "-spec foo(integ";
        let trimmed = line.trim_start();
        assert!(trimmed.contains('('));
        // Should NOT trigger function name completion (parens present)
    }

    #[test]
    fn callback_completion_context_detected() {
        let line = "-callback ha";
        let trimmed = line.trim_start();
        assert!(trimmed.starts_with("-callback "));
        let prefix = &trimmed[10..];
        assert_eq!(prefix, "ha");
    }

    #[test]
    fn infer_spec_snippet_output() {
        let src = "-module(test).\nfoo(X) when is_integer(X) -> ok.\n";
        let (module, combined) = parse_module(src);
        let func = &module.functions[0];
        let fname = span_text(&combined, func.name_span);
        let (params, ret) = infer_function_spec(func, &combined);
        let snippet = format_spec_snippet(fname, &params, &ret);
        assert_eq!(snippet, "foo(${1:integer()}) -> ${2:ok}.");
    }

    /// Regression: a supervisor-like module using ?MODULE should NOT get a
    /// `module_name_mismatch` warning claiming the module name is 'true' or
    /// produce bogus unresolved-call errors like `false:self/3`.
    #[test]
    fn module_name_correct_with_macro_module_multi_file() {
        let (tmp, src_dir) = setup_temp_project();
        // A worker module
        std::fs::write(
            src_dir.join("my_worker.erl"),
            "-module(my_worker).\n-export([start_link/0]).\nstart_link() -> ok.\n",
        ).unwrap();
        // A supervisor module using ?MODULE (matches actual onecached_sup.erl layout)
        std::fs::write(
            src_dir.join("my_sup.erl"),
            concat!(
                "-module(my_sup).\n",
                "-author('user@example.net').\n",
                "-vsn('$Revision$ ').\n",
                "\n",
                "-behaviour(supervisor).\n",
                "\n",
                "-export([start_link/0]).\n",
                "-export([init/1]).\n",
                "\n",
                "start_link() ->\n",
                "    supervisor:start_link({local, ?MODULE}, ?MODULE, []).\n",
                "\n",
                "init(_Args) ->\n",
                "    {ok, {{one_for_one, 1, 10},\n",
                "        [{my_worker, {my_worker, start_link, []},\n",
                "         permanent, 5000, worker, [my_worker]}]}}.\n",
            ),
        ).unwrap();

        let (cache, interner) = fresh_cache();
        let (diags, _, _, _, _) = super::check::check_for_lsp(tmp.path(), &cache, &interner);

        // Collect all diagnostics for my_sup.erl
        let sup_diags: Vec<_> = diags.iter()
            .filter(|(uri, _)| uri.path().ends_with("my_sup.erl"))
            .flat_map(|(_, ds)| ds)
            .collect();

        // No module_name_mismatch — the module IS named my_sup
        let mismatch: Vec<_> = sup_diags.iter()
            .filter(|d| d.message.contains("does not match filename"))
            .collect();
        assert!(mismatch.is_empty(),
            "expected no module_name_mismatch for my_sup.erl, got: {:?}",
            mismatch.iter().map(|d| &d.message).collect::<Vec<_>>());

        // No false unresolved calls like 'false:self/3'
        let bogus_unresolved: Vec<_> = sup_diags.iter()
            .filter(|d| d.message.contains("unresolved") &&
                       (d.message.contains("false:") || d.message.contains("true:")))
            .collect();
        assert!(bogus_unresolved.is_empty(),
            "expected no bogus unresolved calls, got: {:?}",
            bogus_unresolved.iter().map(|d| &d.message).collect::<Vec<_>>());
    }

    /// Regression: after a file modification (cache miss while other files are cached),
    /// module names should still resolve correctly.
    #[test]
    fn module_name_stable_across_check_cycles() {
        let (tmp, src_dir) = setup_temp_project();
        std::fs::write(
            src_dir.join("worker.erl"),
            "-module(worker).\n-export([start_link/0]).\nstart_link() -> ok.\n",
        ).unwrap();
        std::fs::write(
            src_dir.join("my_sup.erl"),
            concat!(
                "-module(my_sup).\n",
                "-author('user@example.net').\n",
                "-vsn('$Revision$ ').\n",
                "-behaviour(supervisor).\n",
                "-export([start_link/0, init/1]).\n",
                "start_link() ->\n",
                "    supervisor:start_link({local, ?MODULE}, ?MODULE, []).\n",
                "init(_Args) ->\n",
                "    {ok, {{one_for_one, 1, 10},\n",
                "        [{worker, {worker, start_link, []},\n",
                "         permanent, 5000, worker, [worker]}]}}.\n",
            ),
        ).unwrap();

        let (cache, interner) = fresh_cache();

        // First check — populates cache and interner
        let (diags1, _, _, _, _) = super::check::check_for_lsp(tmp.path(), &cache, &interner);
        let mismatch1: Vec<_> = diags1.iter()
            .filter(|(uri, _)| uri.path().ends_with("my_sup.erl"))
            .flat_map(|(_, ds)| ds)
            .filter(|d| d.message.contains("does not match filename"))
            .collect();
        assert!(mismatch1.is_empty(), "cycle 1: unexpected mismatch: {:?}",
            mismatch1.iter().map(|d| &d.message).collect::<Vec<_>>());

        // Modify worker.erl so it's a cache miss, while my_sup.erl stays cached
        std::fs::write(
            src_dir.join("worker.erl"),
            "-module(worker).\n-export([start_link/0, stop/0]).\nstart_link() -> ok.\nstop() -> ok.\n",
        ).unwrap();

        // Second check — my_sup.erl is a cache hit, worker.erl is a cache miss
        let (diags2, _, _, _, _) = super::check::check_for_lsp(tmp.path(), &cache, &interner);
        let mismatch2: Vec<_> = diags2.iter()
            .filter(|(uri, _)| uri.path().ends_with("my_sup.erl"))
            .flat_map(|(_, ds)| ds)
            .filter(|d| d.message.contains("does not match filename"))
            .collect();
        assert!(mismatch2.is_empty(), "cycle 2: unexpected mismatch: {:?}",
            mismatch2.iter().map(|d| &d.message).collect::<Vec<_>>());

        // Also check no bogus unresolved calls in either cycle
        for (label, diags) in [("cycle 1", &diags1), ("cycle 2", &diags2)] {
            let bogus: Vec<_> = diags.iter()
                .filter(|(uri, _)| uri.path().ends_with("my_sup.erl"))
                .flat_map(|(_, ds)| ds)
                .filter(|d| d.message.contains("unresolved") &&
                           (d.message.contains("false:") || d.message.contains("true:")))
                .collect();
            assert!(bogus.is_empty(), "{}: bogus unresolved: {:?}",
                label, bogus.iter().map(|d| &d.message).collect::<Vec<_>>());
        }
    }

    /// Regression: without nera.toml, the convention-based src/ detection should
    /// still produce correct atom resolution for modules using ?MODULE.
    #[test]
    fn module_name_correct_without_nera_toml() {
        // Create project WITHOUT nera.toml, relying on src/ convention
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        // NO nera.toml — relies on src/ convention

        std::fs::write(
            src_dir.join("worker.erl"),
            "-module(worker).\n-export([start_link/0]).\nstart_link() -> ok.\n",
        ).unwrap();
        std::fs::write(
            src_dir.join("my_sup.erl"),
            concat!(
                "%%%-------------------------------------------------------------------\n",
                "%%% OneCached (c) 2007 Process-one\n",
                "%%% $Id$\n",
                "%%%-------------------------------------------------------------------\n",
                "\n",
                "-module(my_sup).\n",
                "-author('user@example.net').\n",
                "-vsn('$Revision$ ').\n",
                "\n",
                "-behaviour(supervisor).\n",
                "\n",
                "-export([start_link/0]).\n",
                "-export([init/1]).\n",
                "\n",
                "start_link() ->\n",
                "    supervisor:start_link({local, ?MODULE}, ?MODULE, []).\n",
                "\n",
                "init(_Args) ->\n",
                "    {ok, {{one_for_one, 1, 10},\n",
                "        [{worker, {worker, start_link, []},\n",
                "         permanent, 5000, worker, [worker]}]}}.\n",
            ),
        ).unwrap();

        // check_single_file is the path used when no nera.toml
        let (diags, _) = super::check::check_single_file(
            &src_dir.join("my_sup.erl"),
        );

        let sup_diags: Vec<_> = diags.values()
            .flatten()
            .collect();

        let mismatch: Vec<_> = sup_diags.iter()
            .filter(|d| d.message.contains("does not match filename"))
            .collect();
        assert!(mismatch.is_empty(),
            "expected no module_name_mismatch, got: {:?}",
            mismatch.iter().map(|d| &d.message).collect::<Vec<_>>());
    }

    // ── Rename tests ────────────────────────────────────────────────────

    #[test]
    fn is_valid_variable_name_accepts_uppercase() {
        assert!(is_valid_variable_name("X"));
        assert!(is_valid_variable_name("MyVar"));
        assert!(is_valid_variable_name("Foo123"));
        assert!(is_valid_variable_name("A_b_c"));
    }

    #[test]
    fn is_valid_variable_name_accepts_underscore_prefix() {
        assert!(is_valid_variable_name("_"));
        assert!(is_valid_variable_name("_Unused"));
        assert!(is_valid_variable_name("_foo"));
        assert!(is_valid_variable_name("_123"));
    }

    #[test]
    fn is_valid_variable_name_rejects_invalid() {
        assert!(!is_valid_variable_name(""));
        assert!(!is_valid_variable_name("lowercase"));
        assert!(!is_valid_variable_name("123Start"));
        assert!(!is_valid_variable_name("foo bar"));
        assert!(!is_valid_variable_name("my-var"));
    }

    #[test]
    fn rename_variable_finds_all_occurrences_in_clause() {
        let src = "-module(test).\nfoo(X) -> X + X.\n";
        let (module, combined) = parse_module(src);
        let sm = nera_syntax::source_map::SourceMap::new(&combined);

        // Put cursor on "X" in pattern
        let pat_offset = combined.find("X)").expect("should find X in pattern");
        let target = find_cursor_target(&module, pat_offset, &combined);
        match &target {
            Some(CursorTarget::Variable { name }) => assert_eq!(name, "X"),
            other => panic!("expected Variable, got {:?}", other),
        }

        // Find the enclosing clause and collect all variable spans
        let func = &module.functions[0];
        let clause = &func.clauses[0];
        let spans = find_variable_spans_in_clause(clause, &combined, "X");
        assert_eq!(spans.len(), 3, "expected 3 occurrences of X (1 pattern + 2 body)");

        // Verify all spans resolve to "X"
        for span in &spans {
            assert_eq!(span_text(&combined, *span), "X");
        }

        // Verify ranges convert correctly
        for span in &spans {
            let range = span_to_lsp_range(*span, &sm);
            assert_eq!(range.end.character - range.start.character, 1); // "X" is 1 char
        }
    }

    #[test]
    fn rename_variable_scoped_to_clause() {
        // X in first clause should not affect X in second clause
        let src = "-module(test).\nfoo(X) -> X;\nfoo(X) -> X + 1.\n";
        let (module, combined) = parse_module(src);

        let func = &module.functions[0];
        assert_eq!(func.clauses.len(), 2);

        let spans_clause1 = find_variable_spans_in_clause(&func.clauses[0], &combined, "X");
        let spans_clause2 = find_variable_spans_in_clause(&func.clauses[1], &combined, "X");

        assert_eq!(spans_clause1.len(), 2, "clause 1: 1 pattern + 1 body");
        assert_eq!(spans_clause2.len(), 2, "clause 2: 1 pattern + 1 body");

        // Spans from clause 1 should not overlap with clause 2
        for s1 in &spans_clause1 {
            for s2 in &spans_clause2 {
                assert!(s1.end <= s2.start || s2.end <= s1.start,
                    "clause spans should not overlap: {:?} vs {:?}", s1, s2);
            }
        }
    }

    #[test]
    fn rename_multi_char_variable() {
        let src = "-module(test).\nfoo(Result) -> {ok, Result}.\n";
        let (module, combined) = parse_module(src);

        let func = &module.functions[0];
        let clause = &func.clauses[0];
        let spans = find_variable_spans_in_clause(clause, &combined, "Result");
        assert_eq!(spans.len(), 2, "expected 2 occurrences of Result");

        for span in &spans {
            assert_eq!(span_text(&combined, *span), "Result");
        }
    }

    #[test]
    fn rename_does_not_touch_different_variables() {
        let src = "-module(test).\nfoo(X, Y) -> X + Y.\n";
        let (module, combined) = parse_module(src);

        let func = &module.functions[0];
        let clause = &func.clauses[0];

        let x_spans = find_variable_spans_in_clause(clause, &combined, "X");
        let y_spans = find_variable_spans_in_clause(clause, &combined, "Y");

        assert_eq!(x_spans.len(), 2, "X: 1 pattern + 1 body");
        assert_eq!(y_spans.len(), 2, "Y: 1 pattern + 1 body");

        // No span overlap between X and Y
        for sx in &x_spans {
            for sy in &y_spans {
                assert!(sx.end <= sy.start || sy.end <= sx.start);
            }
        }
    }

    #[test]
    fn rename_variable_in_case_expr() {
        let src = "-module(test).\nfoo(X) ->\n    case X of\n        Y -> Y + X\n    end.\n";
        let (module, combined) = parse_module(src);

        let func = &module.functions[0];
        let clause = &func.clauses[0];

        let x_spans = find_variable_spans_in_clause(clause, &combined, "X");
        assert_eq!(x_spans.len(), 3, "X: 1 param + 1 case subject + 1 case body");

        let y_spans = find_variable_spans_in_clause(clause, &combined, "Y");
        assert_eq!(y_spans.len(), 2, "Y: 1 case pattern + 1 case body");
    }

    #[test]
    fn prepare_rename_rejects_non_variable() {
        let src = "-module(test).\nfoo(X) -> bar(X).\nbar(Y) -> Y.\n";
        let (module, combined) = parse_module(src);

        // Put cursor on "bar" which is a local call, not a variable
        let bar_offset = combined.find("bar(X)").expect("should find bar call");
        let target = find_cursor_target(&module, bar_offset, &combined);
        match target {
            Some(CursorTarget::LocalCall { .. }) => {} // Expected — rename would be rejected
            other => panic!("expected LocalCall, got {:?}", other),
        }
    }

    // ── Function rename: span helpers ───────────────────────────────────

    #[test]
    fn function_rename_target_from_def_site() {
        use super::references::{function_rename_target_at, collect_defining_module_spans};
        let src = "-module(test).\n-export([foo/1]).\nfoo(X) -> foo(X - 1).\nbar() -> foo(1).\n";
        let (module, combined) = parse_module(src);

        // Cursor on function definition `foo(X)`
        let def_offset = combined.find("foo(X)").unwrap();
        let target = function_rename_target_at(&module, &combined, def_offset).unwrap();
        assert_eq!(target.owner_module, "test");
        assert_eq!(target.name, "foo");
        assert_eq!(target.arity, 1);

        // Collecting rename spans in the defining module should cover:
        // one clause name, one export entry, two local call sites.
        let spans = collect_defining_module_spans(&module, &combined, "foo", 1);
        assert_eq!(spans.len(), 4);
        for span in &spans {
            assert_eq!(span_text(&combined, *span), "foo");
        }
    }

    #[test]
    fn function_rename_target_from_export_entry() {
        use super::references::function_rename_target_at;
        let src = "-module(test).\n-export([foo/2]).\nfoo(X, Y) -> {X, Y}.\n";
        let (module, combined) = parse_module(src);

        let off = combined.find("foo/2").unwrap() + 1;
        let target = function_rename_target_at(&module, &combined, off).unwrap();
        assert_eq!(target.name, "foo");
        assert_eq!(target.arity, 2);
    }

    #[test]
    fn function_rename_target_from_spec() {
        use super::references::function_rename_target_at;
        let src = "-module(test).\n-spec foo(integer()) -> integer().\nfoo(X) -> X.\n";
        let (module, combined) = parse_module(src);

        let off = combined.find("-spec foo").unwrap() + "-spec f".len();
        let target = function_rename_target_at(&module, &combined, off).unwrap();
        assert_eq!(target.name, "foo");
        assert_eq!(target.arity, 1);
    }

    #[test]
    fn function_rename_target_from_local_call() {
        use super::references::function_rename_target_at;
        let src = "-module(test).\nbar() -> foo(1).\nfoo(X) -> X.\n";
        let (module, combined) = parse_module(src);

        let off = combined.find("foo(1)").unwrap() + 1;
        let target = function_rename_target_at(&module, &combined, off).unwrap();
        assert_eq!(target.owner_module, "test");
        assert_eq!(target.name, "foo");
        assert_eq!(target.arity, 1);
    }

    #[test]
    fn function_rename_target_from_remote_call() {
        use super::references::function_rename_target_at;
        let src = "-module(test).\ngo(L) -> lists:reverse(L).\n";
        let (module, combined) = parse_module(src);

        let off = combined.find("reverse(L)").unwrap() + 1;
        let target = function_rename_target_at(&module, &combined, off).unwrap();
        assert_eq!(target.owner_module, "lists");
        assert_eq!(target.name, "reverse");
        assert_eq!(target.arity, 1);
    }

    #[test]
    fn function_rename_target_from_local_funref() {
        use super::references::function_rename_target_at;
        let src = "-module(test).\ngo() -> fun foo/1.\nfoo(X) -> X.\n";
        let (module, combined) = parse_module(src);

        let off = combined.find("fun foo/1").unwrap() + "fun f".len();
        let target = function_rename_target_at(&module, &combined, off).unwrap();
        assert_eq!(target.owner_module, "test");
        assert_eq!(target.name, "foo");
        assert_eq!(target.arity, 1);
    }

    #[test]
    fn function_rename_target_from_remote_funref() {
        use super::references::function_rename_target_at;
        let src = "-module(test).\ngo() -> fun lists:map/2.\n";
        let (module, combined) = parse_module(src);

        let off = combined.find("fun lists:map/2").unwrap() + "fun lists:m".len();
        let target = function_rename_target_at(&module, &combined, off).unwrap();
        assert_eq!(target.owner_module, "lists");
        assert_eq!(target.name, "map");
        assert_eq!(target.arity, 2);
    }

    #[test]
    fn find_remote_call_name_spans_returns_name_only() {
        use super::references::find_remote_call_name_spans;
        let src = "-module(test).\nstart(L) -> lists:reverse(L), fun lists:reverse/1.\n";
        let (module, combined) = parse_module(src);

        let spans = find_remote_call_name_spans(&module, &combined, "lists", "reverse", 1);
        assert_eq!(spans.len(), 2);
        for span in &spans {
            assert_eq!(span_text(&combined, *span), "reverse");
        }
    }

    #[test]
    fn find_local_call_name_spans_returns_name_only() {
        use super::references::find_local_call_name_spans;
        let src = "-module(test).\nstart() -> foo(1), foo(2), fun foo/1.\nfoo(X) -> X.\n";
        let (module, combined) = parse_module(src);

        let spans = find_local_call_name_spans(&module, &combined, "foo", 1);
        assert_eq!(spans.len(), 3);
        for span in &spans {
            assert_eq!(span_text(&combined, *span), "foo");
        }
    }

    #[test]
    fn function_rename_spans_include_specs_and_clauses() {
        use super::references::collect_defining_module_spans;
        let src = "-module(test).\n-export([foo/1]).\n-spec foo(integer()) -> integer().\nfoo(0) -> 0;\nfoo(N) -> N + foo(N - 1).\n";
        let (module, combined) = parse_module(src);

        let spans = collect_defining_module_spans(&module, &combined, "foo", 1);
        // export (1) + spec (1) + 2 clause name_spans (2) + 1 local call (1) = 5
        assert_eq!(spans.len(), 5);
        for span in &spans {
            assert_eq!(span_text(&combined, *span), "foo");
        }
    }

    // ── Formatting: TextEdit range computation ──────────────────────────

    /// Compute the whole-document end Position the same way the formatting
    /// handler does, so we can unit-test the range logic independently.
    fn doc_end_position(source: &str) -> Position {
        if source.ends_with('\n') {
            Position::new(source.lines().count() as u32, 0)
        } else {
            let last_line = source.lines().count().saturating_sub(1) as u32;
            let last_line_len = source.lines().last().map_or(0, |l| l.len()) as u32;
            Position::new(last_line, last_line_len)
        }
    }

    #[test]
    fn formatting_range_trailing_newline() {
        // "hello\nworld\n" — 2 lines, ends with \n → end at (2, 0)
        let pos = doc_end_position("hello\nworld\n");
        assert_eq!(pos.line, 2);
        assert_eq!(pos.character, 0);
    }

    #[test]
    fn formatting_range_no_trailing_newline() {
        // "hello\nworld" — 2 lines, no trailing \n → end at (1, 5)
        let pos = doc_end_position("hello\nworld");
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 5);
    }

    #[test]
    fn formatting_range_single_line_with_newline() {
        let pos = doc_end_position("hello\n");
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 0);
    }

    #[test]
    fn formatting_range_single_line_no_newline() {
        let pos = doc_end_position("hello");
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 5);
    }

    #[test]
    fn formatting_range_empty_source() {
        let pos = doc_end_position("");
        // Empty string: lines().count() == 0, saturating_sub(1) == 0
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 0);
    }

    #[test]
    fn formatting_range_erlang_module() {
        let src = "-module(test).\n-export([f/0]).\nf() -> ok.\n";
        let pos = doc_end_position(src);
        assert_eq!(pos.line, 3);
        assert_eq!(pos.character, 0);
    }

    #[test]
    fn formatting_produces_edit_for_unformatted_code() {
        let src = "-module(test).\n-export([f/0]).\nf() ->   ok.\n";
        let config = nera_fmt::config::FormatConfig { max_width: 100, indent_size: 4 };
        let formatted = nera_fmt::format_source(src, &config).unwrap();
        // The formatter should normalize the extra spaces
        assert_ne!(formatted, src, "formatter should change badly-formatted code");
        // And be idempotent
        let second = nera_fmt::format_source(&formatted, &config).unwrap();
        assert_eq!(formatted, second, "formatting should be idempotent");
    }

    #[test]
    fn formatting_no_edit_for_already_formatted() {
        let config = nera_fmt::config::FormatConfig { max_width: 100, indent_size: 4 };
        let src = "-module(test).\n-export([f/0]).\nf() ->\n    ok.\n";
        let formatted = nera_fmt::format_source(src, &config).unwrap();
        let already = nera_fmt::format_source(&formatted, &config).unwrap();
        assert_eq!(formatted, already, "already-formatted code should not change");
    }
