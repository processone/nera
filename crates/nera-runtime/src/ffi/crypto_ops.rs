//! crypto:hash/2 and crypto:strong_rand_bytes/1, with OTP 29's results.
use super::helpers::{flatten_iolist, raise_badarg, with_ctx_heap};
use crate::atom_table::global_atom_table;
use crate::process::ProcessContext;
use crate::term::ErlTerm;
use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha224, Sha256, Sha384, Sha512};

/// Digest of data for an OTP digest type name, or None for unsupported types.
fn digest(kind: &str, data: &[u8]) -> Option<Vec<u8>> {
    Some(match kind {
        "md5" => Md5::digest(data).to_vec(),
        "sha" => Sha1::digest(data).to_vec(),
        "sha224" => Sha224::digest(data).to_vec(),
        "sha256" => Sha256::digest(data).to_vec(),
        "sha384" => Sha384::digest(data).to_vec(),
        "sha512" => Sha512::digest(data).to_vec(),
        _ => return None,
    })
}

/// crypto:hash/2 — digest of an iodata for md5, sha, sha224, sha256, sha384
/// and sha512. Other types, and data that is not iodata, raise badarg.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_crypto_hash(
    ctx: *mut ProcessContext,
    type_term: u64,
    data_term: u64,
) -> u64 {
    let Some(kind) = ErlTerm(type_term)
        .as_atom_id()
        .and_then(|id| global_atom_table().resolve(id))
    else {
        return raise_badarg(ctx);
    };
    let mut buf = Vec::new();
    if !unsafe { flatten_iolist(ErlTerm(data_term), &mut buf) } {
        return raise_badarg(ctx);
    }
    match digest(&kind, &buf) {
        Some(out) => with_ctx_heap(ctx, |heap| heap.alloc_binary_auto(&out).0),
        None => raise_badarg(ctx),
    }
}

/// crypto:strong_rand_bytes/1 — N bytes from the operating system's
/// cryptographically secure generator. N must be a non-negative integer.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_crypto_strong_rand_bytes(
    ctx: *mut ProcessContext,
    n_term: u64,
) -> u64 {
    let n = match ErlTerm(n_term).as_small_int() {
        Some(n) if n >= 0 => n as usize,
        _ => return raise_badarg(ctx),
    };
    let mut buf = Vec::new();
    if buf.try_reserve_exact(n).is_err() {
        return raise_badarg(ctx);
    }
    buf.resize(n, 0);
    if getrandom::getrandom(&mut buf).is_err() {
        return raise_badarg(ctx);
    }
    with_ctx_heap(ctx, |heap| heap.alloc_binary_auto(&buf).0)
}

#[cfg(test)]
mod tests {
    use super::digest;

    // Published test vectors for "abc" (FIPS 180, RFC 1321).
    #[test]
    fn digests_match_published_vectors() {
        let hex = |k: &str| {
            digest(k, b"abc").unwrap().iter().map(|b| format!("{b:02x}")).collect::<String>()
        };
        assert_eq!(hex("md5"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(hex("sha"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert!(hex("sha256").starts_with("ba7816bf8f01cfea"));
        assert!(digest("sha3_256", b"").is_none());
    }
}
