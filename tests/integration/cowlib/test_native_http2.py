"""Captain's cow_http2 stage: validate real campaign readings."""
import json
import os
from pathlib import Path
import unittest

from native_eunit import project_suite_passed, suite_passed
from runner import sha


@unittest.skipUnless(os.environ.get('COWLIB_HTTP2_REPORT'),
                     'requires a real external cow_http2 campaign report')
class Http2CampaignReadings(unittest.TestCase):
    def test_original_suite_readings(self):
        path = Path(os.environ['COWLIB_HTTP2_REPORT'])
        report = json.loads(path.read_text())
        self.assertEqual(report['status'], 'passed')
        self.assertEqual(list(report['modules']), ['cow_http2'])
        module = report['modules']['cow_http2']
        inventory = {'parse_ping_test': 1, 'parse_windows_update_test': 1,
                     'parse_settings_test': 1}
        self.assertEqual(module['inventory'], inventory)
        count = sum(inventory.values())
        self.assertEqual(module['expected'], count)
        self.assertEqual(module['properties'], [])  # No upstream PropEr suite at this pin.
        self.assertEqual(module['removed'], [])
        for engine, label in (('beam', 'cow_http2-beam'), ('native', 'cow_http2-native-test')):
            with self.subTest(engine=engine):
                command = next(c for c in report['commands'] if c['label'] == label)
                output = path.parent / (label + '.stdout')
                self.assertEqual(sha(output), command['stdout_sha256'])
                good = output.read_text()
                check = ((lambda code, out: suite_passed(code, out, count, 'cow_http2'))
                         if engine == 'beam' else
                         (lambda code, out: project_suite_passed(code, out, count)))
                self.assertTrue(check(command['returncode'], good))
                self.assertTrue(module[engine]['passed'])
        for engine in ('beam', 'native'):
            self.assertTrue(report['framework_controls'][engine]['passed'])


if __name__ == '__main__':
    unittest.main()
