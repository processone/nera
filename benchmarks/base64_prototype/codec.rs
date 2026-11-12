//! Safe byte-only core. None delegates malformed input to the Erlang reference.
pub fn encode(bytes: &[u8], url: bool, padding: bool) -> Vec<u8> {
    let alphabet = if url {
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
    } else {
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
    };
    let mut out = Vec::with_capacity(bytes.len().div_ceil(3).checked_mul(4).expect("output size"));
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        out.push(alphabet[(a >> 2) as usize]);
        out.push(alphabet[(((a & 3) << 4) | (b >> 4)) as usize]);
        if chunk.len() > 1 {
            out.push(alphabet[(((b & 15) << 2) | (c >> 6)) as usize]);
        } else if padding {
            out.push(b'=');
        }
        if chunk.len() > 2 {
            out.push(alphabet[(c & 63) as usize]);
        } else if padding {
            out.push(b'=');
        }
    }
    out
}
fn value(c: u8, url: bool) -> Option<u8> {
    Some(match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' if !url => 62,
        b'/' if !url => 63,
        b'-' if url => 62,
        b'_' if url => 63,
        b'=' => 64,
        _ => return None,
    })
}
fn group(g: &[u8; 4], out: &mut Vec<u8>) -> Option<bool> {
    let [a, b, c, d] = *g;
    if a >= 64 || b >= 64 {
        return None;
    }
    if c == 64 {
        if d != 64 {
            return None;
        }
        out.push((a << 2) | (b >> 4));
        return Some(true);
    }
    out.push((a << 2) | (b >> 4));
    out.push((b << 4) | (c >> 2));
    if d == 64 {
        return Some(true);
    }
    out.push((c << 6) | d);
    Some(false)
}
pub fn decode(bytes: &[u8], url: bool, padding: bool) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity((bytes.len() / 4).checked_mul(3)?.checked_add(3)?);
    let mut g = [64; 4];
    let mut n = 0;
    let mut finished = false;
    for &c in bytes {
        if matches!(c, b' ' | b'\t' | b'\r' | b'\n') {
            continue;
        }
        if finished {
            return None;
        }
        g[n] = value(c, url)?;
        n += 1;
        if n == 4 {
            finished = group(&g, &mut out)?;
            g = [64; 4];
            n = 0;
        }
    }
    if n != 0 {
        if padding || n == 1 {
            return None;
        }
        group(&g, &mut out)?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_vectors_and_noncanonical_tails() {
        for (raw, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode(raw.as_bytes(), false, true), encoded.as_bytes());
            assert_eq!(
                decode(encoded.as_bytes(), false, true),
                Some(raw.as_bytes().to_vec())
            );
        }
        assert_eq!(encode(&[251, 255, 239], true, false), b"-__v");
        for tail in [b"Zg==".as_slice(), b"Zh==", b" Z\tg=\r=\n"] {
            assert_eq!(decode(tail, false, true), Some(b"f".to_vec()));
        }
        for tail in [b"Zg".as_slice(), b"Zg="] {
            assert_eq!(decode(tail, false, false), Some(b"f".to_vec()));
            assert_eq!(decode(tail, false, true), None);
        }
        assert_eq!(decode(b"Zm9=", false, true), Some(b"fo".to_vec()));
    }
    #[test]
    fn malformed_inputs_delegate() {
        for input in [
            "A", "***=", "Y===", "*AAA", "Zg===", "=A", "A=A", "A*", "Zg==A", "Zg==Zg==",
        ] {
            assert_eq!(decode(input.as_bytes(), false, false), None, "{input}");
        }
        assert_eq!(decode(b"-__v", false, true), None);
        assert_eq!(decode(b"+//v", true, true), None);
    }
    #[test]
    fn all_lengths_and_options_roundtrip() {
        let bytes: Vec<u8> = (0..4096).map(|i| (i * 73) as u8).collect();
        for n in 0..=bytes.len() {
            for url in [false, true] {
                for pad in [false, true] {
                    assert_eq!(
                        decode(&encode(&bytes[..n], url, pad), url, pad).as_deref(),
                        Some(&bytes[..n])
                    );
                }
            }
        }
    }
}
