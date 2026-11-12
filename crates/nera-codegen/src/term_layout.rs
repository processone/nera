//! Memory layout constants for ErlTerm that are shared between
//! the runtime and code generator.
//!
//! These must match the definitions in nera-runtime/src/term.rs exactly.
pub use nera_runtime::term::BoxedSubtype;

use nera_runtime::process as rt_process;

// Primary tag (low 3 bits)
pub const TAG_BITS: u32 = 3;
pub const TAG_MASK: u64 = 0b111;

pub const TAG_BOXED: u64 = 0b000;
pub const TAG_CONS: u64 = 0b001;
pub const TAG_SMALL_INT: u64 = 0b010;
pub const TAG_ATOM: u64 = 0b011;
pub const TAG_PID: u64 = 0b100;
pub const TAG_NIL: u64 = 0b101;

// Boxed header subtags (low 4 bits of header word)
pub const BOXED_TUPLE: u64 = 0x0;
pub const BOXED_FLOAT: u64 = 0x1;
pub const BOXED_BIGINT: u64 = 0x2;
pub const BOXED_HEAP_BINARY: u64 = 0x3;
pub const BOXED_BINARY: u64 = BOXED_HEAP_BINARY;
pub const BOXED_CLOSURE: u64 = 0x4;
pub const BOXED_MAP: u64 = 0x5;
pub const BOXED_REFC_BINARY: u64 = 0x6;
pub const BOXED_REFERENCE: u64 = 0x7;
pub const BOXED_MATCH_CTX: u64 = 0x8;

/// Bit 63 marks a term as "external" (points into ETS storage).
pub const EXTERNAL_BIT: u64 = 1u64 << 63;

// Sizes (in bytes)
pub const WORD_SIZE: usize = 8;
pub const CONS_CELL_SIZE: usize = 2 * WORD_SIZE;

/// ProcessContext field offsets (must match repr(C) layout in process.rs)
/// These are re-exported from `nera-runtime` so codegen and runtime always
/// agree on ABI layout.
pub const CTX_OFFSET_PID: i32 = rt_process::CTX_OFFSET_PID;
pub const CTX_OFFSET_REDUCTIONS: i32 = rt_process::CTX_OFFSET_REDUCTIONS;
pub const CTX_OFFSET_STATUS: i32 = rt_process::CTX_OFFSET_STATUS;
pub const CTX_OFFSET_RESULT: i32 = rt_process::CTX_OFFSET_RESULT;
pub const CTX_OFFSET_HEAP_PTR: i32 = rt_process::CTX_OFFSET_HEAP_PTR;
pub const CTX_OFFSET_RESUME_RECEIVE: i32 = rt_process::CTX_OFFSET_RESUME_RECEIVE;
pub const CTX_OFFSET_RECEIVE_TIMEOUT: i32 = rt_process::CTX_OFFSET_RECEIVE_TIMEOUT;
pub const CTX_OFFSET_STACK_PTR: i32 = rt_process::CTX_OFFSET_STACK_PTR;
pub const CTX_OFFSET_RESULT2: i32 = rt_process::CTX_OFFSET_RESULT2;
pub const CTX_OFFSET_EXCEPTION_TRACE_PTR: i32 = rt_process::CTX_OFFSET_EXCEPTION_TRACE_PTR;
pub const CTX_OFFSET_EXCEPTION_CLASS: i32 = rt_process::CTX_OFFSET_EXCEPTION_CLASS;
pub const CTX_OFFSET_MAILBOX_PTR: i32 = rt_process::CTX_OFFSET_MAILBOX_PTR;
pub const CTX_OFFSET_HEAP_CURSOR: i32 = rt_process::CTX_OFFSET_HEAP_CURSOR;
pub const CTX_OFFSET_HEAP_LIMIT: i32 = rt_process::CTX_OFFSET_HEAP_LIMIT;
pub const CTX_OFFSET_STACK_CURSOR: i32 = rt_process::CTX_OFFSET_STACK_CURSOR;
pub const CTX_OFFSET_STACK_LIMIT: i32 = rt_process::CTX_OFFSET_STACK_LIMIT;
pub const CTX_OFFSET_STACK_BASE: i32 = rt_process::CTX_OFFSET_STACK_BASE;
pub const CTX_OFFSET_TAIL_CALL_KIND: i32 = rt_process::CTX_OFFSET_TAIL_CALL_KIND;
pub const CTX_OFFSET_TAIL_CALL_MOD: i32 = rt_process::CTX_OFFSET_TAIL_CALL_MOD;
pub const CTX_OFFSET_TAIL_CALL_FUN: i32 = rt_process::CTX_OFFSET_TAIL_CALL_FUN;
pub const CTX_OFFSET_TAIL_CALL_ARG1: i32 = rt_process::CTX_OFFSET_TAIL_CALL_ARG1;
pub const CTX_OFFSET_TAIL_CALL_ARG2: i32 = rt_process::CTX_OFFSET_TAIL_CALL_ARG2;
pub const CTX_OFFSET_RUNTIME: i32 = rt_process::CTX_OFFSET_RUNTIME;
pub const STATUS_TAIL_CALL: u32 = rt_process::STATUS_TAIL_CALL as u32;

/// CompactFrame layout constants.
pub const COMPACT_FRAME_SIZE: i32 = rt_process::COMPACT_FRAME_SIZE;
pub const CF_OFFSET_FUNC_INDEX: i32 = rt_process::CF_OFFSET_FUNC_INDEX;
pub const CF_OFFSET_LINE: i32 = rt_process::CF_OFFSET_LINE;

/// FastMailbox field offsets (for inline receive_peek / receive_accept).
pub const FAST_MAILBOX_OFFSET_BUF: i32 = rt_process::FAST_MAILBOX_OFFSET_BUF;
pub const FAST_MAILBOX_OFFSET_CAPACITY: i32 = rt_process::FAST_MAILBOX_OFFSET_CAPACITY;
pub const FAST_MAILBOX_OFFSET_HEAD: i32 = rt_process::FAST_MAILBOX_OFFSET_HEAD;
pub const FAST_MAILBOX_OFFSET_LEN: i32 = rt_process::FAST_MAILBOX_OFFSET_LEN;
pub const FAST_MAILBOX_OFFSET_SCAN_POS: i32 = rt_process::FAST_MAILBOX_OFFSET_SCAN_POS;
pub const FAST_MAILBOX_OFFSET_TOMBSTONE_COUNT: i32 = rt_process::FAST_MAILBOX_OFFSET_TOMBSTONE_COUNT;

/// Scheduling constants re-exported from runtime.
pub const YIELD_CHECK_COST: i64 = rt_process::YIELD_CHECK_COST;

/// Status codes (must match process.rs STATUS_* constants)
pub const STATUS_OK: u32 = rt_process::STATUS_OK as u32;
pub const STATUS_YIELD: u32 = rt_process::STATUS_YIELD as u32;
pub const STATUS_RECEIVE_WAIT: u32 = rt_process::STATUS_RECEIVE_WAIT as u32;
pub const STATUS_EXCEPTION: u32 = rt_process::STATUS_EXCEPTION as u32;

/// Small int range (must match nera-runtime/src/term.rs).
pub const SMALL_INT_MIN: i64 = -(1_i64 << 60);
pub const SMALL_INT_MAX: i64 = (1_i64 << 60) - 1;

/// Bias for single-comparison small-int overflow detection.
/// `((result + SMALL_INT_OVERFLOW_BIAS) as u64) < SMALL_INT_RANGE` means result fits.
pub const SMALL_INT_OVERFLOW_BIAS: i64 = 1_i64 << 60; // = -SMALL_INT_MIN
pub const SMALL_INT_RANGE: u64 = 1_u64 << 61; // = SMALL_INT_MAX - SMALL_INT_MIN + 1

/// Check if an i64 value fits in the small int range (61-bit signed).
pub const fn fits_small_int(val: i64) -> bool {
    val >= SMALL_INT_MIN && val <= SMALL_INT_MAX
}

/// Build a small integer ErlTerm value from a constant.
pub const fn encode_small_int(val: i64) -> u64 {
    ((val as u64) << TAG_BITS) | TAG_SMALL_INT
}

/// Build an atom ErlTerm value from an atom id.
pub const fn encode_atom(id: u32) -> u64 {
    ((id as u64) << TAG_BITS) | TAG_ATOM
}

/// Build a pid ErlTerm value from a process id.
pub const fn encode_pid(id: u64) -> u64 {
    (id << TAG_BITS) | TAG_PID
}

/// The nil value.
pub const NIL_VALUE: u64 = TAG_NIL;

/// Tombstone sentinel used by the mailbox to mark removed slots.
pub const TOMBSTONE_VALUE: u64 = nera_runtime::term::TOMBSTONE_VALUE;

/// Returned by `receive_peek` and `receive_next` when the mailbox scan ends.
pub const NO_MESSAGE_VALUE: u64 = nera_runtime::term::NO_MESSAGE_VALUE;

/// Match context header word (subtype=BOXED_MATCH_CTX, arity=3).
pub const MATCH_CTX_HEADER: u64 = encode_boxed_header(BoxedSubtype::MatchContext, 3);

/// Match context allocation size in bytes (4 words: header + orig_binary + bit_offset + total_bits).
pub const MATCH_CTX_ALLOC_BYTES: i64 = nera_runtime::match_context::BYTE_SIZE as i64;
pub const MATCH_CTX_BINARY_OFFSET: i32 = nera_runtime::match_context::BINARY_OFFSET;
pub const MATCH_CTX_BIT_OFFSET: i32 = nera_runtime::match_context::BIT_OFFSET;
pub const MATCH_CTX_END_OFFSET: i32 = nera_runtime::match_context::END_OFFSET;

/// Byte offset of the data pointer in the runtime's `repr(C)` binary owner.
/// Re-exported from the runtime for inline refc binary data extraction.
pub const BINARY_DATA_PTR_BYTE_OFFSET: i32 = nera_runtime::binary::BINARY_DATA_PTR_BYTE_OFFSET;

/// Build a boxed header word.
/// ```compile_fail
/// use nera_codegen::term_layout::encode_boxed_header;
/// let _ = encode_boxed_header(0x10, 0);
/// ```
pub const fn encode_boxed_header(tag: BoxedSubtype, arity: u64) -> u64 {
    (arity << 4) | tag as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use nera_runtime::term as rt_term;

    // ---------------------------------------------------------------
    // Cross-validate constants against nera-runtime/term.rs
    // ---------------------------------------------------------------

    #[test]
    fn tag_constants_match_runtime() {
        assert_eq!(TAG_BOXED, rt_term::TAG_BOXED);
        assert_eq!(TAG_CONS, rt_term::TAG_CONS);
        assert_eq!(TAG_SMALL_INT, rt_term::TAG_SMALL_INT);
        assert_eq!(TAG_ATOM, rt_term::TAG_ATOM);
        assert_eq!(TAG_PID, rt_term::TAG_PID);
        assert_eq!(TAG_NIL, rt_term::TAG_NIL);
        assert_eq!(TAG_MASK, rt_term::TAG_MASK);
        assert_eq!(TAG_BITS, rt_term::TAG_BITS);
    }

    #[test]
    fn external_bit_matches_runtime() {
        assert_eq!(EXTERNAL_BIT, rt_term::EXTERNAL_BIT);
    }

    #[test]
    fn boxed_subtag_constants_match_runtime() {
        assert_eq!(BOXED_TUPLE, rt_term::BOXED_TUPLE);
        assert_eq!(BOXED_FLOAT, rt_term::BOXED_FLOAT);
        assert_eq!(BOXED_BIGINT, rt_term::BOXED_BIGINT);
        assert_eq!(BOXED_HEAP_BINARY, rt_term::BOXED_HEAP_BINARY);
        assert_eq!(BOXED_BINARY, rt_term::BOXED_BINARY);
        assert_eq!(BOXED_CLOSURE, rt_term::BOXED_CLOSURE);
        assert_eq!(BOXED_MAP, rt_term::BOXED_MAP);
        assert_eq!(BOXED_REFC_BINARY, rt_term::BOXED_REFC_BINARY);
        assert_eq!(BOXED_REFERENCE, rt_term::BOXED_REFERENCE);
        assert_eq!(BOXED_MATCH_CTX, rt_term::BOXED_MATCH_CTX);
    }

    #[test]
    fn small_int_range_matches_runtime() {
        assert_eq!(SMALL_INT_MIN, rt_term::SMALL_INT_MIN);
        assert_eq!(SMALL_INT_MAX, rt_term::SMALL_INT_MAX);
    }

    #[test]
    fn status_codes_match_runtime() {
        use nera_runtime::process as rt_proc;
        assert_eq!(STATUS_OK, rt_proc::STATUS_OK as u32);
        assert_eq!(STATUS_YIELD, rt_proc::STATUS_YIELD as u32);
        assert_eq!(STATUS_RECEIVE_WAIT, rt_proc::STATUS_RECEIVE_WAIT as u32);
        assert_eq!(STATUS_EXCEPTION, rt_proc::STATUS_EXCEPTION as u32);
        for (status, raw) in [
            (rt_proc::ExecutionStatus::Ok, 0),
            (rt_proc::ExecutionStatus::Yield, 1),
            (rt_proc::ExecutionStatus::ReceiveWait, 2),
            (rt_proc::ExecutionStatus::Exception, 3),
            (rt_proc::ExecutionStatus::IoWait, 4),
            (rt_proc::ExecutionStatus::Hibernate, 5),
            (rt_proc::ExecutionStatus::TailCall, 6),
        ] {
            assert_eq!(status as u32, raw);
        }

    }

    #[test]
    fn ctx_offsets_match_runtime() {
        use nera_runtime::process as rt_proc;
        assert_eq!(CTX_OFFSET_PID, rt_proc::CTX_OFFSET_PID);
        assert_eq!(CTX_OFFSET_REDUCTIONS, rt_proc::CTX_OFFSET_REDUCTIONS);
        assert_eq!(CTX_OFFSET_STATUS, rt_proc::CTX_OFFSET_STATUS);
        assert_eq!(CTX_OFFSET_RESULT, rt_proc::CTX_OFFSET_RESULT);
        assert_eq!(CTX_OFFSET_HEAP_PTR, rt_proc::CTX_OFFSET_HEAP_PTR);
        assert_eq!(CTX_OFFSET_RESUME_RECEIVE, rt_proc::CTX_OFFSET_RESUME_RECEIVE);
        assert_eq!(CTX_OFFSET_RECEIVE_TIMEOUT, rt_proc::CTX_OFFSET_RECEIVE_TIMEOUT);
        assert_eq!(CTX_OFFSET_STACK_PTR, rt_proc::CTX_OFFSET_STACK_PTR);
        assert_eq!(CTX_OFFSET_EXCEPTION_TRACE_PTR, rt_proc::CTX_OFFSET_EXCEPTION_TRACE_PTR);
        assert_eq!(CTX_OFFSET_RUNTIME, rt_proc::CTX_OFFSET_RUNTIME);
    }

    // ---------------------------------------------------------------
    // fits_small_int
    // ---------------------------------------------------------------

    #[test]
    fn fits_small_int_boundaries() {
        assert!(fits_small_int(0));
        assert!(fits_small_int(1));
        assert!(fits_small_int(-1));
        assert!(fits_small_int(SMALL_INT_MIN));
        assert!(fits_small_int(SMALL_INT_MAX));
        assert!(!fits_small_int(SMALL_INT_MIN - 1));
        assert!(!fits_small_int(SMALL_INT_MAX + 1));
        assert!(!fits_small_int(i64::MIN));
        assert!(!fits_small_int(i64::MAX));
    }

    // ---------------------------------------------------------------
    // encode_small_int
    // ---------------------------------------------------------------

    #[test]
    fn encode_small_int_tag_bits() {
        let encoded = encode_small_int(0);
        assert_eq!(encoded & TAG_MASK, TAG_SMALL_INT);

        let encoded = encode_small_int(42);
        assert_eq!(encoded & TAG_MASK, TAG_SMALL_INT);

        let encoded = encode_small_int(-1);
        assert_eq!(encoded & TAG_MASK, TAG_SMALL_INT);
    }

    #[test]
    fn encode_small_int_roundtrip() {
        for val in [0i64, 1, -1, 42, -42, 1000, SMALL_INT_MAX, SMALL_INT_MIN] {
            let encoded = encode_small_int(val);
            // Decode: arithmetic right shift by TAG_BITS
            let decoded = (encoded as i64) >> TAG_BITS;
            assert_eq!(decoded, val, "roundtrip failed for {}", val);
        }
    }

    // ---------------------------------------------------------------
    // encode_atom
    // ---------------------------------------------------------------

    #[test]
    fn encode_atom_tag_bits() {
        let encoded = encode_atom(0);
        assert_eq!(encoded & TAG_MASK, TAG_ATOM);

        let encoded = encode_atom(42);
        assert_eq!(encoded & TAG_MASK, TAG_ATOM);
    }

    #[test]
    fn encode_atom_roundtrip() {
        for id in [0u32, 1, 42, 1000, u32::MAX >> 3] {
            let encoded = encode_atom(id);
            let decoded = (encoded >> TAG_BITS) as u32;
            assert_eq!(decoded, id, "atom roundtrip failed for {}", id);
        }
    }

    // ---------------------------------------------------------------
    // encode_pid
    // ---------------------------------------------------------------

    #[test]
    fn encode_pid_tag_bits() {
        let encoded = encode_pid(0);
        assert_eq!(encoded & TAG_MASK, TAG_PID);

        let encoded = encode_pid(99);
        assert_eq!(encoded & TAG_MASK, TAG_PID);
    }

    #[test]
    fn encode_pid_roundtrip() {
        for id in [0u64, 1, 42, 1000] {
            let encoded = encode_pid(id);
            let decoded = encoded >> TAG_BITS;
            assert_eq!(decoded, id, "pid roundtrip failed for {}", id);
        }
    }

    // ---------------------------------------------------------------
    // encode_boxed_header
    // ---------------------------------------------------------------

    #[test]
    fn encode_boxed_header_preserves_tag() {
        let tags = [
            BOXED_TUPLE, BOXED_FLOAT, BOXED_BIGINT, BOXED_HEAP_BINARY,
            BOXED_CLOSURE, BOXED_MAP, BOXED_REFC_BINARY, BOXED_REFERENCE, BOXED_MATCH_CTX,
        ];
        for tag in tags {
            let header = encode_boxed_header(BoxedSubtype::try_from(tag).unwrap(), 5);
            assert_eq!(header, nera_runtime::term::BoxedHeader::new(BoxedSubtype::try_from(tag).unwrap(), 5).0);
            assert_eq!(header & 0xF, tag, "tag not preserved for subtag {:#x}", tag);
        }
    }

    #[test]
    fn encode_boxed_header_preserves_arity() {
        for arity in [0u64, 1, 3, 10, 255, 1024] {
            let header = encode_boxed_header(BoxedSubtype::Tuple, arity);
            let decoded_arity = header >> 4;
            assert_eq!(decoded_arity, arity, "arity not preserved for {}", arity);
        }
    }

    // ---------------------------------------------------------------
    // NIL_VALUE
    // ---------------------------------------------------------------

    #[test]
    fn nil_value_is_tag_nil() {
        assert_eq!(NIL_VALUE, TAG_NIL);
        assert_eq!(NIL_VALUE & TAG_MASK, TAG_NIL);
    }

    // ---------------------------------------------------------------
    // Size constants
    // ---------------------------------------------------------------

    #[test]
    fn word_size_is_8() {
        assert_eq!(WORD_SIZE, 8);
        assert_eq!(WORD_SIZE, std::mem::size_of::<u64>());
    }

    #[test]
    fn cons_cell_is_two_words() {
        assert_eq!(CONS_CELL_SIZE, 2 * WORD_SIZE);
        assert_eq!(CONS_CELL_SIZE, 16);
    }

    // ---------------------------------------------------------------
    // Tag uniqueness
    // ---------------------------------------------------------------

    #[test]
    fn primary_tags_are_unique() {
        let tags = [TAG_BOXED, TAG_CONS, TAG_SMALL_INT, TAG_ATOM, TAG_PID, TAG_NIL];
        for (i, &a) in tags.iter().enumerate() {
            for (j, &b) in tags.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "tags at index {} and {} collide", i, j);
                }
            }
        }
    }

    #[test]
    fn boxed_subtags_are_unique() {
        let tags = [
            BOXED_TUPLE, BOXED_FLOAT, BOXED_BIGINT, BOXED_HEAP_BINARY,
            BOXED_CLOSURE, BOXED_MAP, BOXED_REFC_BINARY, BOXED_REFERENCE,
        ];
        for (i, &a) in tags.iter().enumerate() {
            for (j, &b) in tags.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "boxed subtags at index {} and {} collide", i, j);
                }
            }
        }
    }

    #[test]
    fn primary_tags_fit_in_3_bits() {
        let tags = [TAG_BOXED, TAG_CONS, TAG_SMALL_INT, TAG_ATOM, TAG_PID, TAG_NIL];
        for tag in tags {
            assert!(tag <= TAG_MASK, "tag {:#b} exceeds 3-bit range", tag);
        }
    }

    #[test]
    fn boxed_subtags_fit_in_4_bits() {
        let tags = [
            BOXED_TUPLE, BOXED_FLOAT, BOXED_BIGINT, BOXED_HEAP_BINARY,
            BOXED_CLOSURE, BOXED_MAP, BOXED_REFC_BINARY, BOXED_REFERENCE,
        ];
        for tag in tags {
            assert!(tag <= 0xF, "boxed subtag {:#x} exceeds 4-bit range", tag);
        }
    }
}
