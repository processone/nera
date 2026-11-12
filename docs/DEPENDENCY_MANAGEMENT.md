# Dependency Management Plan

## Goals

Compile standard Erlang project layouts (`src/`, `include/`) and their
dependency trees, integrating with the existing Hex ecosystem while keeping
the tooling simple and predictable.

---

## Phase 0 — Project-Aware Compilation (CLI flags) ✅

Low-level flags that make nera usable on real projects today, without
any dependency resolution logic.

| Flag               | Purpose                                                   |
|--------------------|-----------------------------------------------------------|
| `--src <dir>`      | Auto-discover `.erl` files (recursive glob)               |
| `-I <dir>`         | Add include search path for `-include()`                  |
| `--deps-dir <dir>` | Scan each subdirectory as an OTP app (`src/`, `include/`) |

**Example:**

```bash
rebar3 get-deps                          # user fetches deps with existing tools
nera --src src/ -I include/ \
      --deps-dir _build/default/lib \
      -o myapp -e myapp:start
```

The preprocessor tries relative resolution first, then searches `-I` paths.
`--deps-dir` auto-discovers `<dep>/src/*.erl` and adds `<dep>/include/` to
the include search path for each dependency.

This phase has no new file formats and no network access.

---

## Phase 1 — Project Manifest (`nera.toml`) ✅

Declare dependencies in `nera.toml`, fetch from hex.pm or git with one
command, and `nera build` picks them up automatically.

```toml
[project]
name = "myapp"
entry = "myapp:start"
output = "myapp"

[deps]
jsx = "3.1.0"                                                    # hex.pm
ranch = { git = "https://github.com/ninenines/ranch.git", tag = "1.8.0" }
mylib = { git = "https://github.com/user/repo.git", branch = "main" }
utils = { path = "../shared_utils" }
```

**Commands:**

| Command                     | Description                                          |
|-----------------------------|------------------------------------------------------|
| `nera init`                 | Generate `nera.toml` in the current directory        |
| `nera deps get`             | Fetch all deps into `.nera/deps/`                    |
| `nera deps add <pkg> [ver]` | Add a hex dep to `nera.toml` and fetch it            |
| `nera deps import [file]`   | Migrate `rebar.config` / `mix.exs` deps to nera.toml |
| `nera build`                | Compile `src/` + fetched deps into a native binary   |

**Dep sources:**

1. **hex.pm** — `jsx = "3.1.0"` downloads from `repo.hex.pm/tarballs/`
2. **git** — `{ git = "...", tag/branch/rev = "..." }` cloned via `git` CLI
3. **path** — `{ path = "../shared_utils" }` symlinked into `.nera/deps/`

**Build integration:** `nera build` auto-discovers `.nera/deps/` when a
`nera.toml` exists. Each subdirectory is scanned as an OTP application
(`src/*.erl` + `include/`). No manual `--deps-dir` flag needed.

**Not in scope (Phase 1):** transitive resolution, lock file, global cache,
semver ranges, OTP source compilation.

---

## Phase 1.5 — OTP as a Dependency Source

### Problem

Nera ships a small set of hand-written stdlib modules (`lists`, `maps`,
`gen_server`, `supervisor`, `application`, `proc_lib`, `timer`), but OTP
has 100+ modules across multiple applications (stdlib, kernel, mnesia,
crypto, inets, etc.). Reimplementing them all is not viable.

Since Nera compiles `.erl` files, many OTP modules that are pure Erlang
with minimal BIF dependencies can be compiled directly from OTP's own
sources. Modules like `string`, `proplists`, `orddict`, `queue`, `sets`,
`gb_trees`, `gb_sets`, `array`, `dict`, `filename`, `filelib` are prime
candidates.

### Multi-app dependency support

OTP's repository is structured as `lib/stdlib/src/`, `lib/kernel/src/`,
`lib/mnesia/src/`, etc. — not as individual packages. Nera's dep scanner
expects `{dep}/src/*.erl`.

New fields on the dep table:

| Field     | Type       | Purpose                                               |
|-----------|------------|-------------------------------------------------------|
| `apps`    | `[string]` | Extract specific OTP apps from a multi-app repository |
| `exclude` | `[string]` | Module names to skip (won't compile or aren't needed) |

```toml
[deps]
otp = { git = "https://github.com/erlang/otp.git", tag = "OTP-27.0",
apps = ["stdlib", "kernel", "mnesia"],
exclude = ["erl_eval", "erl_parse", "erl_lint", "compile", "code",
    "net_kernel", "rpc", "global", "global_group",
    "erl_boot_server", "heart"] }
```

When `apps` is set, `nera deps get` extracts each listed app from
`lib/{app}/` into `.nera/deps/{app}/`, producing the standard OTP app
layout (`src/`, `include/`) that `discover_files()` already understands.
When `exclude` is set, matching `.erl` files are removed after extraction.

This also works for any multi-app repository, not just OTP.
 
### Module precedence

Three-tier shadowing rule:

1. **Nera bundled stdlib** — always takes precedence (embedded modules)
2. **User modules** — project source in `src/`
3. **Dependency modules** — from `.nera/deps/`

If an OTP dependency provides `lists.erl` but Nera ships its own
optimised `lists` module, Nera's version is used. If the user writes
their own `my_server.erl` and a dep also provides `my_server.erl`, the
user's version wins.

This prevents OTP dependency sources from accidentally overriding
Nera-native implementations that may use different BIF bindings or
runtime-specific optimisations.

### Non-fatal compilation for dependency modules

Not all OTP modules will compile with Nera. Modules that use
`-on_load` (NIF loading), `erts_internal:*` BIFs, parse transforms,
distribution primitives (`net_kernel`, `rpc`), hot code loading (`code`),
or `erl_eval` (dynamic code evaluation) will fail.

For dependency modules, compilation failures are **non-fatal warnings**
rather than errors. Failed modules are recorded as "unavailable".
A user module that calls into an unavailable dep module gets a clear
error at validation time:

```
error: module `erl_eval` is not available (compilation failed, see above)
  --> src/my_app.erl:42:5
```

This lets `nera build` succeed with partial OTP coverage: modules that
compile are available; modules that don't are silently skipped until
someone references them.

### Include path and header resolution

OTP modules use `-include_lib("stdlib/include/assert.hrl")` and similar.
Because `nera deps get` places OTP apps in `.nera/deps/stdlib/`,
`.nera/deps/kernel/`, etc., the existing `AppRegistry` automatically
maps application names to the correct include paths. No additional
mechanism is needed.

---

## Phase 2 — Transitive Resolution with MVS

Adopt Go's **Minimum Version Selection** algorithm.

### Why MVS

- No SAT-solver: pick the lowest version satisfying all constraints.
- Deterministic without a lockfile (though we still generate `nera.sum`).
- Adding a dependency never silently upgrades an unrelated one.

### How it works

Each fetched dep may contain its own `nera.toml` (or we parse its
`rebar.config` / hex metadata for declared deps). The resolver builds a
requirement graph and selects, for each package, the maximum of the minimum
versions requested across all dependants.

### Integrity — `nera.sum`

```
cowboy 2.10.0 sha256:a8f3b2c...
ranch  1.8.0  sha256:c4d5e6f...
```

- One line per resolved package.
- Hash covers the source tarball (hex) or tree hash (git).
- Committed to version control for reproducible builds.

---

## Phase 3 — Automatic Dependency Tidying

```bash
nera deps tidy
```

Scans all remote calls (`module:function`) across the project source,
compares against declared deps, and:

- **Adds** missing deps (prompts for version / searches hex.pm).
- **Removes** deps no longer referenced.

Analogous to `go mod tidy`.

---

## Phase 4 — Global Cache

Store downloaded packages in a shared cache directory
(`~/.nera/cache/` or `$NERA_CACHE`), keyed by name + version + hash.

Projects link (or copy) from the cache instead of re-downloading. Same
idea as Go's `GOMODCACHE`.

---

## Phase 5 — Dependency Graph & Incremental Builds

Once OTP dependencies bring dozens of additional modules into the build,
recompiling everything on every `nera build` becomes a bottleneck.
Incremental builds and dependency management share the same foundation:
a module-level dependency graph.

### Dependency graph

A `DepGraph` tracks per-module metadata across builds:

| Field            | Purpose                                                   |
|------------------|-----------------------------------------------------------|
| `source_hash`    | Hash of combined source (after preprocessing)             |
| `include_deps`   | Canonical paths of all transitively included `.hrl` files |
| `include_hashes` | Content hash of each included file                        |
| `call_deps`      | Set of `(module, function, arity)` remote call targets    |
| `exports`        | Set of `(function, arity)` — the module's public API      |
| `ir_hash`        | Hash of the lowered IR                                    |
| `object_hash`    | Hash of the generated `.o` file                           |

All identifiers are strings (not session-local `AtomId`s) so the graph
survives across build invocations.

Stored as `.nera/cache/manifest.json`. Cached object files go in
`.nera/cache/objects/<module>.o`.

### Include tracking

The preprocessor currently merges included files into `combined_source`
but does not record which files were included. A small extension makes
it emit the list of included file paths, enabling cache invalidation
when a `.hrl` changes.

### Incremental build algorithm

1. **Load manifest.**  If missing, version mismatch, or target changed,
   fall back to full build.
2. **Identify dirty modules.**  For each source, preprocess and compare
   `source_hash` + `include_hashes` against the cached values.
3. **Recompile dirty modules only** (parse → lower → codegen).
4. **Use cached `.o` files** for clean modules.
5. **Validation delta.**  If no module's export surface changed, skip
   the whole-program `check_unresolved_calls` pass entirely. If some
   exports changed, only revalidate modules that call into the changed
   modules (using the `call_deps` reverse index).
6. **Link** only if any `.o` file changed.
7. **Save** updated manifest.

### Relationship to other phases

- **Phase 3 (`nera deps tidy`)** uses `call_deps` from the graph to
  determine which dependencies are actually referenced.
- **Phase 4 (global cache)** stores downloaded packages; Phase 5 stores
  compiled artefacts. They are complementary: `~/.nera/cache/` for
  downloads, `.nera/cache/` for build output.

### Cache layout

```
.nera/
  deps/                  # fetched dependencies (Phase 1)
    stdlib/src/*.erl
    kernel/include/*.hrl
    jsx/src/*.erl
  cache/                 # incremental build cache (Phase 5)
    manifest.json        # dependency graph
    objects/             # per-module object files
      string.o
      proplists.o
      my_module.o
```

---

## Mapping to Go Modules

| Go                     | nera                        | Notes                            |
|------------------------|-----------------------------|----------------------------------|
| `go.mod`               | `nera.toml`                 | TOML instead of custom syntax    |
| `go.sum`               | `nera.sum`                  | Package checksums                |
| `proxy.golang.org`     | hex.pm API                  | Existing Erlang package registry |
| `go get pkg@v1.2`      | `nera deps add cowboy 2.10` | Adds to manifest                 |
| `go mod tidy`          | `nera deps tidy`            | Reconcile source ↔ manifest      |
| `GOMODCACHE`           | `~/.nera/cache/`            | Shared download cache            |
| `replace` directive    | `path` dep or `[overrides]` | Local dev overrides              |
| `GOROOT` std library   | OTP `apps` dep + shadowing  | Compile OTP sources as deps      |
| incremental `go build` | `.nera/cache/` manifest     | Per-module `.o` caching          |

### Intentionally omitted

- **Major-version suffixes** (`/v2`): not an Erlang convention; Hex handles
  versioning separately.
- **Custom module proxy protocol**: hex.pm already provides a stable API.
- **Vendoring**: `path` deps cover the same use case.

---

## Interop with rebar3 / Mix

Rather than replace these tools, nera can coexist:

- **Phase 0** already works with `rebar3 get-deps` output via `--deps-dir`.
- **Phase 1+** reads hex.pm directly, but a project can still use rebar3 for
  fetching and nera for compiling (just point `--deps-dir` at `_build/`).
- A future **rebar3 plugin** (`rebar3_nera`) could integrate as a compiler
  backend inside rebar3's own build graph.

The priority is native nera tooling; rebar3 plugin support is a nice-to-have
once the core pipeline is solid.

### Migration helper: `nera deps import`

One-shot command for adopting nera on an existing Erlang project:

```bash
nera deps import                    # auto-detects rebar.config or mix.exs
nera deps import rebar.config       # explicit file
nera deps import mix.exs            # Elixir projects with Erlang deps
```

Reads dependency declarations from the source file and writes the
equivalent `[deps]` entries into `nera.toml`. Run once when migrating
a project to nera; after that, `nera.toml` is the source of truth.

**rebar.config** parsing covers the common forms:

```erlang
{deps, [
    {jsx, "3.1.0"},                                    %% → jsx = "3.1.0"
    {cowboy, {git, "https://...", {tag, "2.10.0"}}},   %% → cowboy = { git = "...", tag = "..." }
    {ranch, "1.8.0"}                                   %% → ranch = "1.8.0"
]}.
```

**mix.exs** parsing covers `defp deps do` blocks:

```elixir
{:jason, "~> 1.4"},                                    # → jason = "1.4.0" (resolve ~> to latest patch)
{:cowboy, git: "https://...", tag: "2.10.0"}           # → cowboy = { git = "...", tag = "..." }
```

The parser does not need to be a full Erlang/Elixir evaluator — it only
needs to handle the common literal dependency declaration patterns.
Exotic forms (dynamic deps, OS-conditional deps, override blocks) are
skipped with a warning.

After import, the user reviews the generated `[deps]` section and runs
`nera deps get` to fetch everything.

---

## Implementation Priority

1. **Phase 0** ✅ — unblocks real project compilation with no new formats.
2. **Phase 1** ✅ — gives nera its own identity as a build tool.
3. **Phase 1.5** — grows stdlib coverage without reimplementation; unblocks
   real-world projects that need `string`, `proplists`, `filename`, etc.
4. **Phase 2** — required once projects have non-trivial dep trees.
5. **Phase 5** — incremental builds become important once OTP deps add
   dozens of modules to the build. The dependency graph also provides
   the `call_deps` data needed by Phase 3.
6. **Phase 3–4** — polish and developer experience.
