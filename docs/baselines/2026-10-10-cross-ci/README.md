# Cross-compilation CI isolation — 2026-10-10

The previous PR run (38074383034) failed both cross-compilation smoke tests:
Hello World was compiled from the repository root, so CLI discovery also loaded
297 fixture files, producing thirteen unresolved-call errors.

Both matrix targets now use `scripts/test-cross.sh`. The script resolves the
compiler path before changing directory, creates and cleans up a private
temporary directory, compiles only the smoke program and standard library, and
requires a nonempty `hello.o` with the expected target object format. The old
fallback to inspecting an unrelated `lists.o` is removed.

Local controls using the validated Rust 1.99.0 / Cranelift 0.129.2 compiler:

- `x86_64-apple-darwin`: pass; Mach-O 64-bit x86_64.
- `aarch64-unknown-linux-gnu`: pass; ELF 64-bit ARM aarch64.
- Workflow validation with `actionlint .github/workflows/ci.yml`: pass.
- Shell syntax and Git whitespace checks: pass.

These are object-generation tests, not execution on either target. The compiler
source and dependencies are unchanged from the [deprecation baseline](../2026-10-10-deprecation/README.md):
1,686 Rust passes, 285 Erlang passes and two skips, with installed archive checks
passing. No repeated full local suite is needed for this workflow-only follow-up.
`results.json` records the commands and observed formats; `source-manifest.json`
records the script and workflow hashes. Remote CI is checked after publication
and reported in the PR conversation; these results are the pre-push local checks.
