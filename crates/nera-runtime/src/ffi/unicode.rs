//! UTF-8 Unicode chardata conversion, including fragments spanning binaries.
use crate::process::ProcessContext;
use crate::term::{BoxedHeader, ErlTerm};
use super::helpers::*;

/// A `unicode:encoding()`: `unicode` is an alias of `utf8`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Encoding { Latin1, Utf8, Utf16 { big: bool }, Utf32 { big: bool } }

impl Encoding {
    /// Read `latin1 | unicode | utf8 | utf16 | utf32 | {utf16 | utf32, big | little}`.
    unsafe fn from_term(term: ErlTerm) -> Option<Self> {
        let table = crate::atom_table::global_atom_table();
        let name = |t: ErlTerm| t.as_atom_id().and_then(|id| table.resolve(id));
        if term.is_tuple() {
            let ptr = term.as_boxed_ptr()?;
            if unsafe { BoxedHeader(*ptr).arity() } != 2 { return None; }
            let (kind, order) = unsafe { (ErlTerm(*ptr.add(1)), ErlTerm(*ptr.add(2))) };
            let big = match name(order)?.as_str() { "big" => true, "little" => false, _ => return None };
            return match name(kind)?.as_str() {
                "utf16" => Some(Self::Utf16 { big }),
                "utf32" => Some(Self::Utf32 { big }),
                _ => None,
            };
        }
        match name(term)?.as_str() {
            "latin1" => Some(Self::Latin1),
            "unicode" | "utf8" => Some(Self::Utf8),
            "utf16" => Some(Self::Utf16 { big: true }),
            "utf32" => Some(Self::Utf32 { big: true }),
            _ => None,
        }
    }

    /// Append one scalar in this encoding; false when it cannot be represented.
    fn encode(self, ch: char, out: &mut Vec<u8>) -> bool {
        let cp = ch as u32;
        match self {
            Self::Latin1 => { if cp > 255 { return false; } out.push(cp as u8); }
            Self::Utf8 => { let mut buf = [0; 4]; out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes()); }
            Self::Utf16 { big } => {
                let mut units = [0u16; 2];
                for unit in ch.encode_utf16(&mut units) {
                    out.extend_from_slice(&if big { unit.to_be_bytes() } else { unit.to_le_bytes() });
                }
            }
            Self::Utf32 { big } => out.extend_from_slice(&if big { cp.to_be_bytes() } else { cp.to_le_bytes() }),
        }
        true
    }
}

/// Result of decoding the first character of a byte string.
enum Decoded { Char(char, usize), Incomplete, Invalid }

/// Decode one character; a short but still valid prefix is `Incomplete`.
fn decode_one(enc: Encoding, b: &[u8]) -> Decoded {
    if b.is_empty() { return Decoded::Incomplete; }
    match enc {
        Encoding::Latin1 => Decoded::Char(b[0] as char, 1),
        Encoding::Utf8 => match utf8_width(b[0]) {
            Some(width) if b.len() < width =>
                if incomplete_prefix(b) { Decoded::Incomplete } else { Decoded::Invalid },
            Some(width) => match std::str::from_utf8(&b[..width]).ok().and_then(|s| s.chars().next()) {
                Some(ch) => Decoded::Char(ch, width),
                None => Decoded::Invalid,
            },
            None if b[0] < 0x80 => Decoded::Char(b[0] as char, 1),
            None => Decoded::Invalid,
        },
        Encoding::Utf16 { big } => {
            if b.len() < 2 { return Decoded::Incomplete; }
            let unit = |i: usize| if big { u16::from_be_bytes([b[i], b[i + 1]]) } else { u16::from_le_bytes([b[i], b[i + 1]]) };
            let first = unit(0);
            match first {
                0xd800..=0xdbff => {
                    if b.len() == 2 { return Decoded::Incomplete; }
                    if b.len() == 3 {
                        // Only a big-endian third byte reveals the next unit's high byte.
                        return if big && (0xdc..=0xdf).contains(&b[2]) || !big { Decoded::Incomplete } else { Decoded::Invalid };
                    }
                    let second = unit(2);
                    if (0xdc00..=0xdfff).contains(&second) {
                        let cp = 0x10000 + (((first as u32) - 0xd800) << 10) + (second as u32 - 0xdc00);
                        char::from_u32(cp).map_or(Decoded::Invalid, |ch| Decoded::Char(ch, 4))
                    } else { Decoded::Invalid }
                }
                0xdc00..=0xdfff => Decoded::Invalid,
                _ => Decoded::Char(char::from_u32(first as u32).unwrap(), 2),
            }
        }
        Encoding::Utf32 { big } => {
            if b.len() >= 4 {
                let word = [b[0], b[1], b[2], b[3]];
                let cp = if big { u32::from_be_bytes(word) } else { u32::from_le_bytes(word) };
                return char::from_u32(cp).map_or(Decoded::Invalid, |ch| Decoded::Char(ch, 4));
            }
            // Fewer than four bytes: valid only while some scalar can still begin this way.
            // Bytes are named from most to least significant. OTP checks the
            // surrogate range only for big endian; little endian only bounds the
            // third byte once three have arrived.
            let (top_ok, mid, low) = if big {
                (b[0] == 0, b.get(1).copied(), b.get(2).copied())
            } else if b.len() == 3 {
                (true, Some(b[2]), None)
            } else { (true, None, None) };
            let valid = top_ok && mid.is_none_or(|m| m <= 0x10)
                && !(mid == Some(0) && low.is_some_and(|l| (0xd8..=0xdf).contains(&l)));
            if valid { Decoded::Incomplete } else { Decoded::Invalid }
        }
    }
}

struct Failure {
    rest: ErlTerm,
    skip: Option<usize>,
    parents: Vec<ErlTerm>,
    pending: Vec<u8>,
    /// Wrap the failing list element in a one-element list, as OTP does for
    /// integers a non-UTF-8 output cannot represent.
    wrap: bool,
    flat: bool,
}

struct Converter {
    input: Encoding,
    output: Encoding,
    bytes: Vec<u8>,
    pending: Vec<u8>,
    failure: Option<Failure>,
}

impl Default for Converter {
    fn default() -> Self {
        Self::new(Encoding::Utf8, Encoding::Utf8)
    }
}

fn utf8_width(first: u8) -> Option<usize> {
    match first { 0xc0..=0xdf => Some(2), 0xe0..=0xef => Some(3), 0xf0..=0xf7 => Some(4), _ => None }
}

// OTP reports incomplete prefixes before validating scalar bounds. For example,
// an isolated C0 and ED A0 are incomplete even though completing them is invalid.
fn incomplete_prefix(bytes: &[u8]) -> bool {
    !bytes.is_empty() && utf8_width(bytes[0]).is_some_and(|width|
        bytes.len() < width && bytes[1..].iter().all(|&b| b & 0xc0 == 0x80))
}

impl Converter {
    fn new(input: Encoding, output: Encoding) -> Self {
        Self { input, output, bytes: Vec::new(), pending: Vec::new(), failure: None }
    }

    fn fail(&mut self, rest: ErlTerm, skip: Option<usize>, parents: &[ErlTerm]) -> bool {
        self.failure = Some(Failure { rest, skip, parents: parents.to_vec(), pending: std::mem::take(&mut self.pending), wrap: false,
            flat: self.input != Encoding::Utf8 || self.output != Encoding::Utf8 });
        false
    }

    /// Fail on a list cell whose integer head cannot be converted.
    fn fail_cell(&mut self, cell: ErlTerm, parents: &[ErlTerm]) -> bool {
        // OTP has fast paths for latin1 and UTF-8 input to UTF-8 output that
        // report the cell itself; every other conversion wraps the element.
        let wrap = !(self.output == Encoding::Utf8 && matches!(self.input, Encoding::Latin1 | Encoding::Utf8));
        self.fail(cell, None, parents);
        if let Some(failure) = &mut self.failure { failure.wrap = wrap; }
        false
    }

    /// Bytes the character at the start of `b` occupies in the input encoding.
    fn char_width(&self, b: &[u8]) -> usize {
        match self.input {
            Encoding::Latin1 => 1,
            Encoding::Utf8 => b.first().and_then(|&f| utf8_width(f)).unwrap_or(1),
            Encoding::Utf16 { big } => {
                let high = if big { b.first() } else { b.get(1) };
                if b.len() >= 2 && high.is_some_and(|h| (0xd8..=0xdb).contains(h)) { 4 } else { 2 }
            }
            Encoding::Utf32 { .. } => 4,
        }
    }

    /// Convert a binary whose input or output is not plain UTF-8: decode one
    /// character at a time, carrying a trailing partial character forward.
    unsafe fn generic_binary(&mut self, term: ErlTerm, parents: &[ErlTerm]) -> Result<bool, ()> {
        let (ptr, len) = unsafe { binary_data(term) }.ok_or(())?;
        let data = unsafe { std::slice::from_raw_parts(ptr, len) };
        let carried = self.pending.len();
        let mut buf = std::mem::take(&mut self.pending);
        buf.extend_from_slice(data);
        let mut pos = 0;
        // A character begun in an earlier binary is not validated until all its
        // bytes have arrived, as in OTP.
        if carried > 0 && buf.len() < self.char_width(&buf) {
            self.pending = buf;
            return Ok(true);
        }
        while pos < buf.len() {
            match decode_one(self.input, &buf[pos..]) {
                Decoded::Char(ch, width) => {
                    if !self.output.encode(ch, &mut self.bytes) {
                        // A character begun in an earlier binary is reported whole: the
                        // bytes it took from this binary join the pending prefix.
                        if pos < carried {
                            let end = pos + width;
                            self.pending = buf[pos..end].to_vec();
                            return Ok(self.fail(term, Some(end - carried), parents));
                        }
                        return Ok(self.fail(term, Some(pos - carried), parents));
                    }
                    pos += width;
                }
                Decoded::Incomplete => { self.pending = buf[pos..].to_vec(); return Ok(true); }
                Decoded::Invalid => {
                    if pos < carried {
                        // The bad character began in an earlier binary: report the bytes
                        // that decided it as part of the pending prefix.
                        let end = (pos + 4).min(buf.len());
                        self.pending = buf[..end].to_vec();
                        return Ok(self.fail(term, Some(end - carried), parents));
                    }
                    return Ok(self.fail(term, Some(pos - carried), parents));
                }
            }
        }
        Ok(true)
    }

    /// Source graphs remain live and unmoved; no heap allocation or GC occurs.
    unsafe fn binary(&mut self, term: ErlTerm, parents: &[ErlTerm]) -> Result<bool, ()> {
        if self.input != Encoding::Utf8 || self.output != Encoding::Utf8 {
            return unsafe { self.generic_binary(term, parents) };
        }
        let (ptr, len) = unsafe { binary_data(term) }.ok_or(())?;
        let data = unsafe { std::slice::from_raw_parts(ptr, len) };
        let mut consumed = 0;
        if !self.pending.is_empty() {
            let Some(width) = utf8_width(self.pending[0]) else { return Ok(self.fail(term, Some(0), parents)); };
            let missing = width - self.pending.len();
            if len < missing { self.pending.extend_from_slice(data); return Ok(true); }
            let mut complete = self.pending.clone();
            complete.extend_from_slice(&data[..missing]);
            if std::str::from_utf8(&complete).is_err() { return Ok(self.fail(term, Some(0), parents)); }
            self.bytes.extend_from_slice(&complete);
            self.pending.clear();
            consumed = missing;
        }
        match std::str::from_utf8(&data[consumed..]) {
            Ok(_) => { self.bytes.extend_from_slice(&data[consumed..]); Ok(true) }
            Err(error) => {
                let end = consumed + error.valid_up_to();
                self.bytes.extend_from_slice(&data[consumed..end]);
                if incomplete_prefix(&data[end..]) {
                    self.pending.extend_from_slice(&data[end..]);
                    Ok(true)
                } else { Ok(self.fail(term, Some(end), parents)) }
            }
        }
    }

    /// Source graphs remain live and unmoved; no heap allocation or GC occurs.
    unsafe fn visit(&mut self, term: ErlTerm, parents: &mut Vec<ErlTerm>) -> Result<bool, ()> {
        if term.is_nil() { return Ok(true); }
        if term.is_binary() { return unsafe { self.binary(term, parents) }; }
        let mut rest = term;
        while let Some(cell) = rest.as_cons_ptr() {
            let (head, tail) = unsafe { ((*cell).head, (*cell).tail) };
            if head.as_small_int().is_some() || crate::bigint::term_to_bigint(head).is_some() {
                if !self.pending.is_empty() { return Ok(self.fail(rest, None, parents)); }
                let cp = head.as_small_int().and_then(|n| u32::try_from(n).ok())
                    .filter(|&n| self.input != Encoding::Latin1 || n <= 255)
                    .and_then(char::from_u32);
                let Some(ch) = cp else { return Ok(self.fail_cell(rest, parents)); };
                if !self.output.encode(ch, &mut self.bytes) { return Ok(self.fail_cell(rest, parents)); }
            } else {
                parents.push(tail);
                let complete = unsafe { self.visit(head, parents) }?;
                parents.pop();
                if !complete { return Ok(false); }
            }
            rest = tail;
        }
        if rest.is_nil() { Ok(true) }
        else if rest.is_binary() { unsafe { self.binary(rest, parents) } }
        else { Err(()) }
    }
}

/// Decode valid Unicode chardata for formatting without allocating on the Erlang heap.
/// # Safety
/// The complete source graph must remain live, immutable and unmoved.
pub(crate) unsafe fn chardata_to_utf8(data: ErlTerm) -> Result<String, ()> {
    let mut converter = Converter::default();
    if !unsafe { converter.visit(data, &mut Vec::new()) }? || converter.failure.is_some() || !converter.pending.is_empty() {
        return Err(());
    }
    String::from_utf8(converter.bytes).map_err(|_| ())
}

/// Build the `{error | incomplete, Converted, Rest}` tuple, or return the
/// converted value when the whole input converted.
unsafe fn finish(
    ctx: *mut ProcessContext,
    converter: Converter,
    convert: impl FnOnce(&mut crate::heap::ProcessHeap, &[u8]) -> ErlTerm,
) -> u64 {
    let incomplete = !converter.pending.is_empty()
        && (converter.input != Encoding::Utf8 || incomplete_prefix(&converter.pending));
    unsafe { with_ctx_heap(ctx, |heap| {
        let converted = convert(heap, &converter.bytes);
        if let Some(failure) = converter.failure {
            let mut rest = if let Some(skip) = failure.skip {
                let (ptr, len) = binary_data(failure.rest).unwrap();
                heap.alloc_binary(&std::slice::from_raw_parts(ptr, len)[skip..])
            } else if failure.wrap {
                let cell = &*failure.rest.as_cons_ptr().unwrap();
                let element = heap.alloc_cons(cell.head, ErlTerm::nil());
                heap.alloc_cons(element, cell.tail)
            } else { failure.rest };
            for tail in failure.parents.into_iter().rev() { rest = heap.alloc_cons(rest, tail); }
            if !failure.pending.is_empty() {
                let pending = heap.alloc_binary(&failure.pending);
                // Between UTF-8 and UTF-8 the rest follows the prefix as one element;
                // any other conversion continues the list with it.
                let suffix = if failure.flat { rest } else { heap.alloc_cons(rest, ErlTerm::nil()) };
                rest = heap.alloc_cons(pending, suffix);
            }
            let tag = ErlTerm::atom(crate::atom_table::global_atom_table().intern("error"));
            heap.alloc_tuple(&[tag, converted, rest]).0
        } else if !converter.pending.is_empty() {
            let pending = heap.alloc_binary(&converter.pending);
            let tag = ErlTerm::atom(crate::atom_table::global_atom_table().intern(if incomplete { "incomplete" } else { "error" }));
            heap.alloc_tuple(&[tag, converted, pending]).0
        } else { converted.0 }
    }) }
}

unsafe fn to_binary(ctx: *mut ProcessContext, data: u64, input: Encoding, output: Encoding) -> u64 {
    let mut converter = Converter::new(input, output);
    if unsafe { converter.visit(ErlTerm(data), &mut Vec::new()) }.is_err() { return unsafe { raise_badarg(ctx) }; }
    unsafe { finish(ctx, converter, |heap, bytes| heap.alloc_binary(bytes)) }
}

unsafe fn to_list(ctx: *mut ProcessContext, data: u64, input: Encoding) -> u64 {
    let mut converter = Converter::new(input, Encoding::Utf8);
    if unsafe { converter.visit(ErlTerm(data), &mut Vec::new()) }.is_err() { return unsafe { raise_badarg(ctx) }; }
    unsafe { finish(ctx, converter, |heap, bytes| {
        // The converter only ever wrote whole valid UTF-8 characters.
        let chars: Vec<ErlTerm> = String::from_utf8_lossy(bytes).chars().map(|c| ErlTerm::small_int(c as i64)).collect();
        heap.alloc_list_from_slice(&chars)
    }) }
}

#[no_mangle]
/// # Safety
/// The context and complete source graph must remain live, immutable and unmoved.
/// Heap access must be exclusive as required by `with_ctx_heap`.
pub unsafe extern "C" fn nera_rt_unicode_characters_to_binary(ctx: *mut ProcessContext, data: u64) -> u64 {
    unsafe { to_binary(ctx, data, Encoding::Utf8, Encoding::Utf8) }
}

#[no_mangle]
/// # Safety
/// As [`nera_rt_unicode_characters_to_binary`]; `input` is an encoding term.
pub unsafe extern "C" fn nera_rt_unicode_characters_to_binary_2(ctx: *mut ProcessContext, data: u64, input: u64) -> u64 {
    let Some(input) = (unsafe { Encoding::from_term(ErlTerm(input)) }) else { return unsafe { raise_badarg(ctx) }; };
    unsafe { to_binary(ctx, data, input, Encoding::Utf8) }
}

#[no_mangle]
/// # Safety
/// As [`nera_rt_unicode_characters_to_binary`]; `input` and `output` are encoding terms.
pub unsafe extern "C" fn nera_rt_unicode_characters_to_binary_3(ctx: *mut ProcessContext, data: u64, input: u64, output: u64) -> u64 {
    let (Some(input), Some(output)) = (unsafe { Encoding::from_term(ErlTerm(input)) }, unsafe { Encoding::from_term(ErlTerm(output)) })
        else { return unsafe { raise_badarg(ctx) }; };
    unsafe { to_binary(ctx, data, input, output) }
}

#[no_mangle]
/// # Safety
/// As [`nera_rt_unicode_characters_to_binary`].
pub unsafe extern "C" fn nera_rt_unicode_characters_to_list(ctx: *mut ProcessContext, data: u64) -> u64 {
    unsafe { to_list(ctx, data, Encoding::Utf8) }
}

#[no_mangle]
/// # Safety
/// As [`nera_rt_unicode_characters_to_binary`]; `input` is an encoding term.
pub unsafe extern "C" fn nera_rt_unicode_characters_to_list_2(ctx: *mut ProcessContext, data: u64, input: u64) -> u64 {
    let Some(input) = (unsafe { Encoding::from_term(ErlTerm(input)) }) else { return unsafe { raise_badarg(ctx) }; };
    unsafe { to_list(ctx, data, input) }
}
