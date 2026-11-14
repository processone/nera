#!/usr/bin/env python3
"""Reject incomplete execution and failed assertions using real error_2 copies.

These disposable mutations test the runner, not alternative language semantics.
The tracked fixture and all its expectations remain unchanged.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import shutil
import tempfile


SPEC = importlib.util.spec_from_file_location(
    "runtime_contracts", Path(__file__).with_name("test-runtime-contracts.py"))
contracts = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(contracts)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--nera", type=Path, default=contracts.ROOT / "target/release/nera")
    parser.add_argument("--runtime", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--temp-parent", type=Path)
    args = parser.parse_args()
    compiler = args.nera.resolve(strict=True)
    runtime = (args.runtime or compiler.with_name("libnera_runtime.a")).resolve(strict=True)
    original = (contracts.ROOT / "tests/error_2.erl").read_text(encoding="utf-8")
    variants = {
        "control": original,
        "missing-marker": original.replace('io:format("error_2_ok~n")', "ok"),
        "early-exit": original.replace('io:format("error_2_ok~n")', 'io:format("error_2_ok~n"), halt(0)'),
        "assertion": original.replace("{Name, {error, x}} =", "{Name, {error, contract_wrong_reason}} ="),
    }
    outcomes = {}
    with tempfile.TemporaryDirectory(prefix="nera contract rejection ", dir=args.temp_parent) as temporary:
        root = Path(temporary).resolve()
        tools = root / "tools"
        tools.mkdir()
        isolated_compiler = tools / compiler.name
        isolated_runtime = tools / runtime.name
        shutil.copy2(compiler, isolated_compiler)
        shutil.copy2(runtime, isolated_runtime)
        for name, text in variants.items():
            folder = root / name
            folder.mkdir()
            source = folder / "error_2.erl"
            source.write_text(text, encoding="utf-8")
            result = contracts.run_contract(source, isolated_compiler, isolated_runtime,
                                            "erl", folder, 30)
            outcomes[name] = result
        # A marker-only negative must actually return successfully in all modes;
        # a compile error is not evidence that completion checking works.
        control = outcomes["control"]["passed"]
        missing = outcomes["missing-marker"]["runs"]
        assertion = outcomes["assertion"]["runs"]
        early = outcomes["early-exit"]["runs"]
        checks = {
            "control": control,
            "missing-marker": len(missing) == len(contracts.MODES) and all(
                r.get("returncode") == 0 and r.get("return_completed")
                and not r.get("fixture_completed") and not r["passed"] for r in missing),
            "early-exit": len(early) == len(contracts.MODES) and all(
                r.get("returncode") == 0 and r.get("fixture_completed")
                and not r.get("return_completed") and not r["passed"] for r in early),
            "assertion": len(assertion) == len(contracts.MODES) and all(
                r.get("returncode") not in (None, 0) and not r.get("return_completed")
                and not r["passed"] and "badmatch" in r.get("stderr", "") + r.get("stdout", "")
                for r in assertion),
        }
        report = {"passed": all(checks.values()), "checks": checks, "outcomes": outcomes,
                  "compiler_sha256": contracts.digest(compiler), "runtime_sha256": contracts.digest(runtime)}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(checks, sort_keys=True))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
