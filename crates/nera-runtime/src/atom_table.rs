//! Global atom table.
//!
//! Atoms are interned strings identified by a `u32` index. Interning goes
//! through a `parking_lot::RwLock`-protected name map. Names are read by
//! index without a lock, so that atom ordering and name lookups do not
//! contend. A global singleton is lazily initialised on first use and
//! pre-populated with the standard Erlang built-in atoms.

use parking_lot::RwLock;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// Built-in atom constants
// ---------------------------------------------------------------------------

pub const ATOM_OK: u32 = 0;
pub const ATOM_ERROR: u32 = 1;
pub const ATOM_TRUE: u32 = 2;
pub const ATOM_FALSE: u32 = 3;
pub const ATOM_UNDEFINED: u32 = 4;
pub const ATOM_NORMAL: u32 = 5;
pub const ATOM_BADARG: u32 = 6;
pub const ATOM_BADARITH: u32 = 7;
pub const ATOM_BADMATCH: u32 = 8;
pub const ATOM_FUNCTION_CLAUSE: u32 = 9;

// Network / POSIX error atoms (matching Erlang's inet error reasons)
pub const ATOM_CLOSED: u32 = 10;
pub const ATOM_EACCES: u32 = 11;
pub const ATOM_EADDRINUSE: u32 = 12;
pub const ATOM_EADDRNOTAVAIL: u32 = 13;
pub const ATOM_ECONNREFUSED: u32 = 14;
pub const ATOM_ECONNRESET: u32 = 15;
pub const ATOM_ECONNABORTED: u32 = 16;
pub const ATOM_ENOTCONN: u32 = 17;
pub const ATOM_EPIPE: u32 = 18;
pub const ATOM_ETIMEDOUT: u32 = 19;
pub const ATOM_ENOENT: u32 = 20;
pub const ATOM_EINVAL: u32 = 21;

// OTP / link / monitor atoms
pub const ATOM_EXIT_SIGNAL: u32 = 22; // "EXIT"
pub const ATOM_DOWN: u32 = 23;        // "DOWN"
pub const ATOM_PROCESS: u32 = 24;     // "process"
pub const ATOM_KILL: u32 = 25;        // "kill"
pub const ATOM_KILLED: u32 = 26;      // "killed"
pub const ATOM_NOPROC: u32 = 27;      // "noproc"
pub const ATOM_TRAP_EXIT: u32 = 28;   // "trap_exit"

// File I/O POSIX error atoms
pub const ATOM_EEXIST: u32 = 29;      // "eexist"  — file already exists
pub const ATOM_EISDIR: u32 = 30;      // "eisdir"  — is a directory
pub const ATOM_ENOSPC: u32 = 31;      // "enospc"  — no space left on device
pub const ATOM_MESSAGE_QUEUE_LEN: u32 = 32; // "message_queue_len"

// Additional POSIX error atoms
pub const ATOM_EMFILE: u32 = 33;      // "emfile"  — too many open files (per-process)
pub const ATOM_ENFILE: u32 = 34;      // "enfile"  — too many open files (system-wide)

// Process management atoms
pub const ATOM_FULLSWEEP_AFTER: u32 = 35;  // "fullsweep_after"
pub const ATOM_MAX_HEAP_SIZE: u32 = 36;    // "max_heap_size"
pub const ATOM_HIBERNATE: u32 = 37;        // "hibernate"
pub const ATOM_MIN_HEAP_SIZE: u32 = 38;    // "min_heap_size"
pub const ATOM_LINK: u32 = 39;             // "link"
pub const ATOM_MONITOR_OPT: u32 = 40;      // "monitor"
pub const ATOM_INFINITY: u32 = 42;
pub const ATOM_TIMEOUT_VALUE: u32 = 43;
pub const ATOM_SYSTEM_LIMIT: u32 = 41;     // "system_limit"

/// Maximum number of atoms allowed. Matches the BEAM VM default.
/// Atoms are never garbage-collected, so this prevents unbounded growth.
pub const ATOM_LIMIT: u32 = 1_048_576;

/// Names per block. A block is allocated when its first atom is interned.
const NAME_BLOCK_LEN: usize = 1024;

// ---------------------------------------------------------------------------
// AtomTable
// ---------------------------------------------------------------------------

/// An interned atom's name and the ordering key derived from it.
struct AtomName {
    /// The name's first eight bytes, big-endian, padded with zero bytes.
    /// When two keys differ, the names order as the keys do, so most
    /// comparisons never read the text.
    order_key: u64,
    text: Box<str>,
}

impl AtomName {
    fn new(text: &str) -> Self {
        let mut prefix = [0u8; 8];
        let len = text.len().min(prefix.len());
        prefix[..len].copy_from_slice(&text.as_bytes()[..len]);
        AtomName { order_key: u64::from_be_bytes(prefix), text: text.into() }
    }
}

type NameBlock = Box<[OnceLock<AtomName>]>;

pub struct AtomTable {
    to_index: RwLock<HashMap<String, u32>>,
    /// Names by index in blocks of `NAME_BLOCK_LEN`. Each block and each
    /// name is written once, while `to_index` is write-locked and before the
    /// index is returned, so a reader holding an atom reads it lock-free.
    names: Box<[OnceLock<NameBlock>]>,
    limit: u32,
}

impl Default for AtomTable {
    fn default() -> Self {
        Self::new()
    }
}

impl AtomTable {
    /// Create an empty atom table with the default limit (`ATOM_LIMIT`).
    pub fn new() -> Self {
        Self::with_capacity_limit(ATOM_LIMIT)
    }

    /// Create an atom table with a custom limit.
    /// Useful for testing atom exhaustion without filling 1M entries.
    #[cfg(test)]
    pub fn with_limit(limit: u32) -> Self {
        Self::with_capacity_limit(limit)
    }

    fn with_capacity_limit(limit: u32) -> Self {
        let blocks = (limit as usize).div_ceil(NAME_BLOCK_LEN);
        AtomTable {
            to_index: RwLock::new(HashMap::new()),
            names: (0..blocks).map(|_| OnceLock::new()).collect(),
            limit,
        }
    }

    /// Intern a string and return its atom index.
    ///
    /// If the atom already exists the existing index is returned. Otherwise a
    /// new index is allocated. Panics if the atom table is full (exceeds
    /// `ATOM_LIMIT`), matching BEAM's `system_limit` behavior.
    ///
    /// This method is safe to call from multiple threads.
    pub fn intern(&self, name: &str) -> u32 {
        match self.try_intern(name) {
            Some(id) => id,
            None => panic!(
                "system_limit: atom table full ({} atoms)",
                ATOM_LIMIT
            ),
        }
    }

    /// Try to intern a string. Returns `None` if the atom table is full.
    ///
    /// If the atom already exists the existing index is returned. Otherwise a
    /// new index is allocated, unless the table has reached `ATOM_LIMIT`.
    pub fn try_intern(&self, name: &str) -> Option<u32> {
        // Fast path: read lock only.
        {
            let map = self.to_index.read();
            if let Some(&id) = map.get(name) {
                return Some(id);
            }
        }

        // Slow path: acquire write lock, double-check, insert.
        let mut map = self.to_index.write();
        if let Some(&id) = map.get(name) {
            return Some(id);
        }

        let count = map.len() as u32;
        if count >= self.limit {
            return None;
        }
        // Publish the name before the index can escape this lock.
        let block = self.names[count as usize / NAME_BLOCK_LEN]
            .get_or_init(|| (0..NAME_BLOCK_LEN).map(|_| OnceLock::new()).collect());
        if block[count as usize % NAME_BLOCK_LEN].set(AtomName::new(name)).is_err() {
            unreachable!("atom index {count} was issued twice");
        }
        map.insert(name.to_owned(), count);
        Some(count)
    }

    /// Look up an atom by name without interning it.
    /// Returns `Some(id)` if the atom already exists, `None` otherwise.
    pub fn lookup_by_name(&self, name: &str) -> Option<u32> {
        let map = self.to_index.read();
        map.get(name).copied()
    }

    /// Return the current number of interned atoms.
    pub fn count(&self) -> usize {
        self.to_index.read().len()
    }

    /// Look up the string for an atom index.
    pub fn resolve(&self, id: u32) -> Option<String> {
        self.entry(id).map(|name| name.text.to_string())
    }

    /// Read an interned name without taking a lock.
    fn entry(&self, id: u32) -> Option<&AtomName> {
        let id = id as usize;
        self.names.get(id / NAME_BLOCK_LEN)?.get()?[id % NAME_BLOCK_LEN].get()
    }

    /// Order two atoms as BEAM does: by their UTF-8 names, byte by byte, with
    /// a name before every longer name that starts with it. The order never
    /// depends on when the atoms were created. Equal indices are one atom.
    pub fn compare(&self, a: u32, b: u32) -> Ordering {
        if a == b {
            return Ordering::Equal;
        }
        match (self.entry(a), self.entry(b)) {
            (Some(a), Some(b)) => a
                .order_key
                .cmp(&b.order_key)
                .then_with(|| a.text.as_bytes().cmp(b.text.as_bytes())),
            // No interned atom has an index the table never issued. Keep the
            // order total for such raw terms: after every name, by index.
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => a.cmp(&b),
        }
    }

    /// Pre-register the standard built-in atoms at their well-known indices.
    pub fn register_builtins(&self) {
        let builtins = [
            "ok",              // 0
            "error",           // 1
            "true",            // 2
            "false",           // 3
            "undefined",       // 4
            "normal",          // 5
            "badarg",          // 6
            "badarith",        // 7
            "badmatch",        // 8
            "function_clause", // 9
            // Network / POSIX error atoms (matching Erlang's inet reasons)
            "closed",          // 10
            "eacces",          // 11
            "eaddrinuse",      // 12
            "eaddrnotavail",   // 13
            "econnrefused",    // 14
            "econnreset",      // 15
            "econnaborted",    // 16
            "enotconn",        // 17
            "epipe",           // 18
            "etimedout",       // 19
            "enoent",          // 20
            "einval",          // 21
            // OTP / link / monitor atoms
            "EXIT",            // 22
            "DOWN",            // 23
            "process",         // 24
            "kill",            // 25
            "killed",          // 26
            "noproc",          // 27
            "trap_exit",       // 28
            // File I/O POSIX error atoms
            "eexist",          // 29
            "eisdir",          // 30
            "enospc",          // 31
            // process_info keys
            "message_queue_len", // 32
            // Additional POSIX error atoms
            "emfile",          // 33
            "enfile",          // 34
            // Process management atoms
            "fullsweep_after", // 35
            "max_heap_size",   // 36
            "hibernate",       // 37
            "min_heap_size",   // 38
            "link",            // 39
            "monitor",         // 40
            "system_limit",    // 41
            "infinity",        // 42
            "timeout_value",   // 43
        ];
        for name in &builtins {
            self.intern(name);
        }
    }
}

// ---------------------------------------------------------------------------
// Global singleton
// ---------------------------------------------------------------------------

static GLOBAL_ATOMS: OnceLock<AtomTable> = OnceLock::new();

/// Return a reference to the global atom table, lazily initialising it
/// and registering built-in atoms on first access.
pub fn global_atom_table() -> &'static AtomTable {
    GLOBAL_ATOMS.get_or_init(|| {
        let table = AtomTable::new();
        table.register_builtins();
        table
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_and_resolve() {
        let table = AtomTable::new();
        let id = table.intern("hello");
        assert_eq!(table.resolve(id), Some("hello".to_string()));
    }

    #[test]
    fn intern_is_idempotent() {
        let table = AtomTable::new();
        let id1 = table.intern("world");
        let id2 = table.intern("world");
        assert_eq!(id1, id2);
    }

    #[test]
    fn distinct_atoms_get_distinct_ids() {
        let table = AtomTable::new();
        let a = table.intern("foo");
        let b = table.intern("bar");
        assert_ne!(a, b);
    }

    #[test]
    fn resolve_unknown_returns_none() {
        let table = AtomTable::new();
        assert_eq!(table.resolve(999), None);
    }

    #[test]
    fn builtins_registered_at_expected_indices() {
        let table = AtomTable::new();
        table.register_builtins();

        assert_eq!(table.resolve(ATOM_OK), Some("ok".to_string()));
        assert_eq!(table.resolve(ATOM_ERROR), Some("error".to_string()));
        assert_eq!(table.resolve(ATOM_TRUE), Some("true".to_string()));
        assert_eq!(table.resolve(ATOM_FALSE), Some("false".to_string()));
        assert_eq!(table.resolve(ATOM_UNDEFINED), Some("undefined".to_string()));
        assert_eq!(table.resolve(ATOM_NORMAL), Some("normal".to_string()));
        assert_eq!(table.resolve(ATOM_BADARG), Some("badarg".to_string()));
        assert_eq!(table.resolve(ATOM_BADARITH), Some("badarith".to_string()));
        assert_eq!(table.resolve(ATOM_BADMATCH), Some("badmatch".to_string()));
        assert_eq!(
            table.resolve(ATOM_FUNCTION_CLAUSE),
            Some("function_clause".to_string())
        );
        assert_eq!(table.resolve(ATOM_EXIT_SIGNAL), Some("EXIT".to_string()));
        assert_eq!(table.resolve(ATOM_DOWN), Some("DOWN".to_string()));
        assert_eq!(table.resolve(ATOM_PROCESS), Some("process".to_string()));
        assert_eq!(table.resolve(ATOM_KILL), Some("kill".to_string()));
        assert_eq!(table.resolve(ATOM_KILLED), Some("killed".to_string()));
        assert_eq!(table.resolve(ATOM_NOPROC), Some("noproc".to_string()));
        assert_eq!(table.resolve(ATOM_TRAP_EXIT), Some("trap_exit".to_string()));
        assert_eq!(table.resolve(ATOM_EEXIST), Some("eexist".to_string()));
        assert_eq!(table.resolve(ATOM_EISDIR), Some("eisdir".to_string()));
        assert_eq!(table.resolve(ATOM_ENOSPC), Some("enospc".to_string()));
    }

    #[test]
    fn global_atom_table_has_builtins() {
        let table = global_atom_table();
        assert_eq!(table.resolve(ATOM_OK), Some("ok".to_string()));
        assert_eq!(table.resolve(ATOM_TRUE), Some("true".to_string()));
    }

    #[test]
    fn global_atom_table_intern() {
        let table = global_atom_table();
        let id = table.intern("my_custom_atom");
        assert_eq!(table.resolve(id), Some("my_custom_atom".to_string()));
    }

    #[test]
    fn network_builtins_registered() {
        let table = AtomTable::new();
        table.register_builtins();
        assert_eq!(table.resolve(ATOM_CLOSED), Some("closed".to_string()));
        assert_eq!(table.resolve(ATOM_EACCES), Some("eacces".to_string()));
        assert_eq!(table.resolve(ATOM_EADDRINUSE), Some("eaddrinuse".to_string()));
        assert_eq!(table.resolve(ATOM_EINVAL), Some("einval".to_string()));
    }

    #[test]
    fn try_intern_returns_none_when_full() {
        let table = AtomTable::with_limit(5);
        // Fill the table to its limit.
        for i in 0..5 {
            let name = format!("atom_{}", i);
            assert!(table.try_intern(&name).is_some(), "atom {} should succeed", i);
        }
        assert_eq!(table.count(), 5);
        // Next new atom should fail.
        assert_eq!(table.try_intern("one_too_many"), None);
        // But re-interning an existing atom still succeeds.
        assert!(table.try_intern("atom_0").is_some());
    }

    #[test]
    #[should_panic(expected = "system_limit")]
    fn intern_panics_when_full() {
        let table = AtomTable::with_limit(3);
        table.intern("a");
        table.intern("b");
        table.intern("c");
        // Table is full — this should panic.
        table.intern("d");
    }

    #[test]
    fn lookup_by_name_returns_none_for_unknown() {
        let table = AtomTable::new();
        assert_eq!(table.lookup_by_name("nonexistent"), None);
    }

    #[test]
    fn lookup_by_name_returns_id_for_known() {
        let table = AtomTable::new();
        let id = table.intern("hello");
        assert_eq!(table.lookup_by_name("hello"), Some(id));
    }

    #[test]
    fn lookup_by_name_does_not_intern() {
        let table = AtomTable::new();
        table.intern("existing");
        let before = table.count();
        assert_eq!(table.lookup_by_name("novel"), None);
        assert_eq!(table.count(), before);
    }

    #[test]
    fn count_tracks_atom_count() {
        let table = AtomTable::new();
        assert_eq!(table.count(), 0);
        table.intern("a");
        assert_eq!(table.count(), 1);
        table.intern("b");
        assert_eq!(table.count(), 2);
        // Duplicate doesn't increase count.
        table.intern("a");
        assert_eq!(table.count(), 2);
    }

    // Regression: atoms ordered by index, which is creation order, so zebra
    // sorted before apple when created first. BEAM orders atoms by name.
    #[test]
    fn compare_orders_by_name_not_creation() {
        let table = AtomTable::new();
        // Created last name first, as a module that mentions them so does.
        let names = ["zebra", "mango", "apple", "last_event_id", "event_type", "data"];
        let mut ids: Vec<u32> = names.iter().map(|name| table.intern(name)).collect();
        ids.sort_by(|&a, &b| table.compare(a, b));
        let sorted: Vec<String> = ids.iter().map(|&id| table.resolve(id).unwrap()).collect();
        let mut expected: Vec<String> = names.iter().map(|name| name.to_string()).collect();
        expected.sort();
        assert_eq!(sorted, expected);
        assert_eq!(table.compare(ids[0], ids[0]), Ordering::Equal);
    }

    // BEAM compares atom names as UTF-8 bytes, a name before every longer
    // name that starts with it (checked against OTP 29 on random atoms).
    // These names share or end inside the eight-byte order key.
    #[test]
    fn compare_matches_utf8_byte_order_past_the_order_key() {
        let table = AtomTable::new();
        let names = [
            "abcdefgi", "abcdefgh_z", "abcdefgh_a", "abcdefgh_", "abcdefgh", "a\0", "a",
            "", "Zebra", "_apple", "\u{1F600}", "\u{4E2D}", "\u{100}", "\u{FF}", "\u{E9}", "z",
            "a\u{100}", "a\u{FF}", "a\u{E9}z", "a\u{E9}",
        ];
        let ids: Vec<u32> = names.iter().map(|name| table.intern(name)).collect();
        for (&a, name_a) in ids.iter().zip(names) {
            for (&b, name_b) in ids.iter().zip(names) {
                assert_eq!(
                    table.compare(a, b),
                    name_a.as_bytes().cmp(name_b.as_bytes()),
                    "{name_a:?} vs {name_b:?}"
                );
            }
        }
    }

    // Names are read without a lock: an atom interned on one thread must be
    // readable and ordered on another, across name block boundaries.
    #[test]
    fn names_interned_on_other_threads_are_readable() {
        let table = std::sync::Arc::new(AtomTable::new());
        let (sender, receiver) = std::sync::mpsc::channel();
        let writers: Vec<_> = (0..4)
            .map(|thread| {
                let table = table.clone();
                let sender = sender.clone();
                std::thread::spawn(move || {
                    for i in 0..1500 {
                        let name = format!("t{thread}_{i:04}");
                        let id = table.intern(&name);
                        sender.send((id, name)).unwrap();
                    }
                })
            })
            .collect();
        drop(sender);
        let mut seen: Vec<(u32, String)> = Vec::new();
        for (id, name) in receiver {
            assert_eq!(table.resolve(id).as_deref(), Some(name.as_str()));
            if let Some((other, other_name)) = seen.last() {
                assert_eq!(table.compare(id, *other), name.cmp(other_name));
            }
            seen.push((id, name));
        }
        for writer in writers {
            writer.join().unwrap();
        }
        assert_eq!(seen.len(), 6000);
        assert_eq!(table.count(), 6000);
    }
}
