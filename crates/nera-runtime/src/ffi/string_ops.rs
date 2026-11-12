//! Unicode-aware string operations. No Erlang heap allocation occurs while
//! decoding input terms; the result is allocated only after owned decoding.
use super::helpers::{binary_data, raise_badarg, raise_error, with_ctx_heap};
use crate::atom_table::{global_atom_table, ATOM_BADARG};
use crate::process::ProcessContext;
use crate::term::ErlTerm;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, PartialEq)]
enum TrimPattern {
    Scalar(i64),
    Cluster(Vec<i64>),
    Binary,
}

fn trim_bounds(text: &str, patterns: &[TrimPattern], leading: bool, trailing: bool) -> (usize, usize) {
    let matches = |g: &str| {
        let points: Vec<i64> = g.chars().map(|c| c as i64).collect();
        let key = if points.len() == 1 { TrimPattern::Scalar(points[0]) } else { TrimPattern::Cluster(points) };
        patterns.contains(&key)
    };
    let mut start = 0;
    let mut end = text.len();
    if leading {
        for (offset, g) in text.grapheme_indices(true) {
            if !matches(g) { break; }
            start = offset + g.len();
        }
    }
    if trailing {
        for (offset, g) in text[start..].grapheme_indices(true).rev() {
            if !matches(g) { break; }
            end = start + offset;
        }
    }
    (start, end)
}

// The offending binary is retained for OTP's {badarg, Binary} UTF-8 error.
enum InputError { Clause, Utf8(ErlTerm, usize) }

unsafe fn decode_chardata(input: ErlTerm) -> Result<String, InputError> {
    let mut text = String::new();
    let mut pending = vec![(input, false)];
    while let Some((term, scalar_allowed)) = pending.pop() {
        if term.is_nil() { continue; }
        if term.is_binary() {
            let (ptr, len) = binary_data(term).unwrap();
            let bytes = std::slice::from_raw_parts(ptr, len);
            text.push_str(std::str::from_utf8(bytes).map_err(|e| InputError::Utf8(term, e.valid_up_to()))?);
        } else if let Some(ptr) = term.as_cons_ptr() {
            pending.push(((*ptr).tail, false));
            pending.push(((*ptr).head, true));
        } else if scalar_allowed {
            let cp = term.as_small_int().and_then(|v| u32::try_from(v).ok()).and_then(char::from_u32)
                .ok_or(InputError::Clause)?;
            text.push(cp);
        } else { return Err(InputError::Clause); }
    }
    Ok(text)
}

/// string:trim/3. Binary input stays binary; list input may be normalized to
/// a flat codepoint list. Empty trim sets return the original term unchanged.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_string_trim(
    ctx: *mut ProcessContext, input: u64, direction: u64, characters: u64,
) -> u64 {
    let input = ErlTerm(input);
    let mut cursor = ErlTerm(characters);
    if cursor.is_nil() { return input.0; }
    let clause = || ErlTerm::atom(global_atom_table().intern("function_clause")).0;
    let direction = ErlTerm(direction).as_atom_id();
    let leading = direction == global_atom_table().lookup_by_name("leading");
    let trailing = direction == global_atom_table().lookup_by_name("trailing");
    let both = direction == global_atom_table().lookup_by_name("both");
    if direction.is_none() || !(leading || trailing || both) { return raise_error(ctx, clause()); }
    if !cursor.is_cons() { return raise_error(ctx, clause()); }
    let mut patterns = Vec::new();
    while let Some(ptr) = cursor.as_cons_ptr() {
        let item = (*ptr).head;
        let pattern = if let Some(cp) = item.as_small_int() { TrimPattern::Scalar(cp) }
        else if item.is_binary() { TrimPattern::Binary }
        else if item.is_cons() || item.is_nil() {
            let mut points = Vec::new();
            let mut rest = item;
            while let Some(cell) = rest.as_cons_ptr() {
                match (*cell).head.as_small_int() {
                    Some(cp) => points.push(cp),
                    None => return raise_error(ctx, clause()),
                }
                rest = (*cell).tail;
            }
            if !rest.is_nil() { return raise_badarg(ctx); }
            TrimPattern::Cluster(points)
        } else { return raise_error(ctx, clause()); };
        patterns.push(pattern);
        cursor = (*ptr).tail;
    }
    if !cursor.is_nil() { return raise_badarg(ctx); }
    let text = match decode_chardata(input) {
        Ok(text) => text,
        Err(InputError::Clause) => return raise_error(ctx, clause()),
        Err(InputError::Utf8(binary, _)) => {
            let reason = with_ctx_heap(ctx, |heap| heap.alloc_tuple(&[ErlTerm::atom(ATOM_BADARG), binary]));
            return raise_error(ctx, reason.0);
        }
    };
    let (start, end) = trim_bounds(&text, &patterns, leading || both, trailing || both);
    if start == 0 && end == text.len() { return input.0; }
    let result = &text[start..end];
    if input.is_binary() {
        with_ctx_heap(ctx, |heap| heap.alloc_binary_auto(result.as_bytes()).0)
    } else {
        let points: Vec<_> = result.chars().map(|c| ErlTerm::small_int(c as i64)).collect();
        with_ctx_heap(ctx, |heap| heap.alloc_list_from_slice(&points).0)
    }
}

/// `{badarg, Rest}` where Rest is the binary from the first invalid byte on,
/// which is the payload OTP's string functions report for bad UTF-8.
unsafe fn raise_badarg_with(ctx: *mut ProcessContext, payload: ErlTerm) -> u64 {
    let reason = with_ctx_heap(ctx, |heap| heap.alloc_tuple(&[ErlTerm::atom(ATOM_BADARG), payload]));
    raise_error(ctx, reason.0)
}

unsafe fn raise_badarg_rest(ctx: *mut ProcessContext, binary: ErlTerm, offset: usize) -> u64 {
    if offset == 0 { return raise_badarg_with(ctx, binary); }
    let (ptr, len) = binary_data(binary).unwrap();
    let rest = std::slice::from_raw_parts(ptr, len)[offset..].to_vec();
    let rest = with_ctx_heap(ctx, |heap| heap.alloc_binary_auto(&rest));
    raise_badarg_with(ctx, rest)
}

unsafe fn raise_input_error(ctx: *mut ProcessContext, error: InputError) -> u64 {
    match error {
        InputError::Clause => raise_error(ctx, ErlTerm::atom(global_atom_table().intern("function_clause")).0),
        InputError::Utf8(binary, offset) => raise_badarg_rest(ctx, binary, offset),
    }
}

unsafe fn is_flat_codepoint_list(input: ErlTerm) -> bool {
    let mut cursor = input;
    while let Some(ptr) = cursor.as_cons_ptr() {
        if (*ptr).head.as_small_int().is_none() { return false; }
        cursor = (*ptr).tail;
    }
    cursor.is_nil()
}

unsafe fn codepoint_list(ctx: *mut ProcessContext, text: impl Iterator<Item = char>) -> u64 {
    let points: Vec<_> = text.map(|c| ErlTerm::small_int(c as i64)).collect();
    with_ctx_heap(ctx, |heap| heap.alloc_list_from_slice(&points).0)
}

/// string:lowercase/1. Full Unicode lowercase mapping applied one codepoint
/// at a time with no context rules (final sigma stays plain sigma), as OTP
/// does. Unchanged binaries and flat codepoint lists are returned as given;
/// any other changed or nested chardata comes back as a flat codepoint list.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_string_lowercase(ctx: *mut ProcessContext, input: u64) -> u64 {
    let input = ErlTerm(input);
    if input.is_nil() { return input.0; }
    if input.is_binary() {
        let (ptr, len) = binary_data(input).unwrap();
        let bytes = std::slice::from_raw_parts(ptr, len);
        let text = match std::str::from_utf8(bytes) {
            Ok(text) => text,
            Err(e) => return raise_badarg_rest(ctx, input, e.valid_up_to()),
        };
        let lowered: String = text.chars().flat_map(char::to_lowercase).collect();
        if lowered.as_bytes() == bytes { return input.0; }
        return with_ctx_heap(ctx, |heap| heap.alloc_binary_auto(lowered.as_bytes()).0);
    }
    if !input.is_cons() { return raise_badarg_with(ctx, input); }
    let text = match decode_chardata(input) {
        Ok(text) => text,
        Err(error) => return raise_input_error(ctx, error),
    };
    let lowered: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    if is_flat_codepoint_list(input) && lowered.len() == text.chars().count()
        && lowered.iter().copied().eq(text.chars()) {
        return input.0;
    }
    codepoint_list(ctx, lowered.into_iter())
}

enum IntArg { Small(i64), BigPositive, BigNegative }

unsafe fn int_arg(term: ErlTerm) -> Option<IntArg> {
    if let Some(v) = term.as_small_int() { return Some(IntArg::Small(v)); }
    if term.is_bigint() {
        let big = crate::bigint::term_to_bigint(term)?;
        return Some(if big.sign() == num_bigint::Sign::Minus { IntArg::BigNegative } else { IntArg::BigPositive });
    }
    None
}

/// Grapheme counts for the binary path, which mirrors OTP's lazy decoding:
/// invalid UTF-8 only matters once the scan has to look at it.
struct BinaryScan { ends: Vec<usize>, valid: usize, invalid: bool }

fn scan_binary(bytes: &[u8]) -> BinaryScan {
    let (text, invalid) = match std::str::from_utf8(bytes) {
        Ok(text) => (text, false),
        Err(e) => (std::str::from_utf8(&bytes[..e.valid_up_to()]).unwrap(), true),
    };
    let mut ends = vec![0];
    ends.extend(text.grapheme_indices(true).map(|(i, g)| i + g.len()));
    BinaryScan { ends, valid: text.len(), invalid }
}

/// Byte range of the slice, or the offset of the invalid byte that OTP trips on.
fn slice_binary_range(bytes: &[u8], skip: usize, take: Option<usize>) -> Result<(usize, usize), usize> {
    let scan = scan_binary(bytes);
    let graphemes = scan.ends.len() - 1;
    let start = if skip == 0 { 0 }
        else if skip <= graphemes { scan.ends[skip] }
        else if scan.invalid { return Err(scan.valid) }
        else { return Ok((bytes.len(), bytes.len())) };
    let Some(take) = take else { return Ok((start, bytes.len())) };
    if start == bytes.len() { return Ok((start, start)); }
    if skip == graphemes && scan.invalid { return Err(scan.valid); }
    if skip == 0 && graphemes == 0 { return Err(0); }
    let wanted = skip.saturating_add(take);
    // The last valid grapheme is only complete once the next byte is known.
    if wanted < graphemes || (wanted == graphemes && !scan.invalid) { return Ok((start, scan.ends[wanted])); }
    if scan.invalid { Err(scan.valid) } else { Ok((start, bytes.len())) }
}

/// string:slice/3 over graphemes. Binaries yield binaries; lists and mixed
/// chardata are decoded up front and yield flat codepoint lists (OTP may
/// hand back the original tail structure, and decodes lazily).
#[no_mangle]
pub unsafe extern "C" fn nera_rt_string_slice(
    ctx: *mut ProcessContext, input: u64, start: u64, length: u64,
) -> u64 {
    let input = ErlTerm(input);
    let clause = || raise_error(ctx, ErlTerm::atom(global_atom_table().intern("function_clause")).0);
    let length = ErlTerm(length);
    let take: Option<usize> = match int_arg(length) {
        Some(IntArg::Small(0)) => {
            return if input.is_binary() { with_ctx_heap(ctx, |heap| heap.alloc_binary_auto(&[]).0) } else { ErlTerm::nil().0 };
        }
        Some(IntArg::Small(n)) if n > 0 => Some(n as usize),
        Some(IntArg::BigPositive) => Some(usize::MAX),
        _ if length.as_atom_id().is_some() && length.as_atom_id() == global_atom_table().lookup_by_name("infinity") => None,
        _ => return clause(),
    };
    let skip: usize = match int_arg(ErlTerm(start)) {
        Some(IntArg::Small(n)) if n >= 0 => n as usize,
        Some(IntArg::BigPositive) => usize::MAX,
        _ => return clause(),
    };
    if input.is_binary() {
        let (ptr, len) = binary_data(input).unwrap();
        let bytes = std::slice::from_raw_parts(ptr, len);
        if skip == 0 && take.is_none() { return input.0; }
        return match slice_binary_range(bytes, skip, take) {
            Ok((from, to)) => {
                if from == 0 && to == len { return input.0; }
                let part = bytes[from..to].to_vec();
                with_ctx_heap(ctx, |heap| heap.alloc_binary_auto(&part).0)
            }
            Err(offset) => raise_badarg_rest(ctx, input, offset),
        };
    }
    if !input.is_cons() && !input.is_nil() { return clause(); }
    if skip == 0 && take.is_none() { return input.0; }
    let text = match decode_chardata(input) {
        Ok(text) => text,
        Err(error) => return raise_input_error(ctx, error),
    };
    let kept = text.graphemes(true).skip(skip).take(take.unwrap_or(usize::MAX)).flat_map(str::chars);
    codepoint_list(ctx, kept)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn graphemes_are_not_split_and_directions_are_independent() {
        let text = "a\u{301}xa\u{301}";
        assert_eq!(trim_bounds(text, &[TrimPattern::Scalar(97)], true, true), (0, text.len()));
        let cluster = [TrimPattern::Cluster(vec![97, 769])];
        assert_eq!(trim_bounds(text, &cluster, true, false), (3, 7));
        assert_eq!(trim_bounds(text, &cluster, false, true), (0, 4));
        assert_eq!(trim_bounds(text, &cluster, true, true), (3, 4));
    }
    #[test]
    fn crlf_and_empty_results() {
        assert_eq!(trim_bounds("\r\nx\r\n", &[TrimPattern::Scalar(13), TrimPattern::Scalar(10)], true, true), (0, 5));
        assert_eq!(trim_bounds("\r\nx\r\n", &[TrimPattern::Cluster(vec![13,10])], true, true), (2, 3));
        assert_eq!(trim_bounds("   ", &[TrimPattern::Scalar(32)], true, true), (3, 3));
    }
}
