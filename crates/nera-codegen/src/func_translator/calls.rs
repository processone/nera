//! Function call emission (local, remote, apply).

use std::collections::HashMap;

use cranelift_codegen::ir::{types, InstBuilder, Value};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_frontend::{FunctionBuilder, Variable};
use cranelift_module::Module;
use cranelift_object::ObjectModule;

use nera_ir::ir::{CallTarget, VarId};
use crate::term_layout::*;
use super::{ModuleContext, declare_runtime_func, FuncTranslator, TranslateError};

impl FuncTranslator {
    #[expect(clippy::too_many_arguments, reason = "Explicit compiler state and continuation operands are kept aligned with the existing lowering interface")]
    pub(super) fn emit_call(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        target: &CallTarget,
        ctx_val: Value,
        ctx_var: Variable,
        arg_vals: &[Value],
        var_map: &HashMap<VarId, Variable>,
        mod_ctx: &ModuleContext,
    ) -> Result<Value, TranslateError> {
        match target {
            CallTarget::Local(name) => {
                // Look up the mangled name in the module context
                if let Some(mangled) = mod_ctx.module_funcs.get(&(name.0, arg_vals.len() as u32)) {
                    if let Some(&fid) = mod_ctx.func_ids.get(mangled) {
                        let func_ref = module.declare_func_in_func(fid, builder.func);
                        let mut call_args = vec![ctx_val];
                        call_args.extend_from_slice(arg_vals);
                        let call = builder.ins().call(func_ref, &call_args);
                        return Ok(builder.inst_results(call)[0]);
                    }
                }
                // Function not found — return nil
                Ok(builder.ins().iconst(types::I64, NIL_VALUE as i64))
            }

            CallTarget::Remote(mod_atom, fun_atom) => {
                // Route known BIFs to runtime FFI, or direct-link co-compiled modules
                self.emit_remote_call(module, builder, mod_atom.0, fun_atom.0, ctx_val, arg_vals, mod_ctx)
            }

            CallTarget::Bif(name) => {
                // Inline fast paths for receive operations
                match name.as_str() {
                    "receive_peek" => {
                        return self.emit_inline_receive_peek(module, builder, ctx_var);
                    }
                    "receive_accept" => {
                        return self.emit_inline_receive_accept(module, builder, ctx_var);
                    }
                    "receive_next" => {
                        return self.emit_inline_receive_next(module, builder, ctx_var);
                    }
                    _ => {}
                }
                // BIFs map to runtime FFI functions
                let ffi_name = format!("nera_rt_{}", name.strip_prefix("bif_").unwrap_or(name));
                let fid = declare_runtime_func(module, &ffi_name, arg_vals.len(), &mut self.runtime_funcs, self.c_call_conv)?;
                let func_ref = module.declare_func_in_func(fid, builder.func);
                let mut call_args = vec![ctx_val];
                call_args.extend_from_slice(arg_vals);
                let call = builder.ins().call(func_ref, &call_args);
                Ok(builder.inst_results(call)[0])
            }

            CallTarget::Closure(var_id) => {
                let closure_val = builder.use_var(var_map[var_id]);
                let nargs = arg_vals.len();
                let nargs_val = builder.ins().iconst(types::I64, nargs as i64);

                if nargs > 0 {
                    let slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                        cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                        (nargs * 8) as u32,
                        3,
                    ));
                    for (i, arg) in arg_vals.iter().enumerate() {
                        builder.ins().stack_store(*arg, slot, (i * 8) as i32);
                    }
                    let args_ptr = builder.ins().stack_addr(types::I64, slot, 0);
                    self.emit_ffi_call(module, builder, "nera_rt_call_closure", ctx_var,
                        &[closure_val, args_ptr, nargs_val])
                } else {
                    let null_ptr = builder.ins().iconst(types::I64, 0i64);
                    self.emit_ffi_call(module, builder, "nera_rt_call_closure", ctx_var,
                        &[closure_val, null_ptr, nargs_val])
                }
            }

            CallTarget::Apply(mod_var, fun_var) => {
                // Dynamic apply: build args list and call nera_rt_apply(ctx, mod, fun, args_list)
                let mod_val = builder.use_var(var_map[mod_var]);
                let fun_val = builder.use_var(var_map[fun_var]);

                // Build an Erlang list from the arguments (cons chain in reverse)
                let cons_fid = declare_runtime_func(module, "nera_rt_alloc_cons", 2, &mut self.runtime_funcs, self.c_call_conv)?;
                let cons_ref = module.declare_func_in_func(cons_fid, builder.func);

                let mut args_list = builder.ins().iconst(types::I64, NIL_VALUE as i64);
                for arg in arg_vals.iter().rev() {
                    let call = builder.ins().call(cons_ref, &[ctx_val, *arg, args_list]);
                    args_list = builder.inst_results(call)[0];
                }

                // Call nera_rt_apply(ctx, mod_atom, fun_atom, args_list)
                let apply_fid = declare_runtime_func(module, "nera_rt_apply", 3, &mut self.runtime_funcs, self.c_call_conv)?;
                let apply_ref = module.declare_func_in_func(apply_fid, builder.func);
                let call = builder.ins().call(apply_ref, &[ctx_val, mod_val, fun_val, args_list]);
                Ok(builder.inst_results(call)[0])
            }
        }
    }

    /// Emit a remote (module:function) call.
    #[expect(clippy::too_many_arguments, reason = "Explicit compiler state and continuation operands are kept aligned with the existing lowering interface")]
    pub(super) fn emit_remote_call(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        mod_id: u32,
        fun_id: u32,
        ctx_val: Value,
        arg_vals: &[Value],
        mod_ctx: &ModuleContext,
    ) -> Result<Value, TranslateError> {
        let arity = arg_vals.len();

        // Resolve atom IDs to strings and look up the FFI function
        let mod_name = self.resolve_atom(mod_id).map(|s| s.to_string());
        let fun_name = self.resolve_atom(fun_id).map(|s| s.to_string());

        // Look up FFI symbol from the unified BIF table (single source of truth).
        let ffi_name = nera_ir::bif_table::lookup_ffi_symbol(
            mod_name.as_deref().unwrap_or(""),
            fun_name.as_deref().unwrap_or(""),
            arity,
        );


        if let Some(name) = ffi_name {
            // Inline is_boolean: val == true_atom || val == false_atom
            if name == "nera_rt_is_boolean" && arg_vals.len() == 1 {
                let val = arg_vals[0];
                let true_atom = self.load_true_atom(module, builder)?;
                let false_atom = self.load_false_atom(module, builder)?;
                let is_true = builder.ins().icmp(IntCC::Equal, val, true_atom);
                let is_false = builder.ins().icmp(IntCC::Equal, val, false_atom);
                let is_bool = builder.ins().bor(is_true, is_false);
                let result_true = self.load_true_atom(module, builder)?;
                let result_false = self.load_false_atom(module, builder)?;
                return Ok(builder.ins().select(is_bool, result_true, result_false));
            }

            let fid = declare_runtime_func(module, name, arity, &mut self.runtime_funcs, self.c_call_conv)?;
            let func_ref = module.declare_func_in_func(fid, builder.func);
            let mut call_args = vec![ctx_val];
            // Some FFI functions expect arguments in a different order than Erlang.
            // Erlang:  maps:get(Key, Map)            -> FFI: nera_rt_map_get(ctx, Map, Key)
            // Erlang:  maps:put(Key, Val, Map)       -> FFI: nera_rt_map_put(ctx, Map, Key, Val)
            // Erlang:  maps:get(Key, Map, Default)   -> FFI: nera_rt_map_get_3(ctx, Map, Key, Default)
            // Erlang:  maps:update(Key, Val, Map)    -> FFI: nera_rt_map_update(ctx, Map, Key, Val)
            // Erlang:  maps:take(Key, Map)           -> FFI: nera_rt_map_take(ctx, Map, Key)
            if name == "nera_rt_map_get" && arg_vals.len() == 2 {
                call_args.push(arg_vals[1]); // map
                call_args.push(arg_vals[0]); // key
            } else if name == "nera_rt_map_put" && arg_vals.len() == 3 {
                call_args.push(arg_vals[2]); // map
                call_args.push(arg_vals[0]); // key
                call_args.push(arg_vals[1]); // value
            } else if name == "nera_rt_map_get_3" && arg_vals.len() == 3 {
                call_args.push(arg_vals[1]); // map
                call_args.push(arg_vals[0]); // key
                call_args.push(arg_vals[2]); // default
            } else if name == "nera_rt_map_update" && arg_vals.len() == 3 {
                call_args.push(arg_vals[2]); // map
                call_args.push(arg_vals[0]); // key
                call_args.push(arg_vals[1]); // value
            } else if name == "nera_rt_map_take" && arg_vals.len() == 2 {
                call_args.push(arg_vals[1]); // map
                call_args.push(arg_vals[0]); // key
            } else {
                call_args.extend_from_slice(arg_vals);
            }
            let call = builder.ins().call(func_ref, &call_args);
            Ok(builder.inst_results(call)[0])
        } else if let Some(&func_id) = mod_ctx.cross_module_funcs.get(&(mod_id, fun_id, arity as u32)) {
            // Direct cross-module call (co-compiled module) — same ABI as local calls
            let func_ref = module.declare_func_in_func(func_id, builder.func);
            let mut call_args = vec![ctx_val];
            call_args.extend_from_slice(arg_vals);
            let call = builder.ins().call(func_ref, &call_args);
            Ok(builder.inst_results(call)[0])
        } else {
            // Unknown remote call — dispatch via nera_rt_apply (runtime lookup)
            // Build an Erlang list from the arguments (in reverse, cons chain)
            let cons_fid = declare_runtime_func(module, "nera_rt_alloc_cons", 2, &mut self.runtime_funcs, self.c_call_conv)?;
            let cons_ref = module.declare_func_in_func(cons_fid, builder.func);

            let mut args_list = builder.ins().iconst(types::I64, NIL_VALUE as i64);
            for arg in arg_vals.iter().rev() {
                let call = builder.ins().call(cons_ref, &[ctx_val, *arg, args_list]);
                args_list = builder.inst_results(call)[0];
            }

            // Load module and function atom terms
            let mod_val = self.load_atom_term(module, builder, mod_id)?;
            let fun_val = self.load_atom_term(module, builder, fun_id)?;

            // Call nera_rt_apply(ctx, mod_atom, fun_atom, args_list)
            let apply_fid = declare_runtime_func(module, "nera_rt_apply", 3, &mut self.runtime_funcs, self.c_call_conv)?;
            let apply_ref = module.declare_func_in_func(apply_fid, builder.func);
            let call = builder.ins().call(apply_ref, &[ctx_val, mod_val, fun_val, args_list]);
            Ok(builder.inst_results(call)[0])
        }
    }
}
