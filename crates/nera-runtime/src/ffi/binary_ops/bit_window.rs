//! Borrowed bit windows. Bounds are established before any byte is accessed.

pub(super) struct BitReader<'a> {
    // Exactly the complete logical bytes, plus one carry byte when shifted.
    bytes: &'a [u8],
    shift: u32,
}

impl<'a> BitReader<'a> {
    #[inline(always)]
    pub(super) fn new(bytes: &'a [u8], shift: u32) -> Option<Self> {
        if shift >= 8 {
            return None;
        }
        Some(Self { bytes, shift })
    }

    #[inline(always)]
    pub(super) fn read<const N: usize>(&self) -> Option<[u8; N]> {
        if self.shift == 0 {
            return self.bytes.get(..N)?.try_into().ok();
        }
        self.read_shifted()
    }

    // Keep carry-byte handling out of the aligned instruction path.
    #[cold]
    #[inline(never)]
    fn read_shifted<const N: usize>(&self) -> Option<[u8; N]> {
        let source = self.bytes.get(..N.checked_add(1)?)?;
        let mut result = [0; N];
        for i in 0..N {
            result[i] = (source[i] << self.shift) | (source[i + 1] >> (8 - self.shift));
        }
        Some(result)
    }
}

pub(super) struct BitWriter<'a> {
    bytes: &'a mut [u8],
    offset: usize,
    remaining: usize,
}

impl<'a> BitWriter<'a> {
    #[inline(always)]
    pub(super) fn new(bytes: &'a mut [u8], total: usize, offset: usize) -> Option<Self> {
        let remaining = total.checked_sub(offset)?;
        if total.div_ceil(8) > bytes.len() {
            return None;
        }
        Some(Self {
            bytes,
            offset,
            remaining,
        })
    }

    /// Write whole source bytes while preserving neighboring destination bits.
    #[inline(always)]
    pub(super) fn write(&mut self, source: &[u8]) -> Option<()> {
        if source.len() > self.remaining / 8 {
            return None;
        }
        let start = self.offset / 8;
        let shift = self.offset % 8;
        if shift == 0 {
            self.bytes[start..start + source.len()].copy_from_slice(source);
        } else {
            self.write_shifted(source)?;
        }
        Some(())
    }

    #[cold]
    #[inline(never)]
    fn write_shifted(&mut self, source: &[u8]) -> Option<()> {
        let start = self.offset / 8;
        let shift = self.offset % 8;
        let destination = self
            .bytes
            .get_mut(start..start + source.len().checked_add(1)?)?;
        let prefix = u8::MAX << (8 - shift);
        for (i, &byte) in source.iter().enumerate() {
            destination[i] = (destination[i] & prefix) | (byte >> shift);
            destination[i + 1] = (destination[i + 1] & !prefix) | (byte << (8 - shift));
        }
        Some(())
    }
}
