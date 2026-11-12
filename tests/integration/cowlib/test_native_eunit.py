import unittest
import subprocess
import sys
import tempfile
from pathlib import Path
from native_eunit import native_source, suite_passed, controls_passed, project_suite_passed, production_dependencies, cookie_semantic_validation


class NativeEUnitTests(unittest.TestCase):
    def cookie_diagnostic(self, beam=None, native=None):
        return {'beam': {'returncode': 0, 'stdout': beam or 'COOKIE_ORDER:sample=value; Domain=example.org; HttpOnly\n'},
                'native_build_returncode': 0,
                'native': {'returncode': 0, 'stdout': native or 'COOKIE_ORDER:sample=value; HttpOnly; Domain=example.org\n'}}

    def test_semantic_cookie_probe_passes_without_promoting_raw_suite(self):
        result = cookie_semantic_validation(self.cookie_diagnostic())
        self.assertTrue(result['passed'])
        self.assertFalse(result['wire_equal'])
        identical = 'COOKIE_ORDER:sample=value; Domain=example.org; HttpOnly\n'
        exact = cookie_semantic_validation(self.cookie_diagnostic(identical, identical))
        self.assertTrue(exact['passed'])
        self.assertTrue(exact['wire_equal'])
        self.assertFalse(project_suite_passed(1, 'Failed: 1. Skipped: 0. Passed: 53.\n', 54))

    def test_semantic_cookie_probe_rejects_values_missing_and_duplicate_attributes(self):
        for wire in ('sample=other; Domain=example.org; HttpOnly',
                     'sample=value; Domain=other.org; HttpOnly',
                     'sample=value; Domain=example.org',
                     'sample=value; Domain=example.org; HttpOnly; HttpOnly',
                     'sample=value; Domain=example.org; Domain=other.org; HttpOnly',
                     'sample=value; Domain=example.org; HttpOnly; Secure'):
            with self.subTest(wire=wire):
                result = cookie_semantic_validation(self.cookie_diagnostic(native='COOKIE_ORDER:'+wire+'\n'))
                self.assertFalse(result['passed'])
        wrong = 'COOKIE_ORDER:wrong=value; Domain=example.org; HttpOnly\n'
        self.assertFalse(cookie_semantic_validation(self.cookie_diagnostic(wrong, wrong))['passed'])

    def test_semantic_cookie_probe_requires_execution_and_exact_completion(self):
        for output in ('', 'COOKIE_ORDER:sample=value; Domain=example.org; HttpOnly\n'*2,
                       'unrecognized output\nCOOKIE_ORDER:sample=value; Domain=example.org; HttpOnly\n',
                       'COOKIE_ORDER:sample=value; Domain=example.org; HttpOnly\r\n'):
            diagnostic = self.cookie_diagnostic()
            diagnostic['native']['stdout'] = output
            self.assertFalse(cookie_semantic_validation(diagnostic)['passed'])
        for key in ('beam', 'native'):
            diagnostic = self.cookie_diagnostic()
            diagnostic[key]['returncode'] = 1
            self.assertFalse(cookie_semantic_validation(diagnostic)['passed'])
        diagnostic = self.cookie_diagnostic()
        diagnostic['native_build_returncode'] = 1
        self.assertFalse(cookie_semantic_validation(diagnostic)['passed'])

    def test_single_test_summary_from_real_eunit(self):
        # cow_http2_machine has one upstream test; OTP uses a singular summary.
        with tempfile.TemporaryDirectory() as directory:
            file = Path(directory)/'single_suite.erl'
            file.write_text('-module(single_suite).\n-include_lib("eunit/include/eunit.hrl").\nidentity_test() -> ?assertEqual(7, abs(-7)).\n')
            subprocess.run(['erlc', '-o', directory, str(file)], check=True, capture_output=True)
            result = subprocess.run(['erl', '+S', '1', '-noshell', '-pa', directory, '-eval',
                'ok = eunit:test(single_suite, [verbose]), io:format("NATIVE_EUNIT_OK:single_suite~nNERA_EUNIT_OK~n"), halt().'],
                text=True, capture_output=True)
        for check in (lambda rc, out: suite_passed(rc, out, 1, 'single_suite'),
                      lambda rc, out: project_suite_passed(rc, out, 1)):
            self.assertTrue(check(result.returncode, result.stdout), result.stdout+result.stderr)
            for mutation in (result.stdout.replace('Test passed.', 'All 2 tests passed.'),
                             result.stdout+'  Test passed.\n',
                             result.stdout+'  All 2 tests passed.\n',
                             result.stdout.replace('Test passed.', 'Test passed. extra')):
                self.assertFalse(check(0, mutation), mutation)

    def test_cli_selects_http2_machine_suite(self):
        # Captain's final HTTP/2 stage must accept the public selector.
        result = subprocess.run([sys.executable, str(Path(__file__).with_name('native_eunit.py')),
                                 '--modules', 'cow_http2_machine', '--help'], text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_machine_preparation_keeps_callable_mailbox_helper(self):
        # Minimal project-owned reproduction of the upstream termination test's dependency.
        source = '''-module(cow_http2_machine).
-export([cleanup_test/0]).
-ifdef(TEST).
cleanup_test() -> self() ! cleanup, [] = collect_timeout_messages(make_ref(), make_ref()).
collect_timeout_messages(A, B) ->
    receive cleanup -> collect_timeout_messages(A, B) after 0 -> [] end.
-endif.
'''
        adapted, _ = native_source(source, 'cow_http2_machine')
        with tempfile.TemporaryDirectory() as directory:
            file = Path(directory)/'cow_http2_machine.erl'
            file.write_text(adapted)
            built = subprocess.run(['erlc', '-o', directory, str(file)], capture_output=True, text=True)
            self.assertEqual(built.returncode, 0, built.stdout+built.stderr)
            result = subprocess.run(['erl', '+S', '1', '-noshell', '-pa', directory, '-eval',
                                     '[] = cow_http2_machine:cleanup_test(), io:format("MAILBOX_HELPER_OK~n"), halt().'], capture_output=True, text=True)
            self.assertEqual((result.returncode, result.stdout), (0, 'MAILBOX_HELPER_OK\n'), result.stderr)

    def test_machine_dependencies_include_runtime_closure(self):
        # These modules implement real calls reachable from the machine's public APIs.
        dependencies = production_dependencies(['cow_http2_machine'])
        for module in ('cow_hpack', 'cow_http2', 'cow_http', 'cow_http_hd', 'cow_iolists', 'cow_link'):
            self.assertIn(module, dependencies)
        self.assertNotIn('cow_http2_machine', dependencies)
        self.assertNotIn('cow_http2', production_dependencies(['cow_http2_machine', 'cow_http2']))

    def test_cli_selects_http2_suite(self):
        # Captain's next Cowlib stage must accept the real public selector.
        result = subprocess.run([sys.executable, str(Path(__file__).with_name('native_eunit.py')),
                                 '--modules', 'cow_http2', '--help'], text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_cli_exposes_hpack_suite(self):
        # Captain's cow_hpack campaign must be selectable through the public runner.
        result = subprocess.run([sys.executable, str(Path(__file__).with_name('native_eunit.py')), '--help'], text=True, capture_output=True, check=True)
        self.assertIn('cow_hpack', result.stdout)

    def test_hpack_preparation_keeps_callable_functional_helpers(self):
        # Project-owned minimal fixture: losing either helper breaks a real call.
        source = '''-module(cow_hpack).
-export([probe_test/0]).
-ifdef(TEST).
encode_indexed_names() -> [<<"x-project-probe">>].
dyn_field_names({fields, Fields}) -> Fields;
dyn_field_names(_) -> [].
probe_test() -> [Name] = encode_indexed_names(), [Name] = dyn_field_names({fields, [Name]}), ok.
horse_probe() -> unavailable_benchmark:run().
-endif.
'''
        adapted, _ = native_source(source, 'cow_hpack')
        with tempfile.TemporaryDirectory() as directory:
            file = Path(directory)/'cow_hpack.erl'
            file.write_text(adapted)
            built = subprocess.run(['erlc', '-o', directory, str(file)], capture_output=True, text=True)
            self.assertEqual(built.returncode, 0, built.stdout+built.stderr)
            result = subprocess.run(['erl', '+S', '1', '-noshell', '-pa', directory, '-eval',
                                     'ok = cow_hpack:probe_test(), io:format("HELPERS_OK~n"), halt().'], capture_output=True, text=True)
            self.assertEqual((result.returncode, result.stdout), (0, 'HELPERS_OK\n'), result.stderr)

    def test_cli_exposes_http1_and_transfer_encoding_suites(self):
        result = subprocess.run([sys.executable, str(Path(__file__).with_name('native_eunit.py')), '--help'], text=True, capture_output=True, check=True)
        self.assertIn('cow_http1', result.stdout)
        self.assertIn('cow_http_te', result.stdout)

    def test_transfer_encoding_keeps_recursive_test_helpers_verbatim(self):
        helper = 'dripfeed(<<>>, Acc, State, F) -> F(Acc, State).\n'
        chunks = 'do_body_to_chunks(_, <<>>, Acc) -> Acc;\ndo_body_to_chunks(N, B, Acc) -> do_body_to_chunks(N, <<>>, [B|Acc]).\n'
        body = 'fragment_test() -> dripfeed(<<>>, <<>>, {}, fun(_, _) -> ok end).\n'
        source = '-module(cow_http_te).\n-ifdef(TEST).\n'+helper+chunks+body+'horse_run() -> horse:run().\n-endif.\n'
        adapted, removed = native_source(source, 'cow_http_te')
        for span in (helper, chunks, body):
            self.assertIn(span, adapted)
        self.assertEqual(removed, ['horse_run'])

    def test_http1_transfer_dependencies_include_http_runtime_closure(self):
        dependencies = production_dependencies(['cow_http1', 'cow_http_te'])
        self.assertIn('cow_http', dependencies)
        self.assertIn('cow_http_hd', dependencies)
        self.assertIn('cow_date', dependencies)
        self.assertNotIn('cow_http1', dependencies)
        self.assertNotIn('cow_http_te', dependencies)

    def test_cli_exposes_cookie_and_date_suites(self):
        result = subprocess.run([sys.executable, str(Path(__file__).with_name('native_eunit.py')), '--help'], text=True, capture_output=True, check=True)
        self.assertIn('--modules', result.stdout)
        self.assertIn('cow_cookie', result.stdout)
        self.assertIn('cow_date', result.stdout)

    def test_selected_suites_are_never_overwritten_by_dependencies(self):
        self.assertEqual(production_dependencies(['cow_cookie', 'cow_date']), [])
        self.assertEqual(production_dependencies(['cow_cookie']), ['cow_date'])
        dependencies = production_dependencies(['cow_http', 'cow_http_hd', 'cow_cookie'])
        self.assertNotIn('cow_cookie', dependencies)
        self.assertNotIn('cow_http_hd', dependencies)
        self.assertIn('cow_date', dependencies)

    def test_date_assertions_preserved_without_properties_or_benchmarks(self):
        body = 'http_date_test_() -> [{"test", fun() -> sentinel = sentinel end}].\n'
        source = '-module(cow_date).\n-ifdef(TEST).\n-include_lib("proper/include/proper.hrl").\nday_name() -> oneof([]).\nprop_http_date() -> dummy.\n'+body+'horse_date() -> horse:run().\n-endif.\n'
        adapted, removed = native_source(source, 'cow_date')
        self.assertIn(body, adapted)
        self.assertIn('-define(TEST, true).', adapted)
        self.assertNotIn('proper/include', adapted)
        self.assertNotIn('prop_http_date()', adapted)
        self.assertEqual(removed, ['day_name', 'prop_http_date', 'horse_date'])

    def test_cookie_preparation_keeps_expiry_window_helper(self):
        # Cowlib 2.22.0's Max-Age/Expires limit test calls this helper; the
        # native cow_cookie suite did not compile while preparation dropped it.
        helper = ('expiry_in_window(Got, Before, After) ->\n'
                  '\tGotSecs = calendar:datetime_to_gregorian_seconds(Got),\n'
                  '\tLow = calendar:datetime_to_gregorian_seconds(Before) + ?MAX_COOKIE_AGE,\n'
                  '\tHigh = calendar:datetime_to_gregorian_seconds(After) + ?MAX_COOKIE_AGE,\n'
                  '\tGotSecs >= Low andalso GotSecs =< High.\n')
        body = ('expiry_probe_test() ->\n'
                '\tNow = {{2026,1,1},{0,0,0}},\n'
                '\tLimit = calendar:gregorian_seconds_to_datetime(calendar:datetime_to_gregorian_seconds(Now) + ?MAX_COOKIE_AGE),\n'
                '\ttrue = expiry_in_window(Limit, Now, Now),\n'
                '\tfalse = expiry_in_window(Now, Now, Now),\n'
                '\tok.\n')
        source = ('-module(cow_cookie).\n-export([probe_test/0]).\n-define(MAX_COOKIE_AGE, 34560000).\n'
                  'probe_test() -> expiry_probe_test().\n-ifdef(TEST).\n'+body+helper+'horse_parse() -> horse:run().\n-endif.\n')
        adapted, removed = native_source(source, 'cow_cookie')
        self.assertEqual(removed, ['horse_parse'])
        with tempfile.TemporaryDirectory() as directory:
            file = Path(directory)/'cow_cookie.erl'
            file.write_text(adapted)
            built = subprocess.run(['erlc', '-o', directory, str(file)], capture_output=True, text=True)
            self.assertEqual(built.returncode, 0, built.stdout+built.stderr)
            result = subprocess.run(['erl', '+S', '1', '-noshell', '-pa', directory, '-eval',
                                     'ok = cow_cookie:probe_test(), io:format("HELPER_OK~n"), halt().'], capture_output=True, text=True)
            self.assertEqual((result.returncode, result.stdout), (0, 'HELPER_OK\n'), result.stderr)

    def test_completion_requires_exit_count_and_single_marker(self):
        # Reject ambiguous summaries and non-standalone markers without an external report.
        summary, marker = 'All 102 tests passed.', 'NATIVE_EUNIT_OK:cow_http'
        good = f'  {summary}\n{marker}\n'
        self.assertTrue(suite_passed(0, good, 102, 'cow_http'))
        for rc, out in [(1, good), ('timeout', good), (0, ''),
                        (0, good.replace('102', '101')),
                        (0, good+marker+'\n'),
                        (0, good.replace('cow_http', 'cow_http_hd')),
                        (0, good+summary+'\n'),
                        (0, good+summary.replace('102', '101')+'\n'),
                        (0, good.replace(summary, summary+' extra')),
                        (0, good.replace(marker, 'prefix_'+marker))]:
            with self.subTest(code=rc, output=out):
                self.assertFalse(suite_passed(rc, out, 102, 'cow_http'))

    def test_project_runner_requires_success_count_and_one_completion(self):
        summary, marker = 'All 396 tests passed.', 'NERA_EUNIT_OK'
        good = f'  {summary}\n{marker}\n'
        self.assertTrue(project_suite_passed(0, good, 396))
        for code, out in [(1, good), ('timeout', good), (0, ''),
                          (0, good.replace('396', '395')),
                          (0, good.replace(marker, 'WRONG_COMPLETION')),
                          (0, good+marker+'\n'),
                          (0, good+summary+'\n'),
                          (0, good+summary.replace('396', '395')+'\n'),
                          (0, good.replace(summary, summary+' extra')),
                          (0, good.replace(marker, 'prefix_'+marker))]:
            with self.subTest(code=code, output=out):
                self.assertFalse(project_suite_passed(code, out, 396))

    def test_framework_controls_reject_reporter_crash_and_false_completion(self):
        self.assertTrue(controls_passed(0, 'eunit_native_ok\n', ''))
        for code, out, err in [(1,'eunit_native_ok\n',''), (0,'',''),
                               (0,'eunit_native_ok\neunit_native_ok\n',''),
                               (0,'eunit_native_ok\n','Error in process <0.32.0>: error:badarg')]:
            self.assertFalse(controls_passed(code,out,err))

    def test_assertion_body_preserved_and_no_closure_adapter_added(self):
        body = 'sample_test() -> ?assertEqual(expected, parse(<<"x">>)).\n'
        source = '-module(cow_http).\nparse(X) -> X.\n-ifdef(TEST).\n'+body+'horse_run() -> horse:run().\n-endif.\n'
        adapted, removed = native_source(source, 'cow_http')
        self.assertIn(body, adapted)
        self.assertIn('-compile({parse_transform,eunit_autoexport}).', adapted)
        self.assertNotIn('nera_upstream_start', adapted)
        self.assertEqual(removed, ['horse_run'])


if __name__ == '__main__':
    unittest.main()
