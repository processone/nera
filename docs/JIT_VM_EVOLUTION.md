# Evolving nera into a JIT-Based VM

## Overview

The nera architecture — Cranelift backend, modular compilation pipeline, runtime
with process scheduler and apply-based dispatch — is well-positioned to evolve from
an ahead-of-time (AOT) compiler into a JIT-based virtual machine. This document
outlines the path, the key changes required, and the harder problems to solve.

## Current Architecture (AOT)

```
  .erl source
      │
      ▼
  nera-syntax  (parse → AST)
      │
      ▼
  nera-ir      (lower → IR)
      │
      ▼
  nera-codegen (Cranelift ObjectModule → .o file)
      │
      ▼
  System linker → standalone native binary
```

Code is compiled once, linked against the runtime, and produces a self-contained
executable. There is no runtime code loading.

## Target Architecture (JIT VM)

```
┌──────────────────────────────────────────┐
│              nera VM process             │
│                                          │
│  ┌───────────┐    ┌──────────────────┐   │
│  │  Loader   │───▶│  JIT Compiler    │   │
│  │ (.erl src │    │  (Cranelift      │   │
│  │  or .beam)│    │   JITModule)     │   │
│  └───────────┘    └────────┬─────────┘   │
│                            ▼             │
│  ┌───────────────────────────────────┐   │
│  │  Module Registry                  │   │
│  │  {module, function, arity}        │   │
│  │       → fn_ptr (current + old)    │   │
│  └───────────────────────────────────┘   │
│                            ▼             │
│  ┌───────────────────────────────────┐   │
│  │  Scheduler + Processes            │   │
│  └───────────────────────────────────┘   │
└──────────────────────────────────────────┘
```

The compiler crates become libraries linked into the VM. Loading a module means:
parse → lower → JIT compile → register function pointers. No files written to disk.

## Why It Fits

**Cranelift is a JIT engine.** It was built for Wasmtime's JIT. Switching from
`cranelift-object::ObjectModule` to `cranelift-jit::JITModule` gives in-memory
executable code and raw function pointers with no linking step.

**All calls already go through indirection.** Remote calls dispatch via
`nera_rt_apply` (a runtime function table keyed by module/function/arity).
Closures dispatch via `nera_rt_call_closure`. These are the exact dispatch
points a VM needs — hot code swapping means updating the pointer in the registry.

**The pipeline is modular.** `nera-syntax`, `nera-ir`, and `nera-codegen` are
separate crates. Embedding the compiler in the runtime is a matter of linking them
as library dependencies rather than driving them from a CLI binary.

**Per-process heaps and scheduling exist.** Process spawning, message passing,
receive/after, and timeouts are already implemented. The runtime already looks like
a VM scheduler.

## Key Changes

### 1. JITModule Backend

Replace `ObjectModule` with Cranelift's `JITModule` in `nera-codegen`. This
produces executable memory pages and function pointers directly — no `.o` file,
no system linker. The `FuncTranslator` and IR lowering remain unchanged.

### 2. Module Registry with Versioning

BEAM keeps two versions of each module: **current** and **old**. When a new version
is loaded, the dispatch table is updated to point to new code. Processes still
executing old code keep their function pointers alive. The old version is freed
only when no process references it.

The existing `nera_rt_apply` dispatch table becomes the "current" version.
Old versions are reference-counted per module and freed on purge.

### 3. Embed the Compiler in the Runtime

The VM binary links `nera-syntax`, `nera-ir`, and `nera-codegen`. A
`load_module(source)` function runs the full pipeline and registers the resulting
function pointers. This is what the CLI does today, just at runtime instead of
build time.

### 4. Code Loading Interface

Two options with different trade-offs:

| Approach        | Effort | Benefit                              |
|-----------------|--------|--------------------------------------|
| From `.erl` src | Low    | Already implemented in the pipeline  |
| From `.beam`    | High   | OTP library compatibility            |

Loading from `.erl` source works immediately. Loading from BEAM bytecode requires
a BEAM → nera-ir translator but unlocks the entire OTP standard library ecosystem.

## Harder Problems

### JIT Code Memory Management

Each `JITModule` owns executable memory pages. When a module is reloaded, the old
`JITModule` must stay alive as long as any process has a stack frame or closure
pointing into it. This requires reference counting or epoch-based reclamation of
module versions.

### Code Purging

BEAM's `code:purge/1` forcibly kills processes still running old code. Implementing
this requires the ability to enumerate which processes reference a given module
version — either via stack walking or by tracking module references per process.

### Compilation Latency

Cranelift is fast (microseconds per function), but loading a large module has
measurable latency. Options to mitigate:

- **Lazy compilation**: only JIT a function on first call.
- **Tiered compilation**: interpret or use a baseline compiler first, then JIT
  hot functions with optimizations (significant additional complexity).
- **Ahead-of-time caching**: cache JIT output to disk for known modules.

### Distribution Protocol

A production BEAM-compatible VM needs the Erlang distribution protocol for
clustering. This is orthogonal to JIT but essential for real OTP workloads.

## Incremental Path

1. **Replace ObjectModule with JITModule** — validate that the same IR produces
   working code via the JIT path. This can coexist with AOT as a compile flag.
2. **Build `load_module/1`** — a runtime function that takes source, JIT-compiles
   it, and registers function pointers in the dispatch table.
3. **Add module versioning** — current/old scheme with reference counting.
4. **Implement a REPL/shell** — a process that reads Erlang expressions, compiles
   them as anonymous modules, and executes them. This is the Erlang shell.
5. **BEAM bytecode loader** (optional) — translate BEAM instructions to nera-ir
   for OTP compatibility.
