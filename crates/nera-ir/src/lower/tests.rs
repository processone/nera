use super::*;
use nera_syntax::ast::BinOp;
use nera_syntax::parser::Parser;

    fn lower_source(source: &str) -> Result<IrModule, LowerError> {
        let mut parser = Parser::new(source);
        let result = parser.parse_module();
        if result.has_errors() {
            return Err(LowerError::Unsupported(format!("{:?}", result.errors), None));
        }
        let module = result.module;

        // Build atom name map from the parser's interner
        let mut atom_names = HashMap::new();
        let mut true_id = None;
        let mut false_id = None;
        let mut self_id = None;
        for id in 0..1000u32 {
            if let Some(name) = parser.resolve(AtomId(id)) {
                atom_names.insert(AtomId(id), name.to_string());
                if name == "true" { true_id = Some(id); }
                else if name == "false" { false_id = Some(id); }
                else if name == "self" { self_id = Some(id); }
            }
        }
        // Fallback: use high IDs that won't conflict
        let true_atom_id = AtomId(true_id.unwrap_or(9998));
        let false_atom_id = AtomId(false_id.unwrap_or(9999));
        let self_atom_id = AtomId(self_id.unwrap_or(9997));

        let (ir_module, _atoms) = IrBuilder::lower_module(&module, true_atom_id, false_atom_id, self_atom_id, atom_names, None)?;
        let errors = crate::verify::verify_module(&ir_module);
        assert!(errors.is_empty(), "lowered IR failed structural verification: {errors:#?}\n{source}");
        Ok(ir_module)
    }

    #[test]
    fn test_lower_simple_function() {
        let source = r#"
-module(test).
-export([id/1]).

id(X) -> X.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].arity, 1);
    }

    #[test]
    fn test_lower_string_pattern() {
        let source = r#"
-module(test).
-export([greet/1]).

greet("hi") -> matched;
greet(_) -> other.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        // The string pattern "hi" should be desugared into cons-cell matching.
        // Verify the IR contains ListHead and ListTail instructions (from cons destructuring)
        // and Integer constants for 'h' (104) and 'i' (105).
        let blocks = &ir.functions[0].blocks;
        let has_list_head = blocks.iter().any(|block| {
            block.instructions.iter().any(|inst| {
                matches!(inst, Instruction::ListHead(_, _))
            })
        });
        let has_integer_104 = blocks.iter().any(|block| {
            block.instructions.iter().any(|inst| {
                matches!(inst, Instruction::Const(_, Literal::Integer(104)))
            })
        });
        let has_integer_105 = blocks.iter().any(|block| {
            block.instructions.iter().any(|inst| {
                matches!(inst, Instruction::Const(_, Literal::Integer(105)))
            })
        });
        assert!(has_list_head, "Expected ListHead from string pattern desugaring");
        assert!(has_integer_104, "Expected integer 104 ('h') from string pattern");
        assert!(has_integer_105, "Expected integer 105 ('i') from string pattern");
    }

    #[test]
    fn test_lower_empty_string_pattern() {
        let source = r#"
-module(test).
-export([check/1]).

check("") -> empty;
check(_) -> nonempty.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        // Empty string "" should desugar to a Nil pattern (empty list []).
        // Verify the IR contains a Nil constant and ExactEq comparison.
        let blocks = &ir.functions[0].blocks;
        let has_nil = blocks.iter().any(|block| {
            block.instructions.iter().any(|inst| {
                matches!(inst, Instruction::Const(_, Literal::Nil))
            })
        });
        assert!(has_nil, "Expected Nil constant for empty string pattern");
    }

    #[test]
    fn test_lower_arithmetic() {
        let source = r#"
-module(test).
-export([add/2]).

add(A, B) -> A + B.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        // Check that the IR contains a BinOp instruction
        let has_binop = ir.functions[0].blocks.iter().any(|block| {
            block.instructions.iter().any(|inst| {
                matches!(inst, Instruction::BinOp(_, BinOp::Add, _, _))
            })
        });
        assert!(has_binop, "Expected Add BinOp in IR");
    }

    // ── Adversarial pattern matching tests ──────────────────────

    /// Multiple clauses with overlapping tuple patterns (Maranget column selection).
    #[test]
    fn test_pattern_overlapping_tuples() {
        let source = r#"
-module(test).
-export([f/1]).

f({a, 1}) -> one;
f({a, 2}) -> two;
f({b, _}) -> b_any;
f(_) -> other.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        // Should produce a decision tree that first tests the tuple tag,
        // then tests element 0, then element 1 in some branches.
        // Verify it compiles without panicking and has multiple blocks.
        assert!(ir.functions[0].blocks.len() >= 4, "expected multiple blocks for overlapping tuple patterns");
    }

    /// Nested tuple patterns — ensures recursive pattern compilation works.
    #[test]
    fn test_pattern_nested_tuples() {
        let source = r#"
-module(test).
-export([f/1]).

f({ok, {value, X}}) -> X;
f({error, Reason}) -> Reason;
f(_) -> unknown.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert!(ir.functions[0].blocks.len() >= 3);
    }

    /// Variable binding across multiple columns.
    #[test]
    fn test_pattern_multi_arg_variables() {
        let source = r#"
-module(test).
-export([f/3]).

f(a, X, Y) -> {X, Y};
f(b, X, _) -> X;
f(_, _, Z) -> Z.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].arity, 3);
    }

    /// List cons pattern with nested matching.
    #[test]
    fn test_pattern_nested_cons() {
        let source = r#"
-module(test).
-export([f/1]).

f([{ok, V}|_]) -> V;
f([_|T]) -> f(T);
f([]) -> not_found.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        // Should destructure cons, then match head against {ok, V}
        let has_list_head = ir.functions[0].blocks.iter().any(|b|
            b.instructions.iter().any(|i| matches!(i, Instruction::ListHead(_, _)))
        );
        assert!(has_list_head, "expected cons destructuring");
    }

    /// Wildcard-only clauses (should always match).
    #[test]
    fn test_pattern_all_wildcards() {
        let source = r#"
-module(test).
-export([f/2]).

f(_, _) -> ok.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        // With only wildcards, the decision tree should be trivial.
        // The pattern compiler should not emit any type tests.
    }

    /// Guards in case expressions.
    #[test]
    fn test_pattern_case_with_guards() {
        let source = r#"
-module(test).
-export([f/1]).

f(X) ->
    case X of
        N when N > 0 -> positive;
        0 -> zero;
        _ -> negative
    end.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        // Guard should create a conditional branch
    }

    /// Integer literal patterns with overlap.
    #[test]
    fn test_pattern_integer_overlap() {
        let source = r#"
-module(test).
-export([f/1]).

f(1) -> one;
f(2) -> two;
f(3) -> three;
f(N) -> {other, N}.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        // Should produce a chain of equality tests
        let const_count = ir.functions[0].blocks.iter()
            .flat_map(|b| &b.instructions)
            .filter(|i| matches!(i, Instruction::Const(_, Literal::Integer(_))))
            .count();
        assert!(const_count >= 3, "expected at least 3 integer constants for pattern comparisons");
    }

    /// Atom literal patterns (first column dispatch).
    #[test]
    fn test_pattern_atom_dispatch() {
        let source = r#"
-module(test).
-export([f/1]).

f(ok) -> success;
f(error) -> failure;
f(undefined) -> missing;
f(_) -> unknown.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
    }

    /// Mixed constructor types in same column: tuple, list, atom, integer.
    #[test]
    fn test_pattern_mixed_constructors() {
        let source = r#"
-module(test).
-export([f/1]).

f({X, Y}) -> {tuple, X, Y};
f([H|T]) -> {list, H, T};
f(42) -> {int, 42};
f(hello) -> {atom, hello};
f(_) -> other.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        // Should produce type tests to discriminate between constructors
    }

    /// Alias (match) patterns: X = {ok, V}.
    #[test]
    fn test_pattern_alias() {
        let source = r#"
-module(test).
-export([f/1]).

f(X = {ok, V}) -> {X, V};
f(_) -> error.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
    }

    /// Case expression with binary patterns.
    #[test]
    fn test_pattern_binary_case() {
        let source = r#"
-module(test).
-export([f/1]).

f(Bin) ->
    case Bin of
        <<1, 2, 3>> -> exact;
        <<H:8, _Rest/binary>> -> {first, H};
        <<>> -> empty
    end.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
    }

    /// Map patterns in case expressions.
    #[test]
    fn test_pattern_map_case() {
        let source = r#"
-module(test).
-export([f/1]).

f(M) ->
    case M of
        #{status := ok, value := V} -> {ok, V};
        #{status := error, reason := R} -> {error, R};
        #{} -> empty_map
    end.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        // Should emit IsMap type tests and MapGet instructions
        let has_map_get = ir.functions[0].blocks.iter().any(|b|
            b.instructions.iter().any(|i| matches!(i, Instruction::MapGet(_, _, _)))
        );
        assert!(has_map_get, "expected MapGet instructions for map pattern matching");
    }

    /// Deep nesting: tuple containing list containing tuple.
    #[test]
    fn test_pattern_deep_nesting() {
        let source = r#"
-module(test).
-export([f/1]).

f({a, [{b, X}|_]}) -> X;
f({a, []}) -> empty;
f(_) -> other.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
    }

    /// Multiple function clauses with different arities shouldn't compile
    /// (but we test that matching the same arity with different patterns works).
    #[test]
    fn test_pattern_function_multi_clause() {
        let source = r#"
-module(test).
-export([f/2]).

f(a, X) -> {a_case, X};
f(b, {Y, Z}) -> {b_case, Y, Z};
f(_, _) -> default.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].arity, 2);
    }

    /// Single variable pattern in case — should bind directly.
    #[test]
    fn test_pattern_single_variable() {
        let source = r#"
-module(test).
-export([f/1]).

f(X) ->
    case X of
        Y -> Y
    end.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
    }

    /// Nil (empty list) pattern matching.
    #[test]
    fn test_pattern_nil() {
        let source = r#"
-module(test).
-export([f/1]).

f([]) -> empty;
f([_|_]) -> nonempty.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
    }

    /// Map expression creation (not pattern matching) should lower to MakeMap.
    #[test]
    fn test_lower_map_creation() {
        let source = r#"
-module(test).
-export([f/0]).

f() -> #{a => 1, b => 2}.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        let has_make_map = ir.functions[0].blocks.iter().any(|b|
            b.instructions.iter().any(|i| matches!(i, Instruction::MakeMap(_, _)))
        );
        assert!(has_make_map, "expected MakeMap instruction for map creation");
    }

    /// Binary expression creation should lower to MakeBinary + BinaryPutInteger.
    #[test]
    fn test_lower_binary_creation() {
        let source = r#"
-module(test).
-export([f/0]).

f() -> <<1, 2, 3>>.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        let has_make_binary = ir.functions[0].blocks.iter().any(|b|
            b.instructions.iter().any(|i| matches!(i, Instruction::MakeBinary(_, _)))
        );
        assert!(has_make_binary, "expected MakeBinary instruction for binary creation");
    }

    /// Tuple with exact arity matching (2-tuple vs 3-tuple).
    #[test]
    fn test_pattern_tuple_arity_discrimination() {
        let source = r#"
-module(test).
-export([f/1]).

f({X}) -> {one, X};
f({X, Y}) -> {two, X, Y};
f({X, Y, Z}) -> {three, X, Y, Z};
f(_) -> other.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        // Should test tuple arity (size) before extracting elements
    }

    /// Pattern that uses the same variable in function head body.
    #[test]
    fn test_pattern_variable_used_in_body() {
        let source = r#"
-module(test).
-export([f/1]).

f({ok, Value}) ->
    {result, Value};
f({error, Reason}) ->
    {failed, Reason}.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
    }

    // ── List comprehension lowering ───────────────────────────

    #[test]
    fn test_lower_simple_list_comp() {
        let source = r#"
-module(test).
-export([f/0]).

f() -> [X * 2 || X <- [1, 2, 3]].
"#;
        let ir = lower_source(source).unwrap();
        // Should have 2 functions: f/0 and the generated __lc_0 helper
        assert_eq!(ir.functions.len(), 2);
        assert_eq!(ir.functions[0].arity, 2); // helper has arity 2 (input list + accumulator)

        // The helper should contain MakeCons instruction (building result list)
        let helper = &ir.functions[0];
        let has_make_cons = helper.blocks.iter().any(|block| {
            block.instructions.iter().any(|inst| {
                matches!(inst, Instruction::MakeCons(_, _, _))
            })
        });
        assert!(has_make_cons, "Helper should build result with MakeCons");
    }

    #[test]
    fn test_lower_list_comp_with_filter() {
        let source = r#"
-module(test).
-export([f/0]).

f() -> [X || X <- [1, 2, 3, 4], X > 2].
"#;
        let ir = lower_source(source).unwrap();
        // f/0 + helper = 2 functions
        assert_eq!(ir.functions.len(), 2);

        // Helper should have a branch for the filter (true -> produce, false -> skip)
        let helper = &ir.functions[0];
        let has_branch = helper.blocks.iter().any(|block| {
            matches!(&block.terminator, Terminator::Branch(_, _, _))
        });
        assert!(has_branch, "Helper with filter should have branch terminator");
    }

    #[test]
    fn test_lower_multiple_list_comps() {
        let source = r#"
-module(test).
-export([f/0]).

f() ->
    A = [X * 2 || X <- [1, 2]],
    B = [X + 1 || X <- [3, 4]],
    {A, B}.
"#;
        let ir = lower_source(source).unwrap();
        // f/0 + __lc_0 + __lc_1 = 3 functions
        assert_eq!(ir.functions.len(), 3);
    }

    // ── Exception handling lowering ──────────────────────────

    #[test]
    fn test_lower_throw() {
        let source = r#"
-module(test).
-export([f/0]).

f() -> throw(hello).
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        let has_raise = ir.functions[0].blocks.iter().any(|b|
            b.instructions.iter().any(|i| matches!(i, Instruction::RaiseException(_, _, _)))
        );
        assert!(has_raise, "expected RaiseException instruction for throw");
    }

    #[test]
    fn test_lower_simple_try_catch() {
        let source = r#"
-module(test).
-export([f/0]).

f() ->
    try
        throw(hello)
    catch
        X -> {caught, X}
    end.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        let f = &ir.functions[0];

        // Should have CheckException instruction
        let has_check = f.blocks.iter().any(|b|
            b.instructions.iter().any(|i| matches!(i, Instruction::CheckException(_)))
        );
        assert!(has_check, "expected CheckException instruction in try/catch");

        // Should have ClearException instruction
        let has_clear = f.blocks.iter().any(|b|
            b.instructions.iter().any(|i| matches!(i, Instruction::ClearException))
        );
        assert!(has_clear, "expected ClearException instruction in catch handler");

        // Should have GetExceptionReason
        let has_reason = f.blocks.iter().any(|b|
            b.instructions.iter().any(|i| matches!(i, Instruction::GetExceptionReason(_)))
        );
        assert!(has_reason, "expected GetExceptionReason in catch handler");
    }

    #[test]
    fn test_lower_try_catch_normal_path() {
        let source = r#"
-module(test).
-export([f/0]).

f() ->
    try
        42
    catch
        _ -> error
    end.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        // Body doesn't throw, so should still have CheckException (checked at compile time)
        // but at runtime the normal path is taken
        let has_check = ir.functions[0].blocks.iter().any(|b|
            b.instructions.iter().any(|i| matches!(i, Instruction::CheckException(_)))
        );
        assert!(has_check, "try/catch always emits CheckException");
    }

    #[test]
    fn test_lower_catch_expr() {
        // `catch Expr` desugars to a synthetic try-catch, so it should
        // produce the same IR instructions as a regular try-catch.
        let source = r#"
-module(test).
-export([f/0]).

f() ->
    catch throw(hello).
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        let f = &ir.functions[0];

        // Should have CheckException (from the desugared try-catch)
        let has_check = f.blocks.iter().any(|b|
            b.instructions.iter().any(|i| matches!(i, Instruction::CheckException(_)))
        );
        assert!(has_check, "catch expr should desugar to try-catch with CheckException");

        // Should have GetExceptionClass (catch dispatches on throw/exit/error)
        let has_class = f.blocks.iter().any(|b|
            b.instructions.iter().any(|i| matches!(i, Instruction::GetExceptionClass(_)))
        );
        assert!(has_class, "catch expr should read exception class");

        // Should have ClearException
        let has_clear = f.blocks.iter().any(|b|
            b.instructions.iter().any(|i| matches!(i, Instruction::ClearException))
        );
        assert!(has_clear, "catch expr should clear exception state");
    }

    #[test]
    fn test_lower_catch_expr_in_case() {
        // Verifies that `case catch Expr of ...` lowers correctly.
        let source = r#"
-module(test).
-export([f/0]).

f() ->
    case catch throw(hello) of
        hello -> ok;
        _ -> error
    end.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        // Should compile without errors and produce CheckException
        let has_check = ir.functions[0].blocks.iter().any(|b|
            b.instructions.iter().any(|i| matches!(i, Instruction::CheckException(_)))
        );
        assert!(has_check, "case catch should desugar the catch sub-expression");
    }

    #[test]
    fn test_lower_undefined_record_pattern_reports_located_error() {
        // Regression: a pattern naming a record with no definition reached the
        // pattern compiler as Pattern::Record, so `nera check` and `nera build`
        // panicked (exit 101). Clauses whose body used a field variable instead
        // reported a misleading "undefined variable". Every pattern position
        // must report the undefined record at the record pattern, like erlc's
        // "record nope undefined".
        let cases = [
            ("function head", "f(#nope{}) -> ok."),
            ("nested function head", "f({ok, #{k := #nope{a = A}}}) -> A."),
            ("fun head", "f() -> F = fun(#nope{}) -> ok end, F(x)."),
            ("match", "f(X) -> #nope{} = X, ok."),
            ("case clause", "f(X) -> case X of #nope{} -> yes; _ -> no end."),
            ("case clause using a field", "f(X) -> case X of #nope{a = A} -> A; _ -> no end."),
            ("receive clause", "f() -> receive #nope{} -> ok after 0 -> timeout end."),
            ("try of clause", "f(X) -> try X of #nope{} -> ok catch _:_ -> error end."),
            ("catch clause", "f(X) -> try throw(X) catch throw:#nope{} -> ok end."),
            ("maybe match", "f(X) -> maybe #nope{} ?= X, ok end."),
            ("maybe else clause", "f(X) -> maybe ok ?= X else #nope{} -> error end."),
            ("comprehension generator", "f(L) -> [A || #nope{a = A} <- L]."),
        ];
        for (position, function) in cases {
            let source = format!("-module(test).\n-record(req, {{method, path}}).\n\n{function}\n");
            let record_pattern_start = source.find("#nope").unwrap();
            // `nera check`, `nera build` and the language server print this
            // message at this span.
            let error = lower_source(&source)
                .expect_err(&format!("{position}: an undefined record must not lower"));
            assert_eq!(error.to_string(), "undefined record 'nope'", "{position}");
            assert_eq!(
                error.span().map(|span| span.start),
                Some(record_pattern_start),
                "{position}: the error must point at the record pattern",
            );
        }
    }

    // ── Map update lowering ─────────────────────────────────────

    #[test]
    fn test_lower_map_update_single() {
        let source = r#"
-module(test).
-export([f/1]).

f(M) -> M#{a := 10}.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        // := produces MapUpdateExact
        let has_map_update_exact = ir.functions[0].blocks.iter().any(|b|
            b.instructions.iter().any(|i| matches!(i, Instruction::MapUpdateExact(_, _, _, _)))
        );
        assert!(has_map_update_exact, "expected MapUpdateExact instruction for := update");
    }

    #[test]
    fn test_lower_map_update_multiple() {
        let source = r#"
-module(test).
-export([f/1]).

f(M) -> M#{a := 1, b => 2, c := 3}.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        // := produces MapUpdateExact, => produces MapPut
        let map_update_exact_count = ir.functions[0].blocks.iter()
            .flat_map(|b| &b.instructions)
            .filter(|i| matches!(i, Instruction::MapUpdateExact(_, _, _, _)))
            .count();
        let map_put_count = ir.functions[0].blocks.iter()
            .flat_map(|b| &b.instructions)
            .filter(|i| matches!(i, Instruction::MapPut(_, _, _, _)))
            .count();
        assert_eq!(map_update_exact_count, 2, "expected 2 MapUpdateExact for := fields");
        assert_eq!(map_put_count, 1, "expected 1 MapPut for => field");
    }

    #[test]
    fn test_lower_map_update_on_literal() {
        let source = r#"
-module(test).
-export([f/0]).

f() -> (#{a => 1})#{a := 2}.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        // Should have MakeMap (for #{a => 1}) and MapUpdateExact (for := update)
        let has_make_map = ir.functions[0].blocks.iter().any(|b|
            b.instructions.iter().any(|i| matches!(i, Instruction::MakeMap(_, _)))
        );
        let has_map_update_exact = ir.functions[0].blocks.iter().any(|b|
            b.instructions.iter().any(|i| matches!(i, Instruction::MapUpdateExact(_, _, _, _)))
        );
        assert!(has_make_map, "expected MakeMap for map creation");
        assert!(has_map_update_exact, "expected MapUpdateExact for := update");
    }

    // ── Record lowering tests ──

    #[test]
    fn test_lower_record_create() {
        let source = r#"
-module(test).
-export([f/0]).
-record(person, {name, age = 0}).

f() -> #person{name = alice}.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        // RecordCreate desugars to MakeTuple with 3 elements (tag + 2 fields)
        let has_make_tuple = ir.functions[0].blocks.iter().any(|b|
            b.instructions.iter().any(|i| match i {
                Instruction::MakeTuple(_, elems) => elems.len() == 3,
                _ => false,
            })
        );
        assert!(has_make_tuple, "expected MakeTuple with 3 elements for record creation");
    }

    #[test]
    fn test_lower_record_create_defaults() {
        let source = r#"
-module(test).
-export([f/0]).
-record(point, {x = 0, y = 0, z = 0}).

f() -> #point{y = 10}.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        // MakeTuple with 4 elements (tag + 3 fields), 2 fields use defaults
        let has_make_tuple = ir.functions[0].blocks.iter().any(|b|
            b.instructions.iter().any(|i| match i {
                Instruction::MakeTuple(_, elems) => elems.len() == 4,
                _ => false,
            })
        );
        assert!(has_make_tuple, "expected MakeTuple with 4 elements for point record");
    }

    #[test]
    fn test_lower_record_access() {
        let source = r#"
-module(test).
-export([f/1]).
-record(person, {name, age = 0}).

f(R) -> R#person.name.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        // RecordAccess desugars to TupleElement(_, _, 1) — index 1 for first field
        let has_tuple_element = ir.functions[0].blocks.iter().any(|b|
            b.instructions.iter().any(|i| match i {
                Instruction::TupleElement(_, _, idx) => *idx == 1,
                _ => false,
            })
        );
        assert!(has_tuple_element, "expected TupleElement with index 1 for record access");
    }

    #[test]
    fn test_lower_record_index() {
        let source = r#"
-module(test).
-export([f/0]).
-record(person, {name, age = 0}).

f() -> #person.name.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        // RecordIndex desugars to Const(Integer(2)) — 1-based Erlang indexing
        let has_const_2 = ir.functions[0].blocks.iter().any(|b|
            b.instructions.iter().any(|i| matches!(i, Instruction::Const(_, Literal::Integer(2))))
        );
        assert!(has_const_2, "expected Const(Integer(2)) for #person.name index");
    }

    #[test]
    fn test_lower_record_update() {
        let source = r#"
-module(test).
-export([f/1]).
-record(person, {name, age = 0}).

f(R) -> R#person{age = 31}.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        // RecordUpdate: extracts fields, rebuilds with MakeTuple (3 elements)
        let has_make_tuple = ir.functions[0].blocks.iter().any(|b|
            b.instructions.iter().any(|i| match i {
                Instruction::MakeTuple(_, elems) => elems.len() == 3,
                _ => false,
            })
        );
        let has_tuple_element = ir.functions[0].blocks.iter().any(|b|
            b.instructions.iter().any(|i| matches!(i, Instruction::TupleElement(_, _, _)))
        );
        assert!(has_make_tuple, "expected MakeTuple for record update rebuild");
        assert!(has_tuple_element, "expected TupleElement for extracting unchanged fields");
    }

    #[test]
    fn test_lower_record_pattern_desugared() {
        // Record patterns should be desugared to tuple patterns before
        // reaching the pattern compiler. Verify it compiles without panic.
        let source = r#"
-module(test).
-export([f/1]).
-record(person, {name, age = 0}).

f(#person{name = N}) -> N.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        // Should compile cleanly — if Pattern::Record reaches the pattern
        // compiler un-desugared, it would panic.
    }

    #[test]
    fn test_lower_nested_record_patterns() {
        // Regression: Cowboy HTTP/2 stream maps must reach the pattern compiler
        // as tagged tuples, including aliases and other enclosing containers.
        let source = include_str!("../../../../tests/record_nested_patterns.erl");
        lower_source(source).expect("nested record patterns must lower to valid IR");
    }

    #[test]
    fn test_multi_clause_binary_rest_pattern() {
        // Regression test: multi-clause functions where each clause matches
        // a binary prefix with a Rest/binary variable.  The pattern compiler
        // used to share variable scopes across clauses, causing the VarId
        // for Rest in one clause to be overwritten by a later clause's
        // binding.  This led to the body referencing a wrong VarId.
        let source = r#"
-module(test).
-export([dispatch/1]).

dispatch(<<"set ", R/binary>>) -> {set, R};
dispatch(<<"get ", R/binary>>) -> {get, R};
dispatch(_Other) -> unknown.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);

        // Each clause body block must have its own BsMaterialize (or
        // equivalent) for R — NOT sharing a VarId across clauses.
        // Count the number of BsMaterialize instructions in the IR.
        let materialize_count = ir.functions[0].blocks.iter()
            .flat_map(|b| b.instructions.iter())
            .filter(|inst| matches!(inst, Instruction::BsMaterialize(_, _, _, _)))
            .count();

        // There should be at least 2 BsMaterialize: one for each clause that
        // binds R.  (Before the fix, the "get" clause would reference the
        // "set" clause's VarId without its own BsMaterialize.)
        assert!(materialize_count >= 2,
            "Expected at least 2 BsMaterialize instructions (one per clause), got {}",
            materialize_count);
    }

    #[test]
    fn test_multi_clause_binary_rest_different_names() {
        // Multi-clause binary prefix where each clause uses a differently
        // named Rest variable.  This should always work, but verifies the
        // pattern compiler doesn't break even with distinct names.
        let source = r#"
-module(test).
-export([f/1]).

f(<<"a ", X/binary>>) -> X;
f(<<"b ", Y/binary>>) -> Y;
f(_) -> none.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);

        let materialize_count = ir.functions[0].blocks.iter()
            .flat_map(|b| b.instructions.iter())
            .filter(|inst| matches!(inst, Instruction::BsMaterialize(_, _, _, _)))
            .count();
        assert!(materialize_count >= 2,
            "Expected at least 2 BsMaterialize instructions, got {}", materialize_count);
    }

    #[test]
    fn test_multi_clause_binary_rest_with_extra_args() {
        // Multi-clause binary prefix with an additional argument (like
        // onecached's handle_command/2).  Verifies Rest/binary works
        // correctly when there are multiple function parameters.
        let source = r#"
-module(test).
-export([handle/2]).

handle(<<"set ", R/binary>>, State) -> {set, R, State};
handle(<<"get ", R/binary>>, State) -> {get, R, State};
handle(Other, State) -> {unknown, Other, State}.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
    }

    // ── maybe expression tests ───────────────────────────────

    #[test]
    fn test_lower_maybe_basic() {
        let source = r#"
-module(test).
-export([f/1]).

f(X) ->
    maybe
        {ok, V} ?= X,
        V
    end.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        // Should have multiple blocks for the match branching
        assert!(ir.functions[0].blocks.len() > 2);
    }

    #[test]
    fn test_lower_maybe_with_else() {
        let source = r#"
-module(test).
-export([f/1]).

f(X) ->
    maybe
        {ok, V} ?= X,
        V
    else
        {error, R} -> R
    end.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert!(ir.functions[0].blocks.len() > 3);
    }

    #[test]
    fn test_lower_maybe_plain_block() {
        let source = r#"
-module(test).
-export([f/0]).

f() ->
    maybe
        ok
    end.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
    }

    #[test]
    fn test_lower_maybe_multiple_cond_match() {
        let source = r#"
-module(test).
-export([f/2]).

f(X, Y) ->
    maybe
        {ok, A} ?= X,
        {ok, B} ?= Y,
        A + B
    end.
"#;
        let ir = lower_source(source).unwrap();
        assert_eq!(ir.functions.len(), 1);
        // Multiple ?= should create multiple match branches
        assert!(ir.functions[0].blocks.len() > 4);
    }

    // ── Constant folding tests ────────────────────────────────

    #[test]
    fn test_fold_integer_add() {
        let source = r#"
-module(test).
-export([f/0]).

f() -> 1 + 2.
"#;
        let ir = lower_source(source).unwrap();
        // The expression 1 + 2 should be folded to Const(3)
        let has_const_3 = ir.functions[0].blocks.iter().any(|b|
            b.instructions.iter().any(|i| matches!(i, Instruction::Const(_, Literal::Integer(3))))
        );
        assert!(has_const_3, "1 + 2 should be folded to Const(3)");
        // No BinOp::Add should remain for the constant expression
        let has_add = ir.functions[0].blocks.iter().any(|b|
            b.instructions.iter().any(|i| matches!(i, Instruction::BinOp(_, BinOp::Add, _, _)))
        );
        assert!(!has_add, "BinOp::Add should be eliminated by constant folding");
    }

    #[test]
    fn test_fold_integer_sub_mul() {
        let source = r#"
-module(test).
-export([f/0]).

f() -> (10 - 3) * 2.
"#;
        let ir = lower_source(source).unwrap();
        // 10 - 3 = 7, then 7 * 2 = 14
        let has_const_14 = ir.functions[0].blocks.iter().any(|b|
            b.instructions.iter().any(|i| matches!(i, Instruction::Const(_, Literal::Integer(14))))
        );
        assert!(has_const_14, "(10 - 3) * 2 should be folded to Const(14)");
    }

    #[test]
    fn test_fold_div_produces_float() {
        let source = r#"
-module(test).
-export([f/0]).

f() -> 10 / 4.
"#;
        let ir = lower_source(source).unwrap();
        // 10 / 4 = 2.5 (float division)
        let has_float = ir.functions[0].blocks.iter().any(|b|
            b.instructions.iter().any(|i| match i {
                Instruction::Const(_, Literal::Float(f)) => (*f - 2.5).abs() < 1e-10,
                _ => false,
            })
        );
        assert!(has_float, "10 / 4 should be folded to Const(2.5)");
    }

    #[test]
    fn test_fold_div_by_zero_preserved() {
        let source = r#"
-module(test).
-export([f/0]).

f() -> 10 / 0.
"#;
        let ir = lower_source(source).unwrap();
        // Division by zero should NOT be folded — must remain as BinOp
        let has_div = ir.functions[0].blocks.iter().any(|b|
            b.instructions.iter().any(|i| matches!(i, Instruction::BinOp(_, BinOp::Div, _, _)))
        );
        assert!(has_div, "Division by zero should be preserved as runtime BinOp");
    }

    #[test]
    fn test_fold_nonfinite_float_preserved() {
        // A zero divisor or a result beyond the float range must stay a
        // runtime operation, which raises badarith, never an infinite or NaN
        // literal.
        for (expr, op) in [
            ("1.0 / 0.0", BinOp::Div),
            ("1.0 / -0.0", BinOp::Div),
            ("0.0 / 0.0", BinOp::Div),
            ("1 / 0.0", BinOp::Div),
            ("1.0e308 / 1.0e-10", BinOp::Div),
            ("1.0e308 * 10.0", BinOp::Mul),
            ("1.0e308 * 10", BinOp::Mul),
            ("1.0e308 + 1.0e308", BinOp::Add),
            ("-1.0e308 - 1.0e308", BinOp::Sub),
        ] {
            let source = format!("-module(test).\n-export([f/0]).\n\nf() -> {expr}.\n");
            let ir = lower_source(&source).unwrap();
            let instructions = || ir.functions[0].blocks.iter().flat_map(|b| b.instructions.iter());
            assert!(
                instructions().any(|i| matches!(i, Instruction::BinOp(_, o, _, _) if *o == op)),
                "{expr} should be preserved as a runtime BinOp"
            );
            assert!(
                !instructions().any(|i| matches!(i, Instruction::Const(_, Literal::Float(f)) if !f.is_finite())),
                "{expr} should not be folded to a nonfinite float"
            );
        }
    }

    #[test]
    fn test_fold_integer_overflow_preserved() {
        // i64::MAX + 1 would overflow — should be left as runtime op
        let source = r#"
-module(test).
-export([f/0]).

f() -> 4611686018427387903 + 4611686018427387903.
"#;
        let _ir = lower_source(source).unwrap();
        // This fits in i64 (sum < i64::MAX), so it WILL be folded.
        // Use a value that actually overflows:
        // i64::MAX = 9223372036854775807
        // We can't write that in Erlang source easily, but we can test
        // via the unit-test function directly.
        let mut func = IrFunction {
            name: AtomId(0),
            arity: 0,
            mangled_name: "test".into(),
            protected_values: Vec::new(),
            entry_block: BlockId(0),
            params: vec![],
            line: 0,
            blocks: vec![BasicBlock {
                id: BlockId(0),
                params: vec![],
                instructions: vec![
                    Instruction::Const(VarId(0), Literal::Integer(i64::MAX)),
                    Instruction::Const(VarId(1), Literal::Integer(1)),
                    Instruction::BinOp(VarId(2), BinOp::Add, VarId(0), VarId(1)),
                ],
                terminator: Terminator::Return(VarId(2)),
            }],
        };

        assert_valid(&func, "before standalone pass");
        fold_constants(&mut func, AtomId(100), AtomId(101));
        assert_valid(&func, "after standalone pass");

        // Should still be a BinOp (overflow not folded)
        assert!(
            matches!(&func.blocks[0].instructions[2], Instruction::BinOp(_, BinOp::Add, _, _)),
            "i64::MAX + 1 should NOT be folded (overflow)"
        );
    }

    #[test]
    fn test_fold_float_arithmetic() {
        let source = r#"
-module(test).
-export([f/0]).

f() -> 1.5 + 2.5.
"#;
        let ir = lower_source(source).unwrap();
        let has_float_4 = ir.functions[0].blocks.iter().any(|b|
            b.instructions.iter().any(|i| match i {
                Instruction::Const(_, Literal::Float(f)) => (*f - 4.0).abs() < 1e-10,
                _ => false,
            })
        );
        assert!(has_float_4, "1.5 + 2.5 should be folded to Const(4.0)");
    }

    #[test]
    fn test_fold_float_equality_preserves_signed_zero() {
        // NERA-88 F1: literal comparisons must agree with runtime exact equality.
        for (expression, expected) in [
            ("0.0 =:= -0.0", false),
            ("-0.0 =:= 0.0", false),
            ("0.0 =/= -0.0", true),
            ("-0.0 =/= 0.0", true),
            ("0.0 =:= 0.0", true),
            ("-0.0 =:= -0.0", true),
            ("0.0 =/= 0.0", false),
            ("-0.0 =/= -0.0", false),
            ("0.0 == -0.0", true),
            ("0.0 /= -0.0", false),
            ("1.5 =:= 1.5", true),
            ("1.5 =:= 2.5", false),
            ("1.5 =/= 1.5", false),
            ("1.5 =/= 2.5", true),
        ] {
            let source = format!(
                "-module(test). -export([f/0, expected/0]). f() -> {expression}. expected() -> {expected}."
            );
            let ir = lower_source(&source).unwrap();
            let atom_result = |function: &IrFunction| {
                function.blocks.iter().find_map(|block| {
                    let Terminator::Return(result) = block.terminator else { return None; };
                    block.instructions.iter().rev().find_map(|instruction| match instruction {
                        Instruction::Const(var, Literal::Atom(atom)) if *var == result => Some(*atom),
                        _ => None,
                    })
                })
            };
            let expected_atom = atom_result(&ir.functions[1]).unwrap();
            assert_eq!(atom_result(&ir.functions[0]), Some(expected_atom), "{expression}");
            assert!(!ir.functions[0].blocks.iter().flat_map(|block| &block.instructions)
                .any(|instruction| matches!(instruction, Instruction::BinOp(..))),
                "{expression} must fold to a literal");
            println!("folded {expression} -> {expected}");
        }
    }

    #[test]
    fn test_fold_comparison() {
        let source = r#"
-module(test).
-export([f/0]).

f() -> 3 > 2.
"#;
        let ir = lower_source(source).unwrap();
        // Should fold to Const(Atom(true_atom_id)) and no BinOp remains
        let has_gt = ir.functions[0].blocks.iter().any(|b|
            b.instructions.iter().any(|i| matches!(i, Instruction::BinOp(_, BinOp::Gt, _, _)))
        );
        assert!(!has_gt, "3 > 2 should be folded away (no BinOp::Gt)");
    }

    #[test]
    fn test_fold_unary_neg() {
        let source = r#"
-module(test).
-export([f/0]).

f() -> -42.
"#;
        let ir = lower_source(source).unwrap();
        // -42 should be folded to Const(Integer(-42))
        let has_neg42 = ir.functions[0].blocks.iter().any(|b|
            b.instructions.iter().any(|i| matches!(i, Instruction::Const(_, Literal::Integer(-42))))
        );
        assert!(has_neg42, "-42 should be folded to Const(-42)");
    }

    #[test]
    fn test_fold_bitwise_ops() {
        let mut func = IrFunction {
            name: AtomId(0),
            arity: 0,
            mangled_name: "test".into(),
            protected_values: Vec::new(),
            entry_block: BlockId(0),
            params: vec![],
            line: 0,
            blocks: vec![BasicBlock {
                id: BlockId(0),
                params: vec![],
                instructions: vec![
                    Instruction::Const(VarId(0), Literal::Integer(0xFF)),
                    Instruction::Const(VarId(1), Literal::Integer(0x0F)),
                    Instruction::BinOp(VarId(2), BinOp::Band, VarId(0), VarId(1)),
                ],
                terminator: Terminator::Return(VarId(2)),
            }],
        };

        assert_valid(&func, "before standalone pass");
        fold_constants(&mut func, AtomId(100), AtomId(101));
        assert_valid(&func, "after standalone pass");

        assert!(
            matches!(&func.blocks[0].instructions[2], Instruction::Const(_, Literal::Integer(0x0F))),
            "0xFF band 0x0F should fold to 0x0F"
        );
    }

    #[test]
    fn test_fold_unary_not_bool() {
        let mut func = IrFunction {
            name: AtomId(0),
            arity: 0,
            mangled_name: "test".into(),
            protected_values: Vec::new(),
            entry_block: BlockId(0),
            params: vec![],
            line: 0,
            blocks: vec![BasicBlock {
                id: BlockId(0),
                params: vec![],
                instructions: vec![
                    Instruction::Const(VarId(0), Literal::Atom(AtomId(100))), // true
                    Instruction::UnaryOp(VarId(1), nera_syntax::ast::UnaryOp::Not, VarId(0)),
                ],
                terminator: Terminator::Return(VarId(1)),
            }],
        };

        assert_valid(&func, "before standalone pass");
        fold_constants(&mut func, AtomId(100), AtomId(101));
        assert_valid(&func, "after standalone pass");

        // `not true` should fold to `false`
        assert!(
            matches!(&func.blocks[0].instructions[1], Instruction::Const(_, Literal::Atom(AtomId(101)))),
            "`not true` should fold to Atom(false)"
        );
    }

    #[test]
    fn test_fold_unary_bnot() {
        let mut func = IrFunction {
            name: AtomId(0),
            arity: 0,
            mangled_name: "test".into(),
            protected_values: Vec::new(),
            entry_block: BlockId(0),
            params: vec![],
            line: 0,
            blocks: vec![BasicBlock {
                id: BlockId(0),
                params: vec![],
                instructions: vec![
                    Instruction::Const(VarId(0), Literal::Integer(0)),
                    Instruction::UnaryOp(VarId(1), nera_syntax::ast::UnaryOp::Bnot, VarId(0)),
                ],
                terminator: Terminator::Return(VarId(1)),
            }],
        };

        assert_valid(&func, "before standalone pass");
        fold_constants(&mut func, AtomId(100), AtomId(101));
        assert_valid(&func, "after standalone pass");

        assert!(
            matches!(&func.blocks[0].instructions[1], Instruction::Const(_, Literal::Integer(-1))),
            "bnot 0 should fold to -1"
        );
    }

    #[test]
    fn test_fold_chained_constants() {
        // Test that folded results propagate: (1 + 2) + 3 should fold to 6
        let mut func = IrFunction {
            name: AtomId(0),
            arity: 0,
            mangled_name: "test".into(),
            protected_values: Vec::new(),
            entry_block: BlockId(0),
            params: vec![],
            line: 0,
            blocks: vec![BasicBlock {
                id: BlockId(0),
                params: vec![],
                instructions: vec![
                    Instruction::Const(VarId(0), Literal::Integer(1)),
                    Instruction::Const(VarId(1), Literal::Integer(2)),
                    Instruction::BinOp(VarId(2), BinOp::Add, VarId(0), VarId(1)),
                    Instruction::Const(VarId(3), Literal::Integer(3)),
                    Instruction::BinOp(VarId(4), BinOp::Add, VarId(2), VarId(3)),
                ],
                terminator: Terminator::Return(VarId(4)),
            }],
        };

        assert_valid(&func, "before standalone pass");
        fold_constants(&mut func, AtomId(100), AtomId(101));
        assert_valid(&func, "after standalone pass");

        assert!(
            matches!(&func.blocks[0].instructions[4], Instruction::Const(_, Literal::Integer(6))),
            "(1 + 2) + 3 should chain-fold to Const(6)"
        );
    }

    #[test]
    fn test_fold_non_constant_operand_preserved() {
        // If one operand is a runtime variable, BinOp should be preserved
        let source = r#"
-module(test).
-export([f/1]).

f(X) -> X + 1.
"#;
        let ir = lower_source(source).unwrap();
        let has_add = ir.functions[0].blocks.iter().any(|b|
            b.instructions.iter().any(|i| matches!(i, Instruction::BinOp(_, BinOp::Add, _, _)))
        );
        assert!(has_add, "X + 1 should NOT be folded (X is unknown)");
    }

    // ── Dead code elimination tests ──────────────────────────────────────

    #[test]
    fn test_dce_branch_true_simplified() {
        let true_id = AtomId(100);
        let false_id = AtomId(101);
        let mut func = IrFunction {
            name: AtomId(0),
            arity: 0,
            mangled_name: "test".into(),
            protected_values: Vec::new(),
            entry_block: BlockId(0),
            params: vec![],
            line: 0,
            blocks: vec![
                BasicBlock {
                    id: BlockId(0),
                    params: vec![],
                    instructions: vec![
                        Instruction::Const(VarId(0), Literal::Atom(true_id)),
                    ],
                    terminator: Terminator::Branch(VarId(0), BlockId(1), BlockId(2)),
                },
                BasicBlock {
                    id: BlockId(1),
                    params: vec![],
                    instructions: vec![
                        Instruction::Const(VarId(1), Literal::Integer(1)),
                    ],
                    terminator: Terminator::Return(VarId(1)),
                },
                BasicBlock {
                    id: BlockId(2),
                    params: vec![],
                    instructions: vec![
                        Instruction::Const(VarId(2), Literal::Integer(2)),
                    ],
                    terminator: Terminator::Return(VarId(2)),
                },
            ],
        };

        assert_valid(&func, "before standalone pass");
        eliminate_dead_code(&mut func, true_id, false_id);
        assert_valid(&func, "after standalone pass");

        // Branch should be simplified to Jump(BlockId(1))
        assert!(
            matches!(&func.blocks[0].terminator, Terminator::Jump(BlockId(1), _)),
            "Branch on true should become Jump to then-block"
        );
        // Block 2 (else) should be removed as unreachable
        assert_eq!(func.blocks.len(), 2, "Unreachable else-block should be removed");
        assert!(!func.blocks.iter().any(|b| b.id == BlockId(2)));
    }

    #[test]
    fn test_dce_branch_false_simplified() {
        let true_id = AtomId(100);
        let false_id = AtomId(101);
        let mut func = IrFunction {
            name: AtomId(0),
            arity: 0,
            mangled_name: "test".into(),
            protected_values: Vec::new(),
            entry_block: BlockId(0),
            params: vec![],
            line: 0,
            blocks: vec![
                BasicBlock {
                    id: BlockId(0),
                    params: vec![],
                    instructions: vec![
                        Instruction::Const(VarId(0), Literal::Atom(false_id)),
                    ],
                    terminator: Terminator::Branch(VarId(0), BlockId(1), BlockId(2)),
                },
                BasicBlock {
                    id: BlockId(1),
                    params: vec![],
                    instructions: vec![
                        Instruction::Const(VarId(1), Literal::Integer(1)),
                    ],
                    terminator: Terminator::Return(VarId(1)),
                },
                BasicBlock {
                    id: BlockId(2),
                    params: vec![],
                    instructions: vec![
                        Instruction::Const(VarId(2), Literal::Integer(2)),
                    ],
                    terminator: Terminator::Return(VarId(2)),
                },
            ],
        };

        assert_valid(&func, "before standalone pass");
        eliminate_dead_code(&mut func, true_id, false_id);
        assert_valid(&func, "after standalone pass");

        assert!(
            matches!(&func.blocks[0].terminator, Terminator::Jump(BlockId(2), _)),
            "Branch on false should become Jump to else-block"
        );
        assert_eq!(func.blocks.len(), 2, "Unreachable then-block should be removed");
        assert!(!func.blocks.iter().any(|b| b.id == BlockId(1)));
    }

    #[test]
    fn test_dce_switch_constant_scrutinee() {
        let true_id = AtomId(100);
        let false_id = AtomId(101);
        let mut func = IrFunction {
            name: AtomId(0),
            arity: 0,
            mangled_name: "test".into(),
            protected_values: Vec::new(),
            entry_block: BlockId(0),
            params: vec![],
            line: 0,
            blocks: vec![
                BasicBlock {
                    id: BlockId(0),
                    params: vec![],
                    instructions: vec![
                        Instruction::Const(VarId(0), Literal::Integer(2)),
                    ],
                    terminator: Terminator::Switch {
                        scrutinee: VarId(0),
                        cases: vec![
                            (Literal::Integer(1), BlockId(1)),
                            (Literal::Integer(2), BlockId(2)),
                            (Literal::Integer(3), BlockId(3)),
                        ],
                        default: BlockId(4),
                    },
                },
                BasicBlock {
                    id: BlockId(1),
                    params: vec![],
                    instructions: vec![Instruction::Const(VarId(1), Literal::Integer(10))],
                    terminator: Terminator::Return(VarId(1)),
                },
                BasicBlock {
                    id: BlockId(2),
                    params: vec![],
                    instructions: vec![Instruction::Const(VarId(2), Literal::Integer(20))],
                    terminator: Terminator::Return(VarId(2)),
                },
                BasicBlock {
                    id: BlockId(3),
                    params: vec![],
                    instructions: vec![Instruction::Const(VarId(3), Literal::Integer(30))],
                    terminator: Terminator::Return(VarId(3)),
                },
                BasicBlock {
                    id: BlockId(4),
                    params: vec![],
                    instructions: vec![Instruction::Const(VarId(4), Literal::Integer(0))],
                    terminator: Terminator::Return(VarId(4)),
                },
            ],
        };

        assert_valid(&func, "before standalone pass");
        eliminate_dead_code(&mut func, true_id, false_id);
        assert_valid(&func, "after standalone pass");

        // Switch should resolve to Jump(BlockId(2)) since scrutinee == 2
        assert!(
            matches!(&func.blocks[0].terminator, Terminator::Jump(BlockId(2), _)),
            "Switch with constant scrutinee=2 should jump to matching case"
        );
        // Only entry block and the matching case block should remain
        assert_eq!(func.blocks.len(), 2);
        assert!(func.blocks.iter().any(|b| b.id == BlockId(2)));
    }

    #[test]
    fn test_dce_switch_falls_to_default() {
        let true_id = AtomId(100);
        let false_id = AtomId(101);
        let mut func = IrFunction {
            name: AtomId(0),
            arity: 0,
            mangled_name: "test".into(),
            protected_values: Vec::new(),
            entry_block: BlockId(0),
            params: vec![],
            line: 0,
            blocks: vec![
                BasicBlock {
                    id: BlockId(0),
                    params: vec![],
                    instructions: vec![
                        Instruction::Const(VarId(0), Literal::Integer(99)),
                    ],
                    terminator: Terminator::Switch {
                        scrutinee: VarId(0),
                        cases: vec![
                            (Literal::Integer(1), BlockId(1)),
                        ],
                        default: BlockId(2),
                    },
                },
                BasicBlock {
                    id: BlockId(1),
                    params: vec![],
                    instructions: vec![Instruction::Const(VarId(1), Literal::Integer(10))],
                    terminator: Terminator::Return(VarId(1)),
                },
                BasicBlock {
                    id: BlockId(2),
                    params: vec![],
                    instructions: vec![Instruction::Const(VarId(2), Literal::Integer(0))],
                    terminator: Terminator::Return(VarId(2)),
                },
            ],
        };

        assert_valid(&func, "before standalone pass");
        eliminate_dead_code(&mut func, true_id, false_id);
        assert_valid(&func, "after standalone pass");

        assert!(
            matches!(&func.blocks[0].terminator, Terminator::Jump(BlockId(2), _)),
            "Switch with no matching case should jump to default"
        );
        assert_eq!(func.blocks.len(), 2);
        assert!(!func.blocks.iter().any(|b| b.id == BlockId(1)));
    }

    #[test]
    fn test_dce_unreachable_blocks_removed() {
        let true_id = AtomId(100);
        let false_id = AtomId(101);
        // Block 2 is not reachable from entry (block 0 -> block 1 -> return)
        let mut func = IrFunction {
            name: AtomId(0),
            arity: 0,
            mangled_name: "test".into(),
            protected_values: Vec::new(),
            entry_block: BlockId(0),
            params: vec![],
            line: 0,
            blocks: vec![
                BasicBlock {
                    id: BlockId(0),
                    params: vec![],
                    instructions: vec![],
                    terminator: Terminator::Jump(BlockId(1), vec![]),
                },
                BasicBlock {
                    id: BlockId(1),
                    params: vec![],
                    instructions: vec![Instruction::Const(VarId(0), Literal::Integer(42))],
                    terminator: Terminator::Return(VarId(0)),
                },
                BasicBlock {
                    id: BlockId(2),
                    params: vec![],
                    instructions: vec![Instruction::Const(VarId(1), Literal::Integer(99))],
                    terminator: Terminator::Return(VarId(1)),
                },
            ],
        };

        assert_valid(&func, "before standalone pass");
        eliminate_dead_code(&mut func, true_id, false_id);
        assert_valid(&func, "after standalone pass");

        assert_eq!(func.blocks.len(), 2, "Unreachable block 2 should be removed");
        assert!(!func.blocks.iter().any(|b| b.id == BlockId(2)));
    }

    #[test]
    fn test_dce_integration_constant_case() {
        // Erlang source where case expression on a constant should be simplified
        let source = r#"
-module(test).
-export([f/0]).

f() ->
    X = 1,
    case X of
        1 -> yes;
        2 -> no
    end.
"#;
        let ir = lower_source(source).unwrap();
        let func = &ir.functions[0];
        // After constant folding + DCE, the number of blocks should be reduced
        // compared to a full case expansion. At minimum, verify we get a valid IR.
        // The function should return the atom 'yes'.
        assert!(
            !func.blocks.is_empty(),
            "Function should have at least one block after DCE"
        );
        // Check that DCE didn't break the IR — there should be no dangling references
        let block_ids: std::collections::HashSet<_> = func.blocks.iter().map(|b| b.id).collect();
        for block in &func.blocks {
            for succ in block.terminator.successors() {
                assert!(
                    block_ids.contains(&succ),
                    "Block {:?} references unreachable successor {:?}",
                    block.id, succ
                );
            }
        }
    }

/// Check each intermediate representation, not just the final lowering result.
pub(super) fn assert_valid(func: &IrFunction, stage: &str) {
    let errors = crate::verify::verify_function(func);
    assert!(errors.is_empty(), "{stage}: {errors:#?}");
}

#[test]
fn test_binary_rest_case_branch_state() {
    // lower_source also verifies definitions and definite initialization on
    // every CFG path, before code generation can turn an undefined rest into 0.
    lower_source(include_str!("../../../../tests/binary_rest_case.erl")).unwrap();
}

#[test]
fn test_catch_does_not_read_try_body_bindings() {
    // NERA-45 review F1: Nera's lint accepts these body/catch name reuses
    // (OTP rejects them as unsafe). A catch must bind its own X, not read a
    // body register that may be unset when the protected match fails.
    for pattern in [
        "X", "{badmatch, X}", "{badmatch, <<X:8>>}",
        "{badmatch, <<X/utf8>>}", "{badmatch, <<X:1/binary>>}",
        "{badmatch, <<X/binary>>}",
    ] {
        for class in ["error", "Class", "_"] {
            let source = format!(
                "-module(test). f(V) -> try {{ok, X}} = V catch {class}:{pattern} -> X end."
            );
            lower_source(&source).expect("catch bindings must not read try-body registers");
        }
    }
    lower_source(
        "-module(test). f(V) -> try {ok, Class} = V catch Class:R -> {Class, R} end."
    ).expect("catch class must not read a try-body register");
}

#[test]
fn test_try_catch_bound_vars_lowers() {
    // NERA-45: outer names in try-of and catch patterns must compare rather
    // than rebind, including live deferred tails and a shared class/reason name.
    lower_source(include_str!("../../../../tests/try_catch_bound_vars.erl"))
        .expect("bound try/catch pattern variables must lower to valid IR");
}

#[test]
fn binary_generator_reuses_cursor_without_copying_innermost_prefix() {
    // NERA-73 F1: unaligned tails must not be copied per element, and the
    // previously supported byte binary-comprehension needs no prefix copy.
    for body in ["[X || <<X:4>> <= B]", "<< <<X>> || <<X>> <= B >>"] {
        let ir = lower_source(&format!("-module(p). f(B) -> {body}.")).unwrap();
        let instructions: Vec<_> = ir.functions.iter()
            .flat_map(|f| &f.blocks).flat_map(|b| &b.instructions).collect();
        assert_eq!(instructions.iter().filter(|i|
            matches!(i, Instruction::BsMatchCtxAdvance(..))).count(), 1);
        assert!(!instructions.iter().any(|i| matches!(i, Instruction::BsMaterialize(..))),
            "innermost generator copied a prefix or tail: {body}");
    }
}

#[test]
fn binary_generator_rejects_unsized_fields_when_lowering() {
    // NERA-100 F1: embedded modules skip lint, and an unsized <= field
    // consumes the remainder, so the generator could match again without
    // advancing. Module lowering rejects every such pattern itself.
    let in_generator = "binary fields without size are not allowed in patterns of bit string generators";
    let not_last = "a binary segment without size is only allowed at the end of a binary pattern";
    for (source, message) in [
        ("-module(p). f(B) -> [X || <<X/binary>> <= B].", in_generator),
        ("-module(p). f(B) -> << X || <<X/bits>> <= B >>.", in_generator),
        ("-module(p). f(B) -> [X || <<X/binary, _>> <= B].", not_last),
        ("-module(p). f(B) -> (fun() -> [X || <<X/bitstring>> <= B] end)().", in_generator),
        ("-module(p). -record(r, {f = [X || <<X/binary>> <= <<>>]}). f() -> #r{}.", in_generator),
        // NERA-103 F1: these callers discard lowering errors, so the module
        // guard must reject the field before they run.
        ("-module(p). -record(r, {f = [X || <<X/binary>> <= <<>>]}). f() -> ok.", in_generator),
        ("-module(p). f(B) when [X || <<X/binary>> <= B] =:= [] -> ok.", in_generator),
        ("-module(p). f(<<N:([X || <<X/binary>> <= <<>>]), V:N>>) -> V.", in_generator),
        ("-module(p). f(#{[X || <<X/binary>> <= <<>>] := V}) -> V.", in_generator),
        ("-module(p). f(\"p\" ++ [<<V:([X || <<X/binary>> <= <<>>])>>]) -> V.", in_generator),
    ] {
        let error = lower_source(source).expect_err(source);
        assert!(matches!(error, LowerError::InvalidPattern(..)), "{source}: {error}");
        assert_eq!(error.to_string(), message, "{source}");
        assert!(error.span().is_some(), "{source}");
    }
    lower_source("-module(p). f(B) -> [X || <<X:1/binary, _/utf8>> <= B].")
        .expect("sized fields still lower");
}

#[test]
fn typed_string_patterns_are_refused_when_lowering() {
    // NERA-106: embedded modules skip lint, and lowering drops a pattern
    // string's size and reads "ab"/binary as rest captures, so module
    // lowering refuses OTP's typed_literal_string patterns itself.
    let typed = "a literal string in a binary pattern must not have a type or a size";
    for source in [
        r#"-module(p). f(<<"ab":16, R/binary>>) -> R."#,
        r#"-module(p). f(<<X, "ab"/binary>>) -> X."#,
        r#"-module(p). f(B) -> [X || <<"ab"/integer, X>> <= B]."#,
        r#"-module(p). -record(r, {a = fun(<<"ab":16>>) -> ok end}). f() -> ok."#,
    ] {
        let error = lower_source(source).expect_err(source);
        assert!(matches!(error, LowerError::InvalidPattern(..)), "{source}: {error}");
        assert_eq!(error.to_string(), typed, "{source}");
        assert!(error.span().is_some(), "{source}");
    }
    lower_source(r#"-module(p). f(<<"ab", "λ"/utf16, R/binary>>) -> R."#)
        .expect("default and UTF string patterns still lower");
}

#[test]
fn conflicting_binary_specifiers_are_refused_without_lint() {
    // NERA-112: embedded modules skip lint; guards, sizes and map keys can
    // discard lowering errors. None may silently compile a conflicting list.
    for source in [
        "-module(p). f(X) -> <<X/binary-utf8>>.",
        "-module(p). f(<<X/binary-utf8>>) -> X.",
        "-module(p). f(X) when <<X/binary-utf8>> =:= <<>> -> ok.",
        "-module(p). f(<<X:(<<1/binary-utf8>>)>>) -> X.",
        "-module(p). f(#{(<<1/binary-utf8>>) := X}) -> X.",
        "-module(p). -record(r, {a = <<1/binary-utf8>>}). f() -> ok.",
        "-module(p). f(B) -> [X || <<X/binary-utf8>> <= B].",
    ] {
        let error = lower_source(source).expect_err(source);
        assert_eq!(error.to_string(),
            "conflict in type specification for bit field: 'utf8' and 'binary'", "{source}");
        assert!(error.span().is_some(), "{source}");
    }
    lower_source(include_str!("../../../../tests/binary_segment_specifiers.erl"))
        .expect("accepted OTP specifier lists still lower without lint");
}

#[test]
fn test_binary_generator_lowers() {
    // NERA-48: complete <= patterns must consume pattern-sized prefixes in
    // both comprehension forms, including nested helpers and skip paths.
    lower_source(include_str!("../../../../tests/binary_generator.erl"))
        .expect("binary generators must lower to definitely initialized IR");
}

#[test]
fn test_binary_bound_segment_lowers() {
    // Regression (NERA-39): Cowlib cow_multipart matches
    // <<Boundary:BoundarySize/binary, "--", Stream2/bits>> with Boundary bound.
    // A bound segment value must not stop the fresh segments from binding.
    lower_source(include_str!("../../../../tests/binary_bound_segment.erl"))
        .expect("bound binary segment values must lower to valid IR");
}

#[test]
fn test_generator_size_rebind_lowers() {
    // NERA-52: the final binary-consumption branch must preserve initialized
    // generator bindings through guards, sub-patterns and helper continuations.
    lower_source(include_str!("../../../../tests/generator_size_rebind.erl"))
        .expect("generator size rebinding and exact binary ends must lower to valid IR");
}

#[test]
fn test_binary_rest_unit_lowers() {
    // NERA-49: the remainder-unit branch must preserve definite initialization
    // for fresh/bound tails, fallback clauses and dynamic offsets.
    lower_source(include_str!("../../../../tests/binary_rest_unit.erl"))
        .expect("binary rest unit checks must lower to valid IR");
}

#[test]
fn test_branch_export_reads_lowers() {
    // A name bound in a block or in every case/if/receive branch is read
    // after the expression; each branch must define the join's variable.
    lower_source(include_str!("../../../../tests/branch_export_reads.erl"))
        .expect("block and branch exports must lower to valid IR");
}

#[test]
fn unread_branch_tails_stay_deferred() {
    // A binary tail bound in every case branch is copied out of its match
    // context at the join only when code that sees the case's bindings may
    // use it later. Code review of the branch export change found each of
    // these unread forms materializing the tail: a fun head, a generator or
    // an earlier fun body's local with the same name, the same inside code
    // that lowering used to copy, a sibling clause of an enclosing
    // expression, a receive timeout, which runs before the clauses, and a
    // `maybe`'s `else` clause.
    let materialized = |body: &str| {
        let ir = lower_source(&format!("-module(t). f(B) -> {body}.")).unwrap();
        ir.functions.iter().flat_map(|f| &f.blocks).flat_map(|b| &b.instructions)
            .filter(|i| matches!(i, Instruction::BsMaterialize(..))).count()
    };
    let case = "case B of <<1, Rest/binary>> -> ok; <<2, Rest/binary>> -> ok end";
    let local = "F = fun() -> Rest = ok end";
    for body in [
        format!("F = fun(Rest) -> Rest end, {case}, F(ok)"),
        format!("F = fun Rest(0) -> Rest; Rest(_) -> 0 end, {case}, F(1)"),
        format!("{case}, F = fun(Rest) -> Rest end, F(ok)"),
        format!("{case}, [Rest || Rest <- [ok]]"),
        format!("{local}, {case}, F()"),
        format!("[X || X <- begin {local}, {case}, F(), [ok] end]"),
        format!("[X || _ <- [1], X <- begin {local}, {case}, F(), [ok] end]"),
        format!("<< <<X>> || <<X>> <= begin {local}, {case}, F(), <<1>> end >>"),
        format!("[X || X <- [ok], begin {local}, {case}, F() =:= ok end]"),
        format!("catch begin {local}, {case}, F() end"),
        format!("case self() of a -> {case}, ok; _ -> Rest = ok end, ok"),
        format!("if B =:= a -> {case}, ok; true -> Rest = ok end, ok"),
        format!("receive a -> {case}, ok; _ -> Rest = ok end, ok"),
        format!("try {case}, ok catch _:_ -> Rest = ok end, ok"),
        format!("try {case}, ok after Rest = ok end, ok"),
        format!("G = fun(a) -> {case}, ok; (_) -> Rest = ok end, G(a)"),
        format!("receive go -> {case}, ok after (fun() -> Rest = 0 end)() -> ok end"),
        format!("maybe {case}, {{ok, _}} ?= B, ok else _ -> Rest = ok end"),
    ] {
        assert_eq!(materialized(&body), 0, "an unread tail was materialized: {body}");
    }
    // Each of these uses the exported tail: a later read, a later fun body's
    // match, a match whose value holds the case, a read in the same copied
    // source or catch operand, an `of` clause, the rest of the same clause of
    // an enclosing case or the code after it, and a later `?=` in the same
    // `maybe` body.
    for body in [
        format!("{case}, Rest"),
        format!("{case}, {local}, F()"),
        format!("Rest = begin {case}, B end"),
        format!("[X || X <- begin {case}, binary_to_list(Rest) end]"),
        format!("catch begin {case}, Rest end"),
        format!("try {case} of ok -> Rest catch _:_ -> ok end"),
        format!("case self() of a -> {case}, Rest; _ -> ok end"),
        format!("case self() of a -> {case}; _ -> Rest = <<>> end, Rest"),
        format!("maybe {case}, {{ok, T}} ?= {{ok, Rest}}, T else _ -> none end"),
    ] {
        assert!(materialized(&body) > 0, "a later read must materialize the tail: {body}");
    }
}

#[test]
fn binary_pattern_prebinding_checks_constant_size_products() {
    // NERA-79: pre-binding must not overflow in debug builds or wrap in
    // release builds, and must keep the matcher's nonnegative 2^60 limit.
    let limit = crate::pattern::BINARY_PATTERN_SIZE_LIMIT;
    for (size, unit, expected) in [
        (0, 1, 0),
        (1, 1, 1),
        (i64::MAX, 1, limit),
        (0, 256, 0),
        (1, 8, 8),
        (2, 8, 16),
        (limit - 1, 1, limit - 1),
        (limit, 1, limit),
        (limit / 256, 256, limit),
        (limit - 1, 256, limit),
        (i64::MAX, 256, limit),
    ] {
        for typ in ["integer", "binary", "bitstring"] {
            // OTP fixes /bitstring to unit:1; larger explicit units conflict.
            if typ == "bitstring" && unit != 1 { continue; }
            let source = format!("-module(p). f(B) -> case B of <<V:{size}/{typ}-unit:{unit}>> -> V; _ -> fail end.");
            let ir = lower_source(&source).unwrap();
            let instructions: Vec<_> = ir.functions[0].blocks.iter()
                .flat_map(|block| &block.instructions).collect();
            let constants: HashMap<_, _> = instructions.iter().filter_map(|i| match i {
                Instruction::Const(var, Literal::Integer(n)) => Some((*var, *n)),
                _ => None,
            }).collect();
            let widths: Vec<_> = instructions.iter().filter_map(|i| match i {
                Instruction::BsGetIntegerInline(_, _, _, width, _)
                | Instruction::BsMaterialize(_, _, _, width) => constants.get(width),
                _ => None,
            }).collect();
            assert!(!widths.is_empty(), "missing pre-bound extraction: {source}");
            assert!(widths.iter().all(|&&width| width == expected),
                "wrong constant extraction widths {widths:?}, expected {expected}: {source}");
        }
    }
}

#[test]
fn binary_pattern_utf_minimum_sizes_do_not_overflow() {
    // NERA-79 / NERA-120 (review F1): UTF minimums must not overflow
    // after a large fixed prefix, and ordinary UTF minimums must stay exact.
    for (size, unit) in [
        (0_i64, 1_i64),
        (1, 8),
        (i64::MAX - 7, 1),
        ((1_i64 << 60) - 1, 8),
        (i64::MAX, 1),
    ] {
        for (typ, utf_bits) in [("utf8", 8), ("utf16", 16), ("utf32", 32)] {
            let source = format!("-module(p). f(B) -> case B of <<_:{size}/unit:{unit}, C/{typ}>> -> C; _ -> fail end.");
            let ir = lower_source(&source).unwrap();
            let instructions: Vec<_> = ir.functions[0].blocks.iter()
                .flat_map(|block| &block.instructions).collect();
            let expected = (i128::from(size) * i128::from(unit) + utf_bits)
                .min(i128::from(i64::MAX)) as i64;
            let constants: HashMap<_, _> = instructions.iter().filter_map(|i| match i {
                Instruction::Const(var, Literal::Integer(n)) => Some((*var, *n)),
                _ => None,
            }).collect();
            assert!(instructions.iter().any(|i| matches!(i,
                Instruction::BinOp(_, BinOp::GtEq | BinOp::ExactEq, _, minimum)
                    if constants.get(minimum) == Some(&expected)
            )), "missing non-wrapping UTF minimum {expected}: {source}");
        }
    }
}

#[test]
fn test_binary_tail_byte_size_lowers() {
    // NERA-76: rounding deferred tails must preserve definite initialization
    // through guards, dynamic offsets and match-context reuse.
    lower_source(include_str!("../../../../tests/binary_tail_byte_size.erl"))
        .expect("deferred tail byte sizes must lower to valid IR");
}

#[test]
fn imported_function_lowers_to_remote_call() {
    let ir = lower_source("-module(probe). -import(lists, [reverse/1]). -export([test/1]). test(X) -> reverse(X).").unwrap();
    assert!(ir.functions[0].blocks.iter().any(|block| matches!(
        block.terminator, Terminator::TailCall(CallTarget::Remote(_, _), _)
    )), "imported call must be remote, got {ir:?}");
}

#[test]
fn imported_bif_names_use_explicit_module() {
    let source = "-module(probe). -import(other, [float/1, self/0]). -export([test/1]). test(X) -> {float(X), self()}.";
    let ir = lower_source(source).unwrap();
    let mut parser = Parser::new(source);
    parser.parse_module();
    let other = parser.atom_names().into_iter().find(|(_, n)| n == "other").unwrap().0;
    let instructions: Vec<_> = ir.functions[0].blocks.iter().flat_map(|b| &b.instructions).collect();
    assert_eq!(instructions.iter().filter(|i| matches!(i,
        Instruction::Call(_, CallTarget::Remote(m, _), _) if *m == other
    )).count(), 2);

}

#[test]
fn conflicting_imports_are_rejected() {
    for source in [
        "-module(p). -import(a, [f/0]). -import(a, [f/0]). test() -> f().",
        "-module(p). -import(a, [f/0]). -import(b, [f/0]). test() -> f().",
        "-module(p). -import(a, [f/0]). f() -> ok.",
    ] {
        assert!(lower_source(source).unwrap_err().to_string().contains("conflicting import"));
    }
    lower_source("-module(p). -import(a, [f/0]). -import(b, [f/1]). test() -> {f(), f(1)}.").unwrap();
}

#[test]
fn imported_fun_reference_is_rejected_like_beam() {
    let error = lower_source("-module(p). -import(lists, [reverse/1]). test() -> fun reverse/1.").unwrap_err();
    assert!(error.to_string().contains("fun from imported"));
}

#[test]
fn imported_boolean_names_are_static_calls() {
    let ir = lower_source("-module(p). -import(other, [true/0, false/0]). test() -> {true(), false()}.").unwrap();
    assert_eq!(ir.functions[0].blocks.iter().flat_map(|b| &b.instructions).filter(|i| matches!(i, Instruction::Call(_, CallTarget::Remote(_, _), _))).count(), 2);
}

#[test]
fn nested_callee_expression_captures_comprehension_variable() {
    let source = "-module(p). -export([test/0]). test() -> [fun() -> (fun() -> Expected end)() end || Expected <- [42]].";
    let result = lower_source(source);
    assert!(result.is_ok(), "nested callee must capture Expected: {result:?}");
}

#[test]
fn binary_string_uses_codepoints_not_utf8_bytes() {
    let ir = lower_source("-module(p). test() -> <<\"🤔\">>.").unwrap();
    let integers: Vec<_> = ir.functions[0].blocks.iter().flat_map(|b| &b.instructions)
        .filter_map(|i| if let Instruction::Const(_, Literal::Integer(n)) = i { Some(*n) } else { None }).collect();
    assert!(integers.contains(&0x1f914), "missing string codepoint: {integers:?}");
}

#[test]
fn case_clause_reason_retains_scrutinee_in_each_context() {
    for body in [
        "case X of accepted -> ok end",
        "{wrapped, case X of accepted -> ok end}",
        "try case X of V when V =:= accepted -> ok end catch error:R -> R end",
    ] {
        let ir = lower_source(&format!("-module(p). f(X) -> {body}.")).unwrap();
        let function = &ir.functions[0];
        let scrutinee = function.params[0];
        let mut checked = 0;
        for block in &function.blocks {
            for instruction in &block.instructions {
                if let Instruction::RaiseException(_, ExceptionClass::Error, reason) = instruction {
                    assert!(block.instructions.iter().any(|i| matches!(i,
                        Instruction::MakeTuple(dst, fields)
                        if dst == reason && fields.len() == 2 && fields[1] == scrutinee
                    )), "case exception lost its scrutinee in {body}");
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 1, "expected the case failure path in {body}");
    }
}

#[test]
fn try_protection_is_lexical_not_forward_reachable() {
    let ir = lower_source("-module(test). -export([f/0]). f() -> g(), try g() catch throw:E -> E end, ok. g() -> ok.").unwrap();
    let f = &ir.functions[0];
    let calls: Vec<_> = f.blocks.iter().flat_map(|b| &b.instructions)
        .filter_map(|i| match i { Instruction::Call(v, CallTarget::Local(_), _) => Some(*v), _ => None }).collect();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls.iter().filter(|v| f.protected_values.contains(v)).count(), 1);
}

#[test]
fn protected_values_survive_binding_and_case() {
    let ir = lower_source("-module(test). -export([f/3]). f(R,B,O) -> try M = resolve_maps(to_map(R),to_map(B)), case O of [] -> render(M); [return_map] -> M end catch throw:E -> E end. to_map(X)->X. resolve_maps(A,_B)->A. render(X)->X.").unwrap();
    let f = &ir.functions[0];
    let calls: Vec<_> = f.blocks.iter().flat_map(|b| &b.instructions)
        .filter_map(|i| match i { Instruction::Call(v, CallTarget::Local(_), _) => Some(*v), _ => None }).collect();
    assert_eq!(calls.len(), 4);
    assert!(calls.iter().all(|v| f.protected_values.contains(v)), "{:?} != {:?}",calls,f.protected_values);
}

    #[test]
    fn match_context_forwarding_requires_binary_patterns_in_every_clause() {
        let ir = lower_source(r#"
-module(test).
-export([scan/2, loop/1, opaque/1, builtin/1, remote/1]).
scan(<<Byte, Rest/binary>>, Count) when Byte >= $0, Byte =< $9 ->
    scan(Rest, Count + 1);
scan(Rest, Count) -> {Count, Rest}.
loop(<<>>) -> ok;
loop(<<_, Rest/binary>>) -> loop(Rest).
opaque(<<_, Rest/binary>>) -> identity(Rest).
identity(Value) -> Value.
builtin(<<_, Rest/binary>>) -> term_to_binary(Rest).
remote(<<_, Rest/binary>>) -> erlang:term_to_binary(Rest).
"#).unwrap();
        // Only loop's callee always destructures the argument as a binary.
        // The scan fallback and opaque calls must receive a real binary.
        for (index, function) in ir.functions.iter().enumerate() {
            let advances = function.blocks.iter().flat_map(|block| &block.instructions)
                .filter(|instruction| matches!(instruction, Instruction::BsMatchCtxAdvance(_, _)))
                .count();
            assert_eq!(advances, usize::from(index == 1), "function {index}");
        }
    }

#[test]
fn binary_construction_rejects_invalid_units() {
    for unit in [0_i64, -1, 257, 4294967297] {
        let source = format!("-module(p). f(N) -> <<1:N/unit:{unit}>>.");
        assert!(lower_source(&source).is_err(), "accepted unit {unit}");
    }
    for unit in [1, 8, 256] {
        assert!(lower_source(&format!("-module(p). f(N) -> <<1:N/unit:{unit}>>.")).is_ok());
    }
}

#[test]
fn aliased_match_context_arguments_are_materialized_before_forwarding() {
    for arguments in ["T,T", "T,{T}", "{T},T"] {
        let ir = lower_source(&format!(r#"
-module(p).
f(<<_:8,T/bitstring>>) -> both({arguments}).
both(<<A:8,_/bitstring>>, <<B:8,_/bitstring>>) -> {{A,B}}.
"#)).unwrap();
        let advances = ir.functions[0].blocks.iter().flat_map(|b| &b.instructions)
            .filter(|i| matches!(i, Instruction::BsMatchCtxAdvance(..))).count();
        assert_eq!(advances, 0, "shared cursor escaped through {arguments}");
    }
}

#[test]
fn closures_materialize_deferred_binary_captures() {
    for body in [
        "fun() -> T end",
        "fun() -> {T,T} end",
        "fun() -> fun() -> T end end",
        "{fun() -> T end,T}",
        "fun Again(0) -> T; Again(N) -> Again(N-1) end",
    ] {
        let ir = lower_source(&format!("-module(p). f(<<_:3,T/bitstring>>) -> {body}.")).unwrap();
        let f = ir.functions.last().unwrap();
        let materializations = f.blocks.iter().flat_map(|b| &b.instructions)
            .filter(|i| matches!(i, Instruction::BsMaterialize(..))).count();
        assert_eq!(materializations,1,"capture must materialize once: {body}");
    }
}

#[test]
fn nested_functions_isolate_deferred_binary_registers() {
    let mut builder = IrBuilder::new(AtomId(0),AtomId(1),AtomId(2),AtomId(3),HashMap::new());
    let parent = MatchContextInfo { _base_ptr: VarId(1), bit_offset: VarId(2),
        remaining_bits: VarId(3), match_ctx: VarId(4) };
    builder.match_contexts.insert(VarId(0),parent);
    let saved = builder.save_state();
    assert!(builder.match_contexts.is_empty(),"nested register IDs must not inherit parent windows");
    builder.match_contexts.insert(VarId(0),MatchContextInfo { _base_ptr: VarId(5),
        bit_offset: VarId(6), remaining_bits: VarId(7), match_ctx: VarId(8) });
    builder.restore_state(saved);
    assert_eq!(builder.match_contexts.len(),1);
    assert_eq!(builder.match_contexts[&VarId(0)].match_ctx,VarId(4));
}

#[test]
fn shadowing_pattern_inputs_have_valid_dataflow() {
    // NERA-54: the runtime regression owns the BEAM results; this layer checks
    // that captured inputs and newly bound values never use undefined registers.
    lower_source(r#"
-module(pattern_input_scope).
fun_key(K, M) ->
    F = fun(#{a := K, K := V}) -> {K, V} end,
    F(M).
list_size(Sizes, Bins) -> [X || {N, _} <- Sizes, <<X:N, _/binary>> <- Bins].
binary_key(K, Maps) -> << <<V>> || #{K := V} <- Maps >>.
tail_read(Bins, Maps) -> [{K, V} || <<_, K/binary>> <- Bins, #{K := V} <- Maps].
"#).unwrap();
}
