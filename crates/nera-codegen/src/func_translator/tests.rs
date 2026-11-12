    use super::*;
    use nera_ir::ir::*;
    use nera_syntax::ast::{AtomId, BinOp};

    fn var(n: u32) -> VarId { VarId(n) }
    fn atom(n: u32) -> AtomId { AtomId(n) }

    // ---------------------------------------------------------------
    // build_erlang_signature
    // ---------------------------------------------------------------

    #[test]
    fn erlang_signature_arity_0() {
        let sig = build_erlang_signature(0);
        assert_eq!(sig.call_conv, CallConv::Tail);
        // params: just ctx pointer
        assert_eq!(sig.params.len(), 1);
        assert_eq!(sig.params[0].value_type, types::I64);
        // returns: one u64
        assert_eq!(sig.returns.len(), 1);
        assert_eq!(sig.returns[0].value_type, types::I64);
    }

    #[test]
    fn erlang_signature_arity_3() {
        let sig = build_erlang_signature(3);
        // params: ctx + 3 args
        assert_eq!(sig.params.len(), 4);
        for p in &sig.params {
            assert_eq!(p.value_type, types::I64);
        }
        assert_eq!(sig.returns.len(), 1);
    }

    #[test]
    fn erlang_signature_large_arity() {
        let sig = build_erlang_signature(20);
        assert_eq!(sig.params.len(), 21); // ctx + 20 args
    }

    // ---------------------------------------------------------------
    // collect_instruction_defs
    // ---------------------------------------------------------------

    #[test]
    fn defs_const() {
        let mut defs = HashSet::new();
        let inst = Instruction::Const(var(1), Literal::Integer(42));
        collect_instruction_defs(&inst, &mut defs);
        assert!(defs.contains(&var(1)));
        assert_eq!(defs.len(), 1);
    }

    #[test]
    fn defs_binop() {
        let mut defs = HashSet::new();
        let inst = Instruction::BinOp(var(3), BinOp::Add, var(1), var(2));
        collect_instruction_defs(&inst, &mut defs);
        assert!(defs.contains(&var(3)));
        assert_eq!(defs.len(), 1);
    }

    #[test]
    fn defs_make_tuple() {
        let mut defs = HashSet::new();
        let inst = Instruction::MakeTuple(var(5), vec![var(1), var(2), var(3)]);
        collect_instruction_defs(&inst, &mut defs);
        assert!(defs.contains(&var(5)));
        assert_eq!(defs.len(), 1);
    }

    #[test]
    fn defs_check_reductions_is_empty() {
        let mut defs = HashSet::new();
        collect_instruction_defs(&Instruction::CheckReductions, &mut defs);
        assert!(defs.is_empty());
    }

    #[test]
    fn defs_yield_check_is_empty() {
        let mut defs = HashSet::new();
        collect_instruction_defs(&Instruction::YieldCheck, &mut defs);
        assert!(defs.is_empty());
    }

    #[test]
    fn defs_clear_exception_is_empty() {
        let mut defs = HashSet::new();
        collect_instruction_defs(&Instruction::ClearException, &mut defs);
        assert!(defs.is_empty());
    }

    #[test]
    fn defs_binary_put_is_empty() {
        let mut defs = HashSet::new();
        let flags = BinFlags { signedness: 0, endianness: 0 };
        collect_instruction_defs(
            &Instruction::BinaryPutInteger(var(1), var(2), var(3), var(4), flags),
            &mut defs,
        );
        assert!(defs.is_empty());
    }

    #[test]
    fn defs_make_closure() {
        let mut defs = HashSet::new();
        let inst = Instruction::MakeClosure(var(10), "test_fn".into(), 2, vec![var(1), var(2)]);
        collect_instruction_defs(&inst, &mut defs);
        assert!(defs.contains(&var(10)));
        assert_eq!(defs.len(), 1);
    }

    #[test]
    fn defs_make_remote_fun_ref() {
        let mut defs = HashSet::new();
        let inst = Instruction::MakeRemoteFunRef(var(7), atom(1), atom(2), 1);
        collect_instruction_defs(&inst, &mut defs);
        assert!(defs.contains(&var(7)));
        assert_eq!(defs.len(), 1);
    }

    // ---------------------------------------------------------------
    // collect_instruction_uses
    // ---------------------------------------------------------------

    #[test]
    fn uses_const_is_empty() {
        let mut uses = HashSet::new();
        collect_instruction_uses(&Instruction::Const(var(1), Literal::Integer(42)), &mut uses);
        assert!(uses.is_empty());
    }

    #[test]
    fn uses_binop() {
        let mut uses = HashSet::new();
        let inst = Instruction::BinOp(var(3), BinOp::Add, var(1), var(2));
        collect_instruction_uses(&inst, &mut uses);
        assert!(uses.contains(&var(1)));
        assert!(uses.contains(&var(2)));
        assert!(!uses.contains(&var(3))); // result is not a use
        assert_eq!(uses.len(), 2);
    }

    #[test]
    fn uses_make_tuple() {
        let mut uses = HashSet::new();
        let inst = Instruction::MakeTuple(var(5), vec![var(1), var(2), var(3)]);
        collect_instruction_uses(&inst, &mut uses);
        assert!(uses.contains(&var(1)));
        assert!(uses.contains(&var(2)));
        assert!(uses.contains(&var(3)));
        assert!(!uses.contains(&var(5)));
        assert_eq!(uses.len(), 3);
    }

    #[test]
    fn uses_make_cons() {
        let mut uses = HashSet::new();
        let inst = Instruction::MakeCons(var(4), var(1), var(2));
        collect_instruction_uses(&inst, &mut uses);
        assert!(uses.contains(&var(1)));
        assert!(uses.contains(&var(2)));
        assert_eq!(uses.len(), 2);
    }

    #[test]
    fn uses_send() {
        let mut uses = HashSet::new();
        let inst = Instruction::Send(var(5), var(1), var(2));
        collect_instruction_uses(&inst, &mut uses);
        assert!(uses.contains(&var(1))); // pid
        assert!(uses.contains(&var(2))); // msg
        assert_eq!(uses.len(), 2);
    }

    #[test]
    fn uses_get_self_is_empty() {
        let mut uses = HashSet::new();
        collect_instruction_uses(&Instruction::GetSelf(var(1)), &mut uses);
        assert!(uses.is_empty());
    }

    #[test]
    fn uses_map_put() {
        let mut uses = HashSet::new();
        let inst = Instruction::MapPut(var(10), var(1), var(2), var(3));
        collect_instruction_uses(&inst, &mut uses);
        assert!(uses.contains(&var(1))); // map
        assert!(uses.contains(&var(2))); // key
        assert!(uses.contains(&var(3))); // value
        assert_eq!(uses.len(), 3);
    }

    #[test]
    fn uses_make_closure() {
        let mut uses = HashSet::new();
        let inst = Instruction::MakeClosure(var(10), "fn".into(), 2, vec![var(1), var(2)]);
        collect_instruction_uses(&inst, &mut uses);
        assert!(uses.contains(&var(1)));
        assert!(uses.contains(&var(2)));
        assert_eq!(uses.len(), 2);
    }

    #[test]
    fn uses_make_remote_fun_ref_is_empty() {
        let mut uses = HashSet::new();
        collect_instruction_uses(
            &Instruction::MakeRemoteFunRef(var(7), atom(1), atom(2), 1),
            &mut uses,
        );
        assert!(uses.is_empty());
    }

    #[test]
    fn uses_binary_put_integer() {
        let mut uses = HashSet::new();
        let flags = BinFlags { signedness: 0, endianness: 0 };
        let inst = Instruction::BinaryPutInteger(var(1), var(2), var(3), var(4), flags);
        collect_instruction_uses(&inst, &mut uses);
        assert_eq!(uses.len(), 4);
        assert!(uses.contains(&var(1)));
        assert!(uses.contains(&var(2)));
        assert!(uses.contains(&var(3)));
        assert!(uses.contains(&var(4)));
    }

    #[test]
    fn uses_binary_get_integer() {
        let mut uses = HashSet::new();
        let flags = BinFlags { signedness: 0, endianness: 0 };
        let inst = Instruction::BinaryGetInteger(var(10), var(1), var(2), var(3), flags);
        collect_instruction_uses(&inst, &mut uses);
        // bin, offset, size are uses; result (var 10) is not
        assert_eq!(uses.len(), 3);
        assert!(uses.contains(&var(1)));
        assert!(uses.contains(&var(2)));
        assert!(uses.contains(&var(3)));
        assert!(!uses.contains(&var(10)));
    }

    // ---------------------------------------------------------------
    // collect_terminator_uses
    // ---------------------------------------------------------------

    #[test]
    fn term_uses_return() {
        let mut uses = HashSet::new();
        collect_terminator_uses(&Terminator::Return(var(1)), &mut uses);
        assert!(uses.contains(&var(1)));
        assert_eq!(uses.len(), 1);
    }

    #[test]
    fn term_uses_jump() {
        let mut uses = HashSet::new();
        collect_terminator_uses(
            &Terminator::Jump(BlockId(1), vec![var(2), var(3)]),
            &mut uses,
        );
        assert!(uses.contains(&var(2)));
        assert!(uses.contains(&var(3)));
        assert_eq!(uses.len(), 2);
    }

    #[test]
    fn term_uses_branch() {
        let mut uses = HashSet::new();
        collect_terminator_uses(
            &Terminator::Branch(var(5), BlockId(1), BlockId(2)),
            &mut uses,
        );
        assert!(uses.contains(&var(5)));
        assert_eq!(uses.len(), 1);
    }

    #[test]
    fn term_uses_switch() {
        let mut uses = HashSet::new();
        collect_terminator_uses(
            &Terminator::Switch {
                scrutinee: var(7),
                cases: vec![(Literal::Integer(1), BlockId(1))],
                default: BlockId(2),
            },
            &mut uses,
        );
        assert!(uses.contains(&var(7)));
        assert_eq!(uses.len(), 1);
    }

    #[test]
    fn term_uses_tail_call() {
        let mut uses = HashSet::new();
        collect_terminator_uses(
            &Terminator::TailCall(CallTarget::Local(atom(1)), vec![var(2), var(3)]),
            &mut uses,
        );
        assert!(uses.contains(&var(2)));
        assert!(uses.contains(&var(3)));
        assert_eq!(uses.len(), 2);
    }

    #[test]
    fn term_uses_unreachable_is_empty() {
        let mut uses = HashSet::new();
        collect_terminator_uses(&Terminator::Unreachable, &mut uses);
        assert!(uses.is_empty());
    }

    // ---------------------------------------------------------------
    // terminator_successors (delegates to Terminator::successors())
    // ---------------------------------------------------------------

    #[test]
    fn successors_return() {
        assert!(terminator_successors(&Terminator::Return(var(1))).is_empty());
    }

    #[test]
    fn successors_jump() {
        let succs = terminator_successors(&Terminator::Jump(BlockId(5), vec![]));
        assert_eq!(succs, vec![BlockId(5)]);
    }

    #[test]
    fn successors_branch() {
        let succs = terminator_successors(&Terminator::Branch(var(1), BlockId(2), BlockId(3)));
        assert_eq!(succs, vec![BlockId(2), BlockId(3)]);
    }

    #[test]
    fn successors_switch() {
        let succs = terminator_successors(&Terminator::Switch {
            scrutinee: var(1),
            cases: vec![
                (Literal::Integer(0), BlockId(10)),
                (Literal::Integer(1), BlockId(11)),
            ],
            default: BlockId(99),
        });
        assert_eq!(succs, vec![BlockId(10), BlockId(11), BlockId(99)]);
    }

    #[test]
    fn successors_tail_call() {
        let succs = terminator_successors(
            &Terminator::TailCall(CallTarget::Local(atom(1)), vec![]),
        );
        assert!(succs.is_empty());
    }

    #[test]
    fn successors_unreachable() {
        assert!(terminator_successors(&Terminator::Unreachable).is_empty());
    }
