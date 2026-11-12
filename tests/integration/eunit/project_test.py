#!/usr/bin/env python3
"""Regression checks for project-configured `nera test` with external OTP EUnit."""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile


def run(nera, eunit, tree, selection_only=False):
    with tempfile.TemporaryDirectory(prefix='nera-project-eunit-') as tmp:
        root = Path(tmp)
        (root / 'src').mkdir()
        (root / 'spec').mkdir()
        (root / 'tests').mkdir()
        (root / 'include').mkdir()
        (root / 'include/value.hrl').write_text('-define(VALUE, 42).\n')
        (root / 'src/production.erl').write_text('-module(production).\n-export([value/0]).\n-include("value.hrl").\nvalue() -> ?VALUE.\n')
        deps = root / '.nera/deps'
        deps.mkdir(parents=True)
        (deps / 'eunit').symlink_to(eunit, target_is_directory=True)
        support = deps / 'otp_support/src'
        support.mkdir(parents=True)
        (support / 'gb_trees.erl').symlink_to(tree)
        (root / 'spec/probe_other_tests.erl').write_text('-module(probe_other_tests).\n-include_lib("eunit/include/eunit.hrl").\nvalue_test() -> ?assertEqual(42, production:value()).\n')
        fixture = root / 'spec/probe_tests.erl'
        def source(body):
            fixture.write_text('-module(probe_tests).\n-include_lib("eunit/include/eunit.hrl").\n' + body + '\n')
        source('value_test() -> ?assertEqual(42, production:value()).')
        config = root / 'nera.toml'
        base = '[project]\nname = "demo"\nsrc = ["src"]\ninclude = ["include"]\ndefer_undefined = true\n[deps]\neunit = { path = '+json.dumps(str(eunit))+' }\n'
        test = '[test]\nframework = "eunit"\nmodules = ["probe_tests", "probe_other_tests"]\nsrc = ["spec"]\n'
        config.write_text(base + test)
        def invoke(*args):
            return subprocess.run([nera, 'test', *args], cwd=root, capture_output=True, text=True, timeout=180)
        result = invoke('--list')
        assert result.returncode == 0 and 'EUnit modules (2):' in result.stdout and 'probe_tests' in result.stdout, result
        assert not list(root.rglob('*.o')), 'listing must not compile'
        if selection_only:
            print('EUnit project selection: passed')
            return
        result = invoke('probe_t*', '--list')
        assert result.returncode == 0 and 'EUnit modules (1):' in result.stdout and 'probe_other_tests' not in result.stdout, result
        result = invoke('missing*', '--list')
        assert result.returncode != 0 and 'match' in result.stderr, result
        result = invoke('probe*', '--timeout', '20', '--no-warnings')
        assert result.returncode == 0 and result.stdout.splitlines().count('NERA_EUNIT_OK') == 1 and '2 tests passed.' in result.stdout, result
        assert not list(root.rglob('*.o')), 'compilation must be isolated'
        source('value_test() -> ?assertEqual(41, production:value()).')
        result = invoke('--timeout', '20', '--no-warnings')
        assert result.returncode != 0 and 'NERA_EUNIT_OK' not in result.stdout and '*failed*' in result.stdout and 'Failed: 1.' in result.stdout, result
        source('value_test() -> erlang:error(deliberate).')
        result = invoke('--timeout', '20', '--no-warnings')
        assert result.returncode != 0 and 'NERA_EUNIT_OK' not in result.stdout, result
        source('value_test() -> receive never -> ok end.')
        result = invoke('--timeout', '1', '--no-warnings')
        assert result.returncode != 0 and 'timed out' in result.stderr, result
        source('value_test() -> typo().')
        result = invoke('--no-warnings')
        assert result.returncode != 0 and 'compilation failed' in result.stderr, result
        config.write_text(base + test.replace('"eunit"', '"typo"'))
        result = invoke('--list')
        assert result.returncode != 0 and 'invalid' in result.stderr, result
        # Existing test/0 mode remains available without a [test] section.
        config.write_text('[project]\nname = "legacy"\n')
        (root / 'tests/legacy.erl').write_text('%% EXPECT: legacy_ok\n-module(legacy).\n-export([test/0]).\ntest() -> io:format("legacy_ok~n").\n')
        result = invoke('legacy', '--no-warnings')
        assert result.returncode == 0 and '1 passed' in result.stdout, result
    print('EUnit project runner: 10 checks passed')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('nera', 'eunit', 'gb-trees'):
        parser.add_argument('--'+name, required=True, type=Path)
    parser.add_argument('--selection-only', action='store_true')
    args = parser.parse_args()
    run(str(args.nera.resolve()), args.eunit.resolve(), args.gb_trees.resolve(), args.selection_only)
