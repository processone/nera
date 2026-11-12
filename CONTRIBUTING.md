# Contributing to nera

Thank you for your interest in contributing to nera! This document provides
guidelines for contributing to the project.

## Getting Started

### Prerequisites

- **Rust 1.99.0**, selected by `rust-toolchain.toml` and pinned in CI.
  Keep the root `Cargo.lock` under version control; update dependencies separately
  from toolchain changes and rerun the [baseline](docs/PROJECT_STATUS.md).
- **A C linker** — `cc` on PATH (Xcode CLI Tools on macOS, `gcc`/`clang` on Linux)
- **Git** — for cloning and version control

### Building

```bash
git clone https://github.com/processone/nera.git
cd nera
cargo build --release --locked
```

Working copy and target paths may contain spaces; no `CARGO_TARGET_DIR`
workaround is required. On non-MSVC targets, the allocator dependencies are
pinned together to TiKV's jemalloc 5.4.0 update, commit
`9d6e381a16267eb2ec63e96dc4f33d843f92eb62`. The published 0.6/0.7 crates still
bundle older jemalloc versions whose configure step rejects these paths.
The Git pin includes jemalloc's upstream whitespace fix
[`bb0a6aca`](https://github.com/jemalloc/jemalloc/commit/bb0a6aca10ce15b192b7b4453c2096249748c363);
Cargo fetches the source and its pinned submodule into its normal cache.
Do not edit that cache or substitute a system allocator to work around builds.

When changing build dependencies or paths, run the fresh-build regression:

```bash
python3 scripts/test-build-path.py
# Optional: --temp-parent /external/scratch to choose disposable storage.
```

It copies tracked working files into a fresh directory containing spaces,
runs `cargo build --release --locked` with the normal target directory, and
compiles/runs a native Erlang program. It removes the disposable copy afterward.
This slow integration check is not part of the Rust unit suite.

#### Allocator startup profile

Non-Windows builds use jemalloc 5.4 with `tcache_max:4096,narenas:2`.
macOS additionally sets `retain:false` for its measured memory profile.
Elsewhere Nera leaves `retain` at jemalloc's platform default. On 64-bit Linux,
that default is enabled: the [jemalloc 5.4 manual](https://github.com/jemalloc/jemalloc/blob/7a34f18502e7b222724097cdcd499b437d189acc/doc/jemalloc.xml.in#L1032-L1058)
explains that discarding mappings can cause semi-permanent holes in the virtual
memory map and related performance problems. Do not apply the macOS retain
setting to Linux defaults.

Nera exports `_rjem_malloc_conf` with the C pointer ABI expected by the prefixed
allocator; native linking retains it on macOS and Linux. The former unprefixed
`malloc_conf` was not read. The strong Nera override is non-Windows only:
Windows jemalloc owns a non-weak configuration symbol. Dirty/muzzy decay stays
at jemalloc's defaults (10000/0 ms). Nera does not enable background threads,
which this Mach-O build cannot use.

The profile was measured on macOS arm64: it reduces allocator memory at the
end of the nine project benchmark workloads, but peak RSS and throughput
remain workload-dependent. Two automatic arenas may increase contention;
`retain:false` can trade mapping/fork behavior for less reserved virtual memory.
This is not a universal performance guarantee or the fixed 5.3 cache policy.

Override the defaults before starting a generated program (or Nera itself):

```bash
_RJEM_MALLOC_CONF='retain:true,tcache_max:8192,narenas:3' ./my_program
```

`MALLOC_CONF` without the `_RJEM_` prefix does not configure this allocator.
To reject invalid settings instead of only printing warnings, append
`abort_conf:true`. To inspect effective startup options, append
`stats_print:true,stats_print_opts:J` and read the process's JSON `opt` section.
Some legacy 5.3 cache-slot/divisor settings are silently ignored in 5.4; use
supported `tcache_max` / `tcache_ncached_max` settings instead.

After changing the allocator, its configuration or native linking, build the
runtime before the compiler so the embedded library is current, then run:

```bash
cargo build --release --locked -p nera-runtime
cargo build --release --locked -p nera
python3 scripts/test-allocator-config.py
```

The regression compiles a native Nera program and reads its actual allocator
options, checks the defaults and `_RJEM_MALLOC_CONF` override, and enables
`abort_conf:true` to catch rejected options. It expects retain=false on macOS
and jemalloc's enabled retain default on 64-bit Linux. The compiled profile
test skips Windows; MSVC uses the system allocator, while Windows GNU keeps
jemalloc's own configuration symbol and defaults.
Complete Linux x86_64 and aarch64-linux-gnu compiler/distribution builds and
execution remain unverified; cross-building runtime libraries does not prove
native startup or end-to-end behavior.

### Running Tests

```bash
# Workspace tests (continue after a failed target for a complete result)
cargo test --workspace --locked --no-fail-fast

# End-to-end tests
cargo build --release --locked
./target/release/nera test
```

The [current baseline](docs/PROJECT_STATUS.md) records known failures; do not
infer a green workspace from passing one crate. Preserve explicit expected
output/completion checks when adding runtime regressions.

When changing embedded runtime extraction, also run the standalone cache
regression with two compilers built with different native runtime archives.
Build each runtime before its compiler and retain both matching artifacts:

```bash
python3 scripts/test-embedded-runtime.py \
  --compiler-a /path/to/nera-a --runtime-a /path/to/runtime-a.a \
  --compiler-b /path/to/nera-b --runtime-b /path/to/runtime-b.a
```

The regression copies the compilers away from adjacent runtime libraries,
compiles and runs outside the checkout, and compares the linked archive with
its compiler's original runtime. Both extraction orders, cache reuse and
concurrent cold extractions share one disposable temporary folder. On Unix,
archives live inside an owner-only per-user directory, not directly in the
shared temporary folder. For the same-day collision regression, build both
compilers with the same version and build date.

Also run the Unix shared-temp trust regression with those real artifacts:

```bash
python3 scripts/test-runtime-cache-security.py \
  --compiler /path/to/nera-a --runtime /path/to/runtime-a.a \
  --other-runtime /path/to/runtime-b.a
```

It plants a different native archive, unreadable files, directories and symlinks
at the old public archive name and the per-user cache name. Each standalone
build must still link its own runtime. It observes the archive's digest and
private permissions at the actual linker boundary and checks that disposable
fallback directories disappear after linking. With an unsafe non-sticky temp
root, it verifies private home-cache fallback and reuse, refusal to use a planted
temp archive, and disposable home extraction when the home cache is obstructed.
It also places a private home below group/world-writable non-sticky ancestors:
those paths must be refused before archive linkage, while protected or sticky
ancestry keeps working. The script supplies its own isolated `HOME`; it never
uses the account's cache.
These fixtures run as the current account; the Rust tests separately check
rejection of a mismatched owner ID. Also run the standalone and distribution
regressions under `umask 002`, since their fake temporary roots must not inherit
group-write permissions.
Native multi-account Linux testing remains separate coverage, not implied by
this single-account regression. Unix extraction also requires protected temporary
folder ancestry: system/current-user ownership, and sticky permissions wherever
other accounts can write. Normal shared `/tmp` is supported; an unsafe custom
`TMPDIR` is not used for extraction. Instead, Nera uses `$HOME/.nera-runtime`,
created with mode 0700 and checked for effective-user ownership. The configured
account home must be absolute, owned by that user and not group/other-writable.
Its ancestors must also protect entries from rename: no group/other writes
without a sticky bit, and ownership by the user or system root. In a Linux user
namespace, an ancestor reported as the kernel's overflow UID is also allowed.
Linux reports every unmapped host account with that UID, not only system root,
so the UID cannot distinguish those owners. This allowance never bypasses
write/sticky checks and is not used for the home, cache directory or archive
ownership. If no protected location is available,
extraction reports both locations and fails closed.
Obstructed home caches use a disposable private directory under that same home,
not the original unsafe temporary root. Windows retains its account-temp cache
location; symlink cache entries are treated as obstructions on every platform.
Also run `scripts/test-dist.py` on a native distribution to cover both installed
and embedded lookup.

The home fallback at `$HOME/.nera-runtime` retains one uncompressed archive,
normally about 30 MB per compiler build. Builds with identical embedded runtimes
share that archive. Nera does not prune these files. When no Nera build is
running, users can delete individual archives or the entire directory by hand;
the next fallback build recreates the archive it needs. No cleanup is required
before an ordinary build.

When adding or revalidating an external project integration, or changing a
known compatibility blocker, update [COMPATIBILITY.md](COMPATIBILITY.md) with
the observed result, pinned versions, platform, reproduction commands and
evidence. Follow its status definitions; a test harness alone does not prove
compatibility.

## Continuous integration

`.github/workflows/ci.yml` runs on pull requests, on every push to `main` and
on a manual start. It is split into small jobs that run only when the files a
pull request changes need them:

1. **Change map** lists the changed files (`git diff --name-only HEAD^1 HEAD`
   on the pull request's merge commit) and runs `scripts/ci/changes.py`, which
   holds the rules mapping files to jobs. `scripts/ci/test_changes.py` tests
   them against real changed-file lists.
2. **Build compiler** builds `target/release/nera` and the runtime once per
   system and shares them with the test jobs as one artifact.
3. **Rust checks** (clippy, `cargo test`), **End-to-end** (three parts per
   system, `nera test --shard K/3`, each with `-j 1`), **Cowlib**, **Thoas**,
   **Poolboy** (allowed to fail), **Integration (macOS)** and **Cross-compile**
   use that compiler. **VS Code plugin**, **JetBrains plugin** and the
   **Packaging trial** (Linux x86_64, publishes nothing) build on their own.
4. **CI result** always runs last and fails if any started job failed. Jobs the
   change map skips count as passed.

Documentation, examples and benchmarks start no job beyond the change map and
CI result. A file that no rule names, and any change to `ci.yml`, the change
map or its tests, starts every job; so does every push to `main` and every
manual start. When you add a new kind of file or job, add its rule to
`scripts/ci/changes.py` and a case to its test.

Pull requests restore the Cargo caches saved by runs on `main` and save none.
A newer push to a pull request cancels its older run.

To reproduce the Rust and end-to-end checks locally with the pinned Rust
toolchain and dependency lock:

```bash
cargo build --release --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked --no-fail-fast
python3 scripts/test-allocator-config.py
./target/release/nera test -j 1 --timeout 30
```

For runtime shutdown changes, build the runtime before the compiler (so native
linking and the embedded library both use the proposed runtime), then run:

```bash
cargo build --release --locked -p nera-runtime
cargo build --release --locked -p nera
python3 scripts/test-native-shutdown.py --out /path/to/shutdown-results.json
```

This opt-in performance regression runs five real native programs 30 times each
with the default scheduler count and with `NERA_SCHEDULERS=1`. It checks complete
stdout and normal exit, and measures last stdout to exit, excluding startup and
compilation. The packet demo's maps may list their keys in any order, since Nera
orders atom keys by its own atom table; every key, value and line must match. Each median must be at most 5 ms; p95, maximum and individual samples
are recorded. This is not a maximum-latency guarantee or a wall-clock unit gate.
Run it without concurrent builds; investigate significant outliers rather than
attributing them to load without evidence. Linux/Windows require native execution;
cross-compilation alone does not validate their shutdown latency.

For receive-timer or timer-thread changes, build the runtime before the compiler
the same way, then run:

```bash
python3 scripts/test-native-timer.py --out /path/to/timer-results.json
```

This opt-in performance regression runs three real native programs 30 times each
with the default scheduler count and with `NERA_SCHEDULERS=1`: `receive after 37`
in the main process, the same in a spawned process, and five `receive after 10` in
a row. Each prints a line just before it starts waiting; the probe measures from
that line to the timeout's output and subtracts the requested wait. On macOS every
case also runs at utility and background priority (through `taskpolicy`), where
the kernel otherwise postpones ordinary timed waits by up to 75 ms and 100 ms.
Each case's median lateness must be at most 5 ms, and no sample may resume more
than 2 ms early; p95, maximum and individual samples are recorded. Like the
shutdown probe, it is not a maximum-latency guarantee: run it without concurrent
builds. On a busy Mac, background-priority threads can wait tens of milliseconds
for a processor, which shows in the tail. Linux/Windows run only the
normal-priority cases and require native execution.

Also run the relevant integration, cross-compilation and installed-archive
checks for the areas/platforms changed (see the preserved workflows,
[tests/README.md](tests/README.md), integration READMEs and
`scripts/test-cross.sh`). Record commands, results, known failures and missing
platform coverage. A macOS-only result does not establish Linux/Windows or
cross-target coverage. An absent or skipped check is not successful validation.
Do not waive an existing failing or pending check.

Require an independent review and a separate, explicit merge approval.
Protections and permissions are not changed by this pause. If required checks
are introduced and the pause prevents merging, stop for a specific decision;
do not remove requirements or bypass them.

For the manual reactivation and verification procedure once Nera is public,
see [Release Process](docs/RELEASE.md#resuming-workflows-after-going-public).

## How to Contribute

### Reporting Bugs

Open an issue on GitHub with:

- A clear description of the problem
- A minimal Erlang program that reproduces it (if applicable)
- Expected vs actual behavior
- nera version (`nera --version`) and OS/architecture

### Suggesting Features

Open an issue with the "enhancement" label. Describe the use case and how
you'd expect the feature to work. For language features, reference the
relevant section of the Erlang/OTP documentation.

### Submitting Pull Requests

Do not include AI or LLM co-author references, AI `Co-Authored-By` trailers, or generated-by attribution in commits or pull request descriptions (for example, “Generated with …” lines).

1. Fork the repository and create a branch from `main`
2. Make your changes
3. Run and record the local validation above, including any known failures or missing coverage
4. Add tests for new functionality
5. Open a pull request with a clear description of the changes

### What Makes a Good Pull Request

- **Focused**: one logical change per PR
- **Tested**: includes unit tests for new code, and an end-to-end test
  (in `tests/`) if adding a user-visible feature
- **Documented**: update `FEATURES.md` or `CHANGELOG.md` if the change
  affects user-facing behavior
- **Clean**: no unrelated formatting changes or refactors mixed in

## Code Organization

```
crates/
  nera-syntax/    Lexer, preprocessor, parser, AST
  nera-ir/        AST-to-IR lowering, pattern compilation
  nera-codegen/   Cranelift code generation, linker
  nera-runtime/   Runtime: terms, processes, scheduler, GC, BIFs
  nera/           CLI driver, dependency management
erlang/
  stdlib/         Standard library (compiled Erlang modules)
tests/            End-to-end test programs (.erl)
```

## Code Style

- Follow standard Rust conventions (`cargo fmt`, `cargo clippy`)
- Keep functions focused and reasonably sized
- Prefer clarity over cleverness
- Add comments only where the logic is not self-evident
- Match existing patterns in the codebase

## End-to-End Tests

Each test is a `.erl` file in `tests/` that exports `test/0` returning `ok`:

```erlang
-module(my_test).
-export([test/0]).

test() ->
    42 = 6 * 7,
    ok.
```

Run a specific test: `./target/release/nera test my_test`

## License

By contributing to nera, you agree that your contributions will be licensed
under the [Apache License 2.0](LICENSE).
