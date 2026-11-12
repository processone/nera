//! `zlib` module state — streaming deflate/inflate handles (OTP `zlib`).
//!
//! A `zlib:open/0` stream is a typed resource reference like `atomics` and
//! `counters`: the reference term carries a pointer to a [`ZStream`], and the
//! native allocation is retained by a registry for the node's lifetime. The
//! compression state inside is released by `zlib:close/1`.
//!
//! Compression uses the pure-Rust `zlib-rs` implementation, so output is
//! zlib-compatible and no system zlib is linked into generated programs.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use zlib_rs::{
    Deflate, DeflateConfig, DeflateError, DeflateFlush, Inflate, InflateError, InflateFlush,
    Method, Status, Strategy,
};

/// Output chunk sizes used by OTP's zlib NIFs.
pub(crate) const DEFLATE_OUT_CHUNK: usize = 8 << 10;
pub(crate) const INFLATE_OUT_CHUNK: usize = 16 << 10;

/// Failure reasons raised as `error(Reason)`, named as in OTP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ZError {
    NotInitialized,
    AlreadyInitialized,
    StreamError,
    DataError,
    NeedDictionary,
    BadFlushMode,
    BadCompressionLevel,
    BadCompressionMethod,
    BadCompressionStrategy,
    BadMemLevel,
    BadWindowBits,
    Badarg,
    NoMemory,
    NotOnControllingProcess,
}

impl ZError {
    pub(crate) fn name(self) -> &'static str {
        match self {
            ZError::NotInitialized => "not_initialized",
            ZError::AlreadyInitialized => "already_initialized",
            ZError::StreamError => "stream_error",
            ZError::DataError => "data_error",
            ZError::NeedDictionary => "need_dictionary",
            ZError::BadFlushMode => "bad_flush_mode",
            ZError::BadCompressionLevel => "bad_compression_level",
            ZError::BadCompressionMethod => "bad_compression_method",
            ZError::BadCompressionStrategy => "bad_compression_strategy",
            ZError::BadMemLevel => "bad_memlevel",
            ZError::BadWindowBits => "bad_windowbits",
            ZError::Badarg => "badarg",
            ZError::NoMemory => "enomem",
            ZError::NotOnControllingProcess => "not_on_controlling_process",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Flush {
    None,
    Sync,
    Full,
    Finish,
}

impl Flush {
    fn zlib(self) -> DeflateFlush {
        match self {
            Flush::None => DeflateFlush::NoFlush,
            Flush::Sync => DeflateFlush::SyncFlush,
            Flush::Full => DeflateFlush::FullFlush,
            Flush::Finish => DeflateFlush::Finish,
        }
    }
}

/// Validated `deflateInit/6` arguments.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DeflateParams {
    pub level: i32,
    pub window_bits: i32,
    pub mem_level: i32,
    pub strategy: Strategy,
}

impl DeflateParams {
    pub(crate) fn check_window_bits(bits: i64) -> Result<i32, ZError> {
        match bits {
            8..=15 | -15..=-8 | 24..=31 => Ok(bits as i32),
            _ => Err(ZError::BadWindowBits),
        }
    }
    pub(crate) fn check_mem_level(level: i64) -> Result<i32, ZError> {
        if (1..=9).contains(&level) {
            Ok(level as i32)
        } else {
            Err(ZError::BadMemLevel)
        }
    }
    pub(crate) fn check_level(level: i64) -> Result<i32, ZError> {
        if (-1..=9).contains(&level) {
            Ok(level as i32)
        } else {
            Err(ZError::BadCompressionLevel)
        }
    }
}

/// Result of one inflate step.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum InflateStep {
    Continue(Vec<u8>),
    Finished(Vec<u8>),
}

struct DeflateState {
    z: Box<Deflate>,
    /// Bytes fed since the last init/reset, to model `deflateEnd` on a busy stream.
    dirty: bool,
}

struct InflateState {
    z: Box<Inflate>,
    zlib_header: bool,
    window_bits: u8,
    ended: bool,
    /// Queued input not yet consumed by the decompressor.
    pending: Vec<u8>,
}

enum State {
    Uninit,
    Deflate(DeflateState),
    Inflate(InflateState),
    Closed,
}

/// A `zlib:open/0` stream.
pub(crate) struct ZStream {
    state: Mutex<State>,
    /// The process allowed to use the stream, as on OTP.
    owner: Mutex<Option<crate::process::ProcessId>>,
}

// SAFETY: this is the sole native type associated with this resource kind.
unsafe impl crate::resource::ReferenceResource for ZStream {
    const KIND: crate::term::ReferenceKind = crate::term::ReferenceKind::Zlib;
}

struct ZlibRegistry {
    map: RwLock<HashMap<u64, Arc<ZStream>>>,
}

static REGISTRY: OnceLock<ZlibRegistry> = OnceLock::new();

fn registry() -> &'static ZlibRegistry {
    REGISTRY.get_or_init(|| ZlibRegistry {
        map: RwLock::new(HashMap::new()),
    })
}

/// Create an uninitialized stream and return its typed, node-retained handle.
pub(crate) fn zlib_open(owner: crate::process::ProcessId) -> crate::resource::ResourceRef<ZStream> {
    let stream = Arc::new(ZStream::new());
    *stream.owner.lock().unwrap() = Some(owner);
    let resource = crate::resource::ResourceRef::from_immortal_arc(Arc::clone(&stream));
    registry().map.write().unwrap().insert(resource.address(), stream);
    resource
}

fn deflate_err(e: DeflateError) -> ZError {
    match e {
        DeflateError::StreamError => ZError::StreamError,
        DeflateError::DataError => ZError::DataError,
        DeflateError::MemError => ZError::NoMemory,
    }
}

fn inflate_err(e: InflateError) -> ZError {
    match e {
        InflateError::NeedDict { .. } => ZError::NeedDictionary,
        InflateError::StreamError => ZError::StreamError,
        InflateError::DataError => ZError::DataError,
        InflateError::MemError => ZError::NoMemory,
    }
}

impl ZStream {
    pub(crate) fn new() -> Self {
        ZStream { state: Mutex::new(State::Uninit), owner: Mutex::new(None) }
    }

    /// Only the controlling process may use a stream.
    pub(crate) fn check_owner(&self, caller: crate::process::ProcessId) -> Result<(), ZError> {
        match *self.owner.lock().unwrap() {
            Some(owner) if owner != caller => Err(ZError::NotOnControllingProcess),
            _ => Ok(()),
        }
    }

    pub(crate) fn set_owner(&self, owner: crate::process::ProcessId) {
        *self.owner.lock().unwrap() = Some(owner);
    }

    pub(crate) fn deflate_init(&self, p: DeflateParams) -> Result<(), ZError> {
        let mut st = self.state.lock().unwrap();
        if !matches!(*st, State::Uninit) {
            return Err(ZError::AlreadyInitialized);
        }
        let config = DeflateConfig {
            level: p.level,
            method: Method::Deflated,
            window_bits: p.window_bits,
            mem_level: p.mem_level,
            strategy: p.strategy,
        };
        *st = State::Deflate(DeflateState {
            z: Box::new(Deflate::new_with_config(config)),
            dirty: false,
        });
        Ok(())
    }

    /// `window_bits`: 8..15 for a zlib header, -15..-8 for raw deflate.
    pub(crate) fn inflate_init(&self, window_bits: i64) -> Result<(), ZError> {
        let (zlib_header, bits) = match window_bits {
            8..=15 => (true, window_bits as u8),
            -15..=-8 => (false, (-window_bits) as u8),
            _ => return Err(ZError::BadWindowBits),
        };
        let mut st = self.state.lock().unwrap();
        if !matches!(*st, State::Uninit) {
            return Err(ZError::AlreadyInitialized);
        }
        *st = State::Inflate(InflateState {
            z: Box::new(Inflate::new(zlib_header, bits)),
            zlib_header,
            window_bits: bits,
            ended: false,
            pending: Vec::new(),
        });
        Ok(())
    }

    /// Compress `data`, returning all output chunks produced.
    pub(crate) fn deflate(&self, data: &[u8], flush: Flush) -> Result<Vec<Vec<u8>>, ZError> {
        let mut st = self.state.lock().unwrap();
        let State::Deflate(d) = &mut *st else {
            return Err(ZError::NotInitialized);
        };
        let mut chunks = Vec::new();
        let mut input = data;
        if !data.is_empty() {
            d.dirty = true;
        }
        loop {
            let mut out = vec![0u8; DEFLATE_OUT_CHUNK];
            let (in0, out0) = (d.z.total_in(), d.z.total_out());
            let status = d
                .z
                .compress(input, &mut out, flush.zlib())
                .map_err(deflate_err)?;
            let used = (d.z.total_in() - in0) as usize;
            let produced = (d.z.total_out() - out0) as usize;
            input = &input[used..];
            out.truncate(produced);
            if produced > 0 {
                chunks.push(out);
            }
            match status {
                Status::StreamEnd => {
                    d.dirty = false;
                    break;
                }
                Status::BufError if produced == 0 && used == 0 => break,
                _ => {}
            }
            // Output space left over and input drained: nothing more to flush.
            if produced < DEFLATE_OUT_CHUNK && input.is_empty() {
                break;
            }
        }
        Ok(chunks)
    }

    pub(crate) fn deflate_reset(&self) -> Result<(), ZError> {
        let mut st = self.state.lock().unwrap();
        let State::Deflate(d) = &mut *st else {
            return Err(ZError::NotInitialized);
        };
        d.z.reset();
        d.dirty = false;
        Ok(())
    }

    pub(crate) fn deflate_end(&self) -> Result<(), ZError> {
        let mut st = self.state.lock().unwrap();
        let State::Deflate(d) = &*st else {
            return Err(ZError::NotInitialized);
        };
        let busy = d.dirty;
        *st = State::Uninit;
        if busy { Err(ZError::DataError) } else { Ok(()) }
    }

    /// Queue `data` and inflate at most one output chunk.
    pub(crate) fn safe_inflate(&self, data: &[u8]) -> Result<InflateStep, ZError> {
        let mut st = self.state.lock().unwrap();
        let State::Inflate(i) = &mut *st else {
            return Err(ZError::NotInitialized);
        };
        i.pending.extend_from_slice(data);
        let mut out = vec![0u8; INFLATE_OUT_CHUNK];
        let (in0, out0) = (i.z.total_in(), i.z.total_out());
        let status = i
            .z
            .decompress(&i.pending, &mut out, InflateFlush::NoFlush)
            .map_err(inflate_err)?;
        let used = (i.z.total_in() - in0) as usize;
        let produced = (i.z.total_out() - out0) as usize;
        i.pending.drain(..used);
        out.truncate(produced);
        if status == Status::StreamEnd {
            i.ended = true;
            i.pending.clear();
            return Ok(InflateStep::Finished(out));
        }
        if produced == INFLATE_OUT_CHUNK || (!i.pending.is_empty() && used > 0) {
            Ok(InflateStep::Continue(out))
        } else {
            i.pending.clear();
            Ok(InflateStep::Finished(out))
        }
    }

    /// Inflate everything queued, returning all output chunks.
    pub(crate) fn inflate(&self, data: &[u8]) -> Result<Vec<Vec<u8>>, ZError> {
        let mut chunks = Vec::new();
        let mut next = data;
        loop {
            match self.safe_inflate(next)? {
                InflateStep::Continue(c) => {
                    if !c.is_empty() {
                        chunks.push(c);
                    }
                    next = &[];
                }
                InflateStep::Finished(c) => {
                    if !c.is_empty() {
                        chunks.push(c);
                    }
                    return Ok(chunks);
                }
            }
        }
    }

    pub(crate) fn inflate_reset(&self) -> Result<(), ZError> {
        let mut st = self.state.lock().unwrap();
        let State::Inflate(i) = &mut *st else {
            return Err(ZError::NotInitialized);
        };
        i.z.reset(i.zlib_header);
        let _ = i.window_bits;
        i.ended = false;
        i.pending.clear();
        Ok(())
    }

    pub(crate) fn inflate_end(&self) -> Result<(), ZError> {
        let mut st = self.state.lock().unwrap();
        let State::Inflate(i) = &*st else {
            return Err(ZError::NotInitialized);
        };
        let ended = i.ended;
        *st = State::Uninit;
        if ended { Ok(()) } else { Err(ZError::DataError) }
    }

    /// Release the compression state. A closed stream rejects further use.
    pub(crate) fn close(&self) -> Result<(), ZError> {
        let mut st = self.state.lock().unwrap();
        if matches!(*st, State::Closed) {
            return Err(ZError::NotInitialized);
        }
        *st = State::Closed;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> DeflateParams {
        DeflateParams { level: 9, window_bits: -15, mem_level: 8, strategy: Strategy::Default }
    }

    #[test]
    fn raw_sync_flush_roundtrip_keeps_context_across_messages() {
        // Intent: cow_ws permessage-deflate shape, a long-lived deflate
        // context sync-flushed per message and inflated by a peer context.
        let d = ZStream::new();
        d.deflate_init(params()).unwrap();
        let i = ZStream::new();
        i.inflate_init(-15).unwrap();
        for msg in [&b"hello hello hello hello"[..], b"hello hello hello hello", b"another message"] {
            let packed: Vec<u8> = d.deflate(msg, Flush::Sync).unwrap().concat();
            assert!(packed.ends_with(&[0, 0, 0xff, 0xff]));
            let out: Vec<u8> = i.inflate(&packed).unwrap().concat();
            assert_eq!(out, msg);
        }
    }

    #[test]
    fn safe_inflate_continues_when_output_exceeds_one_chunk() {
        let d = ZStream::new();
        d.deflate_init(params()).unwrap();
        let big = vec![b'a'; INFLATE_OUT_CHUNK * 3 + 5];
        let packed: Vec<u8> = d.deflate(&big, Flush::Finish).unwrap().concat();
        let i = ZStream::new();
        i.inflate_init(-15).unwrap();
        let mut got = Vec::new();
        let mut steps = 0;
        let mut step = i.safe_inflate(&packed).unwrap();
        loop {
            steps += 1;
            match step {
                InflateStep::Continue(c) => {
                    got.extend(c);
                    step = i.safe_inflate(&[]).unwrap();
                }
                InflateStep::Finished(c) => {
                    got.extend(c);
                    break;
                }
            }
        }
        assert_eq!(got, big);
        assert!(steps >= 4);
        assert_eq!(i.inflate_end(), Ok(()));
    }

    #[test]
    fn state_errors_match_otp() {
        let z = ZStream::new();
        assert_eq!(z.deflate(b"x", Flush::Sync).unwrap_err(), ZError::NotInitialized);
        z.deflate_init(params()).unwrap();
        assert_eq!(z.deflate_init(params()), Err(ZError::AlreadyInitialized));
        assert_eq!(z.inflate_init(-15), Err(ZError::AlreadyInitialized));
        assert_eq!(z.inflate_reset(), Err(ZError::NotInitialized));
        assert_eq!(z.safe_inflate(b"x").unwrap_err(), ZError::NotInitialized);
        assert_eq!(z.close(), Ok(()));
        assert_eq!(z.close(), Err(ZError::NotInitialized));
    }

    #[test]
    fn deflate_after_finish_is_stream_error_until_reset() {
        let z = ZStream::new();
        z.deflate_init(params()).unwrap();
        z.deflate(b"abc", Flush::Finish).unwrap();
        assert_eq!(z.deflate(b"more", Flush::Sync).unwrap_err(), ZError::StreamError);
        z.deflate_reset().unwrap();
        assert!(!z.deflate(b"more", Flush::Sync).unwrap().is_empty());
    }

    #[test]
    fn garbage_input_is_data_error() {
        let z = ZStream::new();
        z.inflate_init(-15).unwrap();
        assert_eq!(z.safe_inflate(b"garbage garbage").unwrap_err(), ZError::DataError);
    }

    #[test]
    fn bad_parameters_use_otp_reasons() {
        assert_eq!(DeflateParams::check_window_bits(99), Err(ZError::BadWindowBits));
        assert_eq!(DeflateParams::check_mem_level(10), Err(ZError::BadMemLevel));
        assert_eq!(DeflateParams::check_level(12), Err(ZError::BadCompressionLevel));
        assert_eq!(ZStream::new().inflate_init(99), Err(ZError::BadWindowBits));
    }
}
