#!/usr/bin/env python3
"""Regression for the Cowlib 2.22.0 generated MIME table allocation failure.

Intent: compile the unchanged upstream module within 6,800,000 KiB of virtual
address space and preserve every literal lookup in its two generated tables.
Third-party sources and generated probes stay outside the working copy.
Run on Linux, with a release compiler and OTP, when compiler coverage changes.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import resource
import subprocess
import sys
import tempfile
import urllib.request

PIN = json.loads((Path(__file__).parent / "upstream-2.22.0.json").read_text())
SOURCE_SHA256 = "bbdd5d794a0c970cdd0420109ceaf2aaeb4ec70294a5c8a053737e67e3e3779b"
LIMIT_KIB = 6_800_000
EXPECTED_ENTRIES = 1_009


def capped():
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    resource.setrlimit(resource.RLIMIT_AS, (LIMIT_KIB * 1024, LIMIT_KIB * 1024))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--nera", required=True, type=Path)
    parser.add_argument("--source", type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    if sys.platform != "linux":
        parser.error("the address-space regression requires native Linux")
    nera = args.nera.resolve()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    report = {"status": "failed", "pin": PIN, "limit_kib": LIMIT_KIB, "commands": []}

    def run(command, label, cwd, limit=False):
        result = subprocess.run(list(map(str, command)), cwd=cwd, capture_output=True,
                                timeout=180, preexec_fn=capped if limit else None)
        (out / (label + ".stdout")).write_bytes(result.stdout)
        (out / (label + ".stderr")).write_bytes(result.stderr)
        report["commands"].append({"label": label, "returncode": result.returncode})
        if result.returncode:
            raise RuntimeError(f"{label} failed: {result.returncode}")
        return result.stdout.decode()

    try:
        if args.source:
            source = args.source.read_bytes()
        else:
            url = f"https://raw.githubusercontent.com/ninenines/cowlib/{PIN['commit']}/src/cow_mimetypes.erl"
            with urllib.request.urlopen(url, timeout=30) as response:
                source = response.read()
        digest = hashlib.sha256(source).hexdigest()
        if digest != SOURCE_SHA256:
            raise ValueError("source differs from the pinned upstream module")
        report["source_sha256"] = digest
        report["compiler_sha256"] = hashlib.sha256(nera.read_bytes()).hexdigest()
        entries = re.findall(r'^(all|web)_ext\((<<"[^"]+">>)\) -> (\{.*\});$',
                             source.decode(), re.M)
        if len(entries) != EXPECTED_ENTRIES or {entry[0] for entry in entries} != {"all", "web"}:
            raise ValueError(f"expected {EXPECTED_ENTRIES} generated MIME entries, found {len(entries)}")
        assertions = [f'{expected} = cow_mimetypes:{api}(<<"file.{extension[3:-3]}">>)'
                      for api, extension, expected in entries]
        fallback = '{<<"application">>, <<"octet-stream">>, []}'
        for api in ("all", "web"):
            for path in ('"no_extension"', '"file.unknown-extension"', '"file."'):
                assertions.append(f'{fallback} = cow_mimetypes:{api}(<<{path}>>)')
            assertions.append(f'{{<<"text">>, <<"html">>, []}} = cow_mimetypes:{api}(<<"file.HTML">>)')
        report["literal_lookups"] = len(entries)
        report["assertions"] = len(assertions)
        marker = f'MIME_OK {len(assertions)}\n'
        chunks = [assertions[i:i + 25] for i in range(0, len(assertions), 25)]
        functions = [f'check_{i}() ->\n    ' + ',\n    '.join(chunk + ['ok']) + '.\n'
                     for i, chunk in enumerate(chunks)]
        start = ', '.join(f'check_{i}()' for i in range(len(chunks)))
        driver = ('-module(mime_probe).\n-export([start/0]).\n'
                  f'start() -> {start}, io:format("MIME_OK {len(assertions)}~n").\n'
                  + '\n'.join(functions))
        with tempfile.TemporaryDirectory(prefix="nera-mime-") as folder:
            work = Path(folder)
            module = work / "cow_mimetypes.erl"
            module.write_bytes(source)
            probe = work / "mime_probe.erl"
            probe.write_text(driver)
            report["otp"] = run(["erl", "+S", "1", "-noshell", "-eval",
                                 'io:format("~s", [erlang:system_info(system_version)]), halt().'],
                                "otp", work).strip()
            run(["erlc", "-o", work, module, probe], "beam-build", work)
            beam = run(["erl", "+S", "1", "-noshell", "-pa", work, "-eval",
                        "mime_probe:start(), halt()."], "beam", work)
            if beam != marker:
                raise ValueError("BEAM did not complete every assertion")
            run([nera, "build", module, "--emit-obj"], "capped-object", work, True)
            executable = work / "mime-probe"
            run([nera, "build", module, probe, "-e", "mime_probe:start", "-o", executable],
                "capped-link", work, True)
            native = run([executable], "native", work)
            if native != marker:
                raise ValueError("native execution did not complete every assertion")
            report["status"] = "passed"
    except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired) as error:
        report["error"] = str(error)
    finally:
        (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"{report['status']}: {report.get('assertions', 0)} MIME assertions; {out / 'report.json'}")
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
