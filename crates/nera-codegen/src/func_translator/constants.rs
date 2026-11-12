//! Materialize arbitrary-precision integer constants from immutable object data.
use cranelift_codegen::ir::{types, InstBuilder, Value};
use cranelift_frontend::{FunctionBuilder, Variable};
use cranelift_module::{DataDescription, Module};
use cranelift_object::ObjectModule;
use super::{FuncTranslator, TranslateError};

impl FuncTranslator {
    pub(super) fn load_big_integer(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        ctx_var: Variable,
        decimal: &str,
    ) -> Result<Value, TranslateError> {
        let data_id = module.declare_anonymous_data(false, false)
            .map_err(|e| TranslateError::Module(e.to_string()))?;
        let mut data = DataDescription::new();
        data.define(decimal.as_bytes().to_vec().into_boxed_slice());
        module.define_data(data_id, &data)
            .map_err(|e| TranslateError::Module(e.to_string()))?;
        let global = module.declare_data_in_func(data_id, builder.func);
        let ptr = builder.ins().global_value(types::I64, global);
        let len = builder.ins().iconst(types::I64, decimal.len() as i64);
        self.emit_ffi_call(module, builder, "nera_rt_alloc_bigint_decimal", ctx_var, &[ptr, len])
    }
}
