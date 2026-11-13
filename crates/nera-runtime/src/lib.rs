//! `nera-runtime` -- A lightweight Erlang runtime for compiled code.
//!
//! This crate provides the core runtime components needed to execute
//! Erlang code compiled to native machine code by `nera`:
//!
//! - **term** -- 64-bit tagged pointer representation of Erlang terms
//! - **atom_table** -- Global interned atom table
//! - **heap** -- Per-process arena heap with copying GC
//! - **process** -- Process struct, mailbox, and scheduling metadata
//! - **message** -- Deep-copy message passing between process heaps
//! - **scheduler** -- Work-stealing N:M scheduler
//! - **bif** -- Built-in functions (list ops, type checks, I/O, tuples)
//! - **entry** -- C-ABI entry point and function table for codegen
//! - **socket** -- Global socket registry for gen_tcp support

// Use jemalloc as the global allocator on non-MSVC targets.
// Benefits over the system allocator:
// - Thread-local caches reduce contention between worker threads
// - Better fragmentation behaviour under GC arena churn
// - Scalable cross-thread free (sender allocs Arc, receiver's GC frees)
#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

// TiKV's prefixed jemalloc reads this C pointer before main/first allocation.
// Match jemalloc-sys's Option<&c_char> ABI, not a Rust slice's fat pointer.
// Cache allocations up to 4 KiB and use two automatic arenas (which may
// increase contention). Only macOS disables retain for its measured memory
// profile. Elsewhere keep jemalloc's platform default: 64-bit Linux retains
// mappings to avoid semi-permanent holes in its virtual memory map.
// Leave dirty/muzzy decay at jemalloc's defaults (10000/0 ms). Do not enable
// background threads: this jemalloc build excludes them on Mach-O.
// Windows jemalloc owns a non-weak malloc_conf; do not define a second one.
// Users can override these defaults at process startup with _RJEM_MALLOC_CONF;
// the unprefixed MALLOC_CONF variable does not configure this allocator.
#[cfg(all(not(target_env = "msvc"), not(windows)))]
#[used]
#[export_name = "_rjem_malloc_conf"]
pub static MALLOC_CONF: Option<&'static libc::c_char> = {
    let profile = if cfg!(target_os = "macos") {
        c"retain:false,tcache_max:4096,narenas:2"
    } else {
        c"tcache_max:4096,narenas:2"
    };
    // SAFETY: the C string is NUL-terminated and lives for the entire program.
    Some(unsafe { &*profile.as_ptr() })
};

mod base64;
mod io_format;
mod native_work;
pub mod term;
pub mod binary;
pub mod match_context;
pub mod atom_table;
pub mod heap;
mod copy_stack;
pub mod process;
pub mod message;
pub mod scheduler;
pub mod bif;
pub mod ffi;
pub mod bigint;
pub mod timer;
pub mod entry;
pub mod socket;
pub mod io_pool;
#[cfg(mio_supported)]
pub mod reactor;
pub mod ets_pool;
pub mod ets;
pub mod persistent_term;
pub mod counters;
pub mod atomics_module;
pub mod zlib_module;

pub(crate) mod resource;
