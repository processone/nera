# Windows Cross-Compilation Roadmap

Status: **Planning**
Last updated: 2026-02-24

## Goal

Enable `nera` running on macOS or Linux to produce Windows executables via
`--target x86_64-pc-windows-gnu`, using the MinGW-w64 toolchain. MSVC-target
support (`x86_64-pc-windows-msvc`) is a stretch goal.

## Current State

Several pieces are already in place:

| Area                    | Status | Notes |
|-------------------------|--------|-------|
| `--target` / `--linker` CLI flags | Done | Commit `bbac446` |
| Cranelift COFF object emission    | Done | `ObjectModule` auto-selects format from triple |
| `c_call_conv()` → `WindowsFastcall` | Done | `compiler.rs:36-41` |
| Windows linker libs (`-lws2_32 -lbcrypt -luserenv`) | Done | `linker.rs:103-107` |
| `.exe` extension auto-append      | Done | `main.rs:285-291` |
| Cross-target ISA lookup           | Done | `compiler.rs:93-98` |
| Runtime portability (std threads, std::net, parking_lot, crossbeam) | Done | No Unix-specific APIs |

## Roadmap

### Phase 1: Linker Improvements (Small)

**Files:** `crates/nera-codegen/src/linker.rs`

#### 1.1 Smart default linker for Windows-GNU targets

`resolve_linker()` currently falls back to `cc`, which will not produce a
Windows binary. When no explicit `--linker` is provided and no `$CC` is set,
auto-detect the correct cross-linker based on the target triple.

```
Precedence:
  1. Explicit --linker override
  2. $CC environment variable
  3. Target-aware default:
     - x86_64-pc-windows-gnu   → x86_64-w64-mingw32-gcc
     - i686-pc-windows-gnu     → i686-w64-mingw32-gcc
     - *-pc-windows-msvc       → lld-link  (phase 4)
     - everything else         → cc
```

Verify the resolved binary exists on `$PATH` and emit a clear error if missing,
e.g.: *"cross-linker 'x86_64-w64-mingw32-gcc' not found. Install mingw-w64 or
pass --linker explicitly."*

#### 1.2 Runtime library name for Windows

`find_runtime_lib_for_target()` only searches for `libnera_runtime.a`.
On Windows-GNU the Rust toolchain emits `libnera_runtime.a` (same name),
so this already works. Add a secondary search for `nera_runtime.lib` to
future-proof for MSVC targets.

Search order for a Windows target:
```
target/<triple>/release/libnera_runtime.a      (MinGW)
target/<triple>/release/nera_runtime.lib       (MSVC)
target/<triple>/debug/libnera_runtime.a
target/<triple>/debug/nera_runtime.lib
```

#### 1.3 Windows-specific linker flags

The current Windows branch passes `-lws2_32 -lbcrypt -luserenv`. These are
correct for MinGW-style linking. Additionally:

- Add `-static` to prefer static linking of MinGW runtime libs (avoids
  dependency on `libgcc_s_seh-1.dll` etc. at runtime).
- Add `-Wl,--gc-sections` for dead-code elimination (same as Linux).
- For MSVC-style linking (phase 4), the flag syntax differs entirely:
  `/OUT:path`, `ws2_32.lib`, etc.

#### 1.4 Object file extension

Object files are currently written as `module_name.o`. Windows convention is
`.obj` for MSVC, but MinGW accepts `.o`. No change needed for phase 1.

### Phase 2: Cross-Build the Runtime (Small)

**Files:** `scripts/dist.sh`, documentation

The runtime (`nera-runtime`) is a `staticlib` crate. Cross-building it for
MinGW requires:

```bash
# One-time setup
rustup target add x86_64-pc-windows-gnu
brew install mingw-w64              # macOS
# apt install gcc-mingw-w64-x86-64  # Ubuntu/Debian

# Build
cargo build --release -p nera-runtime --target x86_64-pc-windows-gnu
```

This produces `target/x86_64-pc-windows-gnu/release/libnera_runtime.a`.

#### 2.1 Update `dist.sh`

- Detect Windows targets and adjust artifact names (`nera.exe`,
  `libnera_runtime.a` or `nera_runtime.lib`).
- Use `x86_64-w64-mingw32-strip` instead of `strip` for Windows binaries.
- Produce a `.zip` instead of `.tar.gz` for Windows distributions.
- Layout:
  ```
  nera-<version>-x86_64-pc-windows-gnu/
    bin/nera.exe
    lib/libnera_runtime.a
    lib/nera/erlang/stdlib/*.erl
  ```

#### 2.2 End-to-end verification

Manual smoke test:

```bash
# Build runtime for Windows
cargo build --release -p nera-runtime --target x86_64-pc-windows-gnu

# Compile a test program
./target/release/nera tests/01_hello.erl -o hello \
    --target x86_64-pc-windows-gnu \
    --linker x86_64-w64-mingw32-gcc

# Verify it's a PE executable
file hello.exe
# hello.exe: PE32+ executable (console) x86-64, for MS Windows

# Run under Wine (optional)
wine hello.exe
```

### Phase 3: CI and Testing (Medium)

#### 3.1 GitHub Actions cross-compile job

Add a CI job that:

1. Installs `mingw-w64` and `x86_64-pc-windows-gnu` Rust target.
2. Builds the runtime: `cargo build -p nera-runtime --target x86_64-pc-windows-gnu --release`.
3. Compiles a subset of the end-to-end tests to `.exe`.
4. Verifies `file *.exe` reports PE format.
5. Optionally runs tests under Wine for functional verification.

#### 3.2 Test matrix

| Test category        | Cross-compile check | Wine run |
|----------------------|--------------------:|:--------:|
| Basic (hello, atoms) | File format only    | Yes      |
| Arithmetic / data    | File format only    | Yes      |
| Processes / spawn    | File format only    | Yes      |
| Networking (gen_tcp) | File format only    | No (Wine TCP is unreliable) |
| File I/O             | File format only    | Yes      |

#### 3.3 Windows-native CI job (stretch)

Add a `windows-latest` runner that builds nera natively on Windows and runs
the full test suite. This validates both the compiler and runtime on real
Windows without Wine approximation.

### Phase 4: MSVC Target Support (Stretch)

The MSVC target (`x86_64-pc-windows-msvc`) is harder because it needs the
Windows SDK and MSVC linker. This is primarily useful for native Windows
development.

#### 4.1 Linker changes for MSVC

MSVC `link.exe` (or `lld-link`) uses completely different syntax:

| GCC/MinGW            | MSVC `link.exe`       |
|-----------------------|-----------------------|
| `-o output`           | `/OUT:output.exe`     |
| `-Lpath`              | `/LIBPATH:path`       |
| `-lnera_runtime`      | `nera_runtime.lib`    |
| `-lws2_32`            | `ws2_32.lib`          |
| `-Wl,--gc-sections`   | `/OPT:REF`            |

The `link()` method needs a branch:
```rust
match (self.target.operating_system, self.target.environment) {
    (Windows, Msvc) => self.link_msvc(),
    _               => self.link_gcc(),
}
```

#### 4.2 Cross-MSVC from macOS/Linux

Requires either:
- **xwin** + **lld-link**: `cargo xwin build --target x86_64-pc-windows-msvc`
  downloads Windows SDK headers/libs. Experimental but functional.
- **Native Windows build**: simpler and more reliable.

This phase is low priority since MinGW produces working Windows executables.

### Phase 5: Polish (Small)

#### 5.1 Runtime library auto-build

When `find_runtime_lib_for_target()` fails and the workspace is detected,
offer to build it automatically:

```
Runtime library not found for x86_64-pc-windows-gnu.
Run: cargo build -p nera-runtime --target x86_64-pc-windows-gnu --release
```

Or add a `--auto-build-runtime` flag that does it transparently.

#### 5.2 Zig as a cross-linker alternative

`zig cc` is a drop-in C cross-compiler that bundles sysroots for many targets
including Windows. Document this as an alternative to MinGW:

```bash
nera test.erl -o test \
    --target x86_64-pc-windows-gnu \
    --linker "zig cc -target x86_64-windows-gnu"
```

#### 5.3 README and --help updates

- Add Windows cross-compilation examples to the README.
- Document `--target` and `--linker` in `--help` output.
- Add a troubleshooting section for common cross-compile failures.

## Non-Issues (Already Handled)

These areas were audited and require **no changes** for Windows:

| Area | Why it works |
|------|-------------|
| **Threading** | `std::thread`, `parking_lot`, `crossbeam-deque` are cross-platform |
| **Networking** | `std::net::{TcpListener, TcpStream}` abstracts over Winsock |
| **File I/O** | `std::fs::read`, `std::fs::write` work on Windows |
| **Time** | `std::time::SystemTime` works on Windows |
| **CPU detection** | `num_cpus` crate is cross-platform |
| **Heap / GC** | Pure Rust pointer arithmetic. ETS uses platform-specific mmap/VirtualAlloc (handled) |
| **Symbol naming** | Cranelift `ObjectModule` handles COFF symbol decoration |
| **Calling conventions** | `c_call_conv()` returns `WindowsFastcall` for Windows |
| **Object format** | Cranelift emits COFF automatically for Windows triples |
| **Atom table** | Pure Rust `HashMap`, no platform dependencies |
| **Closures / message passing** | Pure Rust, no platform-specific code |

## Dependencies Summary

| Crate           | Windows support |
|-----------------|:--------------:|
| `parking_lot`   | Yes            |
| `crossbeam-deque` | Yes          |
| `crossbeam-utils` | Yes          |
| `bumpalo`       | Yes            |
| `num_cpus`      | Yes            |
| `num-bigint`    | Yes            |
| `num-traits`    | Yes            |
| `num-integer`   | Yes            |
| `crossbeam-queue` | Yes            |
| `crossbeam-channel`| Yes           |
| `mio`             | Yes (net feature uses Winsock2) |
| `libc`            | Yes            |

All runtime dependencies are cross-platform. `libc` is used for ETS mmap on
Unix (with `VirtualAlloc`/`VirtualFree` fallback on Windows). `mio` uses
Winsock2 on Windows. `tikv-jemallocator` is disabled on Windows MSVC
(`#[cfg(not(target_env = "msvc"))]`).

## Priority Order

| Phase | Effort | Impact | Priority |
|-------|--------|--------|----------|
| 1. Linker improvements | Small  | Unlocks the feature | **P0** |
| 2. Cross-build runtime | Small  | Makes it usable     | **P0** |
| 3. CI and testing       | Medium | Prevents regressions | **P1** |
| 4. MSVC target          | Large  | Niche use case      | **P3** |
| 5. Polish               | Small  | Developer experience | **P2** |
