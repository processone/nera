#!/usr/bin/env python3
"""Run the same assertions on BEAM and native Nera, then compare their output."""
import argparse
from pathlib import Path
import subprocess
import tempfile

root = Path(__file__).resolve().parents[3]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--nera', type=Path, default=root / 'target/release/nera')
args = parser.parse_args()
source = root / 'tests/binary_recursive_bits.erl'
with tempfile.TemporaryDirectory(prefix='nera-bit-offset-') as directory:
    temp = Path(directory)
    local_source = temp / source.name
    local_source.write_bytes(source.read_bytes())
    subprocess.run(['erlc', '-o', str(temp), str(local_source)], check=True)
    beam = subprocess.run(['erl', '-noshell', '-pa', str(temp), '-eval',
                           'binary_recursive_bits:test(), halt().'],
                          check=True, capture_output=True, text=True, timeout=30)
    subprocess.run([str(args.nera.resolve()), 'build', str(local_source), '-e',
                    'binary_recursive_bits:test', '-o', str(temp / 'native')], check=True, cwd=temp)
    native = subprocess.run([str(temp / 'native')], check=True,
                            capture_output=True, text=True, timeout=30)
    assert beam.stdout == native.stdout == 'ok\n', (beam.stdout, native.stdout)
    print('BEAM == Nera: ok (minimal case, 2048 offset/value combinations, large binary)')
