import unittest
from native import completed


class CompletionTests(unittest.TestCase):
    def test_requires_both_results_and_completion(self):
        good = 'PROPER_TRUE:true\nPROPER_FALSE:false\nPROPER_DONE\n'
        self.assertTrue(completed(0, good, ''))
        for code, out, err in [
            (1, good, ''), (None, good, ''), (0, '', ''),
            (0, good.replace('PROPER_FALSE:false', 'PROPER_FALSE:true'), ''),
            (0, good.replace('PROPER_DONE\n', ''), ''),
            (0, good + 'PROPER_DONE\n', ''),
            (0, good + 'PROPER_TRUE:true\n', ''),
            (0, good, 'Error in process <0.1.0>'),
            (0, 'Error in process <0.1.0>\n' + good, ''),
            (0, 'PROPER_DONE\nPROPER_TRUE:true\nPROPER_FALSE:false\n', ''),
        ]:
            self.assertFalse(completed(code, out, err), (code, out, err))


if __name__ == '__main__':
    unittest.main()
