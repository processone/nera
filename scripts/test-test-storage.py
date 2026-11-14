#!/usr/bin/env python3
"""Observe real `nera test` linkage in private, disposable storage on Unix.

The linker probe delegates to the native C linker. All hostile paths and HOME
values belong to one disposable fixture; no other account or real home is used.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile


LINKER = r'''#!/usr/bin/env python3
import json
import os
from pathlib import Path
import subprocess
import sys

args = sys.argv[1:]
output = Path(args[args.index("-o") + 1]).resolve()
cwd = Path.cwd()
metadata = cwd.stat()
record = {"directory": str(cwd), "binary": str(output),
          "mode": metadata.st_mode & 0o777, "uid": metadata.st_uid}
with open(os.environ["STORAGE_LOG"], "a") as log:
    log.write(json.dumps(record) + "\n")
if os.environ.get("FAIL_LINK"):
    sys.exit(1)
sys.exit(subprocess.call([os.environ["REAL_CC"], *args]))
'''

# Plant the former PID-based name before exec, so the runner has the same PID.
LAUNCH = r'''
import os
from pathlib import Path
import sys
old = Path(os.environ["TMPDIR"]) / ("nera_tests_" + str(os.getpid()))
kind = os.environ["PLANT_KIND"]
if kind == "symlink":
    old.symlink_to(os.environ["PLANT_TARGET"], target_is_directory=True)
elif kind == "file":
    old.write_text("untouched")
else:
    old.mkdir()
    (old / "sentinel").write_text("untouched")
os.execve(sys.argv[1], sys.argv[1:], os.environ)
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--nera", type=Path, default=Path("target/release/nera"))
    parser.add_argument("--temp-parent", type=Path)
    args = parser.parse_args()
    if os.name != "posix":
        print("SKIP: Unix ownership and permission regression")
        return
    compiler = args.nera.resolve()
    real_cc = shutil.which("cc")
    assert real_cc, "native C linker required"
    with tempfile.TemporaryDirectory(prefix="nera test storage ", dir=args.temp_parent) as directory:
        # macOS exposes temp roots through aliases such as /var -> /private/var.
        # Compare canonical fixture paths with the canonical linker observations.
        root = Path(directory).resolve()
        root.chmod(0o700)
        tools = root / "tools"
        tools.mkdir()
        probe = tools / "cc"
        probe.write_text(LINKER.replace("#!/usr/bin/env python3", f"#!{sys.executable}", 1))
        probe.chmod(0o700)
        project = root / "project"
        project.mkdir()
        tests = project / "tests"
        tests.mkdir()
        for name, body, expect in [
            ("alpha", 'io:format("storage complete~n"), ok', "storage complete"),
            ("beta", 'io:format("storage complete~n"), ok', "storage complete"),
            ("crash", "erlang:error(storage_failure)", "not reached"),
            ("timeout", "receive after 5000 -> ok end", "not reached"),
            ("mismatch", 'io:format("wrong~n"), ok', "not reached"),
        ]:
            (tests / f"{name}.erl").write_text(
                f"%% EXPECT: {expect}\n-module({name}).\n-export([test/0]).\ntest() -> {body}.\n"
            )
        (tests / "skip.erl").write_text("%% SKIP: storage fixture\n")
        home = root / "home"
        home.mkdir(mode=0o700)
        temporary = root / "tmp"
        temporary.mkdir(mode=0o700)
        target = root / "planted target"
        target.mkdir()
        (target / "sentinel").write_text("untouched")
        log = root / "link.jsonl"
        env = dict(os.environ, HOME=str(home), TMPDIR=str(temporary),
                   PATH=str(tools) + os.pathsep + os.environ["PATH"],
                   CC=str(probe), REAL_CC=real_cc, STORAGE_LOG=str(log), PLANT_TARGET=str(target))
        seen = set()

        def run(label, patterns, expected_code=0, expected_links=1, base=temporary,
                plant="symlink", extra_env=None, message=None):
            log.write_text("")
            current_env = dict(env, PLANT_KIND=plant, **(extra_env or {}))
            command = [sys.executable, "-c", LAUNCH, str(compiler), "test", *patterns,
                       "--no-stdlib", "-j", "2", "--timeout", "1"]
            # Include a permissive umask: private storage must not depend on it.
            process = subprocess.Popen(command, cwd=project, env=current_env,
                                       stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                       text=True, umask=0, start_new_session=True)
            try:
                out, err = process.communicate(timeout=120)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.communicate()
                raise
            print(f"=== {label} ===\n{out}{err}", flush=True)
            assert (process.returncode == 0) == (expected_code == 0), process.returncode
            if message:
                assert message in out + err, out + err
            records = [json.loads(line) for line in log.read_text().splitlines()]
            for record in records:
                print(json.dumps(record, sort_keys=True), flush=True)
            old = Path(current_env["TMPDIR"]) / f"nera_tests_{process.pid}"
            if plant == "symlink":
                assert old.is_symlink() and old.resolve() == target
            elif plant == "file":
                assert old.read_text() == "untouched"
            else:
                assert sorted(p.name for p in old.iterdir()) == ["sentinel"]
            assert sorted(p.name for p in target.iterdir()) == ["sentinel"]
            assert len(records) == expected_links, records
            for record in records:
                path = Path(record["directory"])
                assert record["mode"] == 0o700, record
                assert record["uid"] == os.geteuid(), record
                assert Path(record["binary"]).parent == path, record
                assert path.parent == base.resolve(), record
                assert str(path) not in seen, record
                seen.add(str(path))
                assert not path.exists(), f"leaked directory: {path}"
            print(f"{label}: private linkage, result and cleanup verified", flush=True)

        run("parallel / planted symlink", ["alpha", "beta"], expected_links=2)
        run("repeat / planted directory", ["alpha"], plant="directory")
        run("planted file", ["alpha"], plant="file")
        run("compile failure", ["alpha"], expected_code=1,
            extra_env={"FAIL_LINK": "1"}, message="compile error")
        run("runtime failure", ["crash"], expected_code=1, message="runtime crash")
        run("timeout", ["timeout"], expected_code=1, message="timeout after 1s")
        run("output mismatch", ["mismatch"], expected_code=1, message="output mismatch")
        run("skip", ["skip"], expected_links=0, message="1 skipped")
        run("list", ["alpha", "--list"], expected_links=0, message="Available tests (1)")
        temporary.chmod(0o1777)
        run("sticky shared root", ["alpha"])
        temporary.chmod(0o777)
        run("unsafe root / private home fallback", ["alpha"], base=home)
        home.chmod(0o777)
        run("unsafe temp and home / fail closed", ["alpha"], expected_code=1,
            expected_links=0, message="HOME")
        home.chmod(0o700)
        shared = root / "unsafe ancestor"
        shared.mkdir(mode=0o777)
        shared.chmod(0o777)
        nested_home = shared / "home"
        nested_home.mkdir(mode=0o700)
        run("replaceable home ancestor / fail closed", ["alpha"], expected_code=1,
            expected_links=0, extra_env={"HOME": str(nested_home)}, message="HOME")
        temporary.chmod(0o700)
        alias = root / "tmp alias"
        alias.symlink_to(temporary, target_is_directory=True)
        run("canonical temp path", ["alpha"], extra_env={"TMPDIR": str(alias)})

        # A tiny local stand-in isolates EUnit storage from external OTP sources.
        # This checks real compilation/execution and project-root fixture access,
        # not EUnit's own assertions or scheduling.
        eunit_tests = project / "eunit_tests"
        eunit_tests.mkdir()
        shutil.copyfile(tests / "alpha.erl", eunit_tests / "alpha.erl")
        # --src also discovers a neighbouring tests/ by convention. Remove the
        # legacy fixtures so alpha is not supplied twice to the native linker.
        shutil.rmtree(tests)
        (project / "nera.toml").write_text(
            '[project]\nname = "storage"\n[test]\nframework = "eunit"\n'
            'modules = ["alpha"]\nsrc = ["eunit_tests"]\n'
        )
        src = project / "src"
        src.mkdir()
        (project / "fixture.txt").write_text("root fixture")
        (src / "eunit.erl").write_text(
            '-module(eunit).\n-export([test/2, stop/0]).\n'
            'test(_, _) -> {ok, <<"root fixture">>} = file:read_file("fixture.txt"), ok.\n'
            'stop() -> ok.\n'
        )
        run("EUnit storage / project-root execution", ["alpha"], message="NERA_EUNIT_OK")
        temporary.chmod(0o777)
        run("EUnit home fallback", ["alpha"], base=home, message="NERA_EUNIT_OK")
        home.chmod(0o777)
        run("EUnit fail closed", ["alpha"], expected_code=1, expected_links=0, message="HOME")
        print(f"Verified {len(seen)} distinct private executable directories; none leaked.")


if __name__ == "__main__":
    main()
