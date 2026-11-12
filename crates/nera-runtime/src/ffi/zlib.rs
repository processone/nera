//! `zlib` module entry points: stream handles over [`crate::zlib_module`].

use super::helpers::*;
use crate::atom_table::{global_atom_table, ATOM_OK};
use crate::process::ProcessContext;
use crate::resource::ResourceRef;
use crate::term::*;
use crate::zlib_module::{
    zlib_open, DeflateParams, Flush, InflateStep, ZError, ZStream,
};
use zlib_rs::Strategy;

fn atom_name(term: ErlTerm) -> Option<String> {
    global_atom_table().resolve(term.as_atom_id()?)
}

unsafe fn stream(ctx: *mut ProcessContext, term: u64) -> Option<ResourceRef<ZStream>> {
    let Some(resource) = ResourceRef::<ZStream>::from_term(ErlTerm(term)) else {
        raise_badarg(ctx);
        return None;
    };
    if let Err(e) = resource.get().check_owner((*ctx).pid) {
        fail(ctx, e);
        return None;
    }
    Some(resource)
}

unsafe fn fail(ctx: *mut ProcessContext, e: ZError) -> u64 {
    if e == ZError::Badarg {
        return raise_badarg(ctx);
    }
    let atom = global_atom_table().intern(e.name());
    raise_error(ctx, ErlTerm::atom(atom).0)
}

fn ok() -> u64 {
    ErlTerm::atom(ATOM_OK).0
}

/// Run `f` on the stream behind `ref_term`; `Ok(())` answers `ok`.
unsafe fn with_stream(
    ctx: *mut ProcessContext,
    ref_term: u64,
    f: impl FnOnce(&ZStream) -> Result<(), ZError>,
) -> u64 {
    let Some(resource) = stream(ctx, ref_term) else {
        return ErlTerm::nil().0;
    };
    match f(resource.get()) {
        Ok(()) => ok(),
        Err(e) => fail(ctx, e),
    }
}

unsafe fn chunks_term(ctx: *mut ProcessContext, chunks: &[Vec<u8>]) -> ErlTerm {
    with_ctx_heap(ctx, |heap| {
        let mut list = ErlTerm::nil();
        for chunk in chunks.iter().rev() {
            let bin = heap.alloc_binary_auto(chunk);
            list = heap.alloc_cons(bin, list);
        }
        list
    })
}

fn parse_level(term: ErlTerm) -> Result<i32, ZError> {
    if let Some(n) = term.as_small_int() {
        return DeflateParams::check_level(n);
    }
    match atom_name(term).as_deref() {
        Some("none") => Ok(0),
        Some("default") => Ok(-1),
        Some("best_speed") => Ok(1),
        Some("best_compression") => Ok(9),
        _ => Err(ZError::BadCompressionLevel),
    }
}

fn parse_strategy(term: ErlTerm) -> Result<Strategy, ZError> {
    match atom_name(term).as_deref() {
        Some("default") => Ok(Strategy::Default),
        Some("filtered") => Ok(Strategy::Filtered),
        Some("huffman_only") => Ok(Strategy::HuffmanOnly),
        Some("rle") => Ok(Strategy::Rle),
        _ => Err(ZError::BadCompressionStrategy),
    }
}

fn parse_flush(term: ErlTerm) -> Result<Flush, ZError> {
    match atom_name(term).as_deref() {
        Some("none") => Ok(Flush::None),
        Some("sync") => Ok(Flush::Sync),
        Some("full") => Ok(Flush::Full),
        Some("finish") => Ok(Flush::Finish),
        _ => Err(ZError::BadFlushMode),
    }
}

fn parse_int(term: ErlTerm, e: ZError) -> Result<i64, ZError> {
    term.as_small_int().ok_or(e)
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_zlib_open(ctx: *mut ProcessContext) -> u64 {
    let resource = zlib_open((*ctx).pid);
    with_ctx_heap(ctx, |heap| resource.to_term(heap).0)
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_zlib_close(ctx: *mut ProcessContext, z: u64) -> u64 {
    with_stream(ctx, z, |s| s.close())
}

unsafe fn deflate_init(
    ctx: *mut ProcessContext,
    z: u64,
    level: u64,
    method: u64,
    window_bits: u64,
    mem_level: u64,
    strategy: u64,
) -> u64 {
    let params = (|| {
        let level = parse_level(ErlTerm(level))?;
        if atom_name(ErlTerm(method)).as_deref() != Some("deflated") {
            return Err(ZError::BadCompressionMethod);
        }
        let window_bits = DeflateParams::check_window_bits(parse_int(
            ErlTerm(window_bits),
            ZError::BadWindowBits,
        )?)?;
        let mem_level = DeflateParams::check_mem_level(parse_int(
            ErlTerm(mem_level),
            ZError::BadMemLevel,
        )?)?;
        let strategy = parse_strategy(ErlTerm(strategy))?;
        Ok(DeflateParams { level, window_bits, mem_level, strategy })
    })();
    match params {
        Ok(p) => with_stream(ctx, z, |s| s.deflate_init(p)),
        Err(e) => fail(ctx, e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_zlib_deflate_init_2(
    ctx: *mut ProcessContext,
    z: u64,
    level: u64,
) -> u64 {
    let default = |name: &str| ErlTerm::atom(global_atom_table().intern(name)).0;
    deflate_init(
        ctx,
        z,
        level,
        default("deflated"),
        ErlTerm::small_int(15).0,
        ErlTerm::small_int(8).0,
        default("default"),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_zlib_deflate_init(
    ctx: *mut ProcessContext,
    z: u64,
    level: u64,
    method: u64,
    window_bits: u64,
    mem_level: u64,
    strategy: u64,
) -> u64 {
    deflate_init(ctx, z, level, method, window_bits, mem_level, strategy)
}

unsafe fn deflate_with(ctx: *mut ProcessContext, z: u64, data: u64, flush: Flush) -> u64 {
    let Some(resource) = stream(ctx, z) else {
        return ErlTerm::nil().0;
    };
    let Some(bytes) = iodata_to_bytes(ErlTerm(data)) else {
        return raise_badarg(ctx);
    };
    match resource.get().deflate(&bytes, flush) {
        Ok(chunks) => chunks_term(ctx, &chunks).0,
        Err(e) => fail(ctx, e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_zlib_deflate_2(ctx: *mut ProcessContext, z: u64, data: u64) -> u64 {
    deflate_with(ctx, z, data, Flush::None)
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_zlib_deflate(
    ctx: *mut ProcessContext,
    z: u64,
    data: u64,
    flush: u64,
) -> u64 {
    match parse_flush(ErlTerm(flush)) {
        Ok(flush) => deflate_with(ctx, z, data, flush),
        Err(e) => fail(ctx, e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_zlib_deflate_reset(ctx: *mut ProcessContext, z: u64) -> u64 {
    with_stream(ctx, z, |s| s.deflate_reset())
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_zlib_deflate_end(ctx: *mut ProcessContext, z: u64) -> u64 {
    with_stream(ctx, z, |s| s.deflate_end())
}

unsafe fn inflate_init(ctx: *mut ProcessContext, z: u64, window_bits: i64) -> u64 {
    with_stream(ctx, z, |s| s.inflate_init(window_bits))
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_zlib_inflate_init_1(ctx: *mut ProcessContext, z: u64) -> u64 {
    inflate_init(ctx, z, 15)
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_zlib_inflate_init(
    ctx: *mut ProcessContext,
    z: u64,
    window_bits: u64,
) -> u64 {
    match parse_int(ErlTerm(window_bits), ZError::BadWindowBits) {
        Ok(bits) => inflate_init(ctx, z, bits),
        Err(e) => fail(ctx, e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_zlib_inflate(ctx: *mut ProcessContext, z: u64, data: u64) -> u64 {
    let Some(resource) = stream(ctx, z) else {
        return ErlTerm::nil().0;
    };
    let Some(bytes) = iodata_to_bytes(ErlTerm(data)) else {
        return raise_badarg(ctx);
    };
    match resource.get().inflate(&bytes) {
        Ok(chunks) => chunks_term(ctx, &chunks).0,
        Err(e) => fail(ctx, e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_zlib_safe_inflate(
    ctx: *mut ProcessContext,
    z: u64,
    data: u64,
) -> u64 {
    let Some(resource) = stream(ctx, z) else {
        return ErlTerm::nil().0;
    };
    let Some(bytes) = iodata_to_bytes(ErlTerm(data)) else {
        return raise_badarg(ctx);
    };
    match resource.get().safe_inflate(&bytes) {
        Ok(step) => {
            let (tag, chunk) = match step {
                InflateStep::Continue(c) => ("continue", c),
                InflateStep::Finished(c) => ("finished", c),
            };
            let chunks = if chunk.is_empty() { Vec::new() } else { vec![chunk] };
            let list = chunks_term(ctx, &chunks);
            let tag = ErlTerm::atom(global_atom_table().intern(tag));
            with_ctx_heap(ctx, |heap| heap.alloc_tuple(&[tag, list]).0)
        }
        Err(e) => fail(ctx, e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_zlib_inflate_reset(ctx: *mut ProcessContext, z: u64) -> u64 {
    with_stream(ctx, z, |s| s.inflate_reset())
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_zlib_inflate_end(ctx: *mut ProcessContext, z: u64) -> u64 {
    with_stream(ctx, z, |s| s.inflate_end())
}

/// Hand the stream to another process; the caller must own it now.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_zlib_set_controlling_process(
    ctx: *mut ProcessContext,
    z: u64,
    pid: u64,
) -> u64 {
    let Some(owner) = ErlTerm(pid).as_pid_id() else {
        return raise_badarg(ctx);
    };
    with_stream(ctx, z, |s| {
        s.set_owner(owner);
        Ok(())
    })
}
