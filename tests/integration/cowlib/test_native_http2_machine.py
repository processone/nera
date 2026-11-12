"""Captain's cow_http2_machine stage: verify actual campaign readings, not invented traces."""
import json
import os
from pathlib import Path
import unittest

from native_eunit import project_suite_passed, suite_passed
from runner import sha


@unittest.skipUnless(os.environ.get('COWLIB_HTTP2_MACHINE_REPORT'),
                     'requires a real external cow_http2_machine campaign report')
class Http2MachineCampaignReadings(unittest.TestCase):
    def test_original_suite_readings(self):
        path = Path(os.environ['COWLIB_HTTP2_MACHINE_REPORT'])
        report = json.loads(path.read_text())
        self.assertEqual(report['status'], 'passed')
        self.assertEqual(list(report['modules']), ['cow_http2_machine'])
        module = report['modules']['cow_http2_machine']
        inventory = {'terminate_removes_stray_timeout_messages_test': 1}
        self.assertEqual(module['inventory'], inventory)
        count = sum(inventory.values())
        self.assertEqual(module['expected'], count)
        self.assertEqual(module['properties'], [])  # No upstream PropEr properties at the pin.
        self.assertEqual(module['removed'], [])
        for engine, label in (('beam', 'cow_http2_machine-beam'),
                              ('native', 'cow_http2_machine-native-test')):
            with self.subTest(engine=engine):
                command = next(c for c in report['commands'] if c['label'] == label)
                output = path.parent / (label + '.stdout')
                self.assertEqual(sha(output), command['stdout_sha256'])
                check = ((lambda code, out: suite_passed(code, out, count, 'cow_http2_machine'))
                         if engine == 'beam' else
                         (lambda code, out: project_suite_passed(code, out, count)))
                self.assertTrue(check(command['returncode'], output.read_text()))
                self.assertTrue(module[engine]['passed'])
        for engine in ('beam', 'native'):
            self.assertTrue(report['framework_controls'][engine]['passed'])


if __name__ == '__main__':
    unittest.main()
