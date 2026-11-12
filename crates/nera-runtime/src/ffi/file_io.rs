//! File I/O operations: read_file, write_file.

#![allow(unused_unsafe)]

use crate::process::ProcessContext;
use crate::term::*;
use super::helpers::*;


// ---------------------------------------------------------------------------
// File I/O helpers
// ---------------------------------------------------------------------------

/// Extract a filename from an ErlTerm that is either a binary or a list of
/// character codes. Returns `None` if the term is neither.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
unsafe fn term_to_filename(term: ErlTerm) -> Option<String> {
    if term.is_binary() {
        // Binary → extract raw bytes → UTF-8 string
        let bytes = unsafe { term.binary_bytes_any()? };
        std::str::from_utf8(bytes).ok().map(|s| s.to_owned())
    } else if term.is_cons() || term.is_nil() {
        // List of character codes → String
        crate::bif::term_to_string(term).ok()
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// file module — File I/O BIFs
// ---------------------------------------------------------------------------

/// file:read_file(Filename) -> {ok, Binary} | {error, Reason}
///
/// Reads the entire contents of `Filename` and returns them as a binary.
/// Submits to the I/O pool and suspends.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_file_read_file(
    ctx: *mut ProcessContext,
    filename_term: u64,
) -> u64 {
    use crate::process::IoResult;
    let filename = match term_to_filename(ErlTerm(filename_term)) {
        Some(f) => f,
        None => return raise_badarg(ctx),
    };

    submit_io_and_wait(ctx, Box::new(move || {
        match std::fs::read(&filename) {
            Ok(bytes) => IoResult::OkBytes(bytes),
            Err(e) => IoResult::Err(e),
        }
    }))
}

/// file:write_file(Filename, Data) -> ok | {error, Reason}
///
/// Writes `Data` (a binary or iolist) to `Filename`, creating or truncating
/// the file. Extracts data from the heap first, then submits to the I/O pool.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_file_write_file(
    ctx: *mut ProcessContext,
    filename_term: u64,
    data_term: u64,
) -> u64 {
    use crate::process::IoResult;
    let filename = match term_to_filename(ErlTerm(filename_term)) {
        Some(f) => f,
        None => return raise_badarg(ctx),
    };

    let bytes = match iodata_to_bytes(ErlTerm(data_term)) {
        Some(b) => b,
        None => return raise_badarg(ctx),
    };

    submit_io_and_wait(ctx, Box::new(move || {
        match std::fs::write(&filename, &bytes) {
            Ok(()) => IoResult::Ok,
            Err(e) => IoResult::Err(e),
        }
    }))
}

