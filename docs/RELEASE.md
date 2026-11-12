# Release Process

## Overview

**Experimental releases only (2026-11-12).** Read
[PROJECT_STATUS.md](PROJECT_STATUS.md) and [KNOWN_LIMITS.md](KNOWN_LIMITS.md)
before tagging. The first release is `v0.1.0`, published as a full GitHub
release (the tag has no suffix, so the workflow does not mark it as a
prerelease). It stays experimental in the docs until 1.0. Later prereleases
with a suffix, such as `v0.2.0-experimental`, are published as GitHub
prereleases. The packaging repair adds native installed-
archive smoke tests, but the tag workflow still does not run the full test
suite. Packaging success is not release validation.

**CI and Release are active.** The private CI pause (2026-10-12) ended when
the repository went public. Pull requests run CI, and every `v*` tag runs the
Release workflow: binary builds for 4 platforms (no Windows yet), IDE plugin
packaging, and GitHub Release creation.

## Resuming Workflows After Going Public

Changing visibility does not re-enable manually disabled workflows. Once Nera
is open source:

1. Confirm the repository is public, review current GitHub runner/storage
   pricing and the expected checks/protections. Standard hosted runners are
   free for public repositories under current pricing, not a guarantee of a
   zero overall bill. If required checks conflict with the pause, request a
   specific decision without changing or bypassing protections.
2. On GitHub, open **Actions > CI > Enable workflow** and
   **Actions > Release > Enable workflow**. Confirm both are `active` using
   the workflow list or GitHub API (`GET /repos/processone/nera/actions/workflows`).
3. On a real PR targeting `main` or `mr/**`, verify a new CI run and all four
   expected tasks: Linux x86_64, macOS arm64, Cross-compile x86_64-apple-darwin,
   and Cross-compile aarch64-unknown-linux-gnu. Confirm their results on the
   proposed revision; investigate failures instead of treating activation as
   validation.

Events during the pause are not automatically replayed. Do not push a test tag
to verify Release: every `v*` tag, including a release-candidate tag, can publish
a real release once enabled. Verify its active state without a publication.
Reactivation, visibility changes and publication are separate future actions;
this pause does not authorize them.

## Release target

`.github/release-target.json` names the release being prepared, for example
`{"milestone":"0.1.1"}`. It is the only place that version is written for this
purpose. When a pull request opens, the **Release milestone** workflow
(`.github/workflows/release-milestone.yml`, running
`scripts/ci/assign_release_milestone.py`) puts it in the open GitHub milestone
with that exact title. Milestone titles are bare versions (`0.1.1`); tags keep
the `v` (`v0.1.1`).

Rules:

- The workflow runs from `main` on `pull_request_target` and never checks out
  the pull request's code. It covers drafts, every base branch and forks.
- It never creates a milestone. A target that is missing, closed, held by two
  open milestones or unreadable fails the run. Create the milestone first.
- A pull request that already has a milestone keeps it.
- It acts only when a pull request opens. Pull requests opened earlier are not
  moved: one still open when its release ships is moved by hand.
- Editing the target file or the workflow starts no CI job.

## How to Create a Release

0. Create the milestone for the version after this one (for example `0.1.2`
   when releasing `0.1.1`), or reuse an open one. Leave it empty.

1. Update version numbers in:
   - Root `Cargo.toml` (`[workspace.package].version`; the CLI inherits it)
   - `editors/vscode-nera/package.json`
   - `editors/jetbrains-nera/build.gradle.kts`
   - `.github/release-target.json`: set it to the next milestone from step 0,
     so pull requests opened after this commit go to the next release

2. Commit the version bump and the new target together:
   ```bash
   git add -A
   git commit -m "Bump version to 0.2.0"
   ```

3. Tag and push:
   ```bash
   git tag v0.2.0
   git push && git push origin v0.2.0
   ```

4. The release workflow runs automatically. Monitor progress at **Actions > Release** on GitHub.

5. Once complete, a GitHub Release appears with all artifacts attached.

6. Close the shipped milestone (`0.2.0`) on GitHub, and move any pull request
   still open in it to the new target.

## Release Artifacts

| Artifact | Description |
|----------|-------------|
| `nera-<ver>-aarch64-apple-darwin.tar.gz` | macOS Apple Silicon binary |
| `nera-<ver>-x86_64-apple-darwin.tar.gz` | macOS Intel binary |
| `nera-<ver>-x86_64-unknown-linux-gnu.tar.gz` | Linux x86_64 binary |
| `nera-<ver>-aarch64-unknown-linux-gnu.tar.gz` | Linux ARM64 binary |
| `vscode-nera-<ver>.vsix` | VSCode extension |
| `jetbrains-nera-<ver>.zip` | JetBrains plugin (RustRover, IntelliJ, etc.) |

## Workflow Structure

The workflow (`.github/workflows/release.yml`) has 4 jobs:

### 1. `build` (matrix, 5 targets)

Builds nera binaries using `scripts/dist.sh`. Each target produces a tarball containing:
- `bin/nera` — the compiler binary
- `lib/nera/erlang/stdlib/` — standard library sources
- `lib/libnera_runtime.a` — runtime library (`lib/nera_runtime.lib` on MSVC)

The runtime is built before the compiler so it is also embedded in the binary.
External lookup takes precedence; the embedded copy is the fallback. Both the
compiler and runtime in a target-labelled archive are built for that target.
Cross-compilation requires the corresponding Rust target and system linker.
Native macOS/Linux release jobs compile and run an unpacked archive, then remove
its external library and repeat to check embedding. Cross-target execution still
needs equivalent verification. The Windows x86_64 target is not built or released
yet: the release build does not link on Windows.

### 2. `plugin-vscode`

Builds the VSCode extension via `npm install`, `npm run compile`, and `vsce package`.

### 3. `plugin-jetbrains`

Builds the JetBrains plugin with Gradle. In CI, the build uses a downloaded IntelliJ IDEA instead of a local RustRover installation (controlled by the `CI` environment variable in `build.gradle.kts`).

### 4. `release`

Waits for all other jobs, downloads artifacts, and creates the GitHub Release using `softprops/action-gh-release`, as a prerelease when the tag contains a hyphen.

## Prereleases

The workflow marks a GitHub Release as a **prerelease** when the tag has a
suffix after the version number, that is, when it contains a hyphen:
`v0.2.0-experimental`, `v0.2.0-rc1`. A tag such as `v0.2.0` publishes a full
release. The tag name is also the version in archive names, so
`v0.2.0-experimental` produces `nera-0.2.0-experimental-<target>.tar.gz`.

A tag is **not a dry run**: it publishes. Validate locally and meet the gates
in [PROJECT_STATUS.md](PROJECT_STATUS.md) before pushing one:

```bash
git tag v0.2.0-experimental
git push origin v0.2.0-experimental
```

Deleting a release later does not make the action a non-publishing test.

## Local Distribution Build

To build a distribution tarball locally without the workflow:

```bash
# Build for current platform
./scripts/dist.sh --version 0.2.0

# Cross-compile for a different target
./scripts/dist.sh --version 0.2.0 --target aarch64-unknown-linux-gnu
```

Output is placed in the `dist/` directory.
