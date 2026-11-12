import unittest
import runner


class ProtocolTests(unittest.TestCase):
    def test_exit_zero_without_completion_is_failure(self):
        with self.assertRaisesRegex(ValueError, 'completion'):
            runner.read_output('', ['one'])

    def test_missing_case_is_failure(self):
        with self.assertRaisesRegex(ValueError, 'cases'):
            runner.read_output('THOAS_DONE:1\n', ['one'])

    def test_divergence_is_failure(self):
        with self.assertRaisesRegex(ValueError, 'one'):
            runner.compare({'one': ('integer', 1)}, {'one': ('integer', 2)}, [{'id': 'one', 'operation': 'decode'}])


class ComparisonTests(unittest.TestCase):
    def test_etf_integer_encodings_agree(self):
        self.assertEqual(runner.etf(bytes([131, 97, 42])), runner.etf(bytes([131, 98, 0, 0, 0, 42])))
        self.assertEqual(runner.etf(bytes([131, 110, 1, 0, 42])), ('integer', 42))

    def test_etf_preserves_float_bits_and_types(self):
        self.assertEqual(runner.etf(bytes.fromhex('83468000000000000000')), ('float', '8000000000000000'))
        self.assertNotEqual(runner.etf(bytes([131, 97, 0])), runner.etf(bytes.fromhex('83460000000000000000')))

    def test_etf_map_order_is_irrelevant(self):
        a = bytes([97, 1, 97, 2])
        b = bytes([97, 3, 97, 4])
        prefix = bytes([131, 116, 0, 0, 0, 2])
        self.assertEqual(runner.etf(prefix + a + b), runner.etf(prefix + b + a))

    def test_etf_string_and_list_agree(self):
        self.assertEqual(runner.etf(bytes([131, 107, 0, 1, 65])), runner.etf(bytes([131, 108, 0, 0, 0, 1, 97, 65, 106])))

    def test_etf_rejects_truncated_unknown_and_trailing_data(self):
        for data in [b'', bytes([131, 109, 0, 0, 0, 2, 42]), bytes([131, 255]), bytes([131, 106, 106])]:
            with self.subTest(data=data), self.assertRaises(ValueError):
                runner.etf(data)

    def test_protocol_rejects_duplicates_unknown_ids_and_extra_output(self):
        line = 'THOAS_CASE:one:[131,97,42]\n'
        for text in [line + line + 'THOAS_DONE:1\n', line.replace('one', 'other') + 'THOAS_DONE:1\n', line + 'THOAS_DONE:1\nnoise\n', line + 'THOAS_DONE:2\n']:
            with self.subTest(text=text), self.assertRaises(ValueError):
                runner.read_output(text, ['one'])
        self.assertEqual(runner.read_output(line + 'THOAS_DONE:1\n', ['one']), {'one': ('integer', 42)})

    @staticmethod
    def encoded(text):
        return ('tuple', (runner.atom('return'), ('binary', text.encode().hex())))

    def test_encoder_accepts_object_order_and_number_spelling(self):
        case = [{'id': 'one', 'operation': 'encode', 'expect': 'ok'}]
        runner.compare({'one': self.encoded('{"b":2,"a":1.0}')}, {'one': self.encoded('{"a":1e0,"b":2}')}, case)

    def test_encoder_rejects_wrong_values_and_invalid_json(self):
        case = [{'id': 'one', 'operation': 'encode', 'expect': 'ok'}]
        for bad in ['{"a":true}', '{"a":2}', '{"a":NaN}', '[1]', '{"a":1,"a":1}', '{"a":1} trailing']:
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                runner.compare({'one': self.encoded('{"a":1}')}, {'one': self.encoded(bad)}, case)

    def test_duplicate_object_member_order_is_preserved(self):
        case = [{'id': 'one', 'operation': 'encode', 'expect': 'ok'}]
        with self.assertRaises(ValueError):
            runner.compare({'one': self.encoded('{"a":1,"a":2}')}, {'one': self.encoded('{"a":2,"a":1}')}, case)

    def test_wrong_category_fails_even_when_engines_agree(self):
        value = ('tuple', (runner.atom('return'), ('tuple', (runner.atom('error'), runner.atom('badarg')))))
        with self.assertRaisesRegex(ValueError, 'expected ok'):
            runner.compare({'one': value}, {'one': value}, [{'id': 'one', 'operation': 'decode', 'expect': 'ok'}])

    def test_exception_class_and_reason_are_compared(self):
        a = ('tuple', (runner.atom('exception'), runner.atom('error'), runner.atom('badarg')))
        b = ('tuple', (runner.atom('exception'), runner.atom('throw'), runner.atom('badarg')))
        with self.assertRaises(ValueError):
            runner.compare({'one': a}, {'one': b}, [{'id': 'one', 'operation': 'encode', 'expect': 'exception'}])


if __name__ == '__main__':
    unittest.main()
