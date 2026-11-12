# Fuzzing & Property-Based Testing Plan for Nera

## Context

Nera is an Erlang-to-native compiler with a Cranelift backend. This document is
the original testing expansion plan, not a statement that every phase is absent
or complete. As of 2026-10-10, lexer/parser and formatter fuzz targets exist, as
do formatter and runtime-term proptests. The original workspace run found a
formatter property failure; the subsequent grouping repair passes the formatter
suite. No fuzz campaign was run for this baseline. See
[PROJECT_STATUS.md](PROJECT_STATUS.md) for current counts, failures and evidence.

The remaining plan covers compiler correctness and unsafe runtime surfaces
including heap allocation, GC, BIFs and term manipulation. Differential testing
against BEAM remains a priority; the baseline adds a few diagnostic comparisons,
not a comprehensive differential suite.

## Two Complementary Approaches

This plan uses **two** techniques that cover different failure modes:

| | Coverage-guided fuzzing (libfuzzer) | Property-based testing (proptest) |
|---|---|---|
| **Input** | Raw bytes, mutated with coverage feedback | Structured typed generators |
| **Finds** | Crashes, panics, OOM, hangs | Semantic invariant violations |
| **Shrinking** | Crash-artifact minimization | Automatic to minimal counterexample |
| **Integration** | Separate `cargo fuzz` (nightly) | Runs in `cargo test` (stable) |
| **Best for** | Parsers, decoders (byte → structure) | Round-trips, algebraic laws, equivalence checks |

**Fuzzing** answers *"does it crash?"*. **Proptest** answers *"does it compute the right thing?"*. Both are needed: a BIF that silently returns a wrong answer is just as broken as one that panics.

**Recommended library**: `proptest = "1"` — mature, composable strategy combinators (`prop_oneof!`, `prop_flat_map`), automatic shrinking, regression file persistence (`.proptest-regressions`), runs on stable Rust.

## Phased Plan

### Phase 0 — Scaffolding (half a day)

- Install `cargo-fuzz`, run `cargo fuzz init` in `crates/nera-syntax/`
- Add `**/fuzz/corpus/` and `**/fuzz/artifacts/` to `.gitignore`
- Seed initial corpus: copy ~30 representative `.erl` files from `tests/` into `fuzz/corpus/fuzz_parse/`

---

### Phase 1 — Parser Fuzzing (1-2 days) — Highest ROI

**Target**: `crates/nera-syntax/fuzz/fuzz_targets/fuzz_parse.rs`

```rust
#![no_main]
use libfuzzer_sys::fuzz_target;
use nera_syntax::parser::Parser;

fuzz_target!(|data: &[u8]| {
    if let Ok(source) = std::str::from_utf8(data) {
        let mut parser = Parser::new(source);
        let _result = parser.parse_module();
        // Must never panic. ParseResult.errors may be non-empty — that's fine.
    }
});
```

**Run**: `cargo fuzz run fuzz_parse -- -max_len=4096 -jobs=4`

**Likely bugs**: Stack overflows on deep nesting, index panics in lookahead, escape sequence handling in `unescape_erlang_string()`.

**Bonus** — Add `fuzz_lex.rs` targeting `Lexer::lex_all()` the same day.

---

### Phase 2 — Formatter Round-Trip (2-3 days)

**Target**: `crates/nera-fmt/fuzz/fuzz_targets/fuzz_format.rs`

Parse the input first (skip if errors), then assert **idempotency**: `format(format(x)) == format(x)`.

```rust
#![no_main]
use libfuzzer_sys::fuzz_target;
use nera_fmt::{format_source, config::FormatConfig};
use nera_syntax::parser::Parser;

fuzz_target!(|data: &[u8]| {
    if let Ok(source) = std::str::from_utf8(data) {
        let mut parser = Parser::new(source);
        let result = parser.parse_module();
        if result.has_errors() { return; }
        let config = FormatConfig::default();
        if let Ok(formatted) = format_source(source, &config) {
            if let Ok(reformatted) = format_source(&formatted, &config) {
                assert_eq!(formatted, reformatted, "formatter is not idempotent");
            }
        }
    }
});
```

**Likely bugs**: Comment drift, whitespace accumulation, panics on unusual-but-valid AST shapes.

---

### Phase 3 — Preprocessor (2-3 days)

**Target**: `crates/nera-syntax/fuzz/fuzz_targets/fuzz_preproc.rs`

Lex the input, then feed tokens into `Preprocessor::process()`.

```rust
#![no_main]
use libfuzzer_sys::fuzz_target;
use nera_syntax::lexer::Lexer;
use nera_syntax::preproc::Preprocessor;
use std::path::Path;

fuzz_target!(|data: &[u8]| {
    if let Ok(source) = std::str::from_utf8(data) {
        let tokens = match Lexer::lex_all(source) {
            Ok(t) => t,
            Err(_) => return,
        };
        let mut pp = Preprocessor::new(source, Path::new("fuzz_input.erl"));
        let _ = pp.process(tokens);
    }
});
```

**Likely bugs**: Recursive macro expansion edge cases, mismatched `-ifdef`/`-endif` stacks, panics when `?MODULE` is referenced without `-module()`.

---

### Phase 4 — BIF Fuzzing with Structure-Aware Inputs (3-5 days)

**Target**: `crates/nera-runtime/fuzz/fuzz_targets/fuzz_bifs.rs`

Implement a `TermRecipe` enum with `#[derive(Arbitrary)]` to generate valid term trees on a real `ProcessHeap`, then dispatch to randomly-selected BIFs. Assert: returns `Ok` or `Err`, never panics.

```rust
#[derive(Arbitrary, Debug)]
enum TermRecipe {
    SmallInt(i32),
    Atom(u8),
    Nil,
    Float(f64),
    Tuple(Vec<TermRecipe>),
    Cons(Box<TermRecipe>, Box<TermRecipe>),
    Binary(Vec<u8>),
}

fn materialize(heap: &mut ProcessHeap, recipe: &TermRecipe) -> ErlTerm {
    match recipe {
        TermRecipe::SmallInt(v) => ErlTerm::small_int(*v as i64),
        TermRecipe::Atom(id) => ErlTerm::atom(*id as u32),
        TermRecipe::Nil => ErlTerm::nil(),
        TermRecipe::Float(f) => heap.alloc_float(*f),
        TermRecipe::Tuple(elems) => {
            let terms: Vec<ErlTerm> = elems.iter().map(|r| materialize(heap, r)).collect();
            heap.alloc_tuple(&terms)
        }
        TermRecipe::Cons(h, t) => {
            let head = materialize(heap, h);
            let tail = materialize(heap, t);
            heap.alloc_cons(head, tail)
        }
        TermRecipe::Binary(data) => heap.alloc_binary(data),
    }
}
```

**Dependencies**: `arbitrary = { version = "1", features = ["derive"] }` in fuzz Cargo.toml.

**Likely bugs**: Missing argument-count checks, unchecked `as_cons_ptr()` on wrong-tagged terms, integer overflow in `bif_abs()` at `SMALL_INT_MIN`.

---

### Phase 5 — Heap/GC Fuzzing (5-7 days) — Highest unsafe exposure

**Target**: `crates/nera-runtime/fuzz/fuzz_targets/fuzz_gc.rs`

Generate sequences of `HeapOp` (alloc tuple/cons/binary/float, trigger GC, drop root). After each GC, validate all surviving roots: correct tag bits, valid boxed headers, traversable cons chains.

```rust
#[derive(Arbitrary)]
enum HeapOp {
    AllocSmallInt(i32),
    AllocAtom(u8),
    AllocTuple(u8),        // arity, fill with previously allocated terms
    AllocCons,             // head=last, tail=second-to-last
    AllocFloat(f64),
    AllocBinary(Vec<u8>),
    AllocRefcBinary(Vec<u8>),
    AllocMap(u8),
    Collect,               // trigger minor GC
    CollectFull,           // trigger major GC
    DropRoot(u8),          // remove a root
}
```

**Likely bugs**: Forwarding pointers leaked after copy, off-by-one in bigint copy sizing, refc binary double-free, use-after-free from stale old-gen pointers.

---

### Phase 6 — IR Lowering (3-5 days)

Parse-then-lower pipeline. Feed fuzzed Erlang source through `Parser::parse_module()` → `IrBuilder::lower_module()`. Assert no panic.

**Likely bugs**: Unhandled AST variants, stack overflow in recursive expression lowering, variable scope errors in the Maranget pattern compiler.

---

### Phase 7 — CI Integration (1-2 days)

**File**: `.github/workflows/fuzz.yml` — Nightly workflow:

```yaml
name: Fuzz
on:
  schedule:
    - cron: '0 3 * * *'
  workflow_dispatch: {}

jobs:
  fuzz:
    runs-on: ubuntu-26.04
    strategy:
      fail-fast: false
      matrix:
        target:
          - { crate: nera-syntax, name: fuzz_parse }
          - { crate: nera-syntax, name: fuzz_lex }
          - { crate: nera-syntax, name: fuzz_preproc }
          - { crate: nera-fmt,    name: fuzz_format }
          - { crate: nera-runtime, name: fuzz_bifs }
          - { crate: nera-runtime, name: fuzz_gc }
    steps:
      - uses: actions/checkout@v7
      - uses: dtolnay/rust-toolchain@nightly
      - run: cargo install cargo-fuzz
      - name: Restore corpus
        uses: actions/cache@v6
        with:
          path: crates/${{ matrix.target.crate }}/fuzz/corpus/${{ matrix.target.name }}
          key: fuzz-corpus-${{ matrix.target.crate }}-${{ matrix.target.name }}-${{ github.sha }}
          restore-keys: fuzz-corpus-${{ matrix.target.crate }}-${{ matrix.target.name }}-
      - name: Fuzz for 10 minutes
        run: |
          cd crates/${{ matrix.target.crate }}
          cargo fuzz run ${{ matrix.target.name }} -- \
            -max_total_time=600 \
            -max_len=8192
      - name: Upload artifacts on failure
        if: failure()
        uses: actions/upload-artifact@v7
        with:
          name: crash-${{ matrix.target.crate }}-${{ matrix.target.name }}
          path: crates/${{ matrix.target.crate }}/fuzz/artifacts/${{ matrix.target.name }}/
```

---

### Phase 8 — Codegen (stretch goal, 5-7 days)

Full parse → lower → `compile_module()` pipeline. Slow per-iteration but catches Cranelift IR translation bugs.

### Phase 9 — Differential Testing (future project)

Compare Nera vs BEAM output on generated Erlang programs. Requires BEAM in CI, sandboxed execution, a terminating-program generator.

---

## Property-Based Testing (proptest)

These phases run in `cargo test` on stable Rust, complementing the fuzzing targets above.

### Phase P1 — Term Encode/Decode Round-Trip (0.5 days) — Quickest Win

**Crate**: `nera-runtime` (add `proptest = "1"` to `[dev-dependencies]`)

**File**: `crates/nera-runtime/src/term.rs` — inline `#[cfg(test)]` module

The 64-bit tagged pointer scheme (61-bit signed ints, 3-bit primary tags, 4-bit boxed subtags) is the most critical bit-twiddling in the project. Currently covered by a handful of fixed-input unit tests, but the boundary conditions around `SMALL_INT_MIN`/`SMALL_INT_MAX` and tag discrimination deserve exhaustive property coverage.

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn small_int_round_trip(v in SMALL_INT_MIN..=SMALL_INT_MAX) {
        let term = ErlTerm::small_int(v);
        prop_assert!(term.is_small_int());
        prop_assert!(!term.is_atom());
        prop_assert!(!term.is_nil());
        prop_assert_eq!(term.as_i64(), v);
    }

    #[test]
    fn atom_round_trip(id in 0u32..1_000_000) {
        let term = ErlTerm::atom(id);
        prop_assert!(term.is_atom());
        prop_assert!(!term.is_small_int());
        prop_assert_eq!(term.as_atom_id(), id);
    }

    #[test]
    fn boxed_header_round_trip(
        subtype in 0u64..=0xF,
        arity in 0u64..=(1u64 << 58),
    ) {
        let hdr = BoxedHeader::new(subtype, arity);
        prop_assert_eq!(hdr.subtype(), subtype);
        prop_assert_eq!(hdr.arity(), arity);
    }

    #[test]
    fn fits_small_int_boundaries(v in prop::num::i64::ANY) {
        let fits = fits_small_int(v);
        prop_assert_eq!(fits, v >= SMALL_INT_MIN && v <= SMALL_INT_MAX);
        if fits {
            // Must survive round-trip
            prop_assert_eq!(ErlTerm::small_int(v).as_i64(), v);
        }
    }
}
```

**Properties verified**:
- `ErlTerm::small_int(v).as_i64() == v` for all v in range
- `ErlTerm::atom(id).as_atom_id() == id` for all u32
- `BoxedHeader::new(sub, ar).subtype() == sub && .arity() == ar`
- Tag discrimination: each constructor sets exactly one tag predicate to true
- `fits_small_int` is consistent with actual encode/decode success

---

### Phase P2 — Formatter Semantic Properties (1-2 days)

**Crate**: `nera-fmt` (add `proptest = "1"` to `[dev-dependencies]`)

**File**: `crates/nera-fmt/tests/proptest_format.rs`

Existing tests check idempotence for ~10 fixed inputs. Proptest generalizes to any valid Erlang source. The strategy uses the 296 `.erl` test files as a seed pool.

```rust
use proptest::prelude::*;
use proptest::sample::select;
use std::fs;

fn valid_erlang_sources() -> impl Strategy<Value = String> {
    let paths: Vec<_> = glob::glob("../../tests/**/*.erl")
        .unwrap()
        .filter_map(|p| p.ok())
        .collect();
    select(paths).prop_map(|p| fs::read_to_string(p).unwrap())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn idempotent(src in valid_erlang_sources()) {
        let config = FormatConfig::default();
        if let Ok(first) = format_source(&src, &config) {
            let second = format_source(&first, &config).unwrap();
            prop_assert_eq!(first, second, "formatter is not idempotent");
        }
    }

    #[test]
    fn preserves_parse_result(src in valid_erlang_sources()) {
        let config = FormatConfig::default();
        if let Ok(formatted) = format_source(&src, &config) {
            // Both must parse without errors, and produce same function count
            let orig = Parser::new(&src).parse_module();
            let fmt = Parser::new(&formatted).parse_module();
            prop_assert_eq!(
                orig.module.functions.len(),
                fmt.module.functions.len(),
                "formatting changed function count"
            );
        }
    }
}
```

**Properties verified**:
- Idempotence: `format(format(x)) == format(x)`
- Semantics preservation: formatting doesn't change the AST structure
- (Future) Comment preservation: comment count before/after is equal

This complements Phase 2 fuzzing — fuzzing finds panics, proptest finds semantic drift.

---

### Phase P3 — Pattern Match Compilation Equivalence (3-5 days)

**Crate**: `nera-ir` (add `proptest = "1"` to `[dev-dependencies]`)

**File**: `crates/nera-ir/src/pattern_proptest.rs`

Pattern match compilation (Maranget-style decision trees) is the hardest component to test for correctness — bugs don't crash, they just match the wrong clause. This is where proptest is **uniquely** valuable: you need structured pattern generators that byte-level fuzzing cannot produce.

```rust
use proptest::prelude::*;

/// A simplified term value for testing pattern match semantics.
#[derive(Debug, Clone, PartialEq)]
enum TestValue {
    Atom(u8),
    Int(i32),
    Tuple(Vec<TestValue>),
    Cons(Box<TestValue>, Box<TestValue>),
    Nil,
}

/// A simplified pattern for testing.
#[derive(Debug, Clone)]
enum TestPattern {
    Wildcard,
    Var,
    Literal(TestValue),
    Tuple(Vec<TestPattern>),
    Cons(Box<TestPattern>, Box<TestPattern>),
}

fn gen_value(depth: u32) -> BoxedStrategy<TestValue> {
    if depth == 0 {
        prop_oneof![
            (0u8..10).prop_map(TestValue::Atom),
            (-100i32..100).prop_map(TestValue::Int),
            Just(TestValue::Nil),
        ].boxed()
    } else {
        prop_oneof![
            (0u8..10).prop_map(TestValue::Atom),
            (-100i32..100).prop_map(TestValue::Int),
            Just(TestValue::Nil),
            prop::collection::vec(gen_value(depth - 1), 0..4)
                .prop_map(TestValue::Tuple),
            (gen_value(depth - 1), gen_value(depth - 1))
                .prop_map(|(h, t)| TestValue::Cons(Box::new(h), Box::new(t))),
        ].boxed()
    }
}

fn gen_pattern(depth: u32) -> BoxedStrategy<TestPattern> {
    // ... similar recursive generator ...
}

/// Naive reference matcher — trusted, simple, obviously correct.
fn naive_match(value: &TestValue, pattern: &TestPattern) -> bool {
    match (value, pattern) {
        (_, TestPattern::Wildcard | TestPattern::Var) => true,
        (v, TestPattern::Literal(lit)) => v == lit,
        (TestValue::Tuple(vs), TestPattern::Tuple(ps)) =>
            vs.len() == ps.len() && vs.iter().zip(ps).all(|(v, p)| naive_match(v, p)),
        (TestValue::Cons(h, t), TestPattern::Cons(ph, pt)) =>
            naive_match(h, ph) && naive_match(t, pt),
        _ => false,
    }
}

proptest! {
    #[test]
    fn compiled_matches_agree_with_naive(
        patterns in prop::collection::vec(gen_pattern(2), 1..6),
        value in gen_value(2),
    ) {
        let naive_result = patterns.iter().position(|p| naive_match(&value, p));
        let compiled_result = compile_and_run(&patterns, &value);
        prop_assert_eq!(naive_result, compiled_result,
            "compiled pattern match disagrees with naive matcher");
    }
}
```

**Properties verified**:
- Compiled decision tree matches the same clause as naive matching
- Exhaustive patterns produce no failure branch
- No dead branches in the decision tree
- First-match semantics: result is always the first matching clause

This is the **highest-value** proptest target for correctness. Byte-level fuzzing of the pattern compiler can only find crashes — proptest finds wrong answers.

---

### Phase P4 — BIF Safety Properties (2-3 days)

**Crate**: `nera-runtime` (reuses `TermRecipe` from Phase 4 fuzzing)

**File**: `crates/nera-runtime/src/bif_proptest.rs`

The same `TermRecipe` generator designed for fuzzing (Phase 4) works as a proptest strategy. The advantage: runs in `cargo test`, no nightly toolchain, and minimal counterexample shrinking helps diagnose failures.

```rust
use proptest::prelude::*;
use proptest_derive::Arbitrary;

#[derive(Debug, Clone, Arbitrary)]
enum TermRecipe {
    SmallInt(i32),
    Atom(#[proptest(strategy = "0u8..10")] u8),
    Nil,
    Float(f64),
    Tuple(#[proptest(strategy = "prop::collection::vec(any::<TermRecipe>(), 0..4)")] Vec<TermRecipe>),
    Cons(Box<TermRecipe>, Box<TermRecipe>),
    Binary(#[proptest(strategy = "prop::collection::vec(any::<u8>(), 0..64)")] Vec<u8>),
}

proptest! {
    #[test]
    fn bif_element_never_panics(
        tuple in any::<TermRecipe>(),
        index in any::<TermRecipe>(),
    ) {
        let mut heap = test_heap();
        let t = materialize(&mut heap, &tuple);
        let i = materialize(&mut heap, &index);
        // Must return Ok or Err, never panic
        let _ = bif_element(&mut heap, i, t);
    }

    #[test]
    fn bif_hd_never_panics(term in any::<TermRecipe>()) {
        let mut heap = test_heap();
        let t = materialize(&mut heap, &term);
        let _ = bif_hd(t);
    }

    // One proptest! block per BIF family...
}
```

**Properties verified**:
- Every BIF returns `Ok(_)` or `Err(badarg/badarith/...)` — never panics
- BIFs that should be pure: `f(x) == f(x)` (no side-effect variance)
- Arithmetic BIFs: commutativity, associativity where applicable

---

### Phase P5 — Heap/GC Invariants (2-3 days)

**Crate**: `nera-runtime`

Complement to Phase 5 fuzzing. Proptest generates operation sequences and validates structural invariants after every GC cycle.

**Properties verified**:
- After minor GC: all roots are valid terms (correct tags, traversable cons chains)
- After major GC: young gen is empty, all live data is in old gen
- Heap size never exceeds configured limit
- No forwarding pointers leak past GC completion

Proptest's shrinking is especially valuable here — it reduces a 200-step alloc/collect sequence to the minimal ~5-step reproducer.

---

## Summary

| Phase | Target | Approach | Effort | Value |
|-------|--------|----------|--------|-------|
| 0 | Scaffolding | — | 0.5d | Enables all |
| 1 | Parser + Lexer | Fuzzing | 1-2d | Very high |
| 2 | Formatter | Fuzzing | 2-3d | High |
| 3 | Preprocessor | Fuzzing | 2-3d | High |
| 4 | BIFs | Fuzzing | 3-5d | Very high |
| 5 | Heap/GC | Fuzzing | 5-7d | Critical |
| 6 | IR lowering | Fuzzing | 3-5d | High |
| 7 | CI | — | 1-2d | High |
| 8-9 | Codegen + Diff | Fuzzing | weeks | Medium → Very high |
| **P1** | **Term round-trip** | **Proptest** | **0.5d** | **Very high** |
| **P2** | **Formatter semantics** | **Proptest** | **1-2d** | **High** |
| **P3** | **Pattern match equiv.** | **Proptest** | **3-5d** | **Critical** |
| **P4** | **BIF safety** | **Proptest** | **2-3d** | **Very high** |
| **P5** | **Heap/GC invariants** | **Proptest** | **2-3d** | **High** |

| Phase | Target | Effort | Value |
|-------|--------|--------|-------|
| 0 | Scaffolding | 0.5d | Enables all |
| 1 | Parser + Lexer | 1-2d | Very high |
| 2 | Formatter | 2-3d | High |
| 3 | Preprocessor | 2-3d | High |
| 4 | BIFs | 3-5d | Very high |
| 5 | Heap/GC | 5-7d | Critical |
| 6 | IR lowering | 3-5d | High |
| 7 | CI | 1-2d | High |
| 8-9 | Codegen + Diff | weeks | Medium → Very high |

**Recommended order**: 0 → 1 → **P1** → 2 → **P2** → 3 → 4/P4 → 7 → 5/P5 → **P3** → 6 → 8 → 9

Phase P1 (term round-trip) should be done immediately after Phase 1 — it takes 30 minutes and validates the most critical unsafe code. P2 pairs naturally with Phase 2 (formatter). Phase P3 (pattern match equivalence) can start whenever work on the pattern compiler is active.

Fuzzing phases 0 through 3 + P1 + CI can be completed in ~one focused week.

## Key Files

| File | Relevance |
|------|-----------|
| `crates/nera-syntax/src/parser.rs` | Phase 1 — `parse_module()` entry |
| `crates/nera-syntax/src/lexer.rs` | Phase 1b — `lex_all()` |
| `crates/nera-syntax/src/preproc.rs` | Phase 3 — `process()` |
| `crates/nera-fmt/src/lib.rs` | Phase 2 — `format_source()` |
| `crates/nera-runtime/src/bif.rs` | Phase 4 — 85 unsafe blocks |
| `crates/nera-runtime/src/heap.rs` | Phase 5 — 74 unsafe blocks, GC |
| `crates/nera-runtime/src/term.rs` | Phase 4-5 — `ErlTerm(u64)` tagged pointer |
| `crates/nera-ir/src/lower/mod.rs` | Phase 6 — `lower_module()` |
| `crates/nera-codegen/src/compiler.rs` | Phase 8 — `compile_module()` |
| `.github/workflows/ci.yml` | Phase 7 — reference for fuzz workflow |
