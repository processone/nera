//! Incremental byte-only Base64 encoder. No Erlang terms or borrowed pointers
//! survive a scheduler slice; the continuation owns the input's GC root.
pub(crate) const CHUNK_BYTES: usize = 3 * 16 * 1024;

pub(crate) struct Encoder {
    pub output: Vec<u8>,
    pub offset: usize,
    pub url: bool,
    pub padding: bool,
}

pub(crate) fn encoded_len(len: usize, padding: bool) -> Option<usize> {
    let full = (len / 3).checked_mul(4)?;
    let tail = match len % 3 {
        0 => 0,
        _ if padding => 4,
        n => n + 1,
    };
    let size = full.checked_add(tail)?;
    // Binary headers store a bit length, not a byte length.
    (size <= ((u64::MAX >> crate::term::BOXED_TAG_BITS) / 8) as usize).then_some(size)
}

impl Encoder {
    pub fn new(len: usize, url: bool, padding: bool) -> Option<Self> {
        let mut output = Vec::new();
        output.try_reserve_exact(encoded_len(len, padding)?).ok()?;
        Some(Self {
            output,
            offset: 0,
            url,
            padding,
        })
    }
}

impl crate::native_work::BinaryWork for Encoder {
    fn bytes_per_reduction(&self) -> usize {
        32
    }
    fn quantum(&self) -> usize {
        3
    }
    fn max_bytes(&self) -> usize {
        CHUNK_BYTES
    }
    fn offset(&self) -> usize {
        self.offset
    }
    fn output(&mut self) -> &mut Vec<u8> {
        &mut self.output
    }
    fn step(&mut self, bytes: &[u8], limit: usize) -> Result<bool, crate::native_work::WorkError> {
        let end = self.offset.saturating_add(limit).min(bytes.len());
        append(
            &bytes[self.offset..end],
            self.url,
            self.padding,
            &mut self.output,
        );
        self.offset = end;
        Ok(end == bytes.len())
    }
}

fn append(bytes: &[u8], url: bool, padding: bool, out: &mut Vec<u8>) {
    let alphabet = if url {
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
    } else {
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
    };
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
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_size_checks_overflow_and_header_capacity() {
        assert_eq!(encoded_len(0, true), Some(0));
        assert_eq!(encoded_len(1, false), Some(2));
        assert_eq!(encoded_len(2, false), Some(3));
        assert_eq!(encoded_len(3, true), Some(4));
        assert_eq!(encoded_len(usize::MAX, true), None);
        assert_eq!(encoded_len(usize::MAX / 2, false), None);
    }
}

// Byte-only decoder: symbols span slices, including whitespace inside a quartet.
// 64 denotes padding; 65 denotes a malformed symbol (preserved for error order).
pub(crate) struct Decoder {
    output: Vec<u8>,
    offset: usize,
    group: [u8; 4],
    count: usize,
    finished: bool,
    url: bool,
    padding: bool,
}

impl Decoder {
    pub fn new(len: usize, url: bool, padding: bool) -> Option<Self> {
        let capacity = (len / 4).checked_mul(3)?.checked_add(3)?;
        if capacity > ((u64::MAX >> crate::term::BOXED_TAG_BITS) / 8) as usize {
            return None;
        }
        let mut output = Vec::new();
        output.try_reserve_exact(capacity).ok()?;
        Some(Self {
            output,
            offset: 0,
            group: [64; 4],
            count: 0,
            finished: false,
            url,
            padding,
        })
    }

    fn group(&mut self) -> Result<(), crate::native_work::WorkError> {
        use crate::native_work::WorkError::Atom;
        let [a, b, c, d] = self.group;
        // Padded groups perform the trailing shift before validating earlier
        // symbols, matching Erlang's badarith/badarg distinction.
        if c == 64 && d == 64 {
            if b >= 64 {
                return Err(Atom("badarith"));
            }
            if a >= 64 {
                return Err(Atom("badarg"));
            }
            self.output.push((a << 2) | (b >> 4));
            self.finished = true;
        } else if d == 64 {
            if c >= 64 {
                return Err(Atom("badarith"));
            }
            if a >= 64 || b >= 64 {
                return Err(Atom("badarg"));
            }
            self.output.push((a << 2) | (b >> 4));
            self.output.push((b << 4) | (c >> 2));
            self.finished = true;
        } else {
            if [a, b, c, d].iter().any(|&v| v >= 64) {
                return Err(Atom("badarg"));
            }
            self.output.push((a << 2) | (b >> 4));
            self.output.push((b << 4) | (c >> 2));
            self.output.push((c << 6) | d);
        }
        self.count = 0;
        self.group = [64; 4];
        Ok(())
    }
}

impl crate::native_work::BinaryWork for Decoder {
    fn bytes_per_reduction(&self) -> usize {
        16
    }
    fn max_bytes(&self) -> usize {
        32 * 1024
    }
    fn offset(&self) -> usize {
        self.offset
    }
    fn output(&mut self) -> &mut Vec<u8> {
        &mut self.output
    }
    fn step(&mut self, input: &[u8], limit: usize) -> Result<bool, crate::native_work::WorkError> {
        use crate::native_work::{NativeValue, WorkError};
        let end = self.offset.saturating_add(limit).min(input.len());
        while self.offset < end {
            let c = input[self.offset];
            self.offset += 1;
            if matches!(c, b' ' | b'\t' | b'\r' | b'\n') {
                continue;
            }
            let symbol = match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' if !self.url => 62,
                b'/' if !self.url => 63,
                b'-' if self.url => 62,
                b'_' if self.url => 63,
                b'=' => 64,
                _ => 65,
            };
            if self.finished {
                return Err(WorkError::CaseClause(match symbol {
                    64 => NativeValue::Atom("eq"),
                    65 => NativeValue::Atom("bad"),
                    n => NativeValue::Integer(i64::from(n)),
                }));
            }
            self.group[self.count] = symbol;
            self.count += 1;
            if self.count == 4 {
                self.group()?;
            }
        }
        if end != input.len() {
            return Ok(false);
        }
        match self.count {
            0 => (),
            1 => return Err(WorkError::Atom("function_clause")),
            _ if self.padding => return Err(WorkError::Atom("missing_padding")),
            _ => self.group()?,
        }
        Ok(true)
    }
}

#[cfg(test)]
mod decoder_tests {
    use super::*;
    use crate::native_work::{BinaryWork, NativeValue, WorkError};

    fn decode(input: &[u8], url: bool, padding: bool, limit: usize) -> Result<Vec<u8>, WorkError> {
        let mut decoder = Decoder::new(input.len(), url, padding).unwrap();
        loop {
            let before = decoder.offset();
            let result = decoder.step(input, limit);
            assert!(decoder.offset() - before <= limit);
            if result? {
                return Ok(decoder.output);
            }
        }
    }

    #[test]
    fn quartets_padding_and_whitespace_span_arbitrary_slices() {
        for limit in 1..=17 {
            for (input, output, padding) in [
                ("", "", true),
                ("Zg==", "f", true),
                ("Zh==", "f", true),
                ("Zm9=", "fo", true),
                ("Zm9v", "foo", true),
                (" Z\tg=\r=\n", "f", true),
                ("Zg", "f", false),
                ("Zg=", "f", false),
                ("Zm8", "fo", false),
                ("Zm9vYmFy", "foobar", true),
            ] {
                assert_eq!(
                    decode(input.as_bytes(), false, padding, limit),
                    Ok(output.as_bytes().to_vec())
                );
            }
            assert_eq!(decode(b"-__v", true, true, limit), Ok(vec![251, 255, 239]));
        }
        assert!(Decoder::new(usize::MAX, false, true).is_none());
    }

    #[test]
    fn malformed_groups_preserve_error_precedence_across_slices() {
        use WorkError::{Atom, CaseClause};
        for limit in 1..=7 {
            for (input, padding, error) in [
                ("A", true, Atom("function_clause")),
                ("**", true, Atom("missing_padding")),
                ("***", true, Atom("missing_padding")),
                ("Y===", true, Atom("badarith")),
                ("***=", true, Atom("badarith")),
                ("*AAA", true, Atom("badarg")),
                ("*A==", true, Atom("badarg")),
                ("A*", false, Atom("badarith")),
                ("*A", false, Atom("badarg")),
                ("Zg==*", true, CaseClause(NativeValue::Atom("bad"))),
                ("Zg===", true, CaseClause(NativeValue::Atom("eq"))),
                ("Zg== A", true, CaseClause(NativeValue::Integer(0))),
            ] {
                assert_eq!(
                    decode(input.as_bytes(), false, padding, limit),
                    Err(error),
                    "{input}, limit {limit}"
                );
            }
        }
    }
}

// Streaming list front-end, sharing the binary quartet decoder.
// All state is native-owned; the list cursor lives only in the GC continuation.
pub(crate) struct ListDecoder {
    decoder: Decoder,
}
impl ListDecoder {
    pub fn new(url: bool, padding: bool) -> Option<Self> {
        Some(Self {
            decoder: Decoder::new(0, url, padding)?,
        })
    }
    pub fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.decoder.output)
    }
    pub fn symbol(&self, n: i64) -> Result<u8, crate::native_work::WorkError> {
        use crate::native_work::WorkError::Atom;
        // OTP's list lookup spans two 256-entry alphabets. Preserve its observable
        // behavior even for integers outside the usual byte domain.
        let index = n
            .checked_add(if self.decoder.url { 256 } else { 0 })
            .ok_or(Atom("badarg"))?;
        if !(0..512).contains(&index) {
            return Err(Atom("badarg"));
        }
        let url = index >= 256;
        let c = (index % 256) as u8;
        Ok(match c {
            b' ' | b'\t' | b'\r' | b'\n' => 66,
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' if !url => 62,
            b'/' if !url => 63,
            b'-' if url => 62,
            b'_' if url => 63,
            b'=' => 64,
            _ => 65,
        })
    }
    fn group(&mut self) -> Result<(), crate::native_work::WorkError> {
        // Geometric growth avoids a preliminary synchronous list traversal.
        // Reallocation itself can still copy the accumulated output synchronously.
        let max_bytes = ((u64::MAX >> crate::term::BOXED_TAG_BITS) / 8) as usize;
        if self
            .decoder
            .output
            .len()
            .checked_add(3)
            .is_none_or(|len| len > max_bytes)
        {
            return Err(crate::native_work::WorkError::Atom("system_limit"));
        }
        self.decoder
            .output
            .try_reserve(3)
            .map_err(|_| crate::native_work::WorkError::Atom("system_limit"))?;
        self.decoder.group()
    }
    pub fn feed(&mut self, symbol: u8) -> Result<(), crate::native_work::WorkError> {
        use crate::native_work::{NativeValue, WorkError};
        if symbol == 66 {
            return Ok(());
        }
        if self.decoder.finished {
            return Err(WorkError::CaseClause(match symbol {
                64 => NativeValue::Atom("eq"),
                65 => NativeValue::Atom("bad"),
                n => NativeValue::Integer(i64::from(n)),
            }));
        }
        self.decoder.group[self.decoder.count] = symbol;
        self.decoder.count += 1;
        if self.decoder.count == 4 {
            self.group()?;
        }
        Ok(())
    }
    pub fn end(&mut self) -> Result<(), crate::native_work::WorkError> {
        use crate::native_work::WorkError::Atom;
        match self.decoder.count {
            0 => Ok(()),
            1 => Err(Atom("function_clause")),
            _ if self.decoder.padding => Err(Atom("missing_padding")),
            _ => self.group(),
        }
    }
}

#[cfg(test)]
impl ListDecoder {
    pub fn decoder_output_len(&self) -> usize {
        self.decoder.output.len()
    }
}
