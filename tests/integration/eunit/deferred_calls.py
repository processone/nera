#!/usr/bin/env python3
"""Exercise explicit deferred calls; NERA-92 keeps catalogue and availability separate."""
import argparse
from pathlib import Path
import subprocess
import tempfile


def run(nera):
    with tempfile.TemporaryDirectory(prefix='nera-deferred-') as tmp:
        root = Path(tmp)
        (root / 'src').mkdir()
        source = root / 'src' / 'probe.erl'
        source.write_text('''-module(probe).
-export([start/0]).
start() ->
    ok = try missing_dependency:run() catch error:undef -> ok end,
    F = fun missing_dependency:run/0,
    ok = try F() catch error:undef -> ok end,
    ok = try open_port({spawn, "unused"}, []) catch error:undef -> ok end,
    ok = try alias() catch error:undef -> ok end,
    Alias = fun alias/0,
    ok = try Alias() catch error:undef -> ok end,
    io:format("deferred_calls_ok~n").
''')
        config = root / 'nera.toml'
        def build(enabled):
            config.write_text('[project]\nname = "probe"\nentry = "probe:start"\n'
                              'output = "probe"\n' + ('defer_undefined = true\n' if enabled else ''))
            return subprocess.run([nera, 'build', '--no-warnings'], cwd=root, text=True, capture_output=True, timeout=120)
        strict = build(False)
        assert strict.returncode != 0 and ('undefined' in strict.stderr or 'unresolved' in strict.stderr), strict.stderr
        assert "erlang:alias/0" in strict.stderr, strict.stderr
        deferred = build(True)
        assert deferred.returncode == 0, deferred.stderr
        result = subprocess.run([str(root / 'probe')], cwd=root, text=True, capture_output=True, timeout=15)
        assert result.returncode == 0 and result.stdout.strip() == 'deferred_calls_ok', result
        source.write_text('-module(probe).\n-export([start/0]).\nstart() -> typo().\n')
        local = build(True)
        assert local.returncode != 0 and 'undefined' in local.stderr, local.stderr
        source.write_text('-module(probe).\n-export([start/0]).\n-compile(no_auto_import).\nstart() -> self().\n')
        suppressed = build(True)
        assert suppressed.returncode != 0 and 'undefined' in suppressed.stderr, suppressed.stderr
        source.write_text('-module(probe).\n-export([start/0]).\nstart() -> hidden:private().\n')
        (root / 'src' / 'hidden.erl').write_text('-module(hidden).\n-export([public/0]).\npublic() -> private().\nprivate() -> ok.\n')
        private = build(True)
        assert private.returncode != 0 and 'not exported' in private.stderr, private.stderr
    print('deferred_calls: 5 checks passed')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--nera', required=True)
    run(str(Path(parser.parse_args().nera).resolve()))
