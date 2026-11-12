"""Unit tests for the CI change map (scripts/ci/changes.py).

Intent: the captain asked CI to check only what a change touches. The file lists
below are the real changed files of Nera's first three pull requests.
"""
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import changes  # noqa: E402

PR1 = [
    ".github/workflows/ci.yml",
    "README.md",
    "docs/KNOWN_LIMITS.md",
    "docs/PROJECT_STATUS.md",
    "docs/RELEASE.md",
]
PR2 = [
    "COMPATIBILITY.md",
    "benchmarks/cowlib_bench.erl",
    "benchmarks/cowlib_bench.sh",
    "docs/KNOWN_LIMITS.md",
    "examples/cowlib_demo/README.md",
    "scripts/test-cowlib-demo.py",
]
PR3 = [
    ".github/workflows/release.yml",
    "docs/KNOWN_LIMITS.md",
    "docs/RELEASE.md",
    "editors/jetbrains-nera/build.gradle.kts",
    "editors/vscode-nera/package.json",
]


def selected(files, event="pull_request"):
    return {job for job, on in changes.plan(files, event).items() if on}


class ChangeMapTest(unittest.TestCase):
    def test_pr1_changes_ci_so_everything_runs(self):
        self.assertEqual(selected(PR1), set(changes.JOBS))

    def test_pr2_has_a_file_no_rule_names_so_everything_runs(self):
        self.assertEqual(selected(PR2), set(changes.JOBS))
        # Without that script the rest is documentation, benchmarks and examples.
        self.assertEqual(selected([f for f in PR2 if f != "scripts/test-cowlib-demo.py"]), set())

    def test_pr3_runs_only_the_plugin_and_packaging_jobs(self):
        self.assertEqual(selected(PR3), {"vscode", "jetbrains", "packaging"})

    def test_docs_only_starts_no_job(self):
        self.assertEqual(selected(["docs/X.md", "README.md", "tests/README.md",
                                   "editors/emacs-nera/nera.el", "demo/a.erl"]), set())

    def test_formatter_crate_runs_only_rust_checks(self):
        self.assertEqual(selected(["crates/nera-fmt/src/lib.rs"]), {"rust"})

    def test_other_crates_run_compiler_jobs(self):
        self.assertEqual(selected(["crates/nera-runtime/src/lib.rs"]),
                         {"rust", "build", "e2e", "cowlib", "thoas", "poolboy", "cross"})
        self.assertEqual(selected(["Cargo.lock"]), selected(["erlang/stdlib/lists.erl"]))

    def test_test_sources_run_rust_build_and_e2e(self):
        want = {"rust", "build", "e2e"}
        self.assertEqual(selected(["tests/guard_basic.erl"]), want)
        self.assertEqual(selected(["tests/include/helpers.hrl"]), want)

    def test_each_integration_suite_runs_with_the_build(self):
        self.assertEqual(selected(["tests/integration/cowlib/runner.py"]), {"build", "cowlib"})
        self.assertEqual(selected(["tests/integration/thoas/differential/runner.py"]), {"build", "thoas"})
        self.assertEqual(selected(["tests/integration/poolboy/nera.toml"]), {"build", "poolboy"})

    def test_cross_script(self):
        self.assertEqual(selected(["scripts/test-cross.sh"]), {"build", "cross"})

    def test_each_plugin_runs_alone(self):
        self.assertEqual(selected(["editors/vscode-nera/src/extension.ts"]), {"vscode"})
        self.assertEqual(selected(["editors/jetbrains-nera/build.gradle.kts"]), {"jetbrains"})

    def test_packaging_files(self):
        for path in ("scripts/dist.sh", "scripts/test-dist.py", "scripts/install-local.sh"):
            self.assertEqual(selected([path]), {"vscode", "jetbrains", "packaging"}, path)

    def test_unnamed_file_and_change_map_run_everything(self):
        for path in ("scripts/new-thing.py", ".gitignore", "scripts/ci/changes.py"):
            self.assertEqual(selected([path]), set(changes.JOBS), path)

    def test_push_and_manual_start_run_everything(self):
        for event in ("push", "workflow_dispatch"):
            self.assertEqual(selected([], event), set(changes.JOBS))

    def test_union_of_several_files(self):
        self.assertEqual(selected(["crates/nera-fmt/src/lib.rs", "editors/vscode-nera/package.json"]),
                         {"rust", "vscode"})


if __name__ == "__main__":
    unittest.main()
