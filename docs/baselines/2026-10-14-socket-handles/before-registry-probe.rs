use std::{io, sync::Arc};
use parking_lot::{Mutex, RwLock};
use nera_runtime::socket::{Socket, SocketEntry, SocketOpts};
use nera_runtime::term;
const FREE_SENTINEL: u32 = u32::MAX;

#[derive(Clone)]
enum Slot {
    Free {
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

    fn insert(&mut self, entry: Arc<SocketEntry>) -> u64 {
        if self.free_head != FREE_SENTINEL {
            let idx = self.free_head as usize;
            let new_generation = match self.slots[idx] {
                Slot::Free { next_free, last_generation } => {
                    self.free_head = next_free;
                    // Wrap from max back to 1 rather than 0 — a zero
                    // generation combined with slot 0 would give the 0
                    // handle, which some callers treat as "no handle".
                    let next = last_generation.wrapping_add(1);
                    if next == 0 { 1 } else { next }
                }
                Slot::Occupied { .. } => unreachable!("free_head pointed at occupied slot"),
            };
            self.slots[idx] = Slot::Occupied {
                generation: new_generation,
                entry,
            };
            encode_handle(idx as u32, new_generation)
        } else {
            let idx = self.slots.len();
            assert!(idx < (u32::MAX - 1) as usize, "socket registry slot overflow");
            self.slots.push(Slot::Occupied {
                generation: 1,
                entry,
            });
            encode_handle(idx as u32, 1)
        }
    }

    fn get(&self, handle: u64) -> Option<&Arc<SocketEntry>> {
        let (idx, gen) = decode_handle(handle);
        let slot = self.slots.get(idx as usize)?;
        match slot {
            Slot::Occupied { generation, entry } if *generation == gen => Some(entry),
            _ => None,
        }
    }

    fn remove(&mut self, handle: u64) -> Option<Arc<SocketEntry>> {
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
                    Slot::Free {
                        next_free: prev_free_head,
                        last_generation: gen,
                    },
                );
                self.free_head = idx;
                match old {
                    Slot::Occupied { entry, .. } => Some(entry),
                    Slot::Free { .. } => unreachable!(),
                }
            }
            _ => None,
        }
    }

    /// Iterate over all occupied slots. Yields `(handle, &entry)`.
    fn iter(&self) -> impl Iterator<Item = (u64, &Arc<SocketEntry>)> {
        self.slots.iter().enumerate().filter_map(|(idx, slot)| match slot {
            Slot::Occupied { generation, entry } => {
                Some((encode_handle(idx as u32, *generation), entry))
            }
            Slot::Free { .. } => None,
        })
    }
}

#[inline]
fn encode_handle(idx: u32, generation: u32) -> u64 {
    ((generation as u64) << 32) | (idx as u64)
}

#[inline]
fn decode_handle(handle: u64) -> (u32, u32) {
    let idx = handle as u32;
    let generation = (handle >> 32) as u32;
    (idx, generation)
}

/// The global socket table. Writes take an exclusive lock; reads take
/// a shared lock and then do a direct Vec index — no hashing, no probe.
static REGISTRY: RwLock<Registry> = RwLock::new(Registry::new());

/// Get an `Arc<SocketEntry>` for a handle. This is the primary lookup
/// on the hot path — every worker `recv`/`send` and every reactor event
/// dispatch goes through it. Cheap: one RwLock read-acquire, one Vec
/// index, one generation compare, one Arc clone.
pub(crate) fn get_entry(handle: u64) -> Option<Arc<SocketEntry>> {
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
    let live = REGISTRY.write().insert(entry.clone());
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
    let mut handle = registry.insert(entry.clone());
    let start = std::time::Instant::now();
    for _ in 0..200_000 {
        let removed = registry.remove(std::hint::black_box(handle)).unwrap();
        handle = registry.insert(removed);
    }
    println!("reuse {}", start.elapsed().as_micros());
    let mut fresh = Registry::new();
    let start = std::time::Instant::now();
    for _ in 0..50_000 {
        std::hint::black_box(fresh.insert(entry.clone()));
    }
    println!("fresh {}", start.elapsed().as_micros());
}
