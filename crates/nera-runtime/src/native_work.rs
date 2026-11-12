//! Cooperative byte-oriented native work. State is owned by the process and
//! must contain no Erlang terms or borrowed pointers. The adapter roots input
//! separately and reacquires its slice after every suspension/GC.

/// Error values contain no GC-managed terms; the adapter materializes them only
/// after the input borrow ends.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum NativeValue {
    Atom(&'static str),
    Integer(i64),
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WorkError {
    Atom(&'static str),
    CaseClause(NativeValue),
}

pub(crate) trait BinaryWork: Send {
    /// Logical input bytes per reduction, calibrated per operation.
    fn bytes_per_reduction(&self) -> usize;
    /// Non-final steps must end at this input boundary.
    fn quantum(&self) -> usize {
        1
    }
    fn max_bytes(&self) -> usize;
    fn offset(&self) -> usize;
    fn output(&mut self) -> &mut Vec<u8>;
    /// Process at most `limit` input bytes. Return Ok(true) when complete, or a terminal error.
    fn step(&mut self, input: &[u8], limit: usize) -> Result<bool, WorkError>;
}

/// A work bound, not a wall-clock promise: allocation and GC remain synchronous.
pub(crate) fn budget(work: &dyn BinaryWork, reductions: i64) -> usize {
    let quantum = work.quantum();
    let rate = work.bytes_per_reduction();
    assert!(quantum > 0 && rate >= quantum && work.max_bytes() >= quantum);
    let bytes = usize::try_from(reductions.max(0))
        .unwrap_or(usize::MAX)
        .saturating_mul(rate)
        .min(work.max_bytes());
    bytes / quantum * quantum
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exhausted_and_extreme_budgets_are_bounded_and_aligned() {
        let encoder = crate::base64::Encoder::new(100, false, true).unwrap();
        for reductions in [i64::MIN, -1, 0] {
            assert_eq!(budget(&encoder, reductions), 0);
        }
        assert_eq!(budget(&encoder, 1), 30);
        assert_eq!(budget(&encoder, 2), 63);
        assert_eq!(budget(&encoder, i64::MAX), crate::base64::CHUNK_BYTES);
    }
}
