import unittest
from runner import compare_outcome, parse_output, external_workdir
from pathlib import Path

class SafetyTests(unittest.TestCase):
    def test_identical_crash_is_not_success(self):
        with self.assertRaises(ValueError):
            parse_output('', 'one', 1)
    def test_missing_completion(self):
        with self.assertRaises(ValueError):
            parse_output('COWLIB_CASE:one:[131,104,2,119,6,114,101,116,117,114,110,97,1]\n', 'one', 0)
    def test_wrong_id(self):
        with self.assertRaises(ValueError):
            parse_output('COWLIB_CASE:other:[131,97,1]\nCOWLIB_DONE:other\n', 'one', 0)
    def test_duplicate_output(self):
        with self.assertRaises(ValueError):
            parse_output('COWLIB_DONE:one\nCOWLIB_DONE:one\n', 'one', 0)
    def test_expected_exception_required(self):
        v=('tuple', (('atom','return'), ('integer',1)))
        with self.assertRaises(ValueError): compare_outcome(v,v,'exception')
    def test_error_reason_is_compared(self):
        a=('tuple', (('atom','exception'), ('atom','error'), ('atom','badarg')))
        b=('tuple', (('atom','exception'), ('atom','error'), ('atom','undef')))
        with self.assertRaises(ValueError): compare_outcome(a,b,'exception')
    def test_binary_bytes_are_compared(self):
        a=('tuple', (('atom','return'), ('binary','00')))
        b=('tuple', (('atom','return'), ('binary','01')))
        with self.assertRaises(ValueError): compare_outcome(a,b,'return')
    def test_valid_return(self):
        value = parse_output('COWLIB_CASE:one:[131,104,2,119,6,114,101,116,117,114,110,97,1]\nCOWLIB_DONE:one\n', 'one', 0)
        compare_outcome(value, value, 'return')
    def test_valid_exception(self):
        value = ('tuple', (('atom','exception'), ('atom','error'), ('atom','badarg')))
        compare_outcome(value, value, 'exception')
    def test_invalid_etf(self):
        with self.assertRaises(ValueError):
            parse_output('COWLIB_CASE:one:[131,97]\nCOWLIB_DONE:one\n', 'one', 0)
    def test_extra_output(self):
        with self.assertRaises(ValueError):
            parse_output('COWLIB_CASE:one:[131,97,1]\nCOWLIB_DONE:one\nextra\n', 'one', 0)
    def test_timeout_is_failure(self):
        with self.assertRaises(ValueError):
            parse_output('', 'one', 'timeout')
    def test_non_byte_payload(self):
        with self.assertRaises(ValueError):
            parse_output('COWLIB_CASE:one:[256]\nCOWLIB_DONE:one\n', 'one', 0)
    def test_repo_workdir_rejected(self):
        with self.assertRaises(ValueError): external_workdir(Path(__file__).parent)

class CookieAttributeTests(unittest.TestCase):
    @staticmethod
    def outcome(wire):
        return ('tuple', (('atom', 'return'), ('binary', wire.hex())))

    def compare(self, a, b, mode='set_cookie_attributes'):
        compare_outcome(self.outcome(a), self.outcome(b), 'return', mode)

    def test_attribute_order_is_not_significant(self):
        self.compare(b'sid=abc; Path=/; HttpOnly; SameSite=Strict; Secure',
                     b'sid=abc; Secure; HttpOnly; SameSite=Strict; Path=/')

    def test_default_remains_byte_exact(self):
        with self.assertRaises(ValueError):
            compare_outcome(self.outcome(b'a=b; Secure; HttpOnly'),
                            self.outcome(b'a=b; HttpOnly; Secure'), 'return')

    def test_cookie_and_attribute_bytes_are_preserved(self):
        original = b'sid=abc; Path=/; HttpOnly; SameSite=Strict; Secure'
        for changed in (
            original.replace(b'sid=', b'other='),
            original.replace(b'abc', b'def'),
            original.replace(b'Path=/', b'Path=/other'),
            original.replace(b'Strict', b'Lax'),
            original.replace(b'HttpOnly', b'httponly'),
            original.replace(b'; Secure', b''),
            original + b'; Domain=example.org',
            original + b'; Secure',
            original.replace(b'; Path', b';  Path'),
        ):
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                self.compare(original, changed)

    def test_duplicate_attributes_are_rejected_even_when_equal(self):
        for wire in (b'a=b; Secure; Secure', b'a=b; Path=/; Path=/other',
                     b'a=b; Secure; secure'):
            with self.subTest(wire=wire), self.assertRaises(ValueError):
                self.compare(wire, wire)

    def test_malformed_wire_is_rejected_even_when_equal(self):
        for wire in (b'', b'a=b; ', b'a=b;Secure', b'a=b; Secure\r\n'):
            with self.subTest(wire=wire), self.assertRaises(ValueError):
                self.compare(wire, wire)

    def test_non_binary_result_is_rejected(self):
        value = ('tuple', (('atom', 'return'), ('integer', 1)))
        with self.assertRaises(ValueError):
            compare_outcome(value, value, 'return', 'set_cookie_attributes')

    def test_exception_is_not_a_cookie(self):
        value = ('tuple', (('atom', 'exception'), ('atom', 'error'), ('atom', 'badarg')))
        with self.assertRaises(ValueError):
            compare_outcome(value, value, 'return', 'set_cookie_attributes')

    def test_unknown_comparison_fails_closed(self):
        with self.assertRaises(ValueError):
            self.compare(b'a=b', b'a=b', 'unknown')

class PseudoHeaderOrderTests(unittest.TestCase):
    @staticmethod
    def outcome(headers):
        return ('tuple', (('atom', 'return'), ('list', tuple(
            ('tuple', (('binary', name.hex()), ('binary', value.hex())))
            for name, value in headers))))

    def compare(self, a, b, mode='request_pseudo_headers'):
        compare_outcome(self.outcome(a), self.outcome(b), 'return', mode)

    def test_pseudo_prefix_permutation_is_equivalent(self):
        prefix = [(b':method', b'GET'), (b':path', b'/'), (b':scheme', b'https')]
        suffix = [(b'x-repeat', b'first'), (b'x-repeat', b'second')]
        self.compare(prefix + suffix, list(reversed(prefix)) + suffix)

    def test_comparison_is_scoped_to_one_corpus_case(self):
        import json
        cases = json.loads(Path(__file__).with_name('corpus.json').read_text())
        selected = [case['id'] for case in cases
                    if case.get('comparison') == 'request_pseudo_headers']
        self.assertEqual(selected, ['http_merge_request'])

    def test_default_comparison_remains_exact(self):
        a = [(b':method', b'GET'), (b':path', b'/')]
        with self.assertRaises(ValueError):
            self.compare(a, list(reversed(a)), 'exact')

    def test_names_values_and_regular_suffix_are_preserved(self):
        original = [(b':method', b'GET'), (b':path', b'/'),
                    (b'x-repeat', b'first'), (b'x-repeat', b'second')]
        for changed in (
            [(b':method', b'POST')] + original[1:],
            [original[0], (b':path', b'/other')] + original[2:],
            [original[0], (b':scheme', b'/')] + original[2:],
            original[1:], original + [(b':authority', b'host')],
            original[:2] + list(reversed(original[2:])),
            original[:-1], original + [(b'other', b'value')],
        ):
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                self.compare(original, changed)

    def test_duplicate_pseudo_headers_rejected_even_when_equal(self):
        for headers in ([(b':method', b'GET'), (b':method', b'GET')],
                        [(b':path', b'/'), (b':path', b'/other')]):
            with self.subTest(headers=headers), self.assertRaises(ValueError):
                self.compare(headers, headers)

    def test_pseudo_header_after_regular_rejected_even_when_equal(self):
        headers = [(b':method', b'GET'), (b'ordinary', b'value'), (b':path', b'/')]
        with self.assertRaises(ValueError):
            self.compare(headers, headers)

    def test_malformed_fields_rejected_even_when_equal(self):
        malformed = (
            ('binary', '00'), ('improper_list', ()),
            ('list', (('integer', 1),)),
            ('list', (('tuple', (('binary', '3a70617468'), ('integer', 1))),)),
            ('list', (('tuple', (('binary', 'zz'), ('binary', '00'))),)),
            ('list', (('tuple', (('binary', ''), ('binary', '00'))),)),
            ('list', (('tuple', (('binary', b':unknown'.hex()), ('binary', '00'))),)),
            ('list', (('tuple', (('binary', b':METHOD'.hex()), ('binary', '00'))),)),
            ('list', (('tuple', (('atom', 'path'), ('binary', '00'))),)),
        )
        for value in malformed:
            outcome = ('tuple', (('atom', 'return'), value))
            with self.subTest(value=value), self.assertRaises(ValueError):
                compare_outcome(outcome, outcome, 'return', 'request_pseudo_headers')

    def test_exceptions_cannot_use_pseudo_header_comparison(self):
        value = ('tuple', (('atom', 'exception'), ('atom', 'error'), ('atom', 'badarg')))
        with self.assertRaises(ValueError):
            compare_outcome(value, value, 'exception', 'request_pseudo_headers')

class BeamBuildTests(unittest.TestCase):
    def test_successful_exit_without_beam_files_is_rejected(self):
        import tempfile
        from pathlib import Path
        from runner import verify_beam_modules
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(ValueError, 'cow_http, cow_qs'):
                verify_beam_modules(Path(directory), ['cow_qs', 'cow_http'])

    def test_required_module_files_are_present(self):
        import tempfile
        from pathlib import Path
        from runner import verify_beam_modules
        with tempfile.TemporaryDirectory() as directory:
            ebin = Path(directory)
            (ebin / 'cow_qs.beam').write_bytes(b'fixture')
            verify_beam_modules(ebin, ['cow_qs', 'cow_qs'])

if __name__ == '__main__': unittest.main()
