//! Float text shared by term printing and conversion BIFs.

/// Erlang short formatting uses nearest, ties-to-even shortest digits, then
/// chooses the shorter notation (fixed on ties). At |value| >= 2^53 it always
/// uses scientific notation, even when a fixed spelling would be shorter.
pub(crate) fn format_short_float(value: f64) -> String {
    if value == 0.0 || !value.is_finite() {
        return if value == 0.0 {
            if value.is_sign_negative() {
                "-0.0"
            } else {
                "0.0"
            }
            .to_owned()
        } else {
            value.to_string()
        };
    }
    let mut buffer = ryu::Buffer::new();
    let raw = buffer.format_finite(value.abs());
    let (mantissa, exponent) = match raw.split_once('e') {
        Some((mantissa, exponent)) => (mantissa, exponent.parse::<i32>().unwrap()),
        None => (raw, 0),
    };
    let point = mantissa.find('.').unwrap_or(mantissa.len()) as i32;
    let digits = mantissa.replace('.', "");
    let leading = digits.bytes().take_while(|&c| c == b'0').count();
    let exponent = exponent + point - leading as i32 - 1;
    let digits = digits[leading..].trim_end_matches('0');
    let sign = if value.is_sign_negative() { "-" } else { "" };
    let fraction = if digits.len() == 1 { "0" } else { &digits[1..] };
    let scientific = format!("{sign}{}.{fraction}e{exponent}", &digits[..1]);
    if value.abs() >= 9_007_199_254_740_992.0 {
        return scientific;
    }
    let point = exponent + 1;
    let fixed = if point <= 0 {
        format!("{sign}0.{}{digits}", "0".repeat((-point) as usize))
    } else if point as usize >= digits.len() {
        format!(
            "{sign}{digits}{}.0",
            "0".repeat(point as usize - digits.len())
        )
    } else {
        let (whole, fraction) = digits.split_at(point as usize);
        format!("{sign}{whole}.{fraction}")
    };
    if scientific.len() < fixed.len() {
        scientific
    } else {
        fixed
    }
}

/// Legacy scientific spelling: twenty decimals by default, with an explicit
/// exponent sign and at least two exponent digits, unlike shortest spelling.
pub(crate) fn scientific(value: f64, precision: usize) -> String {
    // No Erlang float is infinite or NaN. Should one reach this conversion,
    // keep its native text: it has no exponent separator and must not panic
    // across the C ABI.
    if !value.is_finite() {
        return value.to_string();
    }
    let raw = format!("{value:.precision$e}");
    let (mantissa, exponent) = raw.split_once('e').unwrap();
    let exponent: i32 = exponent.parse().unwrap();
    format!("{mantissa}e{exponent:+03}")
}

/// Fixed conversion rounds exact halfway values away from zero; scientific
/// conversion uses ties-to-even. Rust already handles all non-halfway cases.
fn decimals(value: f64, precision: usize) -> String {
    let bits = value.to_bits();
    let fraction = bits & ((1_u64 << 52) - 1);
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    let (significand, binary_exponent) = if exponent == 0 {
        (fraction, -1074)
    } else {
        (fraction | (1_u64 << 52), exponent - 1023 - 52)
    };
    // Multiplication by 10^precision adds precision powers of two and an
    // odd factor. The scaled value is halfway exactly when one factor of
    // two remains in its denominator after reducing the significand.
    let halfway = significand != 0
        && binary_exponent + precision as i32 + significand.trailing_zeros() as i32 == -1;
    if !halfway {
        return format!("{value:.precision$}");
    }
    let mut text = format!("{:.digits$}", value.abs(), digits = precision + 1).into_bytes();
    text.pop(); // the exact final 5
    if text.last() == Some(&b'.') {
        text.pop();
    }
    let mut carry = true;
    for digit in text.iter_mut().rev() {
        if *digit == b'.' {
            continue;
        }
        if *digit == b'9' {
            *digit = b'0';
        } else {
            *digit += 1;
            carry = false;
            break;
        }
    }
    if carry {
        text.insert(0, b'1');
    }
    if value.is_sign_negative() {
        text.insert(0, b'-');
    }
    String::from_utf8(text).unwrap()
}

/// Format float_to_list/2 options. The last short/decimals/scientific option
/// selects the format; compact affects only fixed conversion.
/// # Safety
/// The option graph must be live, immutable and unmoved; no GC occurs here.
pub(crate) unsafe fn with_options(
    value: f64,
    mut options: crate::term::ErlTerm,
) -> Result<String, ()> {
    use crate::atom_table::global_atom_table;
    use crate::term::{BoxedHeader, ErlTerm, BOXED_TUPLE};
    enum Format {
        Short,
        Decimals(i64),
        Scientific(i64),
    }
    let mut format = Format::Scientific(20);
    let mut compact = false;
    while !options.is_nil() {
        let cell = options.as_cons_ptr().ok_or(())?;
        let option = (*cell).head;
        options = (*cell).tail;
        if let Some(atom) = option.as_atom_id() {
            match global_atom_table().resolve(atom).ok_or(())?.as_str() {
                "short" => format = Format::Short,
                "compact" => compact = true,
                _ => return Err(()),
            }
        } else {
            let ptr = option.as_boxed_ptr().ok_or(())?;
            let header = BoxedHeader(*ptr);
            if header.subtype() != BOXED_TUPLE || header.arity() != 2 {
                return Err(());
            }
            let key = ErlTerm(*ptr.add(1)).as_atom_id().ok_or(())?;
            let precision = ErlTerm(*ptr.add(2)).as_small_int().ok_or(())?;
            format = match global_atom_table().resolve(key).ok_or(())?.as_str() {
                "decimals" => Format::Decimals(precision),
                "scientific" => Format::Scientific(precision),
                _ => return Err(()),
            };
        }
    }
    let (mut output, fixed) = match format {
        Format::Short => (format_short_float(value), false),
        Format::Scientific(n) if n <= 249 => {
            // OTP's scientific formatter uses the default six decimals for
            // negative precision, just as its underlying printf does.
            (scientific(value, if n < 0 { 6 } else { n as usize }), false)
        }
        Format::Decimals(n) if (0..=253).contains(&n) => (decimals(value, n as usize), true),
        _ => return Err(()),
    };
    // OTP's conversion buffer holds at most 255 bytes, including the sign
    // and exponent. Check before compact removes any trailing zeroes.
    if output.len() > 255 {
        return Err(());
    }
    if compact && fixed {
        if let Some(dot) = output.find('.') {
            let end = output.trim_end_matches('0').len().max(dot + 2);
            output.truncate(end);
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// These NEW_FLOAT_EXT payloads are not Erlang floats, and the native ETF
    /// decoder rejects them as BEAM does. Conversion still retains safe text.
    #[test]
    fn scientific_nonfinite_retains_native_text() {
        for (bits, expected) in [
            (0x7ff0_0000_0000_0000, "inf"),
            (0xfff0_0000_0000_0000, "-inf"),
            (0x7ff8_0000_0000_0000, "NaN"),
        ] {
            for precision in [0, 2, 20, 249] {
                assert_eq!(scientific(f64::from_bits(bits), precision), expected);
            }
        }
    }

    /// OTP 29 readings: negative halves, carry through the integer part,
    /// multiple decimal places, and a non-halfway predecessor of 1.25.
    #[test]
    fn fixed_rounding_matches_beam() {
        for (value, precision, expected) in [
            (-1.25, 1, "-1.3"),
            (9.5, 0, "10"),
            (99.5, 0, "100"),
            (1.125, 2, "1.13"),
            (-1.125, 2, "-1.13"),
            (1.2499999999999998, 1, "1.2"),
        ] {
            assert_eq!(decimals(value, precision), expected);
        }
    }

    #[test]
    fn short_float_boundaries_match_beam() {
        for (value, expected) in [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (100.0, "100.0"),
            // Exact halfway value 1331752170181752.25: choose the even last digit.
            (f64::from_bits(0x4312_ece2_dc21_51e1), "1331752170181752.2"),
            (9_007_199_254_740_991.0, "9007199254740991.0"),
            (9_007_199_254_740_992.0, "9.007199254740992e15"),
            (563_761_063_810_007_800.0, "5.637610638100078e17"),
            (1000.0, "1.0e3"),
            (0.0001, "0.0001"),
            (0.00001, "1.0e-5"),
            (1.234e-4, "1.234e-4"),
            (99.99, "99.99"),
            (-9.9e100, "-9.9e100"),
            (0.1 + 0.2, "0.30000000000000004"),
            (f64::from_bits(1), "5.0e-324"),
            (f64::MIN_POSITIVE, "2.2250738585072014e-308"),
            (f64::MAX, "1.7976931348623157e308"),
        ] {
            let text = format_short_float(value);
            assert_eq!(text, expected);
            assert_eq!(text.parse::<f64>().unwrap().to_bits(), value.to_bits());
        }
    }
}
