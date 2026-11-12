//! Module-level compilation orchestration.
//!
//! Takes an IR module and produces a native object file via Cranelift.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use cranelift_codegen::ir::{types, AbiParam, Function, InstBuilder, MemFlags, Signature, Value};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::isa::{CallConv, TargetIsa};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use target_lexicon::Triple;
use std::path::PathBuf;

use nera_ir::ir::{IrFunction, IrModule};
use crate::func_translator::{FuncTranslator, ModuleContext, TranslateError, build_erlang_signature};
use crate::term_layout::*;

/// Error during compilation.
#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    /// ISA / settings setup failed.
    #[error("ISA setup error for target '{triple}': {reason}")]
    IsaSetup { triple: String, reason: String },

    /// Function translation / compilation failed.
    #[error("codegen error in {func}/{arity} (line {line}): {reason}")]
    FunctionCodegen {
        func: String,
        arity: u32,
        line: u32,
        reason: String,
    },

    /// Wrapper function compilation failed.
    #[error("wrapper codegen error for {func}/{arity}: {reason}")]
    WrapperCodegen {
        func: String,
        arity: u32,
        reason: String,
    },

    /// `main()` generation failed.
    #[error("main codegen error for module '{module}': {reason}")]
    MainCodegen { module: String, reason: String },

    /// Object file emission failed.
    #[error("object emission error: {0}")]
    ObjectEmission(String),

    #[error("module error: {0}")]
    Module(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Return the appropriate C calling convention for the given target.
pub fn c_call_conv(triple: &Triple) -> CallConv {
    match triple.operating_system {
        target_lexicon::OperatingSystem::Windows => CallConv::WindowsFastcall,
        _ => CallConv::SystemV,
    }
}

/// Compile an IR module to a native object file.
pub struct ModuleCompiler {
    triple: Triple,
    /// Atom resolution: AtomId(u32) -> String (from the parser's interner)
    atom_table: HashMap<u32, String>,
    /// Cached Cranelift ISA — built once, reused across all module compilations.
    isa: OnceLock<Arc<dyn TargetIsa>>,
}

impl ModuleCompiler {
    /// Create a compiler targeting the host platform.
    pub fn for_host() -> Result<Self, CompileError> {
        Self::for_target(Triple::host())
    }

    /// Create a compiler targeting a specific platform.
    pub fn for_target(triple: Triple) -> Result<Self, CompileError> {
        Ok(Self {
            triple,
            atom_table: HashMap::new(),
            isa: OnceLock::new(),
        })
    }

    /// Return the target triple.
    pub fn target_triple(&self) -> &Triple {
        &self.triple
    }

    /// Set the atom table for resolving AtomId values to strings.
    pub fn set_atom_table(&mut self, table: HashMap<u32, String>) {
        self.atom_table = table;
    }

    /// Build (or return cached) the Cranelift target ISA for our target.
    fn build_isa(&self) -> Result<Arc<dyn TargetIsa>, CompileError> {
        if let Some(isa) = self.isa.get() {
            return Ok(Arc::clone(isa));
        }

        let mut flag_builder = settings::builder();
        let isa_err = |e: cranelift_codegen::settings::SetError| CompileError::IsaSetup {
            triple: self.triple.to_string(),
            reason: e.to_string(),
        };

        flag_builder
            .set("opt_level", "speed")
            .map_err(isa_err)?;
        flag_builder
            .set("is_pic", "true")
            .map_err(isa_err)?;
        // Required for CallConv::Tail on x86_64
        flag_builder
            .set("preserve_frame_pointers", "true")
            .map_err(isa_err)?;

        let mut isa_builder = if self.triple == Triple::host() {
            // Host target: use native builder for CPU feature detection (AVX, etc.)
            cranelift_native::builder()
                .map_err(|e| CompileError::IsaSetup {
                    triple: self.triple.to_string(),
                    reason: e.to_string(),
                })?
        } else {
            // Cross target: use generic ISA lookup
            cranelift_codegen::isa::lookup(self.triple.clone())
                .map_err(|e| CompileError::IsaSetup {
                    triple: self.triple.to_string(),
                    reason: format!("unsupported target: {}", e),
                })?
        };

        // Disable pointer authentication (PAC) for compiled Erlang functions.
        // PAC adds pacibz/autibz on every call/return for ROP protection, but
        // Erlang code has no buffer overflows — all memory is runtime-managed.
        // The runtime library (compiled by rustc) retains its own PAC settings.
        if isa_builder.set("sign_return_address", "false").is_err() {
            // Not all targets support this flag (e.g. x86_64); ignore errors.
        }

        let flags = settings::Flags::new(flag_builder);
        let isa = isa_builder
            .finish(flags)
            .map_err(|e| CompileError::IsaSetup {
                triple: self.triple.to_string(),
                reason: e.to_string(),
            })?;

        Ok(Arc::clone(self.isa.get_or_init(|| isa)))
    }

    /// Resolve an AtomId to its string.
    pub fn resolve_atom(&self, id: u32) -> Option<&str> {
        self.atom_table.get(&id).map(|s| s.as_str())
    }

    /// Compile an IR module to a native object file.
    /// Returns the bytes of the object file.
    ///
    /// If `generate_main` is true, also generates a C `main()` function
    /// that registers ALL modules' functions and starts the scheduler.
    /// `all_modules` should contain all IR modules when `generate_main` is true.
    /// `entry_mod_fun` is (entry_module_name, entry_function_name) for the --entry flag.
    pub fn compile_module(
        &self,
        ir_module: &IrModule,
        generate_main: bool,
        all_modules: &[&IrModule],
        entry_mod_fun: Option<(&str, &str)>,
    ) -> Result<Vec<u8>, CompileError> {
        let isa = self.build_isa()?;

        let obj_builder = ObjectBuilder::new(
            isa,
            format!("module_{}", ir_module.name.0),
            cranelift_module::default_libcall_names(),
        )
        .map_err(|e| CompileError::Module(e.to_string()))?;

        let mut object_module = ObjectModule::new(obj_builder);

        // Create module context with all function declarations
        let module_name = self.resolve_atom(ir_module.name.0)
            .unwrap_or("<unknown>")
            .to_string();
        let mut mod_ctx = ModuleContext::new(&mut object_module, ir_module)
            .map_err(|e| match e {
                TranslateError::Module(reason) => CompileError::Module(reason),
                TranslateError::Codegen { reason, .. } => CompileError::FunctionCodegen {
                    func: module_name.clone(),
                    arity: 0,
                    line: 0,
                    reason,
                },
            })?;

        // Declare cross-module functions for direct linking (bypasses nera_rt_apply)
        for other_module in all_modules {
            if other_module.name.0 == ir_module.name.0 {
                continue; // skip self — already declared as local exports
            }
            for func in &other_module.functions {
                let sig = build_erlang_signature(func.arity);
                let func_id = object_module
                    .declare_function(&func.mangled_name, Linkage::Import, &sig)
                    .map_err(|e| CompileError::Module(e.to_string()))?;
                mod_ctx.cross_module_funcs.insert(
                    (other_module.name.0, func.name.0, func.arity),
                    func_id,
                );
            }
        }

        // Build func_index_map: assign each (module, function, arity) a unique index.
        // The ordering must match emit_main_function's func_entries iteration.
        {
            let mut idx: u32 = 0;
            for other_module in all_modules {
                for func in &other_module.functions {
                    mod_ctx.func_index_map.insert(
                        (other_module.name.0, func.name.0, func.arity),
                        idx,
                    );
                    idx += 1;
                }
            }
        }

        // Declare the global atom lookup table (populated at startup by main).
        // All compiled functions load runtime atom terms from this table.
        let max_atom_id = self.atom_table.keys().max().copied().unwrap_or(0);
        let table_size = ((max_atom_id + 1) as usize) * 8;
        let atom_linkage = if generate_main { Linkage::Export } else { Linkage::Import };
        let atom_table_data_id = object_module
            .declare_data("__nera_atom_table", atom_linkage, true, false)
            .map_err(|e| CompileError::Module(e.to_string()))?;
        if generate_main {
            // Only define the data in the module that generates main
            let mut data_desc = cranelift_module::DataDescription::new();
            data_desc.define(vec![0u8; table_size].into_boxed_slice());
            data_desc.set_align(8); // u64 elements
            object_module
                .define_data(atom_table_data_id, &data_desc)
                .map_err(|e| CompileError::Module(e.to_string()))?;
        }

        // Declare ErlFn-compatible wrappers for each function (get FuncIds first).
        // These FuncIds are needed during function translation for set_resume calls.
        let mut wrapper_ids: HashMap<String, FuncId> = HashMap::new();
        for func in &ir_module.functions {
            let wrapper_id = self.declare_wrapper_function(&mut object_module, func)?;
            wrapper_ids.insert(func.mangled_name.clone(), wrapper_id);
            // Also register wrapper in mod_ctx so MakeClosure can find it
            let wrapper_name = format!("{}_wrapper", func.mangled_name);
            mod_ctx.func_ids.insert(wrapper_name, wrapper_id);
        }

        // Compile each function (passing wrapper FuncIds for receive-wait resume)
        let mut translator = FuncTranslator::new();
        translator.set_c_call_conv(c_call_conv(&self.triple));
        translator.set_atom_table(self.atom_table.clone());
        translator.set_atom_table_data_id(atom_table_data_id);

        for func in &ir_module.functions {
            let w_id = wrapper_ids.get(&func.mangled_name).copied();
            let func_name = self.resolve_atom(func.name.0)
                .unwrap_or(&func.mangled_name)
                .to_string();
            translator
                .translate_function(&mut object_module, func, &mod_ctx, w_id)
                .map_err(|e| match e {
                    TranslateError::Module(reason) => CompileError::Module(reason),
                    TranslateError::Codegen { block_id, reason } => {
                        let reason = match block_id {
                            Some(bid) => format!("[block {}] {}", bid.0, reason),
                            None => reason,
                        };
                        CompileError::FunctionCodegen {
                            func: func_name.clone(),
                            arity: func.arity,
                            line: func.line,
                            reason,
                        }
                    }
                })?;
        }

        // Define the wrapper function bodies.
        for func in &ir_module.functions {
            let wrapper_id = wrapper_ids[&func.mangled_name];
            self.define_wrapper_function(
                &mut object_module,
                func,
                &mod_ctx,
                wrapper_id,
            )?;
        }

        // Generate main() if requested
        if generate_main {
            self.generate_main_function(
                &mut object_module,
                ir_module,
                &mod_ctx,
                &wrapper_ids,
                all_modules,
                entry_mod_fun,
            )?;
        }

        // Emit the object file
        let product = object_module.finish();
        let bytes = product.emit()
            .map_err(|e| CompileError::ObjectEmission(e.to_string()))?;

        Ok(bytes)
    }

    /// Declare (but not define) a wrapper function with the ErlFn ABI.
    /// Returns the FuncId so it can be referenced during function translation.
    fn declare_wrapper_function(
        &self,
        module: &mut ObjectModule,
        ir_func: &IrFunction,
    ) -> Result<FuncId, CompileError> {
        let wrapper_name = format!("{}_wrapper", ir_func.mangled_name);

        // Wrapper signature: (ctx: i64, args: i64, nargs: i32) -> void
        let cc = c_call_conv(&self.triple);
        let mut wrapper_sig = Signature::new(cc);
        wrapper_sig.params.push(AbiParam::new(types::I64)); // ctx
        wrapper_sig.params.push(AbiParam::new(types::I64)); // args ptr
        wrapper_sig.params.push(AbiParam::new(types::I32)); // nargs

        let wrapper_id = module
            .declare_function(&wrapper_name, Linkage::Export, &wrapper_sig)
            .map_err(|e| CompileError::Module(e.to_string()))?;

        Ok(wrapper_id)
    }

    /// Define the wrapper function body. Must be called after the real
    /// function has been defined (so we can reference it).
    fn define_wrapper_function(
        &self,
        module: &mut ObjectModule,
        ir_func: &IrFunction,
        mod_ctx: &ModuleContext,
        wrapper_id: FuncId,
    ) -> Result<(), CompileError> {
        let real_func_id = mod_ctx.func_ids[&ir_func.mangled_name];

        // Wrapper signature: (ctx: i64, args: i64, nargs: i32) -> void
        let cc = c_call_conv(&self.triple);
        let mut wrapper_sig = Signature::new(cc);
        wrapper_sig.params.push(AbiParam::new(types::I64)); // ctx
        wrapper_sig.params.push(AbiParam::new(types::I64)); // args ptr
        wrapper_sig.params.push(AbiParam::new(types::I32)); // nargs

        let mut func = Function::with_name_signature(
            cranelift_codegen::ir::UserFuncName::user(0, wrapper_id.as_u32()),
            wrapper_sig,
        );

        let mut builder_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);

        let block = builder.create_block();
        builder.append_block_params_for_function_params(block);
        builder.switch_to_block(block);
        builder.seal_block(block);

        let params = builder.block_params(block).to_vec();
        let ctx_val = params[0];
        let args_ptr = params[1];
        // nargs = params[2] (not used since we know the arity)

        // Load each argument from args_ptr
        let real_ref = module.declare_func_in_func(real_func_id, builder.func);
        let mut call_args = vec![ctx_val];
        for i in 0..ir_func.arity {
            let offset = (i as i32) * 8;
            let arg = builder.ins().load(types::I64, MemFlags::trusted(), args_ptr, offset);
            call_args.push(arg);
        }

        // Call the real function
        let call = builder.ins().call(real_ref, &call_args);
        let result = builder.inst_results(call)[0];

        // Only store result in ctx.result if no exception is pending.
        // When an exception occurs, the exception reason is already in
        // ctx.result (set by nera_rt_error/throw/exit). Writing the
        // function's return value would clobber it.
        let status = builder.ins().load(types::I32, MemFlags::trusted(), ctx_val, CTX_OFFSET_STATUS);
        let exception_code = builder.ins().iconst(types::I32, STATUS_EXCEPTION as i64);
        let is_exception = builder.ins().icmp(IntCC::Equal, status, exception_code);

        let store_block = builder.create_block();
        let done_block = builder.create_block();

        builder.ins().brif(is_exception, done_block, &[], store_block, &[]);

        builder.switch_to_block(store_block);
        builder.seal_block(store_block);
        builder.ins().store(MemFlags::trusted(), result, ctx_val, CTX_OFFSET_RESULT);
        builder.ins().jump(done_block, &[]);

        builder.switch_to_block(done_block);
        builder.seal_block(done_block);
        builder.ins().return_(&[]);

        builder.finalize();

        let mut ctx = cranelift_codegen::Context::for_function(func);
        module
            .define_function(wrapper_id, &mut ctx)
            .map_err(|e| CompileError::WrapperCodegen {
                func: ir_func.mangled_name.clone(),
                arity: ir_func.arity,
                reason: e.to_string(),
            })?;

        Ok(())
    }

    /// Generate a C `main()` function that:
    /// 1. Initializes the runtime
    /// 2. Interns all atoms used by the module
    /// 3. Registers all compiled functions in the global function table
    /// 4. Starts the scheduler with the entry function
    fn generate_main_function(
        &self,
        module: &mut ObjectModule,
        ir_module: &IrModule,
        _mod_ctx: &ModuleContext,
        wrapper_ids: &HashMap<String, FuncId>,
        all_modules: &[&IrModule],
        entry_mod_fun: Option<(&str, &str)>,
    ) -> Result<(), CompileError> {
        let cc = c_call_conv(&self.triple);

        // Signature: main() -> i32
        let mut main_sig = Signature::new(cc);
        main_sig.returns.push(AbiParam::new(types::I32));

        let main_id = module
            .declare_function("main", Linkage::Export, &main_sig)
            .map_err(|e| CompileError::Module(e.to_string()))?;

        // Declare runtime functions we'll call
        let init_sig = Signature::new(cc);
        let init_id = module
            .declare_function("nera_runtime_init", Linkage::Import, &init_sig)
            .map_err(|e| CompileError::Module(e.to_string()))?;

        // nera_rt_intern_atoms_batch(strings_base: *const u8, offsets: *const u32,
        //                           out: *mut u64, count: u32)
        let mut intern_batch_sig = Signature::new(cc);
        intern_batch_sig.params.push(AbiParam::new(types::I64)); // strings_base
        intern_batch_sig.params.push(AbiParam::new(types::I64)); // offsets
        intern_batch_sig.params.push(AbiParam::new(types::I64)); // out
        intern_batch_sig.params.push(AbiParam::new(types::I32)); // count
        let intern_batch_id = module
            .declare_function("nera_rt_intern_atoms_batch", Linkage::Import, &intern_batch_sig)
            .map_err(|e| CompileError::Module(e.to_string()))?;

        // nera_rt_register_functions_batch(mod_ids: *const u32, fun_ids: *const u32,
        //                                  arities: *const u32, func_ptrs: *const fn, count: u32)
        let mut reg_batch_sig = Signature::new(cc);
        reg_batch_sig.params.push(AbiParam::new(types::I64)); // mod_ids
        reg_batch_sig.params.push(AbiParam::new(types::I64)); // fun_ids
        reg_batch_sig.params.push(AbiParam::new(types::I64)); // arities
        reg_batch_sig.params.push(AbiParam::new(types::I64)); // func_ptrs
        reg_batch_sig.params.push(AbiParam::new(types::I32)); // count
        let reg_batch_id = module
            .declare_function("nera_rt_register_functions_batch", Linkage::Import, &reg_batch_sig)
            .map_err(|e| CompileError::Module(e.to_string()))?;

        // nera_runtime_start_mfa(mod_atom: u64, fun_atom: u64)
        let mut start_sig = Signature::new(cc);
        start_sig.params.push(AbiParam::new(types::I64)); // mod_atom
        start_sig.params.push(AbiParam::new(types::I64)); // fun_atom
        let start_id = module
            .declare_function("nera_runtime_start_mfa", Linkage::Import, &start_sig)
            .map_err(|e| CompileError::Module(e.to_string()))?;

        // nera_rt_set_func_info_table(mod_ids: *const u32, fun_ids: *const u32,
        //                             arities: *const u32, count: u32)
        let mut set_info_sig = Signature::new(cc);
        set_info_sig.params.push(AbiParam::new(types::I64)); // mod_ids
        set_info_sig.params.push(AbiParam::new(types::I64)); // fun_ids
        set_info_sig.params.push(AbiParam::new(types::I64)); // arities
        set_info_sig.params.push(AbiParam::new(types::I32)); // count
        let set_info_id = module
            .declare_function("nera_rt_set_func_info_table", Linkage::Import, &set_info_sig)
            .map_err(|e| CompileError::Module(e.to_string()))?;

        // Collect ALL atoms from the parser's atom table (sorted by ID).
        // We intern them all at startup so runtime atom IDs match parser IDs.
        let mut atoms_sorted: Vec<(u32, String)> = self.atom_table.iter()
            .map(|(id, s)| (*id, s.clone()))
            .collect();
        atoms_sorted.sort_by_key(|(id, _)| *id);

        // Create batch atom data sections:
        // 1. Concatenated atom strings
        // 2. Offsets array: pairs of (offset_u32, len_u32)
        // 3. Results array: writable, N * 8 bytes
        let atom_count = atoms_sorted.len();

        let mut concat_bytes: Vec<u8> = Vec::new();
        let mut offsets_data: Vec<u8> = Vec::new();
        for (_atom_id, atom_str) in &atoms_sorted {
            let offset = concat_bytes.len() as u32;
            let len = atom_str.len() as u32;
            offsets_data.extend_from_slice(&offset.to_le_bytes());
            offsets_data.extend_from_slice(&len.to_le_bytes());
            concat_bytes.extend_from_slice(atom_str.as_bytes());
        }

        let atom_strings_id = module
            .declare_data("__nera_atom_strings", Linkage::Local, false, false)
            .map_err(|e| CompileError::Module(e.to_string()))?;
        {
            let mut desc = cranelift_module::DataDescription::new();
            desc.define(concat_bytes.into_boxed_slice());
            module.define_data(atom_strings_id, &desc)
                .map_err(|e| CompileError::Module(e.to_string()))?;
        }

        let atom_offsets_id = module
            .declare_data("__nera_atom_offsets", Linkage::Local, false, false)
            .map_err(|e| CompileError::Module(e.to_string()))?;
        {
            let mut desc = cranelift_module::DataDescription::new();
            desc.define(offsets_data.into_boxed_slice());
            desc.set_align(4); // u32 elements
            module.define_data(atom_offsets_id, &desc)
                .map_err(|e| CompileError::Module(e.to_string()))?;
        }

        let atom_results_id = module
            .declare_data("__nera_atom_results", Linkage::Local, true, false)
            .map_err(|e| CompileError::Module(e.to_string()))?;
        {
            let mut desc = cranelift_module::DataDescription::new();
            desc.define_zeroinit(atom_count * 8);
            desc.set_align(8); // u64 elements
            module.define_data(atom_results_id, &desc)
                .map_err(|e| CompileError::Module(e.to_string()))?;
        }

        // The global atom table was already declared and defined at module level.
        let atom_table_data_id = module
            .declare_data("__nera_atom_table", Linkage::Export, true, false)
            .map_err(|e| CompileError::Module(e.to_string()))?;

        let mod_name_id = ir_module.name.0;

        // Build the main function
        let mut func = Function::with_name_signature(
            cranelift_codegen::ir::UserFuncName::user(0, main_id.as_u32()),
            main_sig,
        );

        let mut builder_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);

        let entry_block = builder.create_block();
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        // 1. Call nera_runtime_init()
        let init_ref = module.declare_func_in_func(init_id, builder.func);
        builder.ins().call(init_ref, &[]);

        // 2. Batch-intern all atoms and populate the global atom lookup table
        let intern_batch_ref = module.declare_func_in_func(intern_batch_id, builder.func);

        let strings_gv = module.declare_data_in_func(atom_strings_id, builder.func);
        let offsets_gv = module.declare_data_in_func(atom_offsets_id, builder.func);
        let results_gv = module.declare_data_in_func(atom_results_id, builder.func);

        let strings_ptr = builder.ins().global_value(types::I64, strings_gv);
        let offsets_ptr = builder.ins().global_value(types::I64, offsets_gv);
        let results_ptr = builder.ins().global_value(types::I64, results_gv);
        let count_val = builder.ins().iconst(types::I32, atom_count as i64);

        builder.ins().call(intern_batch_ref, &[strings_ptr, offsets_ptr, results_ptr, count_val]);

        // Load results from the batch output and populate the global atom table
        let atom_table_gv = module.declare_data_in_func(atom_table_data_id, builder.func);
        let atom_table_base = builder.ins().global_value(types::I64, atom_table_gv);

        let mut runtime_atom_terms: HashMap<u32, Value> = HashMap::new();

        for (i, (atom_id, _atom_str)) in atoms_sorted.iter().enumerate() {
            // Load the interned atom term from the results array
            let result_offset = (i as i32) * 8;
            let atom_term = builder.ins().load(types::I64, MemFlags::trusted(), results_ptr, result_offset);
            runtime_atom_terms.insert(*atom_id, atom_term);

            // Store in the global atom table at offset = atom_id * 8
            let table_offset = (*atom_id as i32) * 8;
            builder.ins().store(MemFlags::trusted(), atom_term, atom_table_base, table_offset);
        }

        // 3. Batch-register ALL module functions (current + other modules)
        //
        // Build data sections for mod_ids, fun_ids, arities, and func_ptrs,
        // then make a single batch call.

        // Wrapper signature for importing wrapper functions from other modules
        let mut wrapper_sig = Signature::new(cc);
        wrapper_sig.params.push(AbiParam::new(types::I64)); // ctx
        wrapper_sig.params.push(AbiParam::new(types::I64)); // args ptr
        wrapper_sig.params.push(AbiParam::new(types::I32)); // nargs

        // Collect all functions to register
        struct FuncEntry {
            mod_atom_id: u32,
            fun_atom_id: u32,
            arity: u32,
            wrapper_func_id: FuncId,
        }
        let mut func_entries: Vec<FuncEntry> = Vec::new();

        for other_module in all_modules {
            let other_mod_name_id = other_module.name.0;

            for func_ir in &other_module.functions {
                let w_name = format!("{}_wrapper", func_ir.mangled_name);
                let w_id = if let Some(&id) = wrapper_ids.get(&func_ir.mangled_name) {
                    id
                } else {
                    module
                        .declare_function(&w_name, Linkage::Import, &wrapper_sig)
                        .map_err(|e| CompileError::Module(e.to_string()))?
                };

                func_entries.push(FuncEntry {
                    mod_atom_id: other_mod_name_id,
                    fun_atom_id: func_ir.name.0,
                    arity: func_ir.arity,
                    wrapper_func_id: w_id,
                });
            }
        }

        let func_count = func_entries.len();

        if func_count > 0 {
            // Arities are compile-time constants → read-only data section
            let mut arities_data: Vec<u8> = Vec::with_capacity(func_count * 4);
            for entry in &func_entries {
                arities_data.extend_from_slice(&entry.arity.to_le_bytes());
            }

            let func_arities_id = module
                .declare_data("__nera_func_arities", Linkage::Local, false, false)
                .map_err(|e| CompileError::Module(e.to_string()))?;
            {
                let mut desc = cranelift_module::DataDescription::new();
                desc.define(arities_data.into_boxed_slice());
                desc.set_align(4); // u32 elements
                module.define_data(func_arities_id, &desc)
                    .map_err(|e| CompileError::Module(e.to_string()))?;
            }

            // Mod terms, fun terms, and func ptrs are runtime values → writable zeroinit
            let func_mod_terms_id = module
                .declare_data("__nera_func_mod_terms", Linkage::Local, true, false)
                .map_err(|e| CompileError::Module(e.to_string()))?;
            {
                let mut desc = cranelift_module::DataDescription::new();
                desc.define_zeroinit(func_count * 8);
                desc.set_align(8); // u64 elements
                module.define_data(func_mod_terms_id, &desc)
                    .map_err(|e| CompileError::Module(e.to_string()))?;
            }

            let func_fun_terms_id = module
                .declare_data("__nera_func_fun_terms", Linkage::Local, true, false)
                .map_err(|e| CompileError::Module(e.to_string()))?;
            {
                let mut desc = cranelift_module::DataDescription::new();
                desc.define_zeroinit(func_count * 8);
                desc.set_align(8); // u64 elements
                module.define_data(func_fun_terms_id, &desc)
                    .map_err(|e| CompileError::Module(e.to_string()))?;
            }

            let func_ptrs_id = module
                .declare_data("__nera_func_ptrs", Linkage::Local, true, false)
                .map_err(|e| CompileError::Module(e.to_string()))?;
            {
                let mut desc = cranelift_module::DataDescription::new();
                desc.define_zeroinit(func_count * 8);
                desc.set_align(8); // function pointers
                module.define_data(func_ptrs_id, &desc)
                    .map_err(|e| CompileError::Module(e.to_string()))?;
            }

            // Emit the batch call
            let reg_batch_ref = module.declare_func_in_func(reg_batch_id, builder.func);

            let mod_terms_gv = module.declare_data_in_func(func_mod_terms_id, builder.func);
            let fun_terms_gv = module.declare_data_in_func(func_fun_terms_id, builder.func);
            let arities_gv = module.declare_data_in_func(func_arities_id, builder.func);
            let ptrs_gv = module.declare_data_in_func(func_ptrs_id, builder.func);

            let mod_terms_ptr = builder.ins().global_value(types::I64, mod_terms_gv);
            let fun_terms_ptr = builder.ins().global_value(types::I64, fun_terms_gv);
            let arities_ptr = builder.ins().global_value(types::I64, arities_gv);
            let ptrs_ptr = builder.ins().global_value(types::I64, ptrs_gv);

            // Fill arrays from code using runtime values
            for (i, entry) in func_entries.iter().enumerate() {
                let offset_8 = (i as i32) * 8;

                // Module atom term (runtime value from batch intern)
                let mod_atom_val = runtime_atom_terms
                    .get(&entry.mod_atom_id)
                    .copied()
                    .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
                builder.ins().store(MemFlags::trusted(), mod_atom_val, mod_terms_ptr, offset_8);

                // Function atom term (runtime value from batch intern)
                let fun_atom_val = runtime_atom_terms
                    .get(&entry.fun_atom_id)
                    .copied()
                    .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
                builder.ins().store(MemFlags::trusted(), fun_atom_val, fun_terms_ptr, offset_8);

                // Function pointer
                let func_ref = module.declare_func_in_func(entry.wrapper_func_id, builder.func);
                let func_addr = builder.ins().func_addr(types::I64, func_ref);
                builder.ins().store(MemFlags::trusted(), func_addr, ptrs_ptr, offset_8);
            }

            let func_count_val = builder.ins().iconst(types::I32, func_count as i64);

            builder.ins().call(reg_batch_ref, &[
                mod_terms_ptr, fun_terms_ptr, arities_ptr, ptrs_ptr, func_count_val,
            ]);

            // 3b. Set up the global function info table for compact stack frame resolution.
            // Module and function IDs must use runtime atom IDs (not parser IDs),
            // so we use writable zeroinit data filled at startup.
            let fi_mods_id = module
                .declare_data("__nera_func_info_mods", Linkage::Local, true, false)
                .map_err(|e| CompileError::Module(e.to_string()))?;
            {
                let mut desc = cranelift_module::DataDescription::new();
                desc.define_zeroinit(func_count * 4); // u32 per entry
                desc.set_align(4);
                module.define_data(fi_mods_id, &desc)
                    .map_err(|e| CompileError::Module(e.to_string()))?;
            }

            let fi_funs_id = module
                .declare_data("__nera_func_info_funs", Linkage::Local, true, false)
                .map_err(|e| CompileError::Module(e.to_string()))?;
            {
                let mut desc = cranelift_module::DataDescription::new();
                desc.define_zeroinit(func_count * 4); // u32 per entry
                desc.set_align(4);
                module.define_data(fi_funs_id, &desc)
                    .map_err(|e| CompileError::Module(e.to_string()))?;
            }

            let fi_mods_gv = module.declare_data_in_func(fi_mods_id, builder.func);
            let fi_funs_gv = module.declare_data_in_func(fi_funs_id, builder.func);
            let fi_mods_ptr = builder.ins().global_value(types::I64, fi_mods_gv);
            let fi_funs_ptr = builder.ins().global_value(types::I64, fi_funs_gv);

            // Fill mod_ids and fun_ids with runtime atom IDs (extracted from atom terms).
            // Atom term = (atom_id << 3) | TAG_ATOM, so atom_id = term >> 3 = (term as i64 >> 3) as u32.
            for (i, entry) in func_entries.iter().enumerate() {
                let offset_4 = (i as i32) * 4;

                // Module atom ID (runtime): extract from atom term via shift
                let mod_atom_val = runtime_atom_terms
                    .get(&entry.mod_atom_id)
                    .copied()
                    .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
                let mod_id_i64 = builder.ins().ushr_imm(mod_atom_val, 3);
                let mod_id_i32 = builder.ins().ireduce(types::I32, mod_id_i64);
                builder.ins().store(MemFlags::trusted(), mod_id_i32, fi_mods_ptr, offset_4);

                // Function atom ID (runtime): extract from atom term via shift
                let fun_atom_val = runtime_atom_terms
                    .get(&entry.fun_atom_id)
                    .copied()
                    .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
                let fun_id_i64 = builder.ins().ushr_imm(fun_atom_val, 3);
                let fun_id_i32 = builder.ins().ireduce(types::I32, fun_id_i64);
                builder.ins().store(MemFlags::trusted(), fun_id_i32, fi_funs_ptr, offset_4);
            }

            // Re-use the existing arities data section (compile-time constant, already correct)
            let fi_arities_gv = module.declare_data_in_func(func_arities_id, builder.func);
            let fi_arities_ptr = builder.ins().global_value(types::I64, fi_arities_gv);

            let set_info_ref = module.declare_func_in_func(set_info_id, builder.func);
            let fi_count_val = builder.ins().iconst(types::I32, func_count as i64);
            builder.ins().call(set_info_ref, &[
                fi_mods_ptr, fi_funs_ptr, fi_arities_ptr, fi_count_val,
            ]);
        }

        // 4. Start the scheduler with the entry function
        let start_ref = module.declare_func_in_func(start_id, builder.func);

        // Determine entry module and function from --entry flag or fall back to
        // first exported arity-0 function in the primary module.
        let (entry_mod_atom_id, entry_fun_atom_id) = {
            let mut resolved = (None, None);

            if let Some((mod_str, fun_str)) = entry_mod_fun {
                // Look up atom IDs by name from the atom table
                let mod_id = self.atom_table.iter()
                    .find(|(_, s)| s.as_str() == mod_str)
                    .map(|(id, _)| *id);
                let fun_id = self.atom_table.iter()
                    .find(|(_, s)| s.as_str() == fun_str)
                    .map(|(id, _)| *id);
                if mod_id.is_some() && fun_id.is_some() {
                    resolved = (mod_id, fun_id);
                }
            }

            // Fall back to first exported arity-0 function in the primary module
            if resolved.0.is_none() {
                let entry_export = ir_module.exports.iter().find(|exp| exp.arity == 0);
                if let Some(entry) = entry_export {
                    resolved = (Some(mod_name_id), Some(entry.name.0));
                }
            }

            resolved
        };

        if let (Some(mod_id), Some(fun_id)) = (entry_mod_atom_id, entry_fun_atom_id) {
            let mod_term = runtime_atom_terms
                .get(&mod_id)
                .copied()
                .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
            let fun_term = runtime_atom_terms
                .get(&fun_id)
                .copied()
                .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
            builder.ins().call(start_ref, &[mod_term, fun_term]);
        }

        // Return 0
        let ret_zero = builder.ins().iconst(types::I32, 0);
        builder.ins().return_(&[ret_zero]);

        builder.finalize();

        let mut ctx = cranelift_codegen::Context::for_function(func);
        module
            .define_function(main_id, &mut ctx)
            .map_err(|e| CompileError::MainCodegen {
                module: self.resolve_atom(ir_module.name.0)
                    .unwrap_or("<unknown>")
                    .to_string(),
                reason: e.to_string(),
            })?;

        Ok(())
    }

    /// Compile a minimal IR module (no functions) to verify the Cranelift pipeline works.
    #[cfg(test)]
    fn compile_empty_module(&self) -> Result<Vec<u8>, CompileError> {
        let ir_module = IrModule {
            name: nera_syntax::ast::AtomId(0),
            functions: vec![],
            exports: vec![],
        };
        self.compile_module(&ir_module, false, &[], None)
    }

    /// Generate a small metadata object file containing the compiler version and
    /// compilation date as global symbols.  The runtime BIFs `nera:version/0`
    /// and `nera:compile_date/0` reference these symbols.
    pub fn generate_metadata_object(
        &self,
        version: &str,
        compile_date: &str,
    ) -> Result<Vec<u8>, CompileError> {
        let isa = self.build_isa()?;
        let obj_builder = ObjectBuilder::new(
            isa,
            "nera_metadata".to_string(),
            cranelift_module::default_libcall_names(),
        )
        .map_err(|e| CompileError::Module(e.to_string()))?;

        let mut module = ObjectModule::new(obj_builder);

        // Helper closure: define a string + its length as exported data symbols
        let mut define_string = |name: &str, len_name: &str, value: &str| -> Result<(), CompileError> {
            // The string data
            let data_id = module
                .declare_data(name, Linkage::Export, false, false)
                .map_err(|e| CompileError::Module(e.to_string()))?;
            let mut desc = cranelift_module::DataDescription::new();
            desc.define(value.as_bytes().to_vec().into_boxed_slice());
            desc.set_align(1);
            module
                .define_data(data_id, &desc)
                .map_err(|e| CompileError::Module(e.to_string()))?;

            // The length as a u32
            let len_id = module
                .declare_data(len_name, Linkage::Export, false, false)
                .map_err(|e| CompileError::Module(e.to_string()))?;
            let mut len_desc = cranelift_module::DataDescription::new();
            let len_bytes = (value.len() as u32).to_ne_bytes();
            len_desc.define(len_bytes.to_vec().into_boxed_slice());
            len_desc.set_align(4);
            module
                .define_data(len_id, &len_desc)
                .map_err(|e| CompileError::Module(e.to_string()))?;

            Ok(())
        };

        define_string("__nera_compiler_version", "__nera_compiler_version_len", version)?;
        define_string("__nera_compile_date", "__nera_compile_date_len", compile_date)?;

        let product = module.finish();
        let bytes = product
            .emit()
            .map_err(|e| CompileError::ObjectEmission(e.to_string()))?;

        Ok(bytes)
    }

    /// Compile and write to a file.
    /// When `generate_main` is true, `all_modules` should contain all IR modules
    /// so that main() can register all functions.
    pub fn compile_to_file(
        &self,
        ir_module: &IrModule,
        output: &PathBuf,
        generate_main: bool,
        all_modules: &[&IrModule],
        entry_mod_fun: Option<(&str, &str)>,
    ) -> Result<(), CompileError> {
        let bytes = self.compile_module(ir_module, generate_main, all_modules, entry_mod_fun)?;
        std::fs::write(output, bytes)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use target_lexicon::Triple;

    // ---------------------------------------------------------------
    // c_call_conv
    // ---------------------------------------------------------------

    #[test]
    fn c_call_conv_host() {
        let cc = c_call_conv(&Triple::host());
        // On macOS/Linux, should be SystemV
        if cfg!(not(windows)) {
            assert_eq!(cc, CallConv::SystemV);
        }
    }

    #[test]
    fn c_call_conv_windows() {
        let win_triple: Triple = "x86_64-pc-windows-msvc".parse().unwrap();
        assert_eq!(c_call_conv(&win_triple), CallConv::WindowsFastcall);
    }

    #[test]
    fn c_call_conv_linux() {
        let linux_triple: Triple = "x86_64-unknown-linux-gnu".parse().unwrap();
        assert_eq!(c_call_conv(&linux_triple), CallConv::SystemV);
    }

    // ---------------------------------------------------------------
    // ModuleCompiler creation
    // ---------------------------------------------------------------

    #[test]
    fn for_host_succeeds() {
        let compiler = ModuleCompiler::for_host();
        assert!(compiler.is_ok());
    }

    #[test]
    fn for_target_host_triple() {
        let compiler = ModuleCompiler::for_target(Triple::host()).unwrap();
        assert_eq!(*compiler.target_triple(), Triple::host());
    }

    #[test]
    fn for_target_x86_64_linux() {
        let triple: Triple = "x86_64-unknown-linux-gnu".parse().unwrap();
        let compiler = ModuleCompiler::for_target(triple.clone()).unwrap();
        assert_eq!(*compiler.target_triple(), triple);
    }

    #[test]
    fn for_target_aarch64_linux() {
        let triple: Triple = "aarch64-unknown-linux-gnu".parse().unwrap();
        let compiler = ModuleCompiler::for_target(triple.clone()).unwrap();
        assert_eq!(*compiler.target_triple(), triple);
    }

    // ---------------------------------------------------------------
    // Atom table
    // ---------------------------------------------------------------

    #[test]
    fn atom_table_empty_by_default() {
        let compiler = ModuleCompiler::for_host().unwrap();
        assert_eq!(compiler.resolve_atom(0), None);
        assert_eq!(compiler.resolve_atom(42), None);
    }

    #[test]
    fn set_atom_table_and_resolve() {
        let mut compiler = ModuleCompiler::for_host().unwrap();
        let mut table = HashMap::new();
        table.insert(0, "ok".to_string());
        table.insert(1, "error".to_string());
        table.insert(42, "hello".to_string());
        compiler.set_atom_table(table);

        assert_eq!(compiler.resolve_atom(0), Some("ok"));
        assert_eq!(compiler.resolve_atom(1), Some("error"));
        assert_eq!(compiler.resolve_atom(42), Some("hello"));
        assert_eq!(compiler.resolve_atom(99), None);
    }

    #[test]
    fn set_atom_table_replaces_previous() {
        let mut compiler = ModuleCompiler::for_host().unwrap();
        let mut table1 = HashMap::new();
        table1.insert(0, "first".to_string());
        compiler.set_atom_table(table1);
        assert_eq!(compiler.resolve_atom(0), Some("first"));

        let mut table2 = HashMap::new();
        table2.insert(0, "second".to_string());
        compiler.set_atom_table(table2);
        assert_eq!(compiler.resolve_atom(0), Some("second"));
    }

    // ---------------------------------------------------------------
    // Compile empty module (smoke test for Cranelift pipeline)
    // ---------------------------------------------------------------

    #[test]
    fn compile_empty_module_produces_bytes() {
        let mut compiler = ModuleCompiler::for_host().unwrap();
        let mut table = HashMap::new();
        table.insert(0, "test".to_string());
        compiler.set_atom_table(table);

        let bytes = compiler.compile_empty_module().unwrap();
        assert!(!bytes.is_empty(), "object file should not be empty");
    }

    // ---------------------------------------------------------------
    // CompileError display
    // ---------------------------------------------------------------

    #[test]
    fn compile_error_display() {
        let e = CompileError::IsaSetup {
            triple: "x86_64-unknown-linux-gnu".to_string(),
            reason: "bad flag".to_string(),
        };
        assert!(e.to_string().contains("x86_64-unknown-linux-gnu"));
        assert!(e.to_string().contains("bad flag"));

        let e = CompileError::FunctionCodegen {
            func: "handle_call".to_string(),
            arity: 3,
            line: 42,
            reason: "bad instruction".to_string(),
        };
        let msg = e.to_string();
        assert!(msg.contains("handle_call/3"));
        assert!(msg.contains("line 42"));
        assert!(msg.contains("bad instruction"));

        let e = CompileError::WrapperCodegen {
            func: "my_func".to_string(),
            arity: 2,
            reason: "define failed".to_string(),
        };
        assert!(e.to_string().contains("my_func/2"));

        let e = CompileError::MainCodegen {
            module: "my_app".to_string(),
            reason: "oops".to_string(),
        };
        assert!(e.to_string().contains("my_app"));

        let e = CompileError::ObjectEmission("emit failed".to_string());
        assert!(e.to_string().contains("emit failed"));

        let e = CompileError::Module("duplicate func".to_string());
        assert!(e.to_string().contains("duplicate func"));
    }

    // ---------------------------------------------------------------
    // TranslateError → CompileError conversion (block ID propagation)
    // ---------------------------------------------------------------

    #[test]
    fn translate_error_with_block_id() {
        use crate::func_translator::TranslateError;
        use nera_ir::ir::BlockId;

        let te = TranslateError::Codegen {
            block_id: Some(BlockId(7)),
            reason: "verifier failed".to_string(),
        };

        // Simulate the conversion done in compile_module()
        let ce = match te {
            TranslateError::Module(reason) => CompileError::Module(reason),
            TranslateError::Codegen { block_id, reason } => {
                let reason = match block_id {
                    Some(bid) => format!("[block {}] {}", bid.0, reason),
                    None => reason,
                };
                CompileError::FunctionCodegen {
                    func: "my_fun".to_string(),
                    arity: 2,
                    line: 10,
                    reason,
                }
            }
        };

        let msg = ce.to_string();
        assert!(msg.contains("[block 7]"), "should embed block id: {msg}");
        assert!(msg.contains("verifier failed"));
        assert!(msg.contains("my_fun/2"));
        assert!(msg.contains("line 10"));
    }

    #[test]
    fn translate_error_without_block_id() {
        use crate::func_translator::TranslateError;

        let te = TranslateError::Codegen {
            block_id: None,
            reason: "define_function failed".to_string(),
        };

        let ce = match te {
            TranslateError::Module(reason) => CompileError::Module(reason),
            TranslateError::Codegen { block_id, reason } => {
                let reason = match block_id {
                    Some(bid) => format!("[block {}] {}", bid.0, reason),
                    None => reason,
                };
                CompileError::FunctionCodegen {
                    func: "init".to_string(),
                    arity: 0,
                    line: 1,
                    reason,
                }
            }
        };

        let msg = ce.to_string();
        assert!(!msg.contains("[block"), "no block prefix when None: {msg}");
        assert!(msg.contains("define_function failed"));
    }
}
