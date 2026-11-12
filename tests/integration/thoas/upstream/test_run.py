import unittest
from run import verdict


class VerdictTests(unittest.TestCase):
    def test_complete_run_passes(self):
        self.assertEqual(verdict(True, {'total': 338, 'failures': 0, 'excluded': 0, 'skipped': 0}), 'passed')

    def test_missing_tests_cannot_pass(self):
        self.assertEqual(verdict(True, {'total': 332, 'failures': 0, 'excluded': 0, 'skipped': 0}), 'incomplete')

    def test_missing_summary_cannot_pass(self):
        self.assertEqual(verdict(True, None), 'incomplete')

    def test_failure_cannot_pass(self):
        self.assertEqual(verdict(True, {'total': 338, 'failures': 1, 'excluded': 0, 'skipped': 0}), 'failed')

    def test_eunit_failure_cannot_pass(self):
        self.assertEqual(verdict(False, {'total': 338, 'failures': 0, 'excluded': 0, 'skipped': 0}), 'failed')

    def test_skipped_or_excluded_cannot_pass(self):
        for field in ('skipped', 'excluded'):
            stats = {'total': 338, 'failures': 0, 'excluded': 0, 'skipped': 0}
            stats[field] = 1
            self.assertEqual(verdict(True, stats), 'incomplete')

class TransportTests(unittest.TestCase):
    def test_missing_reply_is_failure_even_with_exit_zero(self):
        import tempfile
        from pathlib import Path
        from unittest.mock import patch
        import subprocess
        from run import bridge
        with tempfile.TemporaryDirectory() as directory:
            with patch('run.subprocess.run', return_value=subprocess.CompletedProcess([], 0, b'', b'')):
                self.assertEqual(bridge('native', directory), 1)
            self.assertIn('missing native reply', (Path(directory) / 'transport.json').read_text())

    def test_crash_is_failure_even_if_reply_exists(self):
        import tempfile
        from pathlib import Path
        from unittest.mock import patch
        import subprocess
        from run import bridge
        with tempfile.TemporaryDirectory() as directory:
            (Path(directory) / 'reply.etf').write_bytes(b'partial result')
            with patch('run.subprocess.run', return_value=subprocess.CompletedProcess([], -11, b'', b'')):
                self.assertEqual(bridge('native', directory), 1)

    def test_timeout_is_recorded_as_failure(self):
        import tempfile
        from pathlib import Path
        from unittest.mock import patch
        import subprocess
        from run import bridge
        with tempfile.TemporaryDirectory() as directory:
            with patch('run.subprocess.run', side_effect=subprocess.TimeoutExpired('native', 10, output=b'partial output', stderr=b'diagnostic')):
                self.assertEqual(bridge('native', directory), 1)
            self.assertIn('timed out', (Path(directory) / 'transport.json').read_text())
            self.assertTrue((Path(directory) / 'stdout').is_file(), 'timeout must preserve partial output')
            self.assertEqual((Path(directory) / 'stdout').read_bytes(), b'partial output')
            self.assertEqual((Path(directory) / 'stderr').read_bytes(), b'diagnostic')


if __name__ == '__main__':
    unittest.main()
