//! Cranelift signature builders and runtime function declaration.

use std::collections::HashMap;

use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_codegen::isa::CallConv;
use cranelift_module::{FuncId, Linkage, Module};
use cranelift_object::ObjectModule;

use super::TranslateError;

/// Build the Cranelift signature for a compiled Erlang function.
/// Uses `CallConv::Tail` to enable true tail call optimization via `return_call`.
/// Convention: fn(ctx: *mut ProcessContext, arg1: u64, ..., argN: u64) -> u64
pub fn build_erlang_signature(arity: u32) -> Signature {
    let mut sig = Signature::new(CallConv::Tail);
    // First param: ProcessContext pointer
    sig.params.push(AbiParam::new(types::I64));
    // Erlang arguments (each is an ErlTerm = u64)
    for _ in 0..arity {
        sig.params.push(AbiParam::new(types::I64));
    }
    // Return: ErlTerm (u64)
    sig.returns.push(AbiParam::new(types::I64));
    sig
}

/// Build a signature for a runtime FFI function: (ctx, arg1, ..., argN) -> u64
pub(super) fn build_ffi_signature(n_args: usize, call_conv: CallConv) -> Signature {
    let mut sig = Signature::new(call_conv);
    sig.params.push(AbiParam::new(types::I64)); // ctx
    for _ in 0..n_args {
        sig.params.push(AbiParam::new(types::I64));
    }
    sig.returns.push(AbiParam::new(types::I64));
    sig
}

pub(super) fn build_ffi_signature_void(n_args: usize, call_conv: CallConv) -> Signature {
    let mut sig = Signature::new(call_conv);
    sig.params.push(AbiParam::new(types::I64)); // ctx
    for _ in 0..n_args {
        sig.params.push(AbiParam::new(types::I64));
    }
    sig
}

/// Declare (import) a runtime FFI function in the Cranelift module.
pub(super) fn declare_runtime_func(
    module: &mut ObjectModule,
    name: &str,
    n_args: usize,
    cache: &mut HashMap<String, FuncId>,
    call_conv: CallConv,
) -> Result<FuncId, TranslateError> {
    if let Some(id) = cache.get(name) {
        return Ok(*id);
    }
    let sig = build_ffi_signature(n_args, call_conv);
    let func_id = module
        .declare_function(name, Linkage::Import, &sig)
        .map_err(|e| TranslateError::Module(e.to_string()))?;
    cache.insert(name.to_string(), func_id);
    Ok(func_id)
}

/// Declare (import) a void runtime FFI function (no return value).
pub(super) fn declare_runtime_func_void(
    module: &mut ObjectModule,
    name: &str,
    n_args: usize,
    cache: &mut HashMap<String, FuncId>,
    call_conv: CallConv,
) -> Result<FuncId, TranslateError> {
    if let Some(id) = cache.get(name) {
        return Ok(*id);
    }
    let sig = build_ffi_signature_void(n_args, call_conv);
    let func_id = module
        .declare_function(name, Linkage::Import, &sig)
        .map_err(|e| TranslateError::Module(e.to_string()))?;
    cache.insert(name.to_string(), func_id);
    Ok(func_id)
}

/// Special signature for nera_rt_tuple_element: (ctx, tuple: u64, index: u32) -> u64
pub(super) fn declare_tuple_element(
    module: &mut ObjectModule,
    cache: &mut HashMap<String, FuncId>,
    call_conv: CallConv,
) -> Result<FuncId, TranslateError> {
    let name = "nera_rt_tuple_element";
    if let Some(id) = cache.get(name) {
        return Ok(*id);
    }
    let mut sig = Signature::new(call_conv);
    sig.params.push(AbiParam::new(types::I64)); // ctx
    sig.params.push(AbiParam::new(types::I64)); // tuple
    sig.params.push(AbiParam::new(types::I32)); // index
    sig.returns.push(AbiParam::new(types::I64));
    let func_id = module
        .declare_function(name, Linkage::Import, &sig)
        .map_err(|e| TranslateError::Module(e.to_string()))?;
    cache.insert(name.to_string(), func_id);
    Ok(func_id)
}
