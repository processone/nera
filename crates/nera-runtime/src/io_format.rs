//! Shared field parsing for stdout and io_lib formatting.

use crate::atom_table::global_atom_table;
use crate::bif;
use crate::term::{BoxedHeader, ErlTerm, BOXED_FLOAT};
use std::iter::Peekable;
use std::str::Chars;
use unicode_segmentation::UnicodeSegmentation;

fn argument(args: &[ErlTerm], index: &mut usize) -> Result<ErlTerm, ()> {
    let value = *args.get(*index).ok_or(())?;
    *index += 1;
    Ok(value)
}

fn number(
    chars: &mut Peekable<Chars<'_>>,
    args: &[ErlTerm],
    index: &mut usize,
) -> Result<Option<i64>, ()> {
    if chars.peek() == Some(&'*') {
        chars.next();
        return argument(args, index)?.as_small_int().map(Some).ok_or(());
    }
    let mut value = None;
    while let Some(digit) = chars.peek().and_then(|c| c.to_digit(10)) {
        chars.next();
        value = Some(
            value
                .unwrap_or(0_i64)
                .checked_mul(10)
                .and_then(|v| v.checked_add(digit as i64))
                .ok_or(())?,
        );
    }
    Ok(value)
}

fn size(value: i64) -> Result<usize, ()> {
    usize::try_from(value).map_err(|_| ())
}

fn repeat(ch: char, count: usize) -> Result<String, ()> {
    let mut output = String::new();
    output
        .try_reserve(count.checked_mul(ch.len_utf8()).ok_or(())?)
        .map_err(|_| ())?;
    output.extend(std::iter::repeat_n(ch, count));
    Ok(output)
}

fn adjust(text: &str, padding: &str, left: bool) -> String {
    if left {
        format!("{text}{padding}")
    } else {
        format!("{padding}{text}")
    }
}

/// Terms and numbers overflow to stars, rather than being truncated.
fn term_field(
    text: String,
    width: Option<usize>,
    precision: Option<usize>,
    pad: char,
    left: bool,
) -> Result<String, ()> {
    let Some(width) = width.or(precision) else {
        return Ok(text);
    };
    let length = text.chars().count();
    let limit = length.min(precision.unwrap_or(width).min(width));
    if length > limit {
        Ok(adjust(
            &repeat('*', limit)?,
            &repeat(pad, width - limit)?,
            left,
        ))
    } else {
        Ok(adjust(&text, &repeat(pad, width - length)?, left))
    }
}

fn string_field(
    text: String,
    width: Option<usize>,
    precision: Option<usize>,
    pad: char,
    left: bool,
    unicode: bool,
) -> Result<String, ()> {
    let left = width.is_none() || left;
    let Some(width) = width.or(precision) else {
        return Ok(text);
    };
    let limit = precision.unwrap_or(width);
    if width < limit {
        return Err(());
    }
    let parts: Vec<&str> = if unicode {
        text.graphemes(true).collect()
    } else {
        text.char_indices()
            .map(|(i, ch)| &text[i..i + ch.len_utf8()])
            .collect()
    };
    let length = parts.len().min(limit);
    let mut text = parts[..length].concat();
    if width == limit {
        return Ok(adjust(&text, &repeat(pad, width - length)?, left));
    }
    // Precision alone is left-adjusted; short strings fill their precision
    // before the whole precision-sized field is adjusted within the width.
    text.push_str(&repeat(pad, limit - length)?);
    Ok(adjust(&text, &repeat(pad, width - limit)?, left))
}

/// Read character data without accepting an integer as a root or list tail.
/// Unicode string directives decode each binary independently, falling back to
/// Latin-1 for that entire binary if UTF-8 decoding fails, as OTP does.
/// Prefixes use deep character lists, which do not allow binaries.
/// Source terms remain live and unmoved, with no Erlang heap allocation.
unsafe fn string_chars(term: ErlTerm, unicode: bool, binaries: bool) -> Result<String, ()> {
    let mut output = String::new();
    let mut pending = vec![(term, false)];
    while let Some((term, integer_allowed)) = pending.pop() {
        if term.is_nil() {
            continue;
        }
        if let Some(cell) = term.as_cons_ptr() {
            pending.push(((*cell).tail, false));
            pending.push(((*cell).head, true));
        } else if binaries && term.is_binary() {
            let (ptr, len) = crate::ffi::binary_data(term).ok_or(())?;
            let bytes = std::slice::from_raw_parts(ptr, len);
            if let Some(text) = unicode.then(|| std::str::from_utf8(bytes).ok()).flatten() {
                output.push_str(text);
            } else {
                output.extend(bytes.iter().map(|&b| char::from(b)));
            }
        } else if integer_allowed {
            let cp = term
                .as_small_int()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or(())?;
            if (!unicode && cp > 255) || (!binaries && matches!(cp, 0xfffe | 0xffff)) {
                return Err(());
            }
            output.push(char::from_u32(cp).ok_or(())?);
        } else {
            return Err(());
        }
    }
    Ok(output)
}

/// OTP rounds the 21 significant decimal digits from float_to_list/1, with
/// decimal half-up rounding (not Rust's direct binary-to-fixed tie-to-even).
fn float_field(value: f64, directive: char, precision: usize) -> Result<String, ()> {
    if !value.is_finite() || precision < if directive == 'e' { 2 } else { 1 } {
        return Err(());
    }
    let scientific = format!("{:.20e}", value.abs());
    let (mantissa, exp) = scientific.split_once('e').ok_or(())?;
    let exponent: i32 = exp.parse().map_err(|_| ())?;
    let mut digits: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).collect();
    let mut decimal = exponent + 1;
    let mut kind = directive;
    let mut precision = precision;
    if kind == 'g' {
        let magnitude = value.abs();
        let e: Option<i32> = if magnitude < 0.1 {
            None
        } else if magnitude < 1.0 {
            Some(-1)
        } else if magnitude < 10.0 {
            Some(0)
        } else if magnitude < 100.0 {
            Some(1)
        } else if magnitude < 1000.0 {
            Some(2)
        } else if magnitude < 10000.0 {
            Some(3)
        } else {
            None
        };
        if let Some(e) = e.filter(|&e| precision > (e + 1) as usize) {
            kind = 'f';
            precision = if e == -1 {
                precision
            } else {
                precision - 1 - e as usize
            };
        } else {
            kind = 'e';
            precision = precision.max(2);
        }
    }
    let before_dot = if kind == 'f' {
        if decimal <= 0 {
            let zeros = size(i64::from(1 - decimal))?;
            let mut padded = repeat('0', zeros)?.into_bytes();
            padded.extend(digits);
            digits = padded;
            decimal = 1;
        }
        size(i64::from(decimal))?
    } else {
        1
    };
    let decimals = if kind == 'f' {
        precision
    } else {
        precision - 1
    };
    let keep = before_dot.checked_add(decimals).ok_or(())?;
    let carry = digits.get(keep).is_some_and(|&d| d >= b'5');
    digits.truncate(keep);
    if keep > digits.len() {
        digits.try_reserve(keep - digits.len()).map_err(|_| ())?;
        digits.resize(keep, b'0');
    }
    let mut carry = carry;
    for digit in digits.iter_mut().rev() {
        if !carry {
            break;
        }
        if *digit == b'9' {
            *digit = b'0';
        } else {
            *digit += 1;
            carry = false;
        }
    }
    let mut before_dot = before_dot;
    if carry {
        digits.insert(0, b'1');
        if kind == 'f' {
            before_dot += 1;
        } else {
            digits.pop();
            decimal += 1;
        }
    }
    let mut output = String::from_utf8(digits).map_err(|_| ())?;
    output.insert(before_dot, '.');
    if kind == 'e' {
        output.push_str(&format!("e{:+}", decimal - 1));
    }
    if value.is_sign_negative() {
        output.insert(0, '-');
    }
    Ok(output)
}

/// Format a complete call before publishing any output.
/// # Safety
/// All input terms and reachable objects must remain live, immutable and
/// unmoved by GC. Binary backing bytes must not be concurrently mutated.
pub(crate) unsafe fn format_output(fmt: ErlTerm, args: ErlTerm) -> Result<String, ()> {
    let fmt = bif::format_string(fmt)?;
    let mut values = Vec::new();
    let mut cursor = args;
    while let Some(cell) = cursor.as_cons_ptr() {
        values.push((*cell).head);
        cursor = (*cell).tail;
    }
    if !cursor.is_nil() {
        return Err(());
    }
    render(&fmt, &values)
}

/// Input graphs obey format_output's safety contract; no GC occurs here.
unsafe fn render(fmt: &str, args: &[ErlTerm]) -> Result<String, ()> {
    let mut output = String::new();
    let mut chars = fmt.chars().peekable();
    let mut index = 0;
    while let Some(ch) = chars.next() {
        if ch != '~' {
            output.push(ch);
            continue;
        }
        let negative = chars.peek() == Some(&'-');
        if negative {
            chars.next();
        }
        let mut width = number(&mut chars, args, &mut index)?;
        if negative {
            width = Some(width.ok_or(())?.checked_neg().ok_or(())?);
        }
        let left = width.is_some_and(|w| w < 0);
        let width = width
            .map(|w| w.checked_abs().ok_or(()).and_then(size))
            .transpose()?;
        let precision = if chars.peek() == Some(&'.') {
            chars.next();
            number(&mut chars, args, &mut index)?
                .map(size)
                .transpose()?
        } else {
            None
        };
        let pad = if chars.peek() == Some(&'.') {
            chars.next();
            match chars.next().ok_or(())? {
                '*' => argument(args, &mut index)?
                    .as_small_int()
                    .and_then(|n| u32::try_from(n).ok())
                    .and_then(char::from_u32)
                    .ok_or(())?,
                ch => ch,
            }
        } else {
            ' '
        };
        let mut unicode = false;
        while let Some(modifier @ ('t' | 'l' | 'k' | 'K')) = chars.peek().copied() {
            chars.next();
            match modifier {
                't' => unicode = true,
                'K' => {
                    argument(args, &mut index)?;
                }
                // Basic ~p rendering already leaves printable lists as lists,
                // so it honours ~l. ~P remains the chosen literal placeholder.
                // Map ordering is not modelled: accept ~k/~K without sorting.
                _ => {}
            }
        }
        let directive = chars.next().ok_or(())?;
        let text = match directive {
            'w' | 'd' => term_field(
                bif::format_term(argument(args, &mut index)?),
                width,
                precision,
                pad,
                left,
            )?,
            // Keep Nera's existing term rendering. Width/precision on ~p are
            // pretty-printer line length/indentation, not term-field padding.
            'p' => {
                if left {
                    return Err(());
                }
                bif::format_term(argument(args, &mut index)?)
            }
            'P' | 'W' => {
                // EUnit already relies on these literal placeholders while
                // depth-limited term rendering is unsupported. Consume both
                // arguments so following controls stay aligned, without
                // turning a diagnostic limitation into a reporter exception.
                argument(args, &mut index)?;
                argument(args, &mut index)?.as_small_int().ok_or(())?;
                format!("~{directive}")
            }
            's' => {
                let term = argument(args, &mut index)?;
                let text = if let Some(atom) = term.as_atom_id() {
                    let text = global_atom_table().resolve(atom).ok_or(())?;
                    if !unicode && text.chars().any(|c| c as u32 > 255) {
                        return Err(());
                    }
                    text
                } else {
                    string_chars(term, unicode, true)?
                };
                string_field(text, width, precision, pad, left, unicode)?
            }
            'b' | 'B' | 'x' | 'X' | '#' | '+' => {
                let base = precision.unwrap_or(10);
                if !(2..=36).contains(&base) {
                    return Err(());
                }
                let integer =
                    crate::bigint::term_to_bigint(argument(args, &mut index)?).ok_or(())?;
                let mut text = integer.to_str_radix(base as u32);
                if matches!(directive, 'B' | 'X' | '#') {
                    text.make_ascii_uppercase();
                }
                let prefix = match directive {
                    'x' | 'X' => {
                        let term = argument(args, &mut index)?;
                        if let Some(atom) = term.as_atom_id() {
                            global_atom_table().resolve(atom).ok_or(())?
                        } else {
                            string_chars(term, true, false)?
                        }
                    }
                    '#' | '+' => format!("{base}#"),
                    _ => String::new(),
                };
                text.insert_str(usize::from(text.starts_with('-')), &prefix);
                term_field(text, width, None, pad, left)?
            }
            'f' | 'e' | 'g' => {
                let term = argument(args, &mut index)?;
                let ptr = term.as_boxed_ptr().ok_or(())?;
                if BoxedHeader(*ptr).subtype() != BOXED_FLOAT {
                    return Err(());
                }
                term_field(
                    float_field(
                        f64::from_bits(*ptr.add(1)),
                        directive,
                        precision.unwrap_or(6),
                    )?,
                    width,
                    None,
                    pad,
                    left,
                )?
            }
            'c' | '~' => {
                let ch = if directive == '~' {
                    '~'
                } else {
                    let value = argument(args, &mut index)?.as_small_int().ok_or(())?;
                    let value = if unicode { value } else { value & 255 };
                    u32::try_from(value)
                        .ok()
                        .and_then(char::from_u32)
                        .ok_or(())?
                };
                let count = precision.or(width).unwrap_or(1);
                let width = width.unwrap_or(count);
                if count > width {
                    return Err(());
                }
                adjust(&repeat(ch, count)?, &repeat(pad, width - count)?, left)
            }
            'n' => {
                if left {
                    return Err(());
                }
                repeat('\n', width.unwrap_or(1))?
            }
            'i' => {
                argument(args, &mut index)?;
                String::new()
            }
            _ => return Err(()),
        };
        output.push_str(&text);
    }
    if index != args.len() {
        return Err(());
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn general_float_large_magnitudes_match_beam_without_debug_overflow() {
        // Review of the field-formatting fix found a debug panic at 10000:
        // ~g must select scientific form without an overflowing sentinel.
        for (value, precision, expected) in [
            (10000.0, 6, "1.00000e+4"),
            (-1.0e30, 6, "-1.00000e+30"),
            (10000.0, 1, "1.0e+4"),
        ] {
            assert_eq!(float_field(value, 'g', precision).unwrap(), expected);
        }
    }
}
