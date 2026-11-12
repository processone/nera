//! Record desugaring: create, access, index, update.

use std::collections::HashMap;
use nera_syntax::ast::{self, AtomId};
use nera_syntax::span::Span;
use crate::ir::*;
use super::{IrBuilder, LowerError};
use super::exports::LaterUses;

impl IrBuilder {
    // ---- Record helpers ----

    /// Look up a record definition by name. Returns an error if not found.
    pub(super) fn get_record_def(&self, record_name: AtomId, span: Span) -> Result<ast::RecordDef, LowerError> {
        self.record_def(record_name, span).cloned()
    }

    /// Borrow a record definition, or report the record as undefined at `span`.
    /// Used by record expressions and record patterns alike.
    pub(super) fn record_def(&self, record_name: AtomId, span: Span) -> Result<&ast::RecordDef, LowerError> {
        self.record_defs.get(&record_name).ok_or_else(|| {
            let name = self.atom_names.get(&record_name)
                .cloned()
                .unwrap_or_else(|| format!("{:?}", record_name));
            LowerError::UndefinedRecord(name, Some(span))
        })
    }

    /// Get the 0-based field index within a record definition.
    pub(super) fn get_record_field_index(
        &self,
        rec_def: &ast::RecordDef,
        field_name: AtomId,
        span: Span,
    ) -> Result<usize, LowerError> {
        rec_def.fields.iter().position(|f| f.name == field_name).ok_or_else(|| {
            let fname = self.atom_names.get(&field_name)
                .cloned()
                .unwrap_or_else(|| format!("{:?}", field_name));
            let rname = self.atom_names.get(&rec_def.name)
                .cloned()
                .unwrap_or_else(|| format!("{:?}", rec_def.name));
            LowerError::Unsupported(
                format!("undefined field '{}' in record '{}'", fname, rname),
                Some(span),
            )
        })
    }

    /// Lower `#record_name{field1 = Val1, ...}` to `MakeTuple([tag, Val1, Val2, ...])`.
    /// Unspecified fields use the default from the record definition, or `undefined`.
    pub(super) fn lower_record_create(
        &mut self,
        record_name: AtomId,
        fields: &[ast::RecordFieldAssign],
        span: Span,
    ) -> Result<VarId, LowerError> {
        let rec_def = self.get_record_def(record_name, span)?;

        // Build a map of field_name -> expression for quick lookup
        let field_map: HashMap<AtomId, &ast::Expr> = fields.iter()
            .map(|f| (f.field_name, &f.value))
            .collect();

        // Element 0: record tag atom
        let tag = self.emit_const(Literal::Atom(record_name));
        let mut elem_vars = vec![tag];

        // For each field in definition order
        for field_def in &rec_def.fields {
            let val = if let Some(expr) = field_map.get(&field_def.name) {
                self.lower_expr(expr, false)?
            } else if let Some(ref default_expr) = field_def.default {
                // The default belongs to the record definition: names it
                // binds must not reach the function that creates the record,
                // and what its own code reads decides what its branches export.
                let uses = std::mem::replace(&mut self.later_uses, LaterUses::of_expr(default_expr));
                let value = self.in_local_scope(|builder| builder.lower_expr(default_expr, false));
                self.later_uses = uses;
                value?
            } else {
                // No value and no default → atom 'undefined'
                let undef_id = self.atom_id_for("undefined");
                self.emit_const(Literal::Atom(undef_id))
            };
            elem_vars.push(val);
        }

        let result = self.fresh_var();
        self.emit(Instruction::MakeTuple(result, elem_vars));
        Ok(result)
    }

    /// Lower `Expr#record_name.field_name` to `TupleElement(base, field_index + 1)`.
    /// (+1 because index 0 is the record tag atom.)
    pub(super) fn lower_record_access(
        &mut self,
        base: &ast::Expr,
        record_name: AtomId,
        field_name: AtomId,
        span: Span,
    ) -> Result<VarId, LowerError> {
        let rec_def = self.get_record_def(record_name, span)?;
        let idx = self.get_record_field_index(&rec_def, field_name, span)?;
        let base_var = self.lower_expr(base, false)?;
        let result = self.fresh_var();
        // tuple index = field_index + 1 (element 0 is the tag)
        self.emit(Instruction::TupleElement(result, base_var, (idx + 1) as u32));
        Ok(result)
    }

    /// Lower `#record_name.field_name` to the integer field index (Erlang 1-based:
    /// element 1 is the tag, so the first field is index 2).
    pub(super) fn lower_record_index(
        &mut self,
        record_name: AtomId,
        field_name: AtomId,
        span: Span,
    ) -> Result<VarId, LowerError> {
        let rec_def = self.get_record_def(record_name, span)?;
        let idx = self.get_record_field_index(&rec_def, field_name, span)?;
        // Erlang convention: 1-based, element 1 is tag → first field is index 2
        Ok(self.emit_const(Literal::Integer((idx + 2) as i64)))
    }

    /// Lower `Expr#record_name{field1 = Val1, ...}` — extract-and-rebuild strategy.
    /// Extract all fields from the original, substitute the updated ones, and
    /// build a new tuple.
    pub(super) fn lower_record_update(
        &mut self,
        base: &ast::Expr,
        record_name: AtomId,
        fields: &[ast::RecordFieldAssign],
        span: Span,
    ) -> Result<VarId, LowerError> {
        let rec_def = self.get_record_def(record_name, span)?;
        let base_var = self.lower_expr(base, false)?;

        // Build a map of field_name -> expression for updated fields
        let field_map: HashMap<AtomId, &ast::Expr> = fields.iter()
            .map(|f| (f.field_name, &f.value))
            .collect();

        // Element 0: record tag (extract from original)
        let tag = self.fresh_var();
        self.emit(Instruction::TupleElement(tag, base_var, 0));
        let mut elem_vars = vec![tag];

        // For each field: if updated → lower new value, else → extract from original
        for (i, field_def) in rec_def.fields.iter().enumerate() {
            let val = if let Some(expr) = field_map.get(&field_def.name) {
                self.lower_expr(expr, false)?
            } else {
                let extracted = self.fresh_var();
                self.emit(Instruction::TupleElement(extracted, base_var, (i + 1) as u32));
                extracted
            };
            elem_vars.push(val);
        }

        let result = self.fresh_var();
        self.emit(Instruction::MakeTuple(result, elem_vars));
        Ok(result)
    }

}
