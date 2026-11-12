#!/usr/bin/env python3
"""Map the files a change touches to the CI jobs that must run.

CI's first job runs this script. The rules live here, in one place, and the
unit test next to it (test_changes.py) feeds them real changed-file lists.

    changes.py --event pull_request < changed-files.txt
    changes.py --event push          # no file list needed

Output: one `name=true|false` line per job, written to $GITHUB_OUTPUT when set
and to stdout otherwise.

Rules, first match wins:
  * Documentation and sample trees start no job, nor do the release target
    file and its workflow.
  * A file that no rule names starts every job, so a new kind of file is
    never silently skipped.
  * Pushes to main and manual starts run every job (see `--event`).
"""
import argparse
import os
import sys

JOBS = (
    "rust",
    "build",
    "e2e",
    "cowlib",
    "thoas",
    "poolboy",
    "cross",
    "vscode",
    "jetbrains",
    "packaging",
)

ALL = frozenset(JOBS)
NONE = frozenset()
# Jobs that run the compiler built by the shared build job.
COMPILER_JOBS = frozenset({"e2e", "cowlib", "thoas", "poolboy", "cross"})

RUST = frozenset({"rust"})
COMPILER = frozenset({"rust", "build", "e2e", "cowlib", "thoas", "poolboy", "cross"})
TEST_SOURCES = frozenset({"rust", "build", "e2e"})
PACKAGING = frozenset({"vscode", "jetbrains", "packaging"})

DOC_PREFIXES = ("docs/", "benchmarks/", "examples/", "demo/", "editors/emacs-nera/")

# (path prefix, jobs). Checked in order after the special cases in `jobs_for`.
PREFIX_RULES = (
    ("crates/nera-fmt/", RUST),
    ("crates/", COMPILER),
    ("erlang/", COMPILER),
    ("tests/include/", TEST_SOURCES),
    ("tests/integration/cowlib/", frozenset({"build", "cowlib"})),
    ("tests/integration/thoas/", frozenset({"build", "thoas"})),
    ("tests/integration/poolboy/", frozenset({"build", "poolboy"})),
    ("editors/vscode-nera/", frozenset({"vscode"})),
    ("editors/jetbrains-nera/", frozenset({"jetbrains"})),
)

EXACT_RULES = {
    "Cargo.toml": COMPILER,
    "Cargo.lock": COMPILER,
    "rust-toolchain.toml": COMPILER,
    "scripts/test-cross.sh": frozenset({"build", "cross"}),
    ".github/workflows/release.yml": PACKAGING,
    # Release milestone: runs from main on its own; the target file is its input.
    ".github/workflows/release-milestone.yml": NONE,
    ".github/release-target.json": NONE,
    "scripts/dist.sh": PACKAGING,
    "scripts/test-dist.py": PACKAGING,
    "scripts/install-local.sh": PACKAGING,
}


def jobs_for(path: str) -> frozenset:
    """Jobs one changed file starts. Unnamed files start everything."""
    if path.startswith(DOC_PREFIXES) or path.endswith(".md"):
        return NONE
    # The change map itself and the workflow that runs it change every job.
    if path == ".github/workflows/ci.yml" or path.startswith("scripts/ci/"):
        return ALL
    for prefix, jobs in PREFIX_RULES:
        if path.startswith(prefix):
            return jobs
    if path in EXACT_RULES:
        return EXACT_RULES[path]
    # tests/<name>.erl: the end-to-end tests; some Rust unit tests include_str! them.
    if path.startswith("tests/") and path.endswith(".erl") and "/integration/" not in path:
        return TEST_SOURCES
    return ALL


def plan(files, event: str = "pull_request") -> dict:
    """Return {job: bool} for a changed-file list and the triggering event."""
    if event != "pull_request":
        selected = set(ALL)
    else:
        selected = set()
        for path in files:
            selected |= jobs_for(path)
        # Everything that runs the compiler needs it built first.
        if selected & COMPILER_JOBS:
            selected.add("build")
    return {job: job in selected for job in JOBS}


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--event", default="pull_request",
                        help="GitHub event name; anything but pull_request runs every job")
    args = parser.parse_args(argv)
    files = []
    if args.event == "pull_request":
        files = [line.strip() for line in sys.stdin if line.strip()]
        if not files:
            # Nothing listed (empty diff or failed listing): do not skip checks.
            files = ["<unknown>"]
    result = plan(files, args.event)
    lines = [f"{job}={'true' if on else 'false'}" for job, on in result.items()]
    out = os.environ.get("GITHUB_OUTPUT")
    if out:
        with open(out, "a", encoding="utf-8") as handle:
            handle.write("\n".join(lines) + "\n")
    print("\n".join(lines))
    return 0


if __name__ == "__main__":
    sys.exit(main())
