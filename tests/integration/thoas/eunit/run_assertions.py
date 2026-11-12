#!/usr/bin/env python3
"""Run pinned Thoas assertions natively, with an external entry-point adapter.

BEAM uses the original EUnit runner. Nera uses the original test bodies with
an appended project-owned entry point, not the EUnit scheduling framework.
No upstream source is written into this repository or modified in its checkout.
"""
import argparse
import json
import re
from pathlib import Path
import subprocess

PIN = '7cdcec7e52b80d2869900893b2acf47199d84190'
ROOT = Path(__file__).resolve().parents[4]
ADAPTER = """
nera_assertions() ->
    {ok, <<Index>>} = file:read_file("assertion-index"),
    Simple = [{0, fun true_test/0}, {0, fun false_test/0},
              {0, fun boolean_true_test/0}, {0, fun boolean_false_test/0},
              {0, fun null_test/0}],
    Groups = [Simple, integer_test_(), float_test_(), string_test_(),
              non_recursive_array_test_(), non_recursive_object_test_(),
              encode_test_(), types_test_()],
    Tests = lists:append(Groups),
    36 = length(Tests),
    {_Line, F} = nera_nth(Index, Tests),
    ok = F(),
    io:format("THOAS_ASSERTION_OK:~p~n", [Index]).
nera_nth(1, [Test | _]) -> Test;
nera_nth(Index, [_ | Rest]) -> nera_nth(Index - 1, Rest).
"""


def classify_result(index, result, known):
    if result.returncode == 0 and result.stdout.splitlines() == [f'THOAS_ASSERTION_OK:{index}']:
        return 'unexpected_success' if index in known else 'passed'
    first_line = result.stderr.splitlines()[0] if result.stderr else ''
    match = re.fullmatch(r'Error in process <[0-9.]+>: (.*)', first_line)
    reason = re.sub(r'\{line,\d+\}', '{line,*}', match.group(1)) if match else None
    if (index in known and result.returncode == 1 and not result.stdout
            and result.stderr.count('Error in process ') == 1 and reason == known[index]):
        return 'known_failure'
    return 'unexpected_failure'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--upstream', required=True, type=Path)
    parser.add_argument('--nera', required=True, type=Path)
    parser.add_argument('--work-dir', required=True, type=Path)
    parser.add_argument('--check-known-failures', action='store_true',
                        help='require passing cases and exact recorded failures; fail on any change')
    args = parser.parse_args()
    source, nera, work = (p.resolve() for p in (args.upstream, args.nera, args.work_dir))
    if work.is_relative_to(ROOT) or work.is_relative_to(source):
        parser.error('work directory must be outside the repository and upstream checkout')
    head = subprocess.check_output(['git', '-C', source, 'rev-parse', 'HEAD'], text=True).strip()
    dirty = subprocess.check_output(['git', '-C', source, 'status', '--porcelain', '--untracked-files=all'], text=True)
    if head != PIN or dirty:
        parser.error('expected a clean, pinned Thoas checkout')
    work.mkdir(parents=True, exist_ok=True)
    beam = work / 'beam'
    beam.mkdir(exist_ok=True)

    def run(command, label, timeout=180):
        result = subprocess.run(command, cwd=work, text=True, capture_output=True, timeout=timeout)
        (work / (label + '.stdout')).write_text(result.stdout)
        (work / (label + '.stderr')).write_text(result.stderr)
        if result.returncode:
            raise RuntimeError(f'{label} failed ({result.returncode}); see {work}')
        return result.stdout

    sources = sorted((source / 'src').glob('*.erl'))
    original = source / 'test/thoas_encode_test.erl'
    run(['erlc', '-o', beam, *sources, original], 'beam-build')
    output = run(['erl', '-noshell', '-pa', beam, '-eval',
                  'ok = eunit:test(thoas_encode_test, [verbose]), io:format("THOAS_EUNIT_OK~n"), halt().'], 'beam')
    if 'THOAS_EUNIT_OK' not in output.splitlines() or 'All 36 tests passed.' not in output:
        raise RuntimeError('missing BEAM EUnit completion/count')
    text = original.read_text()
    declaration = '-module(thoas_encode_test).'
    if text.count(declaration) != 1:
        raise RuntimeError('unexpected upstream module declaration')
    adapted = work / 'thoas_encode_test.erl'
    adapted.write_text(text.replace(declaration, declaration + '\n-export([nera_assertions/0]).', 1) + ADAPTER)
    executable = work / 'native-assertions'
    run([nera, 'build', *sources, adapted, '-e', 'thoas_encode_test:nera_assertions', '-o', executable], 'nera-build')
    baseline = json.loads(Path(__file__).with_name('known_failures.json').read_text())
    if baseline['upstream_commit'] != PIN:
        raise RuntimeError('baseline/upstream pin mismatch')
    known = {case['index']: case['reason'] for case in baseline['failures']} if args.check_known_failures else {}
    results = []
    for index in range(1, 37):
        (work / 'assertion-index').write_bytes(bytes([index]))
        result = subprocess.run([executable], cwd=work, text=True, capture_output=True, timeout=60)
        (work / f'nera-{index:02}.stdout').write_text(result.stdout)
        (work / f'nera-{index:02}.stderr').write_text(result.stderr)
        results.append({'index': index, 'status': classify_result(index, result, known),
                        'exit_code': result.returncode})
    counts = {status: sum(r['status'] == status for r in results)
              for status in ('passed', 'known_failure', 'unexpected_failure', 'unexpected_success')}
    passed = not counts['unexpected_failure'] and not counts['unexpected_success']
    report = {'status': ('passed_with_known_failures' if known else 'passed') if passed else 'failed',
              'upstream_commit': PIN, 'beam_eunit_passed': 36, 'native_counts': counts, 'results': results,
              'scope': 'assertion bodies with external adapter; not the native EUnit framework'}
    (work / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(f'BEAM EUnit: 36 passed; Nera: {counts}; status: {report["status"]}')
    if not passed:
        raise SystemExit(1)



if __name__ == '__main__':
    main()
