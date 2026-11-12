#!/usr/bin/env python3
"""A fresh locked release must build in a working copy with spaces.

Run when changing build dependencies or build paths:
    python3 scripts/test-build-path.py [--temp-parent /external/scratch]

Copies the tracked working tree (including local edits), without build output,
into disposable storage. Cargo's normal target directory must remain there;
redirecting it to a space-free directory would hide the original jemalloc bug.
Requires Git, the pinned Rust toolchain, its dependencies, and a C linker.
"""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--temp-parent", type=Path)
    args = parser.parse_args()
    source = Path(__file__).resolve().parent.parent
    files = subprocess.check_output(["git", "ls-files", "-z"], cwd=source)
    env = dict(os.environ)
    for key in list(env):
        if key in {"CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR", "CARGO_BUILD_BUILD_DIR"} or key.endswith("JEMALLOC_OVERRIDE"):
            env.pop(key)

    with tempfile.TemporaryDirectory(prefix="nera build path ", dir=args.temp_parent) as temporary:
        checkout = Path(temporary).resolve() / "working copy"
        checkout.mkdir()
        for name in files.decode().split("\0"):
            if not name:
                continue
            original = source / name
            if not original.exists() and not original.is_symlink():
                continue  # A tracked local deletion is part of this snapshot.
            destination = checkout / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(original, destination, follow_symlinks=False)

        target = checkout / "target"
        assert " " in str(checkout) and not target.exists(), checkout
        lock = (checkout / "Cargo.lock").read_bytes()
        print(f"Fresh working copy: {checkout}", flush=True)
        subprocess.run(["cargo", "build", "--release", "--locked"],
                       cwd=checkout, env=env, check=True)
        assert (checkout / "Cargo.lock").read_bytes() == lock, "lockfile changed"
        binary = target / "release" / ("nera.exe" if os.name == "nt" else "nera")
        runtime = target / "release" / ("nera_runtime.lib" if os.name == "nt" else "libnera_runtime.a")
        assert binary.is_file() and runtime.is_file(), "missing default-target artifacts"
        # Final artifacts can stay here even when user Cargo config redirects
        # build.build-dir (and therefore jemalloc's configure/OUT_DIR) elsewhere.
        # MSVC does not depend on jemalloc.
        target_cfg = subprocess.check_output(
            ["rustc", "--print", "cfg"], cwd=checkout, env=env, text=True)

        if 'target_env="msvc"' not in target_cfg.splitlines():
            assert any(
                out.is_dir() and out.resolve().is_relative_to(checkout)
                for out in (target / "release" / "build").glob("tikv-jemalloc-sys-*/out")
            ), "jemalloc OUT_DIR is missing from the spaced working copy; check build.build-dir"

        work = Path(temporary) / "smoke program"
        work.mkdir()
        program = work / "build_path_smoke.erl"
        program.write_text('''-module(build_path_smoke).
-export([start/0]).
start() ->
    [3,2,1] = lists:reverse([1,2,3]),
    <<"Zg==">> = base64:encode(<<"f">>, #{}),
    io:format("spaced build ok~n").
''')
        executable = work / ("smoke.exe" if os.name == "nt" else "smoke")
        subprocess.run([binary, "build", program, "-e", "build_path_smoke:start", "-o", executable],
                       cwd=work, env=env, check=True, timeout=120)
        result = subprocess.run([executable], cwd=work, env=env, check=True,
                                capture_output=True, text=True, timeout=30)
        assert result.stdout == "spaced build ok\n", result
        print(result.stdout, end="", flush=True)
        print("Locked build and native execution passed with default target paths.", flush=True)


if __name__ == "__main__":
    main()
