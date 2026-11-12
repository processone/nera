# Production Base64 encoding

Integration base: `0a7e907c44778abdbe0558386be514c61e3e800f` (`main`).
The first commit replays `0a7de20f9a930d3d3eddfeb3743bb2b8a1acd447`
from #61. That PR had targeted `mr/binary-match-corrections`, so merging it
after #60 did not put the prototype on main. This branch targets main directly.

## Runtime contract

`base64:encode/2` keeps its Erlang option and flat-byte-list validation, then
calls `base64:encode_binary/3`. The Rust core reserves the checked output size
with `try_reserve_exact` and processes at most 49,152 input bytes per native
call/resumption (a multiple of three). Large inputs yield between blocks.
Reservation failure or an unrepresentable result size raises `system_limit`.
The allocator/runtime can still abort on other allocation failures, as before.

The unfinished output is an unpublished `Vec<u8>` owned by the process context.
A native continuation roots the input in its argument list. Full GC can move
that descriptor; each resumption obtains its pointer again, and no borrowed
pointer survives suspension. Process destruction drops the unfinished buffer.
On completion, results larger than 64 bytes adopt the Vec through BinaryData;
there is no full-size copy or concatenation during finalization. The context's
new field is last, preserving existing generated-code field offsets.

This change does not introduce a dependency or change decoding. Existing list
validation, option defaults, results and exception class/reason are retained;
stacktrace frame identity is not part of the differential comparison.

## Regression evidence

The first native adapter used the synchronous prototype. The regression
`large_encode_yields_and_resumes_without_losing_bytes` failed at its intended
assertion: actual status 0 (`STATUS_OK`), expected 1 (`STATUS_YIELD`). It passes
with the resumable implementation. Additional tests exercise full GC between
resumptions with the continuation as the input's only root, pointer identity
of the adopted output buffer, process destruction while suspended, and size
overflow/header-domain rejection.

`tests/base64_large_encode.erl` covers both alphabets, both padding options,
all three tail lengths, the block boundary, subbinaries, inputs over 2 MB,
direct/apply/fun calls, protected calls, caller roots and process messaging.
It runs unchanged on BEAM. `scripts/test-base64.py` also runs the existing
options/error tests on BEAM and compares production against the frozen Erlang
implementation using the prototype's extensive differential corpus.

## Reproduction

```sh
RUSTC_WRAPPER='' cargo build --release --locked -p nera-runtime
RUSTC_WRAPPER='' cargo build --release --locked
RUSTC_WRAPPER='' cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTC_WRAPPER='' cargo test --workspace --locked --no-fail-fast
./target/release/nera test -j 1 --timeout 30
python3 scripts/test-base64.py --nera "$PWD/target/release/nera"
RUSTC_WRAPPER='' ./scripts/dist.sh --version base64-production
python3 scripts/test-dist.py dist/nera-base64-production-aarch64-apple-darwin.tar.gz
bash scripts/test-cross.sh "$PWD/target/release/nera" x86_64-apple-darwin
bash scripts/test-cross.sh "$PWD/target/release/nera" aarch64-unknown-linux-gnu
python3 benchmarks/base64_prototype/run.py --output target/base64-prototype-validation
```

Validated on macOS ARM64, Rust 1.99.0, OTP 29. Recorded source and executable
hashes are in `validation.json`; logs are alongside this document. Workspace
Rust tests: 1,858 passed, one existing ignored release-only cancellation scaling
test. Erlang tests: 324 passed, two existing OneCached tests skipped because
external modules are unavailable. Clippy, the differential comparison and both
installed runtime lookup paths pass. Cross-target object generation passes for
macOS x86_64 and Linux ARM64; execution on Linux/Windows and full cross-linked
runtime archives were not tested.

The historical prototype harness was also rerun successfully, including timing
and allocation executables, differential assertions and runtime Clippy. Its
sandbox was captured before the final BIF documentation/test-literal edits;
that run validates the restored harness, not the final production tree. It ran
alongside other checks, so its timings are not a new performance baseline.
Historical published results remain unchanged. The harness now freezes its
Erlang baseline rather than accidentally benchmarking production Rust against
the prototype Rust encoder.

GitHub CI remains intentionally paused. Independent review and explicit merge
approval are still required by CONTRIBUTING.md.
