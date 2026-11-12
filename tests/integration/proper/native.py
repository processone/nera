#!/usr/bin/env python3
"""Compare pinned external PropEr on BEAM/Nera, with optional seeded shrinking."""
import argparse
import hashlib
import json
import os
import platform
from pathlib import Path
import subprocess
import sys

PIN = '5640d0715d4b346676267504d8e84398e2a29f75'  # PropEr 1.5.0
DRIVER = '''-module(proper_probe).
-export([start/0]).
start() ->
 T = proper:quickcheck(true, [quiet, {numtests, 3}]),
 io:format("PROPER_TRUE:~p~n", [T]),
 F = proper:quickcheck(false, [quiet, {numtests, 3}]),
 io:format("PROPER_FALSE:~p~n", [F]),
 true = T,
 false = F,
 io:format("PROPER_DONE~n").
'''


def completed(code, out, err):
    lines = out.splitlines()
    return (code == 0 and 'Error in process' not in out + err
            and bool(lines) and lines[-1] == 'PROPER_DONE'
            and all(lines.count(marker) == 1 for marker in
                    ('PROPER_TRUE:true', 'PROPER_FALSE:false', 'PROPER_DONE'))
            and sum(line.startswith('PROPER_TRUE:') for line in lines) == 1
            and sum(line.startswith('PROPER_FALSE:') for line in lines) == 1)


def main(scenario=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--proper', type=Path, required=True)
    parser.add_argument('--nera', type=Path, required=True)
    parser.add_argument('--otp-rand', type=Path, help='external OTP rand.erl source for native runtime seeding')
    parser.add_argument('--otp-ordsets', type=Path, help='external OTP ordsets.erl source for container shrinking')
    parser.add_argument('--work-dir', type=Path, required=True)
    parser.add_argument('--beam-parse-transforms', action='store_true',
                        help='explicitly prepare dependencies with BEAM before native compilation')
    parser.add_argument('--generation', action='store_true',
                        help='exercise native seeded generation, shrinking and counterexample replay')
    if scenario is not None:
        scenario.configure(parser)
    args = parser.parse_args()
    if scenario is not None:
        if args.generation or not args.otp_rand or not args.otp_ordsets or not args.beam_parse_transforms:
            parser.error('external scenarios require explicit preparation and both OTP sources, without --generation')
        scenario.validate_args(args,parser)
    driver = DRIVER
    check_completed = completed
    if args.generation:
        if not args.otp_rand or not args.otp_ordsets or not args.beam_parse_transforms:
            parser.error('--generation requires explicit preparation, --otp-rand and --otp-ordsets on both engines')
        from generation import DRIVER as generation_driver, completed as generation_completed
        driver, check_completed = generation_driver, generation_completed
    if scenario is not None:
        driver = scenario.driver(args)
        check_completed = lambda code,out,err: scenario.completed(code,out,err,args)
    if (args.otp_rand or args.otp_ordsets) and not args.beam_parse_transforms:
        parser.error("OTP dependencies require --beam-parse-transforms")
    if args.otp_ordsets and not args.otp_rand:
        parser.error('--otp-ordsets requires --otp-rand')
    proper, nera, work = (p.resolve() for p in (args.proper, args.nera, args.work_dir))
    repo = Path(__file__).resolve().parents[3]
    if work == repo or repo in work.parents or work == proper or proper in work.parents:
        parser.error('work directory must be outside the repository and dependency')
    work.mkdir(parents=True, exist_ok=True)
    if any(work.iterdir()):
        parser.error('use an empty work directory to prevent stale binaries or dependencies')
    env = os.environ.copy()
    for key in ('ERL_FLAGS', 'ERL_AFLAGS', 'ERL_ZFLAGS', 'ERL_LIBS', 'NUMWORKERS'):
        env.pop(key, None)
    run_timeout = getattr(scenario, 'RUN_TIMEOUT', 60)
    report = {'status': 'blocked', 'run_timeout_seconds': run_timeout, 'scope': 'literal true/false quickcheck smoke test; no generation or shrinking',
              'commands': [], 'proper_pin': PIN, 'platform': platform.platform(),
              'runner_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), 'nera_sha256': hashlib.sha256(nera.read_bytes()).hexdigest()}

    if args.generation:
        report['scope'] = 'native seeded integer/list/binary generation, shrinking and counterexample replay'
        report['generation_runner_sha256'] = hashlib.sha256((Path(__file__).parent/'generation.py').read_bytes()).hexdigest()

    if scenario is not None:
        report['scope'] = scenario.__doc__
        report['scenario_sha256'] = hashlib.sha256(Path(scenario.__file__).read_bytes()).hexdigest()

    def run(name, command, cwd=work, timeout=180):
        try:
            result = subprocess.run([str(x) for x in command], cwd=cwd, env=env,
                                    capture_output=True, text=True, timeout=timeout)
            code, out, err = result.returncode, result.stdout, result.stderr
        except subprocess.TimeoutExpired as error:
            code = None
            out = (error.stdout or b'').decode(errors='replace')
            err = (error.stderr or b'').decode(errors='replace') + '\nTIMEOUT'
        record = {'name': name, 'argv': [str(x) for x in command], 'cwd': str(cwd),
                  'returncode': code, 'stdout': out, 'stderr': err, 'timeout_seconds': timeout}
        report['commands'].append(record)
        return record

    try:
        pin = run('proper-revision', ['git', 'rev-parse', 'HEAD'], proper)
        clean = run('proper-status', ['git', 'status', '--porcelain', '--untracked-files=all'], proper)
        if pin['returncode'] != 0 or pin['stdout'].strip() != PIN or clean['returncode'] != 0 or clean['stdout'].strip():
            raise RuntimeError('requires the pinned, pristine external PropEr checkout')
        report['nera_source'] = run('nera-source', ['git', 'rev-parse', 'HEAD'], repo)['stdout'].strip()
        report['nera_source_diff'] = run('nera-source-diff', ['git', 'diff', '--', 'crates', 'erlang'], repo)['stdout']
        listing = run('nera-source-files', ['git', 'ls-files', '-co', '--exclude-standard', '--',
                      'crates', 'erlang', 'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml'], repo)
        report['source_hashes'] = {name: hashlib.sha256((repo/name).read_bytes()).hexdigest()
                                   for name in sorted(set(listing['stdout'].splitlines()))
                                   if (repo/name).is_file()}
        report['source_snapshot_sha256'] = hashlib.sha256(json.dumps(report['source_hashes'], sort_keys=True).encode()).hexdigest()
        report['preparation_helper_sha256'] = hashlib.sha256((repo/'scripts/prepare_erlang_dependency.py').read_bytes()).hexdigest()
        report['otp_version'] = run('otp-version', ['erl', '-noshell', '-eval', 'io:format("~s", [erlang:system_info(otp_release)]), halt().'])['stdout']
        report['nera_version'] = run('nera-version', [nera, '--version'])['stdout']
        runtime = nera.parent/'libnera_runtime.a'
        if runtime.is_file():
            report['runtime_sha256'] = hashlib.sha256(runtime.read_bytes()).hexdigest()
        source = work/'proper_probe.erl'
        source.write_text(driver)
        beam = work/'beam'
        beam.mkdir()
        # Rebar builds original dependency sources outside our repository.
        cleaned = run('beam-dependency-clean', ['rebar3', 'clean'], proper)
        if cleaned['returncode'] != 0:
            raise RuntimeError('BEAM dependency clean failed')
        built = run('beam-dependency-build', ['rebar3', 'compile'], proper)
        extra_dependencies = {}
        if built['returncode'] == 0 and scenario is not None:
            extra_dependencies = scenario.prepare(args,work,report,run,beam)
        dependency_config = ''.join(name+' = { path = '+json.dumps(str(path))+' }\n'
                                    for name,path in extra_dependencies.items())
        beam_build = run('beam-driver-build', ['erlc', '-o', beam, source])
        if built['returncode'] != 0 or beam_build['returncode'] != 0:
            raise RuntimeError('BEAM setup failed')
        beam_command = ['erl', '-noshell', '-pa', beam, '-pa', proper/'_build/default/lib/proper/ebin']
        if args.otp_rand:
            otp_rand = args.otp_rand.resolve()
            report['otp_rand_source'] = str(otp_rand)
            report['otp_rand_sha256'] = hashlib.sha256(otp_rand.read_bytes()).hexdigest()
            rand_build = run('beam-rand-build', ['erlc', '-W0', '-o', beam, otp_rand])
            if rand_build['returncode'] != 0:
                raise RuntimeError('BEAM rand dependency build failed')
            if args.otp_ordsets:
                otp_ordsets = args.otp_ordsets.resolve()
                report['otp_ordsets_source'] = str(otp_ordsets)
                report['otp_ordsets_sha256'] = hashlib.sha256(otp_ordsets.read_bytes()).hexdigest()
                ordsets_build = run('beam-ordsets-build', ['erlc', '-W0', '-o', beam, otp_ordsets])
                if ordsets_build['returncode'] != 0:
                    raise RuntimeError('BEAM ordsets dependency build failed')
            env['NERA_PROPER_RAND_BEAM'] = str(beam)
            load_ordsets = ''
            if args.otp_ordsets:
                load_ordsets = 'code:unstick_mod(ordsets), {module,ordsets} = code:load_abs(filename:join(os:getenv("NERA_PROPER_RAND_BEAM"), "ordsets")), '
            beam_command.extend(['-eval', 'code:unstick_mod(rand), {module,rand} = code:load_abs(filename:join(os:getenv("NERA_PROPER_RAND_BEAM"), "rand")), ' + load_ordsets + 'proper_probe:start(), halt().'])
        else:
            beam_command.extend(['-s', 'proper_probe', 'start', '-s', 'init', 'stop'])
        beam_run = run('beam-run', beam_command, timeout=run_timeout)
        report['beam_passed'] = check_completed(beam_run['returncode'], beam_run['stdout'], beam_run['stderr'])
        if not report['beam_passed']:
            raise RuntimeError('BEAM reference failed')
        project = work/'project'
        project.mkdir()
        (project/'proper_probe.erl').write_text(driver)
        (project/'nera.toml').write_text('[project]\nname = "proper_probe"\nentry = "proper_probe:start"\ndefer_undefined = true\n\n[deps]\nproper = { path = '+json.dumps(str(proper))+' }\n'+dependency_config)
        deps = run('native-dependencies', [nera, 'deps', 'get'], project)
        native_build = run('native-raw-build', [nera, 'build', 'proper_probe.erl', '-e', 'proper_probe:start', '-o', 'proper_probe', '--no-warnings'], project)
        report['native_source_mode'] = 'original'
        if args.beam_parse_transforms:
            sys.path.insert(0, str(repo/'scripts'))
            from prepare_erlang_dependency import prepare_dependency
            prepared = work/'prepared-proper'
            report['preparation'] = prepare_dependency(proper, [proper/'_build/default/lib/proper/ebin'], prepared)
            otp_dependency = ''
            if args.otp_rand:
                otp_source = args.otp_rand.resolve()
                raw_otp = work/'otp-random-source'
                (raw_otp/'src').mkdir(parents=True)
                (raw_otp/'src'/'rand.erl').symlink_to(otp_source)
                if args.otp_ordsets:
                    (raw_otp/'src'/'ordsets.erl').symlink_to(args.otp_ordsets.resolve())
                prepared_otp = work/'prepared-otp-random'
                report['otp_random_preparation'] = prepare_dependency(raw_otp, [], prepared_otp)
                otp_dependency = 'otp_random = { path = '+json.dumps(str(prepared_otp))+' }\n'
            project = work/'prepared-project'
            project.mkdir()
            (project/'proper_probe.erl').write_text(driver)
            (project/'nera.toml').write_text('[project]\nname = "proper_probe"\nentry = "proper_probe:start"\ndefer_undefined = true\n\n[deps]\nproper = { path = '+json.dumps(str(prepared))+' }\n'+otp_dependency+dependency_config)
            deps = run('native-prepared-dependencies', [nera, 'deps', 'get'], project)
            native_build = run('native-prepared-build', [nera, 'build', 'proper_probe.erl', '-e', 'proper_probe:start', '-o', 'proper_probe', '--no-warnings'], project)
            report['native_source_mode'] = 'beam_prepared'
        if deps['returncode'] != 0 or native_build['returncode'] != 0:
            raise RuntimeError('native dependency compilation blocked')
        native_run = run('native-run', [project/'proper_probe'], project, timeout=run_timeout)
        report['native_passed'] = check_completed(native_run['returncode'], native_run['stdout'], native_run['stderr'])
        if not report['native_passed']:
            raise RuntimeError('native quickcheck execution blocked')
        if args.generation:
            from generation import results, SEEDS
            beam_results = results(beam_run['returncode'], beam_run['stdout'], beam_run['stderr'])
            native_results = results(native_run['returncode'], native_run['stdout'], native_run['stderr'])
            report['trace_sha256'] = {
                'beam': hashlib.sha256(repr(beam_results).encode()).hexdigest(),
                'native': hashlib.sha256(repr(native_results).encode()).hexdigest(),
            }
            report['seeds'] = SEEDS
            report['traces_match'] = beam_results == native_results
            report['generation_summary'] = {'generated_per_seed': 100, 'repetitions_per_seed': 2,
                                             'shrink_properties_per_seed': 3,
                                             'minimal_counterexamples': ['[10]', '[[0,0,0]]', '[<<0,0,0>>]']}
            if not report['traces_match']:
                raise RuntimeError('BEAM and Nera generation/shrink traces differ')
        if scenario is not None:
            beam_values = scenario.results(beam_run['returncode'],beam_run['stdout'],beam_run['stderr'],args)
            native_values = scenario.results(native_run['returncode'],native_run['stdout'],native_run['stderr'],args)
            report['traces_match'] = beam_values == native_values
            report['trace_sha256'] = {'beam':hashlib.sha256(repr(beam_values).encode()).hexdigest(),
                                      'native':hashlib.sha256(repr(native_values).encode()).hexdigest()}
            report['property_summary'] = scenario.summary(beam_values,args)
            if not report['traces_match']:
                raise RuntimeError('BEAM and native upstream property traces differ')
        report['status'] = 'passed'
    except (OSError, RuntimeError, ValueError, subprocess.TimeoutExpired) as error:
        report['blocker'] = str(error)
    (work/'report.json').write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps({key: value for key, value in report.items() if key not in ('commands', 'nera_source_diff', 'preparation', 'otp_random_preparation', 'source_hashes')}))
    return 0 if report['status'] == 'passed' else 1


if __name__ == '__main__':
    raise SystemExit(main())
