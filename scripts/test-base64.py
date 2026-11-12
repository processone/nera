#!/usr/bin/env python3
"""Compare production Base64 assertions against BEAM and the frozen Erlang baseline."""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--nera', type=Path, default=ROOT / 'target/release/nera')
args = parser.parse_args()
compiler = args.nera.resolve()

def run(command, cwd):
    return subprocess.run(list(map(str, command)), cwd=cwd, check=True,
                          capture_output=True, text=True, timeout=120).stdout

with tempfile.TemporaryDirectory(prefix='nera-base64-production-') as directory:
    work = Path(directory)
    for module in ['base64_options', 'base64_large_encode', 'base64_large_decode', 'base64_list_decode']:
        source = work / (module + '.erl')
        source.write_bytes((ROOT / 'tests' / source.name).read_bytes())
        run(['erlc', '-o', work, source], work)
        beam = run(['erl', '-noshell', '-pa', work, '-eval', f'{module}:test(), halt().'], work)
        run([compiler, 'build', source, '-e', f'{module}:test', '-o', work/'native'], work)
        native = run([work/'native'], work)
        assert beam == native == 'ok\n', (module, beam, native)
        print(f'BEAM == Nera: {module}: ok', flush=True)
        source.unlink()
    # Existing extensive value/error corpus, now production against frozen Erlang.
    reference = ROOT / 'benchmarks/base64_prototype'
    (work/'base64_reference.erl').write_bytes((reference/'base64_reference.erl').read_bytes())
    text = (reference/'differential.erl').read_text()
    start = text.index('    %% A native result')
    end = text.index('    All =', start)
    text = text[:start] + text[end:]
    text = text.replace('base64:', 'base64_reference:').replace('outcome(base64,', 'outcome(base64_reference,')
    text = text.replace('base64_rust', 'base64')
    # List decoding now follows BEAM, including formerly divergent errors.
    # Its independent exhaustive corpus below replaces the frozen list oracle.
    text = text.replace('check(F,I,O) ->',
        'check(decode,I,_) when is_list(I) -> ok;\ncheck(F,I,O) ->')
    (work/'differential.erl').write_text(text)
    run([compiler, 'build', work/'differential.erl', work/'base64_reference.erl',
         '-e', 'differential:start', '-o', work/'differential'], work)
    assert run([work/'differential'], work) == 'DIFFERENTIAL_OK\n'
    print('Frozen Erlang == production: encode and binary decode corpus: ok')

    # Independently check the same binary-decoding value/error oracle on BEAM.
    # Lists are independently checked directly against BEAM below.
    beam_text = text.replace('check(F,I,O) ->',
        'check(encode,_,_) -> ok;\n'
        'check(_,I,_) when not is_binary(I) -> ok;\n'
        'check(F,I,O) ->')
    (work/'differential.erl').write_text(beam_text)
    run(['erlc', '-o', work, work/'differential.erl', work/'base64_reference.erl'], work)
    assert run(['erl', '+S', '1:1', '-noshell', '-pa', work, '-s', 'differential', 'start'], work) == 'DIFFERENTIAL_OK\n'
    print('BEAM == frozen Erlang: exhaustive binary decode value/error corpus: ok')

    source = work/'base64_list_corpus.erl'
    source.write_bytes((ROOT/'scripts/fixtures'/source.name).read_bytes())
    run(['erlc', '-o', work, source], work)
    beam = run(['erl', '+S', '1:1', '-noshell', '-pa', work,
                '-s', 'base64_list_corpus', 'start'], work)
    run([compiler, 'build', source, '-e', 'base64_list_corpus:start',
         '-o', work/'list-corpus'], work)
    native = subprocess.run([work/'list-corpus'], cwd=work,
                            env=dict(os.environ, NERA_SCHEDULERS='1'), check=True,
                            capture_output=True, text=True, timeout=120).stdout
    expected, actual = beam.splitlines(), native.splitlines()
    assert expected[-1:] == actual[-1:] == ['COMPATIBILITY_OK']
    assert len(expected) == len(actual), (len(expected), len(actual))
    for index, (want, got) in enumerate(zip(expected, actual), 1):
        assert want == got, f'List corpus result {index}: BEAM={want}, Nera={got}'
    print(f'BEAM == native lists: {len(expected)-1} values/errors, including invalid elements and options')
