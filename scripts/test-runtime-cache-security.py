#!/usr/bin/env python3
"""NERA-61: shared-temp entries must not poison or obstruct standalone builds.

Run on Unix with a compiler and its matching native runtime, built runtime first:
    python3 scripts/test-runtime-cache-security.py --compiler /path/to/nera \
        --runtime /path/to/runtime.a --other-runtime /path/to/different-runtime.a

Uses real archives and the system linker outside the checkout. The attacker
fixtures are created by this process, not a second account; ownership rejection
is also covered by the Rust tests. All files are disposable.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("compiler", "runtime", "other-runtime"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    parser.add_argument("--temp-parent", type=Path)
    args = parser.parse_args()
    if os.name != "posix":
        parser.error("this regression exercises Unix ownership and permissions")
    expected = digest(args.runtime)
    assert digest(args.other_runtime) != expected, "the poison archive must differ"

    with tempfile.TemporaryDirectory(prefix="nera cache security ", dir=args.temp_parent) as temporary:
        root = Path(temporary).resolve()
        install = root / "standalone" / "bin"
        install.mkdir(parents=True)
        compiler = install / "nera"
        shutil.copy2(args.compiler.resolve(), compiler)
        work = root / "work"
        work.mkdir()
        source = work / "cache_security.erl"
        source.write_text('''-module(cache_security).
-export([start/0]).
start() ->
    [3,2,1] = lists:reverse([1,2,3]),
    io:format("cache security ok~n").
''')
        cache = root / "shared-temp"
        cache.mkdir()
        cache.chmod(0o1777)
        home = root / "home"
        home.mkdir(mode=0o700)
        env = dict(os.environ, TMPDIR=str(cache), TMP=str(cache), TEMP=str(cache), HOME=str(home))
        # Observe the exact archive at the linker boundary, including disposable
        # extractions which the compiler removes when linking finishes.
        linker = root / "observe-linker"
        observation = root / "observation.json"
        linker.write_text(f'''#!{sys.executable}
import hashlib, json, os, pathlib, stat, sys
archive = next(pathlib.Path(arg) for arg in sys.argv[1:]
               if pathlib.Path(arg).name.startswith("libnera_runtime-"))
with archive.open("rb") as stream:
    digest = hashlib.file_digest(stream, "sha256").hexdigest()
pathlib.Path({str(observation)!r}).write_text(json.dumps(dict(
    path=str(archive), digest=digest, symlink=archive.is_symlink(),
    uid=archive.stat().st_uid, mode=stat.S_IMODE(archive.stat().st_mode),
    parent_uid=archive.parent.stat().st_uid,
    parent_mode=stat.S_IMODE(archive.parent.stat().st_mode))))
if os.environ.get("NERA_CACHE_FORCE_LINK_FAILURE"):
    sys.exit(1)
os.execv({shutil.which("cc")!r}, [{shutil.which("cc")!r}] + sys.argv[1:])
''')
        linker.chmod(0o700)

        def build(label, check_private=True, fail_link=False, rejected_ancestor=None):
            executable = work / label
            observation.unlink(missing_ok=True)
            build_env = dict(env, NERA_CACHE_FORCE_LINK_FAILURE="1" if fail_link else "")
            result = subprocess.run(
                [compiler, "build", source, "-e", "cache_security:start", "-o", executable,
                 "--linker", linker, "-v"],
                cwd=work, env=build_env, capture_output=True, text=True, timeout=120)
            if rejected_ancestor is not None:
                if observation.exists():
                    print("UNEXPECTED_LINKER_OBSERVATION " + json.dumps({
                        "label": label, **json.loads(observation.read_text())}), flush=True)
                assert result.returncode != 0, (label, "accepted replaceable HOME ancestry", result.stderr)
                assert "Using embedded runtime library: " not in result.stderr, result.stderr
                assert "Linking with runtime: " not in result.stderr, result.stderr
                assert f"HOME ancestor {rejected_ancestor}" in result.stderr, result.stderr
                assert not observation.exists(), "unsafe archive reached the linker"
                diagnostic = next(line for line in result.stderr.splitlines() if "HOME ancestor" in line)
                print(f"{label}: refused before archive linkage; {diagnostic}", flush=True)
                return None
            assert (result.returncode != 0 if fail_link else result.returncode == 0), (
                label, result.stdout, result.stderr)
            prefix = "Using embedded runtime library: "
            paths = [Path(line.removeprefix(prefix)) for line in result.stderr.splitlines()
                     if line.startswith(prefix)]
            assert len(paths) == 1, result.stderr
            archive = paths[0]
            assert f"Linking with runtime: {archive.resolve()}" in result.stderr, result.stderr
            observed = json.loads(observation.read_text())
            assert observed["path"] == str(archive.resolve()), observed
            assert observed["digest"] == expected, (label, "linked another runtime", observed)
            if check_private:
                assert observed["parent_uid"] == os.geteuid(), (label, "foreign cache owner")
                assert observed["parent_mode"] == 0o700, (label, "cache is not private")
                assert not observed["symlink"], (label, "linked a symlink")
                assert observed["uid"] == os.geteuid(), (label, "foreign archive owner")
                assert observed["mode"] == 0o600, (label, "archive is not private")
            if fail_link:
                assert "linking failed" in result.stderr, result.stderr
                print(f"{label}: expected linker failure; private archive observed before cleanup", flush=True)
                return archive
            run = subprocess.run([executable], cwd=work, env=env, check=True,
                                 capture_output=True, text=True, timeout=30)
            assert run.stdout == "cache security ok\n", run
            print(f"{label}: {run.stdout.strip()}; correct runtime sha256={expected}", flush=True)
            return archive

        # Learn the public content key from the real compiler, not a copied hash algorithm.
        archive_name = build("discover", check_private=False).name
        user_cache = cache / f"nera-runtime-{os.geteuid()}"

        def reset():
            shutil.rmtree(cache)
            cache.mkdir()
            cache.chmod(0o1777)

        def plant(path, kind):
            if kind == "directory":
                path.mkdir(mode=0o777)
                path.chmod(0o777)
            elif kind == "dangling-symlink":
                path.symlink_to(root / "absent")
            elif kind == "symlink":
                path.symlink_to(args.other_runtime.resolve())
            else:
                shutil.copy2(args.other_runtime, path)
                path.chmod(0o000 if kind == "unreadable" else 0o666)

        # An entry at the old public archive name is neither read nor replaced.
        for kind in ("archive", "unreadable", "directory", "dangling-symlink", "symlink"):
            reset()
            hostile = cache / archive_name
            plant(hostile, kind)
            before = hostile.lstat()
            extracted = build(f"public-{kind}")
            assert extracted != hostile
            after = hostile.lstat()
            assert (after.st_ino, after.st_mode, after.st_mtime_ns) == (
                before.st_ino, before.st_mode, before.st_mtime_ns), "changed the attacker's entry"

        # Preclaiming the predictable per-user directory must not deny builds.
        for kind in ("archive", "unreadable", "directory", "dangling-symlink", "symlink"):
            reset()
            plant(user_cache, kind)
            before = user_cache.lstat()
            extracted = build(f"blocked-user-cache-{kind}")
            assert extracted.parent != user_cache
            assert not extracted.parent.exists(), "disposable extraction was not cleaned after linking"
            after = user_cache.lstat()
            assert (after.st_ino, after.st_mode, after.st_mtime_ns) == (
                before.st_ino, before.st_mode, before.st_mtime_ns), "changed the attacker's entry"

        # Even unexpected unsafe entries inside a private cache are not trusted.
        for kind in ("archive", "directory", "dangling-symlink", "symlink"):
            reset()
            user_cache.mkdir(mode=0o700)
            plant(user_cache / archive_name, kind)
            extracted = build(f"unsafe-private-entry-{kind}")
            assert extracted.parent != user_cache
            assert not extracted.parent.exists(), "disposable extraction was not cleaned after linking"
        reset()
        plant(user_cache, "directory")
        extracted = build("failed-link-cleanup", fail_link=True)
        assert not extracted.parent.exists(), "failed link leaked disposable extraction"
        # Captain's fallback-home choice: refuse an unsafe TMPDIR even if its
        # planted per-user cache appears private, and keep builds working at HOME.
        reset()
        cache.chmod(0o777)
        user_cache.mkdir(mode=0o700)
        poisoned = user_cache / archive_name
        shutil.copy2(args.other_runtime, poisoned)
        poisoned.chmod(0o600)
        home_cache = home / ".nera-runtime"
        extracted = build("unsafe-temp-home")
        assert extracted.parent == home_cache, "did not use the private home cache"
        assert digest(poisoned) == digest(args.other_runtime), "changed the unsafe temp cache"
        before = extracted.stat()
        reused = build("unsafe-temp-home-reuse")
        assert reused == extracted and reused.stat().st_mtime_ns == before.st_mtime_ns

        # An unexpected archive must also stay at HOME for disposable extraction,
        # rather than returning to the original unsafe TMPDIR.
        extracted.unlink()
        plant(home_cache / archive_name, "symlink")
        extracted = build("unsafe-home-archive")
        assert extracted.parent != home_cache and extracted.parent.parent == home
        assert not extracted.parent.exists(), "home archive fallback was not cleaned"
        assert (home_cache / archive_name).is_symlink(), "changed the unsafe home entry"

        for kind in ("archive", "unreadable", "directory", "dangling-symlink", "symlink"):
            if home_cache.is_dir() and not home_cache.is_symlink():
                shutil.rmtree(home_cache)
            else:
                home_cache.unlink()
            plant(home_cache, kind)
            before = home_cache.lstat()
            extracted = build(f"blocked-home-cache-{kind}")
            assert extracted.parent != home_cache and extracted.parent.parent == home
            assert not extracted.parent.exists(), "home cache fallback was not cleaned"
            after = home_cache.lstat()
            assert (after.st_ino, after.st_mode, after.st_mtime_ns) == (
                before.st_ino, before.st_mode, before.st_mtime_ns), "changed the obstructed home cache"
        extracted = build("failed-home-link-cleanup", fail_link=True)
        assert not extracted.parent.exists(), "failed home link leaked disposable extraction"
        # NERA-116 N1: private HOME/cache modes cannot protect a replaceable
        # ancestor. The current-account poison would pass the archive-only guard.
        for mode in (0o775, 0o777):
            shared = root / f"home-ancestor-{mode:o}"
            shared.mkdir()
            shared.chmod(mode)
            unsafe_home = shared / "alice"
            unsafe_home.mkdir(mode=0o700)
            unsafe_home_cache = unsafe_home / ".nera-runtime"
            unsafe_home_cache.mkdir(mode=0o700)
            poison = unsafe_home_cache / archive_name
            shutil.copy2(args.other_runtime, poison)
            poison.chmod(0o600)
            before = poison.stat()
            env["HOME"] = str(unsafe_home)
            build(f"unsafe-home-ancestor-{mode:o}", rejected_ancestor=shared)
            after = poison.stat()
            assert digest(poison) == digest(args.other_runtime), "changed the rejected home archive"
            assert (after.st_ino, after.st_mode, after.st_mtime_ns) == (
                before.st_ino, before.st_mode, before.st_mtime_ns)

            # A sticky shared parent protects the user's HOME entry; use a new
            # owned home so the control does not reuse the deliberately poisoned cache.
            shared.chmod(mode | 0o1000)
            safe_home = shared / "safe-alice"
            safe_home.mkdir(mode=0o700)
            env["HOME"] = str(safe_home)
            extracted = build(f"sticky-home-ancestor-{mode:o}")
            assert extracted.parent == safe_home / ".nera-runtime"
        print("Shared cache attacks, protected home ancestry/fallback and success/error cleanup passed.", flush=True)


if __name__ == "__main__":
    main()
