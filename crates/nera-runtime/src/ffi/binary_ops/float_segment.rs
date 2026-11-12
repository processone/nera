//! IEEE binary16 conversion, rounding directly from f64 (no f32 double rounding).

#[inline]
fn round_even(value: u64, shift: u32) -> u16 {
    let quotient = value >> shift;
    let remainder = value & ((1_u64 << shift) - 1);
    let halfway = 1_u64 << (shift - 1);
    (quotient + u64::from(remainder > halfway || (remainder == halfway && quotient & 1 != 0))) as u16
}

/// The caller supplies a finite Erlang numeric value. Narrowing may produce
/// infinity, as with IEEE binary32 construction; matching rejects that encoding.
pub(super) fn encode_half(value: f64) -> u16 {
    let raw = value.to_bits();
    let sign = ((raw >> 48) & 0x8000) as u16;
    let exponent = ((raw >> 52) & 0x7FF) as i32 - 1023;
    if exponent > 15 { return sign | 0x7C00; }
    if exponent < -25 { return sign; }
    let significand = (raw & ((1_u64 << 52) - 1)) | (1_u64 << 52);
    if exponent < -14 {
        return sign | round_even(significand, (28 - exponent) as u32);
    }
    sign | ((((exponent + 15) as u16) << 10) + round_even(significand, 42) - 1024)
}

pub(super) fn decode_half(raw: u16) -> Option<f64> {
    let sign = ((raw & 0x8000) as u64) << 48;
    let exponent = (raw >> 10) & 31;
    let fraction = raw & 1023;
    match exponent {
        31 => None,
        0 => {
            let value = fraction as f64 / 16_777_216.0;
            Some(if sign == 0 { value } else { -value })
        }
        _ => Some(f64::from_bits(sign | ((exponent as u64 + 1008) << 52)
            | ((fraction as u64) << 42))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_finite_half_roundtrips_including_subnormals_and_signed_zero() {
        for raw in 0..=u16::MAX {
            if raw & 0x7C00 == 0x7C00 {
                assert!(decode_half(raw).is_none());
            } else {
                assert_eq!(encode_half(decode_half(raw).unwrap()), raw);
            }
        }
    }

    #[test]
    fn rounds_directly_from_f64_with_even_ties_and_overflow() {
        assert_eq!(encode_half(1.00048828125), 0x3C00);
        assert_eq!(encode_half(1.0004882812500002), 0x3C01);
        assert_eq!(encode_half(1.00146484375), 0x3C02);
        assert_eq!(encode_half(2_f64.powi(-25)), 0);
        assert_eq!(encode_half(f64::from_bits(2_f64.powi(-25).to_bits() + 1)), 1);
        assert_eq!(encode_half(65504.0), 0x7BFF);
        assert_eq!(encode_half(65520.0), 0x7C00);
        assert_eq!(encode_half(-1.0e100), 0xFC00);
        assert_eq!(encode_half(-1.0e-100), 0x8000);
    }
}
