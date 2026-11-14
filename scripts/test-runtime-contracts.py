#!/usr/bin/env python3
"""Execute the fixture-owned runtime contracts; see docs/RUNTIME_CONTRACTS.md.

Build runtime before compiler. This is a focused journey, not the project gate.
All process output, including failures and incomplete execution, is kept in JSON.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import tempfile
import time


ROOT = Path(__file__).resolve().parents[1]
CONTRACTS = (
    "try_operation_short_circuit", "try_after_reraise", "error_2",
    "float_arith_badarith", "bin_comp_badarg", "resume_exception",
)
MODES = ("native-default", "native-single", "beam")
DONE = "runtime_contract_returned_ok"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def execute(command, cwd, env, timeout):
    """A missing command, timeout or signal is an outcome, never a skip."""
    record = {"command": [str(x) for x in command], "cwd": str(cwd),
              "returncode": None, "completed": False, "failure": None}
    start = time.monotonic()
    try:
        result = subprocess.run(record["command"], cwd=cwd, env=env,
                                capture_output=True, text=True, timeout=timeout)
        record.update(returncode=result.returncode, completed=True,
                      stdout=result.stdout, stderr=result.stderr)
        if result.returncode != 0:
            record["failure"] = "nonzero exit"
    except subprocess.TimeoutExpired as error:
        def text(value):
            return value.decode(errors="replace") if isinstance(value, bytes) else value or ""
        record.update(failure="timeout", stdout=text(error.stdout), stderr=text(error.stderr))
    except OSError as error:
        record.update(failure=str(error), stdout="", stderr="")
    record["seconds"] = time.monotonic() - start
    return record


def environment():
    env = os.environ.copy()
    # Do not let caller settings silently turn default into single scheduler,
    # change the OTP invocation or load arbitrary modules via Erlang flags.
    for key in ("NERA_SCHEDULERS", "ERL_FLAGS", "ERL_AFLAGS", "ERL_ZFLAGS", "ERL_LIBS"):
        env.pop(key, None)
    return env


def run_contract(source, compiler, runtime, erl, work, timeout):
    """Compile the unchanged source and assert its return plus its own EXPECT."""
    module = source.stem
    text = source.read_text(encoding="utf-8")
    markers = re.findall(r"^%% EXPECT: (.+)$", text, re.MULTILINE)
    result = {"fixture": module, "sha256": digest(source), "builds": [], "runs": [],
              "passed": False, "failures": []}
    if len(markers) != 1:
        result["failures"].append("fixture must own exactly one EXPECT completion marker")
        return result
    marker = markers[0]
    result["completion_marker"] = marker
    wrapper = work / "runtime_contract_entry.erl"
    wrapper.write_text(
        "-module(runtime_contract_entry).\n-export([main/0]).\n"
        f'main() -> ok = {module}:test(), io:format("{DONE}~n"), ok.\n', encoding="utf-8")
    binary = work / (module + (".exe" if os.name == "nt" else ".native"))
    env = environment()
    builds = [
        ("native", [compiler, "build", source, wrapper, "-e", "runtime_contract_entry:main", "-o", binary]),
        ("beam", [erl, "-noshell", "-eval",
                  'lists:foreach(fun(P) -> R=compile:file(P,[{outdir,' + json.dumps(str(work)) +
                  '},return_errors,return_warnings]), io:format("~p~n",[R]), '
                  'case R of {ok,_,_} -> ok; _ -> halt(1) end end, [' +
                  ','.join(json.dumps(str(p)) for p in (source, wrapper)) + ']), halt(0).']),
    ]
    ready = {}
    for engine, command in builds:
        record = execute(command, work, env, 120)
        record["engine"] = engine
        # Prove the native build used the supplied archive, not another checkout
        # or an embedded runtime from a different compiler version.
        if engine == "native" and not record["failure"]:
            linked = f"Linking with runtime: {runtime}"
            if linked not in record["stderr"].splitlines():
                record["failure"] = "supplied runtime linkage was not observed"
        ready[engine] = not record["failure"]
        result["builds"].append(record)
    for mode in MODES:
        engine = "beam" if mode == "beam" else "native"
        if not ready[engine]:
            result["runs"].append({"mode": mode, "completed": False, "passed": False,
                                   "failure": "compilation did not succeed; execution missing"})
            continue
        run_env = env.copy()
        if mode == "native-single":
            run_env["NERA_SCHEDULERS"] = "1"
        command = ([erl, "-noshell", "-pa", work, "-eval",
                    "runtime_contract_entry:main(), halt(0)."]
                   if engine == "beam" else [binary])
        record = execute(command, work, run_env, timeout)
        record.update(mode=mode, scheduler="1" if mode == "native-single" else "default")
        lines = record["stdout"].splitlines()
        record["fixture_completed"] = lines.count(marker) == 1
        record["return_completed"] = lines.count(DONE) == 1
        if not record["failure"] and not (record["fixture_completed"] and record["return_completed"]):
            record["failure"] = "missing or duplicate completion marker"
        record["passed"] = not record["failure"]
        result["runs"].append(record)
    result["failures"] = [f'{r["mode"]}: {r["failure"]}' for r in result["runs"] if not r["passed"]]
    result["passed"] = len(result["runs"]) == len(MODES) and not result["failures"]
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--nera", type=Path, default=ROOT / "target/release/nera")
    parser.add_argument("--runtime", type=Path, help="matching archive; defaults beside compiler")
    parser.add_argument("--compiler-source", required=True,
                        help="full source commit used to build BOTH supplied artifacts")
    parser.add_argument("--erl", default="erl")
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--temp-parent", type=Path)
    parser.add_argument("--timeout", type=float, default=30)
    parser.add_argument("--fixture", choices=CONTRACTS, action="append",
                        help="diagnostic subset; JSON distinguishes it from the full corpus")
    args = parser.parse_args()
    report = {"schema": 1, "passed": False, "failures": [], "contracts": [],
              "platform": platform.platform(), "selected": args.fixture or list(CONTRACTS),
              "full_corpus": not args.fixture or set(args.fixture) == set(CONTRACTS),
              "compiler_source_declared": args.compiler_source}
    try:
        if not re.fullmatch(r"[0-9a-f]{40}", args.compiler_source):
            raise ValueError("compiler source must be a full commit ID")
        if args.timeout <= 0:
            raise ValueError("timeout must be positive")
        compiler = args.nera.resolve(strict=True)
        runtime = (args.runtime or compiler.with_name(
            "nera_runtime.lib" if os.name == "nt" else "libnera_runtime.a")).resolve(strict=True)
        report["compiler"] = {"path": str(compiler), "sha256": digest(compiler)}
        report["runtime"] = {"path": str(runtime), "sha256": digest(runtime)}
        env = environment()
        # Always capture the fixture checkout identity separately from the
        # declared compiler identity (important for historical rejection runs).
        report["fixture_source"] = execute(["git", "rev-parse", "HEAD"], ROOT, env, 10)
        report["fixture_changes"] = execute(["git", "status", "--porcelain", "--", "tests"], ROOT, env, 10)
        for name in ("fixture_source", "fixture_changes"):
            if report[name]["failure"]:
                raise ValueError(f"cannot record {name}")
        with tempfile.TemporaryDirectory(prefix="nera runtime contracts ", dir=args.temp_parent) as temporary:
            work = Path(temporary)
            tools = work / "tools"
            tools.mkdir()
            isolated_compiler = tools / compiler.name
            isolated_runtime = tools / runtime.name
            shutil.copy2(compiler, isolated_compiler)
            shutil.copy2(runtime, isolated_runtime)
            report["compiler"]["version"] = execute([isolated_compiler, "--version"], work, env, 10)
            otp_query = ('P=filename:join([code:root_dir(),"releases",'
                         'erlang:system_info(otp_release),"OTP_VERSION"]),'
                         '{ok,V}=file:read_file(P),io:format("~s",[V]),halt(0).')
            report["otp"] = execute([args.erl, "-noshell", "-eval", otp_query], work, env, 30)
            if report["compiler"]["version"]["failure"]:
                raise ValueError("compiler version probe failed")
            if report["otp"]["failure"] or not re.fullmatch(r"29\.\d+(?:\.\d+)*", report["otp"]["stdout"].strip()):
                raise ValueError("a recorded OTP 29 patch version is required")
            report["otp"]["patch"] = report["otp"]["stdout"].strip()
            for name in report["selected"]:
                folder = work / name
                folder.mkdir()
                report["contracts"].append(run_contract(
                    ROOT / "tests" / f"{name}.erl", isolated_compiler, isolated_runtime,
                    args.erl, folder, args.timeout))
            report["failures"] = [failure for contract in report["contracts"] for failure in contract["failures"]]
            report["passed"] = bool(report["contracts"]) and all(c["passed"] for c in report["contracts"])
    except (OSError, ValueError) as error:
        report["failures"].append(str(error))
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f'Runtime contracts: {"passed" if report["passed"] else "FAILED"}; results: {args.out}')
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
