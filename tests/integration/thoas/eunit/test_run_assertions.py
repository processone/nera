import subprocess
import unittest

from run_assertions import classify_result


class CompletionTests(unittest.TestCase):
    def result(self, code=0, out='', err=''):
        return subprocess.CompletedProcess([], code, out, err)

    def test_recorded_failure_is_explicitly_classified(self):
        reason = 'error:{badmatch,wrong}'
        result = self.result(1, err=f'Error in process <0.1.0>: {reason}\nStacktrace...')
        self.assertEqual(classify_result(27, result, {27: reason}), 'known_failure')

    def test_success_requires_own_marker_and_zero_exit(self):
        self.assertEqual(classify_result(1, self.result(out='THOAS_ASSERTION_OK:1\n'), {}), 'passed')
        for result in [self.result(), self.result(out='THOAS_ASSERTION_OK:2\n'),
                       self.result(1, 'THOAS_ASSERTION_OK:1\n'),
                       self.result(out='noise\nTHOAS_ASSERTION_OK:1\n')]:
            self.assertEqual(classify_result(1, result, {}), 'unexpected_failure')

    def test_known_case_must_not_hide_a_different_failure(self):
        known = {27: 'error:{badmatch,wrong}'}
        for result in [self.result(-11), self.result(1),
                       self.result(1, err='Error in process <0.1.0>: error:badarg\n')]:
            self.assertEqual(classify_result(27, result, known), 'unexpected_failure')

    def test_unlisted_failure_is_a_regression(self):
        result = self.result(1, err='Error in process <0.1.0>: error:badarg\n')
        self.assertEqual(classify_result(1, result, {}), 'unexpected_failure')

    def test_fixed_known_case_requires_baseline_update(self):
        result = self.result(out='THOAS_ASSERTION_OK:27\n')
        self.assertEqual(classify_result(27, result, {27: 'error:badarg'}), 'unexpected_success')

    def test_only_source_line_numbers_are_normalized(self):
        result = self.result(1, err='Error in process <0.1.0>: error:{assertEqual,[{line,149},{value,42}]}\n')
        expected = 'error:{assertEqual,[{line,*},{value,42}]}'
        self.assertEqual(classify_result(21, result, {21: expected}), 'known_failure')
        self.assertEqual(classify_result(21, result, {21: expected.replace('42', '43')}), 'unexpected_failure')
