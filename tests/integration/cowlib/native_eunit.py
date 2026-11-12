"""Run original Cowlib functional EUnit suites using externally installed OTP EUnit."""
import argparse
import json
import os
import platform
from pathlib import Path
import re
import shutil
import subprocess
import sys
from runner import HERE, ROOT, HTTP_DEPENDENCIES, external_workdir, sha, cookie_attributes
from upstream_http import adapt_source, functional_inventory
from upstream_cookie_date import PROPER_COMMIT


DEFAULT_SUITES = ('cow_http', 'cow_http_hd')
SUPPORTED_SUITES = DEFAULT_SUITES + ('cow_cookie', 'cow_date', 'cow_http1', 'cow_http_te', 'cow_hpack', 'cow_http2', 'cow_http2_machine')


def production_dependencies(modules):
    dependencies = set()
    for module in modules:
        if module in DEFAULT_SUITES + ('cow_http1', 'cow_http_te'):
            dependencies.update(('cow_http', *HTTP_DEPENDENCIES))
        elif module == 'cow_http2_machine':
            dependencies.update(('cow_http', *HTTP_DEPENDENCIES, 'cow_hpack', 'cow_iolists'))
        elif module == 'cow_cookie':
            dependencies.add('cow_date')
    # Never overwrite an adapted test module with its production source.
    return sorted(dependencies - set(modules))


def completed_suite(code, output, count, marker):
    # One exact summary and one standalone completion, not a matching substring
    # among duplicate or contradictory summaries from an incomplete campaign.
    summaries = re.findall(r'^[ \t]*(All \d+ tests passed\.|Test passed\.)[ \t]*$', output, re.M)
    expected = 'Test passed.' if count == 1 else f'All {count} tests passed.'
    return code == 0 and summaries == [expected] and output.splitlines().count(marker) == 1


def suite_passed(code, output, count, module):
    return completed_suite(code, output, count, f'NATIVE_EUNIT_OK:{module}')


def project_suite_passed(code, output, count):
    return completed_suite(code, output, count, 'NERA_EUNIT_OK')


def cookie_semantic_validation(diagnostic):
    result = {'case': 'Domain / HttpOnly', 'comparison': 'set_cookie_attributes',
              'scope': 'project-owned probe; independent of raw upstream EUnit verdict',
              'passed': False, 'wire_equal': None}
    try:
        if diagnostic.get('native_build_returncode') != 0:
            raise ValueError('native probe did not compile')
        wires = []
        for engine in ('beam', 'native'):
            execution = diagnostic.get(engine, {})
            if execution.get('returncode') != 0:
                raise ValueError(engine+' probe did not execute successfully')
            output = execution.get('stdout', '')
            if not output.endswith('\n') or output.count('\n') != 1 or not output.startswith('COOKIE_ORDER:'):
                raise ValueError(engine+' probe has missing, duplicate or unexpected output')
            # Remove only the framing newline, preserving every header byte.
            wires.append(output[len('COOKIE_ORDER:'):-1].encode('utf-8'))
        result['wire_equal'] = wires[0] == wires[1]
        # Reuse the existing comparator; its duplicate rejection is essential.
        canonical = lambda wire: cookie_attributes(('binary', wire.hex()))
        expected = canonical(b'sample=value; Domain=example.org; HttpOnly')
        if any(canonical(wire) != expected for wire in wires):
            raise ValueError('cookie name, value or attributes differ from the probe inputs')
        result['passed'] = True
    except ValueError as error:
        result['reason'] = str(error)
    return result


def controls_passed(code, output, stderr):
    return code == 0 and output.splitlines().count('eunit_native_ok') == 1 and 'Error in process' not in stderr


def native_source(source, module):
    # Preserve the original fragmentation helpers, including every clause.
    helpers = {'cow_http_te': ('dripfeed', 'do_body_to_chunks'),
               'cow_hpack': ('encode_indexed_names', 'dyn_field_names'),
               'cow_http2_machine': ('collect_timeout_messages',)}.get(module, ())
    adapted, removed = adapt_source(source, module, test_helpers=helpers)
    adapted = adapted.replace(f'-module({module}).',
        f'-module({module}).\n-compile({{parse_transform,eunit_autoexport}}).', 1)
    return adapted, removed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('upstream', 'proper', 'nera', 'eunit', 'gb-trees', 'work-dir'):
        parser.add_argument('--'+name, type=Path, required=True)
    parser.add_argument('--modules', nargs='+', choices=SUPPORTED_SUITES,
                        default=DEFAULT_SUITES, help='functional EUnit suites (default: HTTP)')
    args = parser.parse_args()
    modules = list(dict.fromkeys(args.modules))
    work = external_workdir(args.work_dir)
    work.mkdir(parents=True, exist_ok=True)
    upstream, proper, nera, eunit, tree = (p.resolve() for p in
        (args.upstream, args.proper, args.nera, args.eunit, args.gb_trees))
    pin = json.loads((HERE / 'upstream.json').read_text())
    report = {'status': 'failed', 'scope': 'Project nera test with external OTP EUnit; Functional tests only; properties are validated separately.',
              'modules': {}, 'commands': [], 'upstream': pin}
    env = {k:v for k,v in os.environ.items() if k not in ('ERL_FLAGS', 'ERL_AFLAGS', 'ERL_ZFLAGS', 'ERL_LIBS', 'EUNIT')}
    env['ERL_LIBS'] = str(proper/'_build/default/lib')
    def run(argv, label, cwd=work, timeout=180):
        argv = [str(arg) for arg in argv]
        try:
            result = subprocess.run(argv, cwd=cwd, env=env, capture_output=True, timeout=timeout)
            out, err, code = result.stdout, result.stderr, result.returncode
        except subprocess.TimeoutExpired as error:
            out, err, code = error.stdout or b'', error.stderr or b'', 'timeout'
        stdout, stderr = work/(label+'.stdout'), work/(label+'.stderr')
        stdout.write_bytes(out); stderr.write_bytes(err)
        report['commands'].append({'argv': argv, 'cwd': str(cwd), 'label': label, 'returncode': code,
                                  'stdout_sha256': sha(stdout), 'stderr_sha256': sha(stderr)})
        return out.decode(errors='replace'), code
    def require(argv, label, cwd=work):
        out, code = run(argv, label, cwd)
        if code != 0:
            raise ValueError(f'{label}: {code}; see external logs')
        return out.strip()
    try:
        external_workdir(upstream)
        for source in (upstream, proper, eunit, ROOT):
            if work == source or source in work.parents or work in source.parents:
                raise ValueError('work directory must be separate from source trees')
        if require(['git', '-C', proper, 'rev-parse', 'HEAD'], 'proper-head') != PROPER_COMMIT:
            raise ValueError('wrong PropEr revision')
        if require(['git', '-C', upstream, 'rev-parse', 'HEAD'], 'cowlib-head') != pin['commit']:
            raise ValueError('wrong Cowlib revision')
        if require(['git', '-C', upstream, 'status', '--porcelain', '--untracked-files=all', '--ignored'], 'cowlib-clean'):
            raise ValueError('dirty Cowlib checkout')
        if not (eunit/'src/eunit.erl').is_file() or not tree.is_file():
            raise ValueError('OTP source distribution required')
        report['harness_sources'] = {name:sha(HERE/name) for name in
            ('native_eunit.py', 'upstream_http.py', 'upstream_cookie_date.py', 'runner.py', 'upstream.json')}
        report['cowlib_sources'] = {str(p.relative_to(upstream)):sha(p)
            for folder in ('src', 'include') for p in sorted((upstream/folder).rglob('*'))
            if p.is_file() and p.suffix in ('.erl', '.hrl')}
        source_names = require(['git', 'ls-files', '--', 'crates', 'erlang', 'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml'], 'nera-source-files', ROOT).splitlines()
        report['nera_sources'] = {name:sha(ROOT/name) for name in source_names}
        report.update(platform=platform.platform(), harness_sha256=sha(Path(__file__)), nera_commit=require(['git', 'rev-parse', 'HEAD'], 'nera-head', ROOT),
                      nera_worktree_status=require(['git', 'status', '--porcelain'], 'nera-status', ROOT),
                      nera_sha256=sha(nera), runtime_sha256=sha(nera.parent/'libnera_runtime.a'),
                      eunit_path=str(eunit), eunit_sources={str(p.relative_to(eunit)):sha(p) for p in sorted(eunit.rglob('*')) if p.is_file() and p.suffix in ('.erl', '.hrl')},
                      gb_trees_source=str(tree), gb_trees_sha256=sha(tree),
                      otp=require(['erl', '-noshell', '-eval', 'io:format("~s",[erlang:system_info(system_version)]),halt().'], 'otp'))
        beam = work/'beam'; beam.mkdir(exist_ok=True)
        dependency_modules = production_dependencies(modules)
        dependencies = [upstream/'src'/f'{m}.erl' for m in dependency_modules]
        if dependencies:
            require(['erlc', '-I', upstream/'include', '-o', beam, *dependencies], 'beam-dependencies')
        require(['erlc', '-DTEST', '+export_all', '+{parse_transform,eunit_autoexport}', '-I', upstream/'include', '-o', beam,
                 *[upstream/'src'/f'{m}.erl' for m in modules]], 'beam-suites')
        project = work/'project'; src = project/'src'; src.mkdir(parents=True, exist_ok=True)
        deps = project/'.nera/deps'; deps.mkdir(parents=True, exist_ok=True)
        if (deps/'eunit').exists() and (deps/'eunit').resolve() != eunit:
            raise ValueError('work directory contains a different EUnit dependency')
        if not (deps/'eunit').exists():
            (deps/'eunit').symlink_to(eunit, target_is_directory=True)
        support = deps/'otp_support/src'; support.mkdir(parents=True, exist_ok=True)
        if not (support/'gb_trees.erl').exists():
            (support/'gb_trees.erl').symlink_to(tree)
        config = ('[project]\nname = "cowlib_eunit"\nentry = "cowlib_eunit:start"\noutput = "run"\n'
                  'defer_undefined = true\ninclude = '+json.dumps([str(upstream/'include'), str(upstream/'src')])+'\n\n'
                  '[deps]\neunit = { path = '+json.dumps(str(eunit))+' }\n')
        (project/'nera.toml').write_text(config)
        for module in modules:
            original = (upstream/'src'/f'{module}.erl').read_text()
            inventory = functional_inventory(original)
            expr = ','.join(f'io:format("COUNT:~w~n",[{"length("+module+":"+name+"())" if gen else "1"}])' for name, gen in inventory)+',halt().'
            out = require(['erl','-noshell','-pa',beam,'-eval',expr], module+'-inventory')
            counts = re.findall(r'^COUNT:(\d+)$',out,re.M)
            if len(counts) != len(inventory): raise ValueError('incomplete test inventory')
            count = sum(map(int, counts))
            if not inventory or len(set(name for name, _ in inventory)) != len(inventory) or count <= 0:
                raise ValueError('empty or duplicate test inventory')
            record = {'expected':count, 'inventory': {name:int(n) for (name, _), n in zip(inventory, counts)},
                      'properties': re.findall(r'^(prop_\w+)\(\) ->', original, re.M)}; report['modules'][module] = record
            expr = f'case eunit:test({module},[verbose]) of ok -> io:format("NATIVE_EUNIT_OK:{module}~n"),halt(); _ -> halt(1) end.'
            out, code = run(['erl','-noshell','-pa',beam,'-eval',expr],module+'-beam')
            record['beam'] = {'returncode':code,'passed':suite_passed(code,out,count,module)}
            adapted, removed = native_source(original,module)
            if functional_inventory(adapted) != inventory:
                raise ValueError('native preparation changed the functional inventory')
            file = src/f'{module}.erl'; file.write_text(adapted)
            record.update(adapted_sha256=sha(file), original_sha256=sha(upstream/'src'/f'{module}.erl'), removed=removed)
        for module in dependency_modules:
            shutil.copy2(upstream/'src'/f'{module}.erl', src)
        for module, record in report['modules'].items():
            # Exercise the public project runner; its entry and executable are
            # generated by Nera, not by this Cowlib harness.
            (project/'nera.toml').write_text(config+'\n[test]\nframework = "eunit"\nmodules = '+json.dumps([module])+'\n')
            out, code = run([nera, 'test', module, '--verbose', '--no-warnings', '--timeout', '120'], module+'-native-test', project, 600)
            record['native'] = {'returncode':code,'passed':project_suite_passed(code,out,record['expected'])}
            record['runner'] = 'nera test (project EUnit configuration)'
            record['failure_lines'] = [line.strip() for line in out.splitlines() if '*failed*' in line or 'Failed:' in line or 'Passed:' in line]
            passed_count = re.search(r'Passed:\s*(\d+)', out)
            failed_count = re.search(r'Failed:\s*(\d+)', out)
            skipped_count = re.search(r'Skipped:\s*(\d+)', out)
            if record['native']['passed']:
                record.update(native_passed=record['expected'], native_failed=0, native_skipped=0)
            elif passed_count and failed_count:
                record.update(native_passed=int(passed_count.group(1)), native_failed=int(failed_count.group(1)), native_skipped=int(skipped_count.group(1)) if skipped_count else None)
            print(module+': '+json.dumps(record['native']),flush=True)
        # API controls use an explicit entry and therefore restore build config.
        (project/'nera.toml').write_text(config)
        # Positive cases and deliberate failures must reach the expected API
        # results without crashing an EUnit reporter process.
        fixture = HERE.parent/'eunit/eunit_native.erl'
        require(['erlc', '-o', beam, fixture], 'framework-controls-beam-build')
        out, code = run(['erl', '-noshell', '-pa', beam, '-eval', 'eunit_native:test(),eunit:stop(),halt().'], 'framework-controls-beam')
        controls = {'fixture_sha256':sha(fixture), 'beam': {'returncode':code, 'passed':controls_passed(code, out, (work/'framework-controls-beam.stderr').read_text(errors='replace'))}}
        report['framework_controls'] = controls
        shutil.copy2(fixture, src/'eunit_native.erl')
        (src/'cowlib_eunit.erl').write_text('-module(cowlib_eunit).\n-export([start/0]).\nstart() -> eunit_native:test(), eunit:stop().\n')
        exe = project/'run'; exe.unlink(missing_ok=True)
        _, code = run([nera, 'build', '--no-warnings'], 'framework-controls-native-build', project, 600)
        controls['native_build_returncode'] = code
        if code == 0:
            controls['executable_sha256'] = sha(exe)
            out, rc = run([exe], 'framework-controls-native', project, 60)
            controls['native'] = {'returncode':rc,'passed':controls_passed(rc, out, (work/'framework-controls-native.stderr').read_text(errors='replace'))}
        if 'cow_cookie' in modules:
            # Own diagnostic inputs, independent of the upstream test literals.
            probe = HERE/'cookie_order_probe.erl'
            require(['erlc', '-o', beam, probe], 'cookie-order-beam-build')
            out, code = run(['erl', '-noshell', '-pa', beam, '-eval', 'cookie_order_probe:start(),halt().'], 'cookie-order-beam')
            diagnostic = {'fixture_sha256':sha(probe), 'beam':{'returncode':code, 'stdout':out}}
            shutil.copy2(probe, src/probe.name)
            exe = project/'cookie_order_probe'
            exe.unlink(missing_ok=True)
            _, code = run([nera, 'build', '--no-warnings', '-e', 'cookie_order_probe:start', '-o', exe], 'cookie-order-native-build', project, 600)
            diagnostic['native_build_returncode'] = code
            if code == 0:
                out, rc = run([exe], 'cookie-order-native', project, 30)
                diagnostic['native'] = {'returncode':rc, 'stdout':out}
                diagnostic['executable_sha256'] = sha(exe)
            diagnostic['semantic_validation'] = cookie_semantic_validation(diagnostic)
            report['cookie_attribute_order'] = diagnostic
            print('cookie semantic: '+json.dumps(diagnostic['semantic_validation']), flush=True)
        for name, digest in report['harness_sources'].items():
            if sha(HERE/name) != digest:
                raise ValueError('harness changed during validation')
        for name, digest in report['nera_sources'].items():
            if sha(ROOT/name) != digest:
                raise ValueError('compiler source changed during validation')
        for name, digest in report['cowlib_sources'].items():
            if sha(upstream/name) != digest:
                raise ValueError('Cowlib source changed during validation')
        if sha(nera) != report['nera_sha256'] or sha(nera.parent/'libnera_runtime.a') != report['runtime_sha256']:
            raise ValueError('compiler/runtime changed during validation')
        if controls['beam']['passed'] and controls.get('native',{}).get('passed',False) and all(r['beam']['passed'] and r.get('native',{}).get('passed',False) for r in report['modules'].values()): report['status']='passed'
    except (OSError, ValueError) as error:
        report['error'] = str(error)
    finally:
        (work/'report.json').write_text(json.dumps(report,indent=2)+'\n')
        print(f'{report["status"]}: {work / "report.json"}',flush=True)
    return 0 if report['status']=='passed' else 1


if __name__ == '__main__':
    sys.exit(main())
