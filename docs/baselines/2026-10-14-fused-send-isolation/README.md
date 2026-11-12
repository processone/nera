# Fused tuple send isolation — 2026-10-14

Base: main `20afa67b6d9454f7d703fa82bb47d35de55cbae3` (fetched before the
branch was created). Runtime and fixture fingerprints are in
[source-manifest.json](source-manifest.json). Validation: macOS arm64,
Rust 1.99.0 with locked dependencies, BEAM OTP 29 / ERTS 17.0.5.

## Scope

The direct receiver path of `nera_rt_send_tuple` previously allocated a tuple
but retained complex child pointers into the sender heap. Sender GC or exit
could invalidate the receiver's graph. The fix uses one existing deep copier
to create the receiver graph, preserving aliases and owning binary descriptors.
An aligned, temporary stack tuple supplies the copy root while the source is
live. No stack or sender pointer is retained by the copied graph.

The helper is moved to module scope for the ownership regression. Only direct
cross-process calls request complex copying. Its self-send behavior, simple immediate/refcounted-binary
fast paths remain unchanged. The fragment helper is byte-for-
byte identical to main. Heap, GC, scheduler and message implementations are
unchanged. None of the abandoned allocation optimizations is included.

## Regression proof and compatibility

`direct_tuple_copy_does_not_borrow_sender_heap` fails on the original helper at
its pointer-inequality assertion, **before** dropping sender storage or reading
an invalid pointer ([RED log](red.log.gz)). After the fix it checks independent
list and binary descriptor addresses, shared children, sender destruction,
receiver full collection and all received binary bytes. A second Rust test
checks self-send children and the returned tuple.

`tests/message_tuple_isolation.erl` checks messages read after sender exit:
map, full list contents, large binary bytes, big integer, repeated values,
return value, self-send and invalid-destination errors. Its producer is a named
function so the optimized IR actually contains the four-element `send_direct`
([IR excerpt](fused-send.ir.txt)); an unoptimized closure alone would exercise
the generic path instead. No compiler pass was changed.

BEAM with one scheduler and Nera with one and four schedulers produce the exact
same complete stdout: `message_tuple_isolation ok`. Logs are preserved here.
These focused cases establish the tested behavior, not universal BEAM parity.

## Reproduction

Build the release runtime first, then the compiler, using the same target dir:

```sh
cargo build --release --locked -p nera-runtime
cargo build --release --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked --no-fail-fast
NERA_SCHEDULERS=1 ./target/release/nera test -j 1 --timeout 30
```

For the direct BEAM comparison, copy the Erlang fixture to an isolated directory,
compile it with `erlc` and with the just-built Nera (`-e
message_tuple_isolation:test`). Keep the matching release runtime archive next
to the Nera executable when copying it. Run BEAM with `+S 1:1`, then the native
executable with `NERA_SCHEDULERS=1` and `4`; compare stdout byte-for-byte. Emit
Nera IR with `--emit-ir` in that same isolated directory to verify fusion.

CI/Release workflows remain manually paused by repository policy. No workflow
was enabled or triggered. Linux/Windows execution and installed-archive checks
were not run for this platform-independent copy correction. Independent review
and explicit merge approval remain required before merging.

## Final results

- Release runtime and compiler: pass.
- Clippy workspace / all targets with `-D warnings`: pass.
- Rust workspace: **1,912 passed, 0 failed, 1 ignored**.
- Native suite, one scheduler: **333 passed, 0 failed, 2 skipped / 335 total**.
  The skips require external OneCached dependencies.
- BEAM/native fixture comparison: exact stdout match with one and four Nera
  schedulers; the named producer is confirmed to compile to fused tuple send.
- Source fingerprints, unchanged GC/fragment scope, and `git diff --check`: pass.

Compressed logs in this directory bind the results to the recorded sources.
This is a correctness fix; no allocation or latency improvement is claimed.
