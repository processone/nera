use std::{io, sync::Arc};
use parking_lot::{Mutex, RwLock};
use nera_runtime::socket::{Socket, SocketEntry, SocketOpts};
use nera_runtime::term;
const FREE_SENTINEL: u32 = u32::MAX;

/// A generation-tagged socket identity in the positive small-integer domain.
/// Domain validity does not imply that the socket is still registered.
///
/// ```compile_fail
/// use nera_runtime::socket::SocketHandle;
/// let _ = SocketHandle(0);
/// ```
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SocketHandle(u64);

impl SocketHandle {
    pub const MAX_GENERATION: u32 = (crate::term::SMALL_INT_MAX as u64 >> 32) as u32;
    pub const MAX_INDEX: u32 = FREE_SENTINEL - 1;

    #[inline]
    pub const fn try_from_raw(raw: u64) -> Option<Self> {
        if raw <= crate::term::SMALL_INT_MAX as u64 && raw >> 32 != 0 && raw as u32 != FREE_SENTINEL
        {
            Some(Self(raw))
        } else {
            None
        }
    }

    #[inline]
    pub fn from_term(term: crate::term::ErlTerm) -> Option<Self> {
        Self::try_from_raw(term.as_small_int()? as u64)
    }

    #[inline]
    pub const fn as_raw(self) -> u64 {
        self.0
    }

    /// Encoding needs no further range check: construction validates the domain.
    #[inline]
    pub const fn to_term(self) -> crate::term::ErlTerm {
        crate::term::ErlTerm::from_raw(
            (self.0 << crate::term::TAG_BITS) | crate::term::TAG_SMALL_INT,
        )
    }
}

impl std::fmt::Display for SocketHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone)]
enum Slot {
    /// No socket owner. Below MAX_GENERATION it is on the free list;
    /// at MAX_GENERATION it is permanently retired and unlinked.
    Vacant {
        /// Next free-list link, or `FREE_SENTINEL` for end of list.
        next_free: u32,
        /// Generation of the *last* occupied state on this slot. On the
        /// next free → occupied transition we bump this and reuse it as
        /// the new live generation.
        last_generation: u32,
    },
    Occupied {
        generation: u32,
        entry: Arc<SocketEntry>,
    },
}

struct Registry {
    slots: Vec<Slot>,
    free_head: u32,
}

impl Registry {
    const fn new() -> Self {
        Registry {
            slots: Vec::new(),
            free_head: FREE_SENTINEL,
        }
    }

    fn insert(&mut self, entry: Arc<SocketEntry>) -> io::Result<SocketHandle> {
        if self.free_head != FREE_SENTINEL {
            let idx = self.free_head as usize;
            let new_generation = match self.slots[idx] {
                Slot::Vacant { next_free, last_generation } => {
                    self.free_head = next_free;
                    // remove() only links slots below the final generation.
                    debug_assert!(last_generation < SocketHandle::MAX_GENERATION);
                    last_generation + 1
                }
                Slot::Occupied { .. } => unreachable!("free_head pointed at non-free slot"),
            };
            self.slots[idx] = Slot::Occupied {
                generation: new_generation,
                entry,
            };
            Ok(encode_handle(idx as u32, new_generation))
        } else {
            let idx = self.slots.len();
            let index = vacant_index(idx)?;
            self.slots.push(Slot::Occupied {
                generation: 1,
                entry,
            });
            Ok(encode_handle(index, 1))
        }
    }

    fn get(&self, handle: SocketHandle) -> Option<&Arc<SocketEntry>> {
        let (idx, gen) = decode_handle(handle);
        let slot = self.slots.get(idx as usize)?;
        match slot {
            Slot::Occupied { generation, entry } if *generation == gen => Some(entry),
            _ => None,
        }
    }

    fn remove(&mut self, handle: SocketHandle) -> Option<Arc<SocketEntry>> {
        let (idx, gen) = decode_handle(handle);
        let slot = self.slots.get_mut(idx as usize)?;
        // Only remove if the generation matches — stale handles silently
        // fail, matching the old "HashMap::remove on stale key" behaviour.
        match slot {
            Slot::Occupied { generation, .. } if *generation == gen => {
                let prev_free_head = self.free_head;
                // Replace the slot with a Free marker, extracting the
                // Arc as we go.
                let old = std::mem::replace(
                    slot,
                    Slot::Vacant {
                        next_free: if gen < SocketHandle::MAX_GENERATION { prev_free_head } else { FREE_SENTINEL },
                        last_generation: gen,
                    },
                );
                if gen < SocketHandle::MAX_GENERATION { self.free_head = idx; }
                match old {
                    Slot::Occupied { entry, .. } => Some(entry),
                    Slot::Vacant { .. } => unreachable!(),
                }
            }
            _ => None,
        }
    }

    /// Iterate over all occupied slots. Yields `(handle, &entry)`.
    fn iter(&self) -> impl Iterator<Item = (SocketHandle, &Arc<SocketEntry>)> {
        self.slots.iter().enumerate().filter_map(|(idx, slot)| match slot {
            Slot::Occupied { generation, entry } => {
                Some((encode_handle(idx as u32, *generation), entry))
            }
            Slot::Vacant { .. } => None,
        })
    }
}

#[inline]
fn encode_handle(idx: u32, generation: u32) -> SocketHandle {
    SocketHandle::try_from_raw(((generation as u64) << 32) | idx as u64)
        .expect("socket registry preserves handle domain")
}

fn vacant_index(len: usize) -> io::Result<u32> {
    if len > SocketHandle::MAX_INDEX as usize {
        Err(io::Error::other("socket handle space exhausted"))
    } else { Ok(len as u32) }
}

#[inline]
fn decode_handle(handle: SocketHandle) -> (u32, u32) {
    let idx = handle.as_raw() as u32;
    let generation = (handle.as_raw() >> 32) as u32;
    (idx, generation)
}

/// The global socket table. Writes take an exclusive lock; reads take
/// a shared lock and then do a direct Vec index — no hashing, no probe.
static REGISTRY: RwLock<Registry> = RwLock::new(Registry::new());

/// Get an `Arc<SocketEntry>` for a handle. This is the primary lookup
/// on the hot path — every worker `recv`/`send` and every reactor event
/// dispatch goes through it. Cheap: one RwLock read-acquire, one Vec
/// index, one generation compare, one Arc clone.
pub(crate) fn get_entry(handle: SocketHandle) -> Option<Arc<SocketEntry>> {
    REGISTRY.read().get(handle).cloned()
}


fn main() {
    println!("slot_bytes {}", std::mem::size_of::<Slot>());
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let entry = Arc::new(SocketEntry {
        socket: Socket::Listener(Arc::new(mio::net::TcpListener::from_std(listener))),
        opts: Mutex::new(SocketOpts::default()),
    });
    let live = REGISTRY.write().insert(entry.clone()).unwrap();
    for threads in [1, 2, 4, 8] {
        let start = std::time::Instant::now();
        std::thread::scope(|scope| {
            for _ in 0..threads {
                scope.spawn(|| {
                    for _ in 0..(1_000_000 / threads) {
                        let owner = get_entry(std::hint::black_box(live)).unwrap();
                        std::hint::black_box(&owner);
                    }
                });
            }
        });
        println!("lookup_{threads} {}", start.elapsed().as_micros());
    }
    let mut registry = Registry::new();
    let mut handle = registry.insert(entry.clone()).unwrap();
    let start = std::time::Instant::now();
    for _ in 0..200_000 {
        let removed = registry.remove(std::hint::black_box(handle)).unwrap();
        handle = registry.insert(removed).unwrap();
    }
    println!("reuse {}", start.elapsed().as_micros());
    let mut fresh = Registry::new();
    let start = std::time::Instant::now();
    for _ in 0..50_000 {
        std::hint::black_box(fresh.insert(entry.clone()).unwrap());
    }
    println!("fresh {}", start.elapsed().as_micros());
}
