"""Build in an isolated checkout: python3 build_probe.py WORKTREE before|after OUTPUT.
Temporarily includes the documented probe inside process.rs, preserving the
original source even on a failed build. Use the same Cargo target and flags for
both revisions. No benchmark module is retained in production sources.
"""
import json, os, pathlib, shutil, subprocess, sys
root = pathlib.Path(sys.argv[1]).resolve()
mode, output = sys.argv[2:]
assert mode in {'before', 'after'}
source = root / 'crates/nera-runtime/src/process.rs'
probe = pathlib.Path(__file__).resolve().with_name('probe-before.rs' if mode == 'before' else 'probe.rs')
original = source.read_bytes()
module = f'\n#[cfg(test)]\n#[path = {json.dumps(str(probe))}]\nmod entry_performance_probe;\n'
try:
    source.write_bytes(original + module.encode())
    env = {**os.environ, 'RUSTC_WRAPPER': ''}
    result = subprocess.run(['cargo', 'test', '-p', 'nera-runtime', '--lib', '--release',
        '--locked', '--no-run', '--message-format=json'], cwd=root, env=env,
        text=True, stdout=subprocess.PIPE, check=True)
    binaries = [row['executable'] for line in result.stdout.splitlines()
        if (row := json.loads(line)).get('reason') == 'compiler-artifact'
        and row.get('executable') and row['target']['name'] == 'nera_runtime']
    assert len(binaries) == 1, binaries
    shutil.copy2(binaries[0], output)
finally:
    source.write_bytes(original)
