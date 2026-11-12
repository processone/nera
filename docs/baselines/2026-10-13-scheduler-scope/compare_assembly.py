"""Compare selected release FFI instruction encodings; no timing claims.
Usage: python3 compare_assembly.py BEFORE_ARCHIVE AFTER_ARCHIVE
"""
import hashlib
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path

SYMBOLS = [
    "nera_rt_send_message", "nera_rt_send_tuple", "nera_rt_spawn",
    "nera_rt_spawn_closure", "nera_rt_link", "nera_rt_unlink",
    "nera_rt_monitor", "nera_rt_demonitor", "nera_rt_register",
    "nera_rt_whereis", "nera_rt_unregister", "nera_rt_registered",
    "nera_rt_spawn_link", "nera_rt_spawn_link_closure",
    "nera_rt_spawn_monitor", "nera_rt_spawn_monitor_closure",
    "nera_rt_is_process_alive", "nera_rt_exit_2", "nera_rt_spawn_opt",
    "nera_rt_spawn_opt_closure", "nera_rt_send_after", "nera_rt_start_timer",
]

def inspect(path):
    members = subprocess.check_output(["ar", "-t", path], text=True).splitlines()
    member = next(m for m in members if ".nera_runtime." in m and "cgu.0" in m)
    with tempfile.TemporaryDirectory(prefix="nera-scheduler-assembly-") as directory:
        obj = Path(directory) / "runtime.o"
        obj.write_bytes(subprocess.check_output(["ar", "-p", path, member]))
        output = subprocess.check_output([
            "xcrun", "llvm-objdump", "--disassemble-symbols=" + ",".join("_" + s for s in SYMBOLS), str(obj)
        ], text=True)
    functions = {}
    current = None
    for line in output.splitlines():
        header = re.fullmatch(r"[0-9a-f]+ <_(nera_rt_\w+)>:", line)
        if header:
            current = header.group(1)
            functions[current] = []
            continue
        insn = re.match(r"\s+[0-9a-f]+:\s+([0-9a-f]{8})\s+", line)
        if insn and current:
            functions[current].append(insn.group(1))
        elif line and not line[0].isspace():
            current = None
    assert set(functions) == set(SYMBOLS), set(SYMBOLS) - set(functions)
    return {name: {"instructions": len(insns), "encoding_sha256": hashlib.sha256("".join(insns).encode()).hexdigest()}
            for name, insns in functions.items()}

before, after = map(inspect, sys.argv[1:])
print(json.dumps({
    "archives": {name: {"path": path, "sha256": hashlib.sha256(open(path, "rb").read()).hexdigest()}
                 for name, path in zip(("before", "after"), sys.argv[1:])},
    "symbols": {name: {"before": before[name], "after": after[name],
                       "identical_instruction_encodings": before[name] == after[name]}
                for name in SYMBOLS},
}, indent=2))
