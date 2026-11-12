//! Atom resolution and loading.

use cranelift_codegen::ir::{types, InstBuilder, MemFlags, Value};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;
use cranelift_object::ObjectModule;

use crate::term_layout::*;
use super::{FuncTranslator, TranslateError};

impl FuncTranslator {
    pub(super) fn resolve_atom(&self, id: u32) -> Option<&str> {
        self.atom_table.get(&id).map(|s: &String| s.as_str())
    }

    /// Load a runtime atom term from the global atom lookup table.
    /// Falls back to inline encode_atom() if the global table is not set.
    pub(super) fn load_atom_term(
        &self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        parser_atom_id: u32,
    ) -> Result<Value, TranslateError> {
        if let Some(data_id) = self.atom_table_data_id {
            let gv = module.declare_data_in_func(data_id, builder.func);
            let base = builder.ins().global_value(types::I64, gv);
            let offset = (parser_atom_id as i32) * 8;
            let val = builder.ins().load(types::I64, MemFlags::trusted(), base, offset);
            Ok(val)
        } else {
            // Fallback: inline encoding (only works if atom IDs match runtime)
            Ok(builder.ins().iconst(types::I64, encode_atom(parser_atom_id) as i64))
        }
    }

    /// Load the runtime `true` atom term.
    pub(super) fn load_true_atom(
        &self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
    ) -> Result<Value, TranslateError> {
        if let Some(id) = self.true_atom_id {
            self.load_atom_term(module, builder, id)
        } else {
            // Fallback: use hardcoded runtime atom ID 2 for true
            Ok(builder.ins().iconst(types::I64, encode_atom(2) as i64))
        }
    }

    /// Load the runtime `false` atom term.
    pub(super) fn load_false_atom(
        &self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
    ) -> Result<Value, TranslateError> {
        if let Some(id) = self.false_atom_id {
            self.load_atom_term(module, builder, id)
        } else {
            // Fallback: use hardcoded runtime atom ID 3 for false
            Ok(builder.ins().iconst(types::I64, encode_atom(3) as i64))
        }
    }
}
