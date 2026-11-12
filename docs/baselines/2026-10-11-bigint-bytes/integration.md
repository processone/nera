# Publication validation — 2026-10-12

Rebased onto main `f84c0f5bb35984ccdd307a2b7ff191e70efc60f6` after PR #28.
Both changelog entries were retained. The runtime diff still only removes
the intermediate limb vector; the new unsafe conversion contract is preserved.

Checks rerun on the integrated sources:

- Runtime: 478 unit tests and 11 doctests passed, one test ignored.
- Runtime/all-targets Clippy with warnings denied passed.
- Release runtime build passed.
- Seven native Erlang scenarios passed: bigint_arith, bigint_bitwise,
  bigint_factorial, bigint_format, ets_counter_integer_domain,
  ets_counter_atomicity and ets_update_counter.

The original README, results and validation.json are historical evidence from
before integration (base 9b64f62). Performance was not remeasured after rebase.
The recorded CPU improvements of 3.79% and 4.05% apply to those earlier
before/after artifacts, on a shared, non-idle machine. No full workspace or
full Erlang suite, cross-platform execution, Miri or sanitizers were run.
