# Publication check — 2026-10-11

The preserved source hashes in `results.json` were verified before publication:
all three matched, including the original report. The focused command
`./target/release/nera test initial_process_keepalive -j 1 --timeout 30`
passed again with the required success marker.

The baseline and mutation evidence in the original report apply to base
`d0420d8800d78e641d592fe15e6b482881a8ddb9`. They remain historical observations;
the full suite was not repeated for publication. At publication, `origin/main`
was `fc3e63f` (eight commits ahead of that base). No runtime logic or test source
was changed during publication preparation.

The TCP failure recorded there was subsequently diagnosed as a lost concurrent
accept request and fixed separately in local commit `81cb1fc` on
`mr/tcp-rearm-timeout`. That fix and its benchmark are not included in this PR.
The earlier failing baseline must not be described as green on this branch.

Scope: the strengthened keepalive fixture, the scheduler comment, and this
validation documentation. Publication status and CI results are reported in the
PR; the original report's uncommitted status describes when its evidence was
collected.
