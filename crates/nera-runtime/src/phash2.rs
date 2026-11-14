// SPDX-License-Identifier: Apache-2.0
// Copyright Ericsson AB 2022-2026. All Rights Reserved.
// Atom-name hashing: Copyright Ericsson AB 1996-2025. All Rights Reserved.
// Adapted to Nera's term layout and explicit work stack from Erlang/OTP 29.0:
// https://github.com/erlang/otp/blob/OTP-29.0/erts/emulator/beam/erl_term_hashing.c
// https://github.com/erlang/otp/blob/OTP-29.0/erts/emulator/beam/atom.c
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy at http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed
// under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
// CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

//! Portable term hashing, following OTP 29's make_hash2.
//! The work stack holds terms and map continuations, never recursive hashes.

use crate::atom_table::global_atom_table;
use crate::term::*;

const H: u32 = 0x9e3779b9;

// Bob Jenkins' MIX, as used by ERTS (not lookup3's newer finalizer).
fn mix(mut a: u32, mut b: u32, mut c: u32) -> u32 {
    mix_words(&mut a, &mut b, &mut c);
    c
}

fn uint_hash(hash: u32, x: u32, y: u32, constant: u32) -> u32 {
    mix(constant.wrapping_add(x), constant.wrapping_add(y), hash)
}

fn hc(n: u32) -> u32 {
    H.wrapping_mul(n)
}

fn block_hash(bytes: &[u8], initial: u32) -> u32 {
    let (mut a, mut b, mut c) = (H, H, initial);
    let mut chunks = bytes.chunks_exact(12);
    for chunk in &mut chunks {
        a = a.wrapping_add(u32::from_le_bytes(chunk[..4].try_into().unwrap()));
        b = b.wrapping_add(u32::from_le_bytes(chunk[4..8].try_into().unwrap()));
        c = c.wrapping_add(u32::from_le_bytes(chunk[8..].try_into().unwrap()));
        // MIX updates all three words; subsequent blocks need a and b too.
        mix_words(&mut a, &mut b, &mut c);
    }
    c = c.wrapping_add(bytes.len() as u32);
    for (i, &byte) in chunks.remainder().iter().enumerate() {
        match i {
            0..=3 => a = a.wrapping_add((byte as u32) << (8 * i)),
            4..=7 => b = b.wrapping_add((byte as u32) << (8 * (i - 4))),
            _ => c = c.wrapping_add((byte as u32) << (8 * (i - 8 + 1))),
        }
    }
    mix(a, b, c)
}

fn mix_words(a: &mut u32, b: &mut u32, c: &mut u32) {
    *a = a.wrapping_sub(*b).wrapping_sub(*c) ^ (*c >> 13);
    *b = b.wrapping_sub(*c).wrapping_sub(*a) ^ (*a << 8);
    *c = c.wrapping_sub(*a).wrapping_sub(*b) ^ (*b >> 13);
    *a = a.wrapping_sub(*b).wrapping_sub(*c) ^ (*c >> 12);
    *b = b.wrapping_sub(*c).wrapping_sub(*a) ^ (*a << 16);
    *c = c.wrapping_sub(*a).wrapping_sub(*b) ^ (*b >> 5);
    *a = a.wrapping_sub(*b).wrapping_sub(*c) ^ (*c >> 3);
    *b = b.wrapping_sub(*c).wrapping_sub(*a) ^ (*a << 10);
    *c = c.wrapping_sub(*a).wrapping_sub(*b) ^ (*b >> 15);
}

fn atom_hash(name: &str) -> u32 {
    let mut bytes = name.bytes().peekable();
    let mut hash = 0u32;
    while let Some(mut byte) = bytes.next() {
        // ERTS preserves its pre-UTF-8 hash for Latin-1 characters.
        if byte & 0xfe == 0xc2 && bytes.peek().is_some_and(|b| b & 0xc0 == 0x80) {
            byte = (byte << 6) | (bytes.next().unwrap() & 0x3f);
        }
        hash = (hash << 4).wrapping_add(byte as u32);
        let high = hash & 0xf0000000;
        hash ^= (high >> 24) ^ high;
    }
    hash
}

enum Work {
    Term(ErlTerm),
    MapPair,
    MapTail { hash: u32, pairs: u32 },
}

/// Hash a live, immutable term graph without native-stack recursion.
///
/// # Safety
/// Every reachable object and binary backing must remain valid and unmoved
/// throughout the call. No allocation on the process heap or moving GC occurs.
pub(crate) unsafe fn hash(term: ErlTerm) -> u32 {
    let mut pending = vec![Work::Term(term)];
    let mut hash = 0u32;
    let mut pairs = 0u32;
    while let Some(work) = pending.pop() {
        let term = match work {
            Work::Term(term) => term,
            Work::MapPair => {
                pairs ^= hash;
                hash = 0;
                continue;
            }
            Work::MapTail {
                hash: outer,
                pairs: outer_pairs,
            } => {
                hash = uint_hash(outer, pairs, 0, hc(19));
                pairs = outer_pairs;
                continue;
            }
        };
        if let Some(n) = term.as_small_int() {
            if (-134217728..134217728).contains(&n) {
                if n < 0 {
                    hash = uint_hash(hash, n.unsigned_abs() as u32, 0, H);
                }
                hash = uint_hash(hash, n as u32, 0, H);
            } else {
                let magnitude = n.unsigned_abs();
                hash = uint_hash(
                    hash,
                    magnitude as u32,
                    (magnitude >> 32) as u32,
                    hc(if n < 0 { 10 } else { 11 }),
                );
            }
        } else if let Some(id) = term.as_atom_id() {
            let value = global_atom_table()
                .resolve(id)
                .map_or(0, |name| atom_hash(&name));
            hash = if hash == 0 {
                value
            } else {
                uint_hash(hash, value, 0, hc(3))
            };
        } else if term.is_nil() {
            hash = if hash == 0 {
                3468870702
            } else {
                uint_hash(hash, 2, 0, hc(2))
            };
        } else if term.is_cons() {
            let mut cursor = term;
            let (mut packed, mut count) = (0u32, 0);
            while let Some(ptr) = cursor.as_cons_ptr() {
                let Some(byte) = (*ptr).head.as_small_int().filter(|n| (0..=255).contains(n))
                else {
                    break;
                };
                packed = (packed << 8) + byte as u32;
                count += 1;
                if count == 4 {
                    hash = uint_hash(hash, packed, 0, hc(4));
                    packed = 0;
                    count = 0;
                }
                cursor = (*ptr).tail;
            }
            if count != 0 {
                hash = uint_hash(hash, packed, 0, hc(4));
            }
            if let Some(ptr) = cursor.as_cons_ptr() {
                pending.push(Work::Term((*ptr).tail));
                pending.push(Work::Term((*ptr).head));
            } else {
                pending.push(Work::Term(cursor));
            }
        } else if let Some(pid) = term.as_pid_id() {
            hash = uint_hash(hash, pid.get() as u32, 0, hc(5));
        } else if let Some(ptr) = term.as_boxed_ptr() {
            let header = BoxedHeader(*ptr);
            match header.subtype() {
                BOXED_TUPLE => {
                    hash = uint_hash(hash, header.arity() as u32, 0, hc(9));
                    for i in (1..=header.arity() as usize).rev() {
                        pending.push(Work::Term(ErlTerm(*ptr.add(i))));
                    }
                }
                BOXED_MAP => {
                    let size = header.arity() as usize;
                    hash = uint_hash(hash, size as u32, 0, hc(16));
                    if size != 0 {
                        pending.push(Work::MapTail { hash, pairs });
                        hash = 0;
                        pairs = 0;
                        for i in (0..size).rev() {
                            pending.push(Work::MapPair);
                            pending.push(Work::Term(ErlTerm(*ptr.add(2 + i * 2))));
                            pending.push(Work::Term(ErlTerm(*ptr.add(1 + i * 2))));
                        }
                    }
                }
                BOXED_BINARY | BOXED_REFC_BINARY => {
                    let bytes = term.binary_bytes_any().unwrap();
                    let bits = header.arity() as usize;
                    // ERTS truncates binary byte lengths to 32 bits for
                    // compatibility with its historical portable hashes.
                    let full = (bits / 8) as u32 as usize;
                    let tail = bits % 8;
                    let initial = hc(13).wrapping_add(hash);
                    hash = if bits == 0 {
                        initial
                    } else {
                        block_hash(&bytes[..full], initial)
                    };
                    if tail != 0 {
                        hash = uint_hash(
                            hash,
                            tail as u32,
                            (bytes[full] >> (8 - tail)) as u32,
                            hc(15),
                        );
                    }
                }
                BOXED_BIGINT => {
                    let constant = hc(if *ptr.add(1) != 0 { 10 } else { 11 });
                    for i in 0..header.arity() as usize {
                        let limb = *ptr.add(2 + i);
                        hash = uint_hash(hash, limb as u32, (limb >> 32) as u32, constant);
                    }
                }
                BOXED_FLOAT => {
                    let mut bits = *ptr.add(1);
                    if f64::from_bits(bits) == 0.0 {
                        bits = 0;
                    }
                    hash = uint_hash(hash, (bits >> 32) as u32, bits as u32, hc(12));
                }
                BOXED_REFERENCE => {
                    hash = uint_hash(hash, *ptr.add(REFERENCE_ID_WORD_INDEX) as u32, 0, hc(7));
                }
                // Native closures do not carry BEAM module/index/old_uniq
                // metadata. Retain an identity-based hash for these terms.
                BOXED_CLOSURE => hash = uint_hash(hash, *ptr.add(1) as u32, 0, H),
                _ => hash = uint_hash(hash, term.0 as u32, 0, H),
            }
        }
    }
    hash
}
