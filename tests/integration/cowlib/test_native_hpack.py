"""Captain's cow_hpack campaign: framing and rejection controls."""
import copy
import json
import os
from pathlib import Path
import unittest
from types import SimpleNamespace
from native_hpack import completed, string_payload


class HpackProtocolTests(unittest.TestCase):
    def test_rfc7541_raw_authority_string_and_huffman_flag(self):
        # RFC 7541 C.3.1/C.4.1 strings, not third-party implementation code.
        raw = bytes.fromhex('0f7777772e6578616d706c652e636f6d')
        coded = bytes.fromhex('8cf1e3c2e5f23a6ba0ab90f4ff')
        self.assertEqual(string_payload(raw, False), b'www.example.com')
        self.assertEqual(string_payload(coded, True), coded[1:])
        for wire, flag in ((raw, True), (coded, False), (raw[:-1], False),
                           (raw+b'\x00', False), (b'', False), (b'\x7f', False)):
            with self.subTest(wire=wire):
                with self.assertRaises(ValueError):
                    string_payload(wire, flag)

    def test_length_extension_boundaries(self):
        for size in (0, 1, 126, 127, 128, 254, 255, 16384):
            # Test-owned byte sequence at RFC integer-prefix boundaries.
            data = bytes(range(256))*(size//256)+bytes(range(size%256))
            extra, remainder = [], max(0, size-127)
            if size >= 127:
                while remainder >= 128:
                    extra.append(128 | (remainder & 127))
                    remainder >>= 7
                extra.append(remainder)
            wire = bytes([min(size, 127), *extra])+data
            self.assertEqual(string_payload(wire, False), data)

    def test_incomplete_failed_and_duplicate_campaigns_never_pass(self):
        args = SimpleNamespace(samples=4)
        for code, out, err in ((0, '', ''), (0, 'COW_HPACK_DONE\n', ''),
                               (0, 'COW_HPACK_DONE\n'*2, ''),
                               (1, 'COW_HPACK_DONE\n', ''),
                               ('timeout', '', ''), (0, '', 'Error in process')):
            self.assertFalse(completed(code, out, err, args))


@unittest.skipUnless(os.environ.get('COWLIB_HPACK_REPORT'), 'requires a real external HPACK campaign report')
class HpackCampaignControls(unittest.TestCase):
    def test_real_engine_outputs_and_corrupted_readings(self):
        # Use actual engine readings, not fabricated passing property traces.
        from test_native_base64url import encode
        from native_hpack import _etf, RUN_TIMEOUT

        def untyped(term):
            kind, value = term
            if kind == 'tuple':
                return tuple(map(untyped, value))
            if kind == 'list':
                return list(map(untyped, value))
            if kind == 'binary':
                return bytes.fromhex(value)
            return value

        report = json.loads(Path(os.environ['COWLIB_HPACK_REPORT']).read_text())
        args = SimpleNamespace(samples=report['samples'])
        self.assertEqual(report['run_timeout_seconds'], RUN_TIMEOUT)
        for engine in ('beam-run', 'native-run'):
            record = next(x for x in report['commands'] if x['name'] == engine)
            self.assertEqual(record['timeout_seconds'], RUN_TIMEOUT)
            good = record['stdout']
            self.assertTrue(completed(record['returncode'], good, record['stderr'], args), engine)
            lines = good.splitlines()
            for code, out, err in ((1, good, ''), (None, good, ''), (0, good, 'Error in process'),
                                   (0, '\n'.join(lines[:-1])+'\n', ''),
                                   (0, good+lines[0]+'\n', ''),
                                   (0, '\n'.join([lines[0], lines[0], *lines[2:]])+'\n', '')):
                self.assertFalse(completed(code, out, err, args))
            first = untyped(_etf.etf(bytes(json.loads(lines[0].split(':', 1)[1]))))
            corruptions = []
            value = copy.deepcopy(first)
            value[2][0].pop()  # Incomplete trial count.
            corruptions.append(value)
            value = copy.deepcopy(first)
            sample = value[2][0][0]
            value[2][0][0] = (*sample[:3], 'false')
            corruptions.append(value)
            value = copy.deepcopy(first)
            sample = value[2][0][0]
            value[2][0][0] = (sample[0], sample[1], (b'wrong', b''), sample[3])
            corruptions.append(value)
            value = copy.deepcopy(first)
            sample = value[2][0][0]
            value[2][0][0] = (sample[0], b'\x01?', sample[2], sample[3])
            corruptions.append(value)
            value = copy.deepcopy(first)
            value[2][1][:] = [b'']  # No actual shrinking.
            corruptions.append(value)
            value = copy.deepcopy(first)
            value[2][2][:] = [b'not-empty']
            corruptions.append(value)
            corruptions.extend([(first[0], (1, 2, 3), first[2]),
                                ('wrong_property', first[1], first[2])])
            for value in corruptions:
                changed = lines.copy()
                changed[0] = 'COW_HPACK_RESULT:'+json.dumps(list(b'\x83'+encode(value)))
                self.assertFalse(completed(0, '\n'.join(changed)+'\n', '', args))


if __name__ == '__main__':
    unittest.main()
