# Recursive bit matching — 2026-10-13

## Cause and correction

A reused match context holds an absolute bit offset. Both match-start variants
return a pointer to its containing byte (`data + offset / 8`) and the correct
remaining bit count. Integer extraction restarted at bit zero relative to that
pointer, losing `offset % 8`. Thus summing `<<1:1,0:1,1:1,1:1>>` returned 4 instead
of BEAM's 3.

`BsMatchBitOffset` captures the missing 0..7 bits as a tagged integer. Both pattern
lowering paths add it to raw integer extraction offsets. Logical remaining sizes,
materialization offsets and context advances stay relative to the match start;
adding the prefix there would count it twice. The instruction does not allocate
or copy binary data, and works with both retained and elided match contexts.
This change concerns integer segments, including recursive one-bit traversal;
it does not change UTF, float or sub-bitstring materialization implementations.

## Regression and reproduction

`tests/binary_recursive_bits.erl` contains assertions shared by BEAM and Nera:

- the original four-bit counterexample;
- all 256 byte values at each of eight starting offsets (2,048 combinations),
  with repeated one-bit extraction across byte boundaries;
- terminal 8/16-bit readers, signed 8-bit and little-endian 16-bit readers;
- a case-expression reader as well as function-clause patterns;
- a 65,536-bit traversal over a refcounted backing binary.

The test is automatically included in `nera test`. For an explicit comparison:

```sh
python3 docs/baselines/2026-10-13-bit-offset/compare.py
```

The script compiles the same source with BEAM and Nera in a temporary directory,
checks successful completion of every assertion, and requires identical `ok`
output. It accepts `--nera /absolute/path/to/nera`.

`before.log` records the failing minimal version of the regression before the
fix (`badmatch,4`); `beam.log` records its successful BEAM execution. The final
regression expands that initial test. `comparison.log` records the final shared
assertion corpus. Platform: macOS arm64, OTP 29 / ERTS 17.0.5.

## Scope and custody

Target: working tree on `main`, base `9e2ddcebaf725e6ea649ffcaafa98ccc682986cc`.
The base is one commit ahead of `origin/main`, zero behind. The earlier allocation
fix and Base64 prototype are retained. No commit, push or PR was requested or
performed. Final source and artifact hashes and checks are recorded in
`preflight.json`. Historical allocation evidence remains unchanged.

Validation is scoped to compiler tests, binary/Base64 regressions, the allocation
invariants, Clippy and a release build. The complete workspace/Erlang suites and
Linux/Windows/cross-target checks were not rerun for this change.
