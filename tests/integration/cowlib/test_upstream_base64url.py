import unittest
import hashlib
from upstream_base64url import corpus, pack_inputs, unpack_inputs, validate_output, summarize


class Base64PropertiesTests(unittest.TestCase):
    def test_reproducible_and_covers_boundaries(self):
        values = corpus()
        self.assertEqual(values, corpus())
        self.assertEqual(len(values), 1563)
        self.assertEqual(hashlib.sha256(pack_inputs(values)).hexdigest(), '0ea75031528097cb037ca7295e2a7ae416521d1e2c6a9b67e4e3c18d9e533ced')
        self.assertIn(b'', values)
        self.assertIn(bytes(range(256)), values)
        self.assertTrue(set(range(257)) <= {len(v) for v in values})
        self.assertGreaterEqual(max(map(len, values)), 16384)
        self.assertNotEqual(corpus('another-seed'), values)

    def test_framing_roundtrip(self):
        values = [b'', b'\x00\xff', bytes(range(256))]
        self.assertEqual(unpack_inputs(pack_inputs(values)), values)

    def test_truncated_frames_rejected(self):
        for raw in (b'\x00', b'\x00\x00\x00\x02x'):
            with self.assertRaises(ValueError):
                unpack_inputs(raw)

    def test_valid_etf_encoding_and_wrong_bytes(self):
        # ETF for {true, true, <<"">>, <<"">>}; explicit wire protocol.
        good = 'COWLIB_CASE:b64_0:[131,104,4,119,4,116,114,117,101,119,4,116,114,117,101,109,0,0,0,0,109,0,0,0,0]\nCOWLIB_DONE:b64_0\nBASE64_DONE:1\n'
        validate_output(0, good, [b''])
        with self.assertRaises(ValueError):
            validate_output(0, good, [b'x'])

    def test_false_property_is_failure_even_with_successful_process(self):
        output = 'COWLIB_CASE:b64_0:[131,104,4,119,5,102,97,108,115,101,119,4,116,114,117,101,109,0,0,0,0,109,0,0,0,0]\nCOWLIB_DONE:b64_0\nBASE64_DONE:1\n'
        with self.assertRaises(ValueError):
            validate_output(0, output, [b''])
        result = summarize(validate_output(0, output, [b''], strict=False), [b''])
        self.assertEqual(result['passed_per_property'], {'prop_identity':0, 'prop_identity_no_padding':1})
        self.assertEqual(len(result['failures']), 1)

    def test_crash_never_counts_as_a_property_pass(self):
        result = summarize([{'index':0, 'passed':False, 'error':'crash'}], [b''])
        self.assertEqual(result['passed_per_property'], {'prop_identity':0, 'prop_identity_no_padding':0})
        self.assertEqual(result['passed_encodings'], 0)

    def test_missing_marker_nonzero_exit_and_false_results_rejected(self):
        for code, output in ((0, ''), (1, 'BASE64_DONE:0\n'),
                             (0, 'false\nBASE64_DONE:0\n'),
                             (0, 'BASE64_DONE:0\nextra\n')):
            with self.assertRaises(ValueError):
                validate_output(code, output, [])
        validate_output(0, 'BASE64_DONE:0\n', [])

if __name__ == '__main__':
    unittest.main()
