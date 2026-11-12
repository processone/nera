#!/usr/bin/env python3
"""Compile and run using only an unpacked native distribution and system tools.

Usage: python3 scripts/test-dist.py dist/nera-VERSION-TARGET.tar.gz
Requires the platform C linker. Run on the archive's advertised target.
"""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile
import zipfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    args = parser.parse_args()
    archive = args.archive.resolve()
    with tempfile.TemporaryDirectory(prefix="nera-installed-smoke-") as temporary:
        root = Path(temporary)
        installed = root / "installed"
        installed.mkdir()
        if archive.suffix == ".zip":
            with zipfile.ZipFile(archive) as bundle:
                bundle.extractall(installed)
        else:
            subprocess.run(["tar", "xzf", archive, "-C", installed], check=True)
        prefixes = list(installed.iterdir())
        assert len(prefixes) == 1, prefixes
        binary = prefixes[0] / "bin" / ("nera.exe" if os.name == "nt" else "nera")
        work = root / "work"
        work.mkdir()
        cache = root / "empty-cache"
        cache.mkdir(mode=0o700)
        env = dict(os.environ, TMPDIR=str(cache), TMP=str(cache), TEMP=str(cache))
        source = work / "preview_smoke.erl"
        source.write_text('''-module(preview_smoke).
-export([start/0]).
start() ->
    [3,2,1] = lists:reverse([1,2,3]),
    <<"Zg==">> = base64:encode(<<"f">>, #{}),
    %% Cross a native encoder slice boundary through the installed stdlib.
    Prefix = binary:copy(<<"AAAA">>, 16384),
    Expected = <<Prefix/binary,"AA==">>,
    Expected = base64:encode(binary:copy(<<0>>, 49153), #{}),
    Raw = binary:copy(<<0>>, 49153),
    Raw = base64:decode(Expected, #{}),
    Raw = base64:decode(binary_to_list(Expected), #{}),
    try base64:decode(lists:duplicate(16384,32)++[512], #{}) of
        _ -> error(expected_bad_list_decode)
    catch error:badarg -> ok end,
    Space = binary:copy(<<32>>, 65536),
    try base64:decode(<<Space/binary,"Y===">>, #{}) of
        _ -> error(expected_bad_decode)
    catch error:badarith -> ok end,
    io:format("installed preview ok~n").
''')
        executable = work / ("preview-smoke.exe" if os.name == "nt" else "preview-smoke")
        def compile_and_run(mode):
            subprocess.run([binary, "build", source, "-e", "preview_smoke:start", "-o", executable],
                           cwd=work, env=env, check=True, timeout=120)
            result = subprocess.run([executable], cwd=work, env=env, check=True,
                                    capture_output=True, text=True, timeout=30)
            assert result.stdout == "installed preview ok\n", result
            print(f"{mode}: {result.stdout}", end="")

        compile_and_run("installed library")
        libraries = [prefixes[0] / "lib" / name
                     for name in ("libnera_runtime.a", "nera_runtime.lib")]
        library = next((path for path in libraries if path.is_file()), None)
        assert library is not None, "distribution is missing its external runtime"
        # Also prove that a clean build embedded the runtime. This only changes
        # our disposable installation; the archive itself stays untouched.
        library.unlink()
        for path in cache.iterdir():
            if path.is_file():
                path.unlink()
        compile_and_run("embedded library")


if __name__ == "__main__":
    main()
