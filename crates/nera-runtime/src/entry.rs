//! Runtime entry point and function table.
//!
//! This module provides the C-ABI entry point that compiled Erlang code
//! calls to bootstrap the runtime. It also defines the `FunctionTable`
//! used to look up compiled module functions by `(module, function, arity)`.

use std::collections::HashMap;
use std::sync::OnceLock;

use parking_lot::RwLock;

use crate::atom_table::global_atom_table;
use crate::process::ErlFn;
use crate::scheduler::Scheduler;

pub(crate) fn runtime_start_instant() -> &'static std::time::Instant {
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    START.get_or_init(std::time::Instant::now)
}

// ---------------------------------------------------------------------------
// FunctionTable
// ---------------------------------------------------------------------------

/// A registry mapping `(module_atom_id, function_atom_id, arity)` to a
/// compiled function pointer. Populated by generated code at startup.
pub struct FunctionTable {
    pub(crate) table: RwLock<HashMap<(u32, u32, u32), ErlFn>>,
}

impl Default for FunctionTable {
    fn default() -> Self {
        Self::new()
    }
}

impl FunctionTable {
    /// Create an empty function table.
    pub fn new() -> Self {
        FunctionTable {
            table: RwLock::new(HashMap::new()),
        }
    }

    /// Register a compiled function.
    pub fn register(&self, module: u32, function: u32, arity: u32, func: ErlFn) {
        self.table.write().insert((module, function, arity), func);
    }

    /// Look up a function by MFA triple.
    pub fn lookup(&self, module: u32, function: u32, arity: u32) -> Option<ErlFn> {
        self.table.read().get(&(module, function, arity)).copied()
    }
}

// ---------------------------------------------------------------------------
// Global function table singleton
// ---------------------------------------------------------------------------

static GLOBAL_FUNCTION_TABLE: OnceLock<FunctionTable> = OnceLock::new();

/// Return a reference to the global function table.
pub fn global_function_table() -> &'static FunctionTable {
    GLOBAL_FUNCTION_TABLE.get_or_init(FunctionTable::new)
}

// ---------------------------------------------------------------------------
// Global function info table (func_index → (module, function, arity))
// ---------------------------------------------------------------------------

/// Maps func_index → (module_atom_id, function_atom_id, arity).
/// Populated once at startup by `nera_rt_set_func_info_table`.
pub(crate) static FUNC_INFO_TABLE: OnceLock<Vec<(u32, u32, u32)>> = OnceLock::new();

/// Return a reference to the global function info table.
/// Returns an empty slice if the table hasn't been set yet.
pub fn global_func_info_table() -> &'static [(u32, u32, u32)] {
    FUNC_INFO_TABLE.get().map(|v| v.as_slice()).unwrap_or(&[])
}

// ---------------------------------------------------------------------------
// Runtime entry point
// ---------------------------------------------------------------------------

/// Main entry point called from generated code.
///
/// Initialises the global atom table, creates the scheduler, and starts
/// the worker threads. The actual entry module/function will be connected
/// by the code generator; for now this is a stub that starts the
/// scheduler and returns when all processes have exited.
#[no_mangle]
pub extern "C" fn nera_runtime_init() {
    let _ = runtime_start_instant();
    // Ensure the global atom table is initialised.
    // NOTE: We do NOT create the scheduler here. The scheduler is created
    // by nera_runtime_start_mfa() after all functions have been registered.
    // Creating a scheduler here would set the global scheduler handle
    // (OnceLock), preventing the real scheduler from being accessible.
    let _ = global_atom_table();
}

/// Placeholder for the codegen entry: spawn the initial process and
/// start the scheduler.
///
/// `entry_fn` is the compiled entry function (e.g., `main:main/0`).
/// This function does **not** return until all processes have exited.
#[no_mangle]
/// # Safety
/// The entry function and all functions it schedules must obey the compiled
/// runtime ABI and preserve valid process-owned roots throughout execution.
pub unsafe extern "C" fn nera_runtime_start(
    entry_fn: ErlFn,
) {
    let _ = global_atom_table();
    // Ensure the global function table is initialized.
    let _ = global_function_table();

    let num_threads = runtime_scheduler_threads();
    let scheduler = Scheduler::new(num_threads);

    let pid = scheduler.next_pid();
    let process = crate::process::Process::with_entry(pid, entry_fn, Vec::new());
    scheduler.spawn_process(process);

    // SAFETY: the ABI caller guarantees valid compiled functions and roots;
    // the initial process is created here with an empty argument list.
    let ok = unsafe { scheduler.run() };
    if !ok {
        std::process::exit(1);
    }
}

/// Start the scheduler using the global function table to look up the
/// entry function. Called from generated `main()` after functions have
/// been registered via `nera_rt_register_function`.
///
/// Takes atom IDs for the module and function names of the entry point.
#[no_mangle]
/// # Safety
/// Registered functions and all scheduled callbacks must obey the compiled
/// runtime ABI and preserve valid process-owned roots throughout execution.
pub unsafe extern "C" fn nera_runtime_start_mfa(
    mod_atom: u64,
    fun_atom: u64,
) {
    use crate::term::ErlTerm;

    let _ = global_atom_table();
    let _ = global_function_table();

    let mod_id = ErlTerm(mod_atom).as_atom_id().unwrap_or(0);
    let fun_id = ErlTerm(fun_atom).as_atom_id().unwrap_or(0);

    let func = match global_function_table().lookup(mod_id, fun_id, 0) {
        Some(f) => f,
        None => {
            eprintln!(
                "nera: entry function {}:{}/0 not found",
                global_atom_table().resolve(mod_id).unwrap_or_else(|| "?".to_string()),
                global_atom_table().resolve(fun_id).unwrap_or_else(|| "?".to_string()),
            );
            std::process::exit(1);
        }
    };

    let num_threads = runtime_scheduler_threads();
    let scheduler = Scheduler::new(num_threads);

    let pid = scheduler.next_pid();
    let process = crate::process::Process::with_entry(pid, func, Vec::new());
    scheduler.spawn_process(process);

    // SAFETY: the ABI caller guarantees valid compiled functions and roots;
    // the initial process is created here with an empty argument list.
    let ok = unsafe { scheduler.run() };
    if !ok {
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_table_register_and_lookup() {
        extern "C" fn dummy(_ctx: *mut crate::process::ProcessContext, _args: *const u64, _nargs: u32) {}

        let ft = FunctionTable::new();
        let mod_id = global_atom_table().intern("my_mod");
        let fun_id = global_atom_table().intern("my_fun");
        ft.register(mod_id, fun_id, 0, dummy);

        assert!(ft.lookup(mod_id, fun_id, 0).is_some());
        assert!(ft.lookup(mod_id, fun_id, 1).is_none());
    }

    #[test]
    fn function_table_empty_lookup() {
        let ft = FunctionTable::new();
        assert!(ft.lookup(0, 0, 0).is_none());
    }

    #[test]
    fn nera_runtime_init_does_not_panic() {
        // Just verify it doesn't crash when no processes are spawned.
        nera_runtime_init();
    }
}

fn runtime_scheduler_threads() -> usize {
    let requested = std::env::var_os("NERA_SCHEDULERS");
    let result = match requested.as_ref() {
        Some(value) => value
            .to_str()
            .ok_or("NERA_SCHEDULERS must be an integer from 1 to 255")
            .and_then(|value| scheduler_thread_count(Some(value), num_cpus::get())),
        None => scheduler_thread_count(None, num_cpus::get()),
    };
    result.unwrap_or_else(|message| {
        eprintln!("nera: {message}");
        std::process::exit(2);
    })
}

// Pure parsing keeps scheduler-count tests independent of the process environment.
fn scheduler_thread_count(
    requested: Option<&str>,
    available: usize,
) -> Result<usize, &'static str> {
    match requested {
        None => Ok(available.clamp(1, 255)),
        Some(value) => value
            .parse::<usize>()
            .ok()
            .filter(|n| (1..=255).contains(n))
            .ok_or("NERA_SCHEDULERS must be an integer from 1 to 255"),
    }
}

#[cfg(test)]
mod scheduler_count_tests {
    use super::scheduler_thread_count;
    #[test]
    fn explicit_single_scheduler_and_invalid_counts() {
        assert_eq!(scheduler_thread_count(Some("1"), 16), Ok(1));
        assert_eq!(scheduler_thread_count(None, 16), Ok(16));
        assert_eq!(scheduler_thread_count(None, 0), Ok(1));
        assert_eq!(scheduler_thread_count(None, 512), Ok(255));
        for value in ["0", "-1", "256", "", "abc"] {
            assert!(
                scheduler_thread_count(Some(value), 16).is_err(),
                "accepted {value}"
            );
        }
    }
}
