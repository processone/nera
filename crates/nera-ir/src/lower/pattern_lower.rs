//! Pattern lowering: variable pre-binding, destructuring, record desugaring.

use std::collections::{HashMap, HashSet};
use nera_syntax::ast::{self, AtomId, Endianness, Signedness};
use nera_syntax::span::Span;
use crate::ir::*;
use crate::pattern::{checked_pattern_segment_bits, BINARY_PATTERN_SIZE_LIMIT};
use super::{IrBuilder, LowerError, MatchContextInfo};

/// Bindings and deferred tails from before a branch; see `enter_branch`.
pub(super) struct BranchState {
    bindings: HashMap<AtomId, VarId>,
    match_contexts: HashMap<VarId, MatchContextInfo>,
}

impl IrBuilder {
    /// Separate a shadowing pattern's inputs from the values it binds.
    pub(super) fn with_pattern_inputs<T>(
        &mut self,
        inputs: HashMap<AtomId, VarId>,
        lower: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let saved = self.pattern_inputs.replace(inputs);
        let result = lower(self);
        self.pattern_inputs = saved;
        result
    }

    /// A captured input is not a pre-bound destination for a fresh segment.
    pub(crate) fn lookup_pattern_var(&self, name: AtomId) -> Option<VarId> {
        self.lookup_var(name).filter(|var| {
            self.pattern_inputs.as_ref().and_then(|inputs| inputs.get(&name)) != Some(var)
        })
    }

    /// Keys read the inputs before the pattern. A size also reads earlier
    /// segments of its binary, but not other values in the surrounding pattern.
    pub(crate) fn lower_pattern_expr(
        &mut self,
        expr: &ast::Expr,
        earlier_segments: &HashSet<AtomId>,
    ) -> Result<VarId, LowerError> {
        let Some(mut inputs) = self.pattern_inputs.clone() else {
            return self.lower_expr(expr, false);
        };
        for name in earlier_segments {
            if let Some(var) = self.lookup_var(*name) {
                inputs.insert(*name, var);
            }
        }
        let scopes = std::mem::replace(&mut self.scopes, vec![inputs]);
        let result = self.lower_expr(expr, false);
        self.scopes = scopes;
        result
    }

    /// Pre-bind pattern variables to VarIds before lowering the body.
    /// This walks the pattern tree and binds each variable name to the
    /// appropriate VarId (either the parameter directly, or a destructured element).
    /// Emits destructuring instructions for nested patterns.
    /// Pre-bind variables from a pattern.
    ///
    /// When `binary_bind_only` is true, binary patterns only create VarId bindings
    /// without emitting extraction instructions. This is used for match expressions
    /// where the body runs in a continuation block AFTER compile_match, so the
    /// pattern compiler's Move instructions define the variables via SSA.
    pub(super) fn pre_bind_pattern_vars(
        &mut self,
        pattern: &ast::Pattern,
        val: VarId,
        binary_bind_only: bool,
    ) {
        // Keys and sizes read deferred tails without consuming them, as in
        // the pattern compiler (see `matching`).
        let outer_matching = std::mem::replace(&mut self.matching, true);
        match pattern {
            ast::Pattern::Variable(name, _) => {
                self.bind_var(*name, val);
            }
            ast::Pattern::Tuple(elems, _) => {
                for (i, elem) in elems.iter().enumerate() {
                    let elem_var = self.fresh_var();
                    self.emit(Instruction::TupleElement(elem_var, val, i as u32));
                    self.pre_bind_pattern_vars(elem, elem_var, binary_bind_only);
                }
            }
            ast::Pattern::Cons(head, tail, _) => {
                let head_var = self.fresh_var();
                self.emit(Instruction::ListHead(head_var, val));
                self.pre_bind_pattern_vars(head, head_var, binary_bind_only);

                let tail_var = self.fresh_var();
                self.emit(Instruction::ListTail(tail_var, val));
                self.pre_bind_pattern_vars(tail, tail_var, binary_bind_only);
            }
            ast::Pattern::Match(p1, p2, _) => {
                self.pre_bind_pattern_vars(p1, val, binary_bind_only);
                self.pre_bind_pattern_vars(p2, val, binary_bind_only);
            }
            ast::Pattern::Binary(segments, _) => {
                // The pattern compiler binds into a scope it discards, so the
                // body needs its own definitions of the segment variables.
                // A variable that is already bound (outside the pattern, or
                // earlier in it) keeps its binding: the pattern compiler checks
                // it for equality. Only the remaining, fresh variables are bound.

                let expanded = crate::pattern::expand_binary_pattern_strings(segments);

                // The name of a segment that binds a fresh variable.
                let fresh_name = |builder: &Self, seg: &ast::BinSegment<ast::Pattern>| match &seg.value {
                    ast::Pattern::Variable(name, _) if builder.lookup_pattern_var(*name).is_none() => Some(*name),
                    _ => None,
                };

                if binary_bind_only {
                    // Match expressions: body runs in cont_block AFTER compile_match.
                    // Just create VarIds and bind names — the pattern compiler's
                    // Move/match_context instructions define them via SSA CFG.
                    for seg in &expanded {
                        if let Some(name) = fresh_name(self, seg) {
                            let var = self.fresh_var();
                            self.bind_var(name, var);
                        }
                    }
                } else {
                    // Case/receive/try: body lowered immediately, needs defined VarIds.
                    // Emit full extraction instructions. All offsets/sizes in BITS.
                    let mc_var = self.fresh_var();
                    let mc_base_ptr = self.fresh_var();
                    let bin_total_bits = self.fresh_var();
                    self.emit(Instruction::BsMatchStart2(mc_var, mc_base_ptr, bin_total_bits, val));
                    let initial_bit_offset = self.fresh_var();
                    self.emit(Instruction::BsMatchBitOffset(initial_bit_offset, val));

                    let mut bit_offset_var = self.emit_const(Literal::Integer(0));

                    let mut earlier_segments = HashSet::new();
                    for seg in &expanded {
                        let unit = Self::effective_unit(&seg.type_spec);
                        match seg.type_spec.typ {
                            ast::BinSegmentKind::Binary | ast::BinSegmentKind::Bitstring if seg.size.is_none() => {
                                // Rest segment: compute remaining bits, register match context
                                if let Some(name) = fresh_name(self, seg) {
                                    let remaining_bits = self.fresh_var();
                                    self.emit(Instruction::BinOp(
                                        remaining_bits,
                                        nera_syntax::ast::BinOp::Sub,
                                        bin_total_bits,
                                        bit_offset_var,
                                    ));
                                    let rest_var = self.fresh_var();
                                    self.bind_var(name, rest_var);
                                    self.match_contexts.insert(rest_var, MatchContextInfo {
                                        _base_ptr: mc_base_ptr,
                                        bit_offset: bit_offset_var,
                                        remaining_bits,
                                        match_ctx: mc_var,
                                    });
                                }
                            }
                            ast::BinSegmentKind::Utf8 | ast::BinSegmentKind::Utf16 | ast::BinSegmentKind::Utf32 => {
                                // UTF segment: use BsGetUtf* instruction
                                let flags = BinFlags {
                                    signedness: 0,
                                    endianness: match seg.type_spec.endianness {
                                        ast::Endianness::Big => 0,
                                        ast::Endianness::Little => 1,
                                        ast::Endianness::Native => 2,
                                    },
                                };
                                let codepoint = self.fresh_var();
                                let bits_consumed = self.fresh_var();
                                // UTF raw readers share integer extraction's
                                // containing-byte base for reused cursors.
                                let extraction_offset = self.fresh_var();
                                self.emit(Instruction::BinOp(extraction_offset,
                                    ast::BinOp::Add, bit_offset_var, initial_bit_offset));
                                let extraction_end = self.fresh_var();
                                self.emit(Instruction::BinOp(extraction_end,
                                    ast::BinOp::Add, bin_total_bits, initial_bit_offset));
                                match seg.type_spec.typ {
                                    ast::BinSegmentKind::Utf8 => {
                                        self.emit(Instruction::BsGetUtf8(
                                            codepoint, bits_consumed, mc_base_ptr, extraction_offset, extraction_end,
                                        ));
                                    }
                                    ast::BinSegmentKind::Utf16 => {
                                        self.emit(Instruction::BsGetUtf16(
                                            codepoint, bits_consumed, mc_base_ptr, extraction_offset, extraction_end, flags,
                                        ));
                                    }
                                    ast::BinSegmentKind::Utf32 => {
                                        self.emit(Instruction::BsGetUtf32(
                                            codepoint, bits_consumed, mc_base_ptr, extraction_offset, extraction_end, flags,
                                        ));
                                    }
                                    _ => unreachable!(),
                                }
                                // Bind variable to codepoint
                                if let Some(name) = fresh_name(self, seg) {
                                    self.bind_var(name, codepoint);
                                }
                                // Advance bit offset by actual bits consumed
                                let new_offset = self.fresh_var();
                                self.emit(Instruction::BinOp(
                                    new_offset,
                                    nera_syntax::ast::BinOp::Add,
                                    bit_offset_var,
                                    bits_consumed,
                                ));
                                bit_offset_var = new_offset;
                            }
                            _ => {
                                // Integer/float/binary segment: compute total_bits = size * unit
                                let seg_total_bits = match &seg.size {
                                    Some(size_expr) => {
                                        if let ast::Expr::Integer(n, _) = size_expr.as_ref() {
                                            // Match the pattern compiler's checked size range.
                                            // An impossible size never reaches this body; use
                                            // the same 2^60 cap as the matcher's fixed tails.
                                            let bits = checked_pattern_segment_bits(*n, unit)
                                                .unwrap_or(BINARY_PATTERN_SIZE_LIMIT);
                                            self.emit_const(Literal::Integer(bits))
                                        } else {
                                            // dynamic size — evaluate the expression
                                            let size_var = self.lower_pattern_expr(size_expr, &earlier_segments).unwrap_or_else(|_| {
                                                self.emit_const(Literal::Integer(8))
                                            });
                                            if unit == 1 {
                                                size_var
                                            } else {
                                                let unit_var = self.emit_const(Literal::Integer(unit));
                                                let product = self.fresh_var();
                                                self.emit(Instruction::BinOp(
                                                    product,
                                                    nera_syntax::ast::BinOp::Mul,
                                                    size_var,
                                                    unit_var,
                                                ));
                                                product
                                            }
                                        }
                                    }
                                    None => self.emit_const(Literal::Integer(
                                        match seg.type_spec.typ {
                                            ast::BinSegmentKind::Integer => 8,
                                            ast::BinSegmentKind::Float => 64,
                                            _ => 8,
                                        } * unit,
                                    )),
                                };

                                let flags = BinFlags {
                                    signedness: match seg.type_spec.signedness {
                                        ast::Signedness::Unsigned => 0,
                                        ast::Signedness::Signed => 1,
                                    },
                                    endianness: match seg.type_spec.endianness {
                                        ast::Endianness::Big => 0,
                                        ast::Endianness::Little => 1,
                                        ast::Endianness::Native => 2,
                                    },
                                };

                                let extracted = self.fresh_var();
                                let is_float_seg = seg.type_spec.typ == ast::BinSegmentKind::Float;
                                let is_binary_seg = matches!(seg.type_spec.typ, ast::BinSegmentKind::Binary | ast::BinSegmentKind::Bitstring);
                                if is_float_seg {
                                    self.emit(Instruction::BinaryGetFloat(
                                        extracted, val, bit_offset_var, seg_total_bits, flags,
                                    ));
                                } else if is_binary_seg {
                                    // Accept both a binary and a reused match
                                    // context, including its current offset.
                                    self.emit(Instruction::BsMaterialize(
                                        extracted, val, bit_offset_var, seg_total_bits,
                                    ));
                                } else {
                                    // BsGetIntegerInline now takes bit_offset and bit_count
                                    let extraction_offset = self.fresh_var();
                                    self.emit(Instruction::BinOp(extraction_offset,
                                        nera_syntax::ast::BinOp::Add, bit_offset_var, initial_bit_offset));
                                    self.emit(Instruction::BsGetIntegerInline(
                                        extracted, mc_base_ptr, extraction_offset, seg_total_bits, flags,
                                    ));
                                }

                                // Bind variable and register match context for binary segments
                                if let Some(name) = fresh_name(self, seg) {
                                    self.bind_var(name, extracted);
                                    if is_binary_seg {
                                        self.match_contexts.insert(extracted, MatchContextInfo {
                                            _base_ptr: mc_base_ptr,
                                            bit_offset: bit_offset_var,
                                            remaining_bits: seg_total_bits,
                                            match_ctx: mc_var,
                                        });
                                    }
                                }

                                // Advance bit offset
                                let new_offset = self.fresh_var();
                                self.emit(Instruction::BinOp(
                                    new_offset,
                                    nera_syntax::ast::BinOp::Add,
                                    bit_offset_var,
                                    seg_total_bits,
                                ));
                                bit_offset_var = new_offset;
                            }
                        }
                        self.collect_pattern_bindings(&seg.value, &mut earlier_segments);
                    }
                }
            }
            ast::Pattern::Map(fields, _) => {
                // Emit MapGet instructions into the body block so variables
                // are defined. The pattern compiler separately handles type
                // checks and branching on failure.
                for field in fields {
                    let key_var = self.lower_pattern_expr(&field.key, &HashSet::new()).unwrap_or_else(|_| {
                        self.emit_const(Literal::Nil)
                    });
                    let val_var = self.fresh_var();
                    self.emit(Instruction::MapGet(val_var, val, key_var));

                    match &field.value {
                        ast::Pattern::Variable(name, _) => {
                            self.bind_var(*name, val_var);
                        }
                        other => self.pre_bind_pattern_vars(other, val_var, binary_bind_only),
                    }
                }
            }
            ast::Pattern::Record { .. } => {
                // Record patterns should be desugared before reaching here.
                // Desugar on the fly as a safety fallback. An undefined record
                // binds nothing: callers report it when they desugar the
                // pattern for the pattern compiler.
                if let Ok(desugared) = self.desugar_record_pattern(pattern) {
                    self.pre_bind_pattern_vars(&desugared, val, binary_bind_only);
                }
            }
            ast::Pattern::Concat(lhs, rhs, _) => {
                // String prefix match: "prefix" ++ Rest
                // Desugar to nested Cons cells and pre-bind the resulting pattern.
                if let ast::Pattern::String(s, span) = lhs.as_ref() {
                    let desugared = Self::string_to_cons_pattern_with_tail(s, *span, rhs);
                    self.pre_bind_pattern_vars(&desugared, val, binary_bind_only);
                }
            }
            // Literal patterns and wildcards don't bind variables
            ast::Pattern::Wildcard(_)
            | ast::Pattern::Integer(_, _)
            | ast::Pattern::BigInteger(_, _)
            | ast::Pattern::Float(_, _)
            | ast::Pattern::Atom(_, _)
            | ast::Pattern::String(_, _)
            | ast::Pattern::Nil(_) => {}
            ast::Pattern::MacroCall { .. } => unreachable!("MacroCall only in formatter path"),
        }
        self.matching = outer_matching;
    }

    /// Point names bound earlier back at their outer variables. After a `=`
    /// or `?=` pattern is pre-bound, its names read copies defined only once
    /// the match succeeds, but the pattern compiler, which runs in this scope,
    /// compares them and reads them in key and size expressions before then.
    pub(super) fn keep_outer_bindings(&mut self, outer_bindings: &HashMap<AtomId, VarId>) {
        for (&name, &var) in outer_bindings {
            if self.lookup_var(name) != Some(var) {
                self.bind_var(name, var);
            }
        }
    }

    /// Record what code outside a branch that may not run relies on: the
    /// variable each name bound so far reads, and the deferred binary tails.
    /// Reading a tail inside the branch rebinds its name to a term defined
    /// only there and consumes its record. `None` when no tail is deferred.
    pub(super) fn enter_branch(&self) -> Option<BranchState> {
        if self.match_contexts.is_empty() {
            return None;
        }
        Some(BranchState {
            bindings: self.collect_all_bindings(),
            match_contexts: self.match_contexts.clone(),
        })
    }

    /// Undo, for the code after a branch (a sibling branch or the join), what
    /// reading a deferred tail inside it changed: names bound before it read
    /// their outer variables again, and their tail records are restored.
    pub(super) fn leave_branch(&mut self, state: &Option<BranchState>) {
        if let Some(state) = state {
            self.keep_outer_bindings(&state.bindings);
            for (&var, info) in &state.match_contexts {
                self.match_contexts.entry(var).or_insert_with(|| info.clone());
            }
        }
    }

    /// Lower code whose bindings stay inside it, such as a comprehension's
    /// first generator source. A deferred tail it reads is rebound only in
    /// its scope, so the code after it reads that tail as after a branch.
    pub(super) fn in_local_scope<T>(&mut self, lower: impl FnOnce(&mut Self) -> T) -> T {
        let branch = self.enter_branch();
        self.push_scope();
        let result = lower(self);
        self.pop_scope();
        self.leave_branch(&branch);
        result
    }

    /// Bind variables from a pattern against a value.
    pub(super) fn bind_pattern(
        &mut self,
        pattern: &ast::Pattern,
        val: VarId,
    ) -> Result<(), LowerError> {
        match pattern {
            ast::Pattern::Wildcard(_) => Ok(()),
            ast::Pattern::Variable(name, _) => {
                self.bind_var(*name, val);
                Ok(())
            }
            ast::Pattern::Tuple(elems, _) => {
                for (i, elem) in elems.iter().enumerate() {
                    let elem_var = self.fresh_var();
                    self.emit(Instruction::TupleElement(elem_var, val, i as u32));
                    self.bind_pattern(elem, elem_var)?;
                }
                Ok(())
            }
            ast::Pattern::Cons(head, tail, _) => {
                let head_var = self.fresh_var();
                self.emit(Instruction::ListHead(head_var, val));
                self.bind_pattern(head, head_var)?;

                let tail_var = self.fresh_var();
                self.emit(Instruction::ListTail(tail_var, val));
                self.bind_pattern(tail, tail_var)?;
                Ok(())
            }
            ast::Pattern::Match(p1, p2, _) => {
                self.bind_pattern(p1, val)?;
                self.bind_pattern(p2, val)?;
                Ok(())
            }
            ast::Pattern::Binary(segments, _) => {
                for seg in segments {
                    if let ast::Pattern::Variable(name, _) = &seg.value {
                        let extracted = self.fresh_var();
                        self.bind_var(*name, extracted);
                    }
                }
                Ok(())
            }
            ast::Pattern::Map(fields, _) => {
                for field in fields {
                    match &field.value {
                        ast::Pattern::Variable(name, _) => {
                            let extracted = self.fresh_var();
                            self.bind_var(*name, extracted);
                        }
                        other => {
                            let dummy = self.fresh_var();
                            self.bind_pattern(other, dummy)?;
                        }
                    }
                }
                Ok(())
            }
            ast::Pattern::Record { .. } => {
                // Record patterns should be desugared before reaching here.
                let desugared = self.desugar_record_pattern(pattern)?;
                self.bind_pattern(&desugared, val)
            }
            ast::Pattern::Concat(lhs, rhs, _) => {
                // String prefix match: "prefix" ++ Rest
                if let ast::Pattern::String(s, span) = lhs.as_ref() {
                    let desugared = Self::string_to_cons_pattern_with_tail(s, *span, rhs);
                    self.bind_pattern(&desugared, val)
                } else {
                    Ok(())
                }
            }
            // Literal patterns don't bind variables (matching is done elsewhere)
            ast::Pattern::Integer(_, _)
            | ast::Pattern::BigInteger(_, _)
            | ast::Pattern::Float(_, _)
            | ast::Pattern::Atom(_, _)
            | ast::Pattern::String(_, _)
            | ast::Pattern::Nil(_) => Ok(()),
            ast::Pattern::MacroCall { .. } => unreachable!("MacroCall only in formatter path"),
        }
    }

    /// Desugar a string literal into nested `Pattern::Cons` cells with a
    /// given tail pattern. Used for `"prefix" ++ Rest` pattern desugaring.
    pub(super) fn string_to_cons_pattern_with_tail(s: &str, span: Span, tail: &ast::Pattern) -> ast::Pattern {
        let mut result = tail.clone();
        for ch in s.chars().rev() {
            result = ast::Pattern::Cons(
                Box::new(ast::Pattern::Integer(ch as i64, span)),
                Box::new(result),
                span,
            );
        }
        result
    }

    /// Desugar `Pattern::Record` into `Pattern::Tuple` recursively.
    ///
    /// `#person{name = N}` → `{person, N, _}` where unspecified fields become
    /// wildcards, and the first element is the record tag atom.
    ///
    /// A record with no definition is an `UndefinedRecord` error located at
    /// that record pattern, so the result never contains `Pattern::Record`.
    pub(super) fn desugar_record_pattern(&self, pat: &ast::Pattern) -> Result<ast::Pattern, LowerError> {
        Ok(match pat {
            ast::Pattern::Record { record_name, fields, span } => {
                let rec_def = self.record_def(*record_name, *span)?;
                // Build field_name -> pattern map
                let field_map: HashMap<AtomId, &ast::Pattern> = fields.iter()
                    .map(|f| (f.field_name, &f.pattern))
                    .collect();

                let mut tuple_elems = Vec::with_capacity(rec_def.fields.len() + 1);
                // Element 0: match the record tag
                tuple_elems.push(ast::Pattern::Atom(*record_name, *span));

                // For each field: if pattern specified → use it (recursively desugar),
                // else → wildcard
                for field_def in &rec_def.fields {
                    if let Some(sub_pat) = field_map.get(&field_def.name) {
                        tuple_elems.push(self.desugar_record_pattern(sub_pat)?);
                    } else {
                        tuple_elems.push(ast::Pattern::Wildcard(*span));
                    }
                }

                ast::Pattern::Tuple(tuple_elems, *span)
            }
            // Recursively desugar patterns that contain sub-patterns
            ast::Pattern::Tuple(elems, span) => {
                let desugared = elems.iter()
                    .map(|p| self.desugar_record_pattern(p))
                    .collect::<Result<Vec<_>, _>>()?;
                ast::Pattern::Tuple(desugared, *span)
            }
            ast::Pattern::Cons(head, tail, span) => {
                ast::Pattern::Cons(
                    Box::new(self.desugar_record_pattern(head)?),
                    Box::new(self.desugar_record_pattern(tail)?),
                    *span,
                )
            }
            ast::Pattern::Match(p1, p2, span) => {
                ast::Pattern::Match(
                    Box::new(self.desugar_record_pattern(p1)?),
                    Box::new(self.desugar_record_pattern(p2)?),
                    *span,
                )
            }
            ast::Pattern::Map(fields, span) => {
                ast::Pattern::Map(fields.iter().map(|field| Ok(ast::MapPatternField {
                    key: field.key.clone(),
                    value: self.desugar_record_pattern(&field.value)?,
                    span: field.span,
                })).collect::<Result<Vec<_>, LowerError>>()?, *span)
            }
            ast::Pattern::Concat(lhs, rhs, span) => {
                ast::Pattern::Concat(
                    Box::new(self.desugar_record_pattern(lhs)?),
                    Box::new(self.desugar_record_pattern(rhs)?),
                    *span,
                )
            }
            // Binary segments cannot contain record values in valid Erlang patterns.
            // Other patterns are leaves (or formatter-only macros).
            _ => pat.clone(),
        })
    }

    // ---- Binary helpers ----

    /// Convert an AST `BinSegmentType` into IR `BinFlags`.
    pub(super) fn ast_type_spec_to_bin_flags(&self, spec: &ast::BinSegmentType) -> BinFlags {
        BinFlags {
            signedness: match spec.signedness {
                Signedness::Unsigned => 0,
                Signedness::Signed => 1,
            },
            endianness: match spec.endianness {
                Endianness::Big => 0,
                Endianness::Little => 1,
                Endianness::Native => 2,
            },
        }
    }

}
