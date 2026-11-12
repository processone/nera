import json
import unittest
from upstream_date import adapt_date, validate_output


def integer(n):
    return bytes([98])+n.to_bytes(4, 'big', signed=True)


def atom(s):
    return bytes([119, len(s)])+s.encode()


def tup(*parts):
    return bytes([104,len(parts)])+b''.join(parts)


DATE = ('tuple', (('tuple', (('integer',2000),('integer',2),('integer',29))),
                  ('tuple', (('integer',0),('integer',0),('integer',60)))))
INPUT = ('tuple', (('atom','fixdate'), ('tuple', (DATE, ('binary', b'example'.hex())))))


def output(passed=True, year=2000):
    expected = tup(tup(integer(2000),integer(2),integer(29)),tup(integer(0),integer(0),integer(60)))
    actual = tup(tup(integer(year),integer(2),integer(29)),tup(integer(0),integer(0),integer(60)))
    payload = bytes([131])+tup(atom('fixdate'),atom('true' if passed else 'false'),expected,actual)
    return 'COWLIB_CASE:date_0:'+json.dumps(list(payload))+'\nCOWLIB_DONE:date_0\nDATE_DONE:1\n'


class DatePropertyTests(unittest.TestCase):
    def test_adapter_keeps_original_property_body(self):
        body = 'prop_http_date() -> ?FORALL({D, B}, fixdate_gen(), D =:= parse_date(B)).\n'
        source = '-module(cow_date).\n-ifdef(TEST).\n-include_lib("proper/include/proper.hrl").\nfixdate_gen() -> dummy.\n'+body+'http_date_test_() -> [].\nhorse_date() -> horse:run().\n-endif.\n'
        adapted = adapt_date(source)
        self.assertIn(body, adapted)
        self.assertNotIn('proper/include', adapted)
        self.assertNotIn('fixdate_gen() ->', adapted)
        self.assertNotIn('horse_date() ->', adapted)
        self.assertIn('fun(Variable) -> Property end', adapted)
        self.assertIn('fun(Variable) -> false end', adapt_date(source, True))

    def test_true_property_requires_exact_parsed_date(self):
        self.assertEqual(len(validate_output(0,output(),[INPUT])),1)
        with self.assertRaises(ValueError):
            validate_output(0,output(year=2001),[INPUT])

    def test_false_property_rejected_and_retained_in_diagnostic_mode(self):
        with self.assertRaises(ValueError):
            validate_output(0,output(False),[INPUT])
        records = validate_output(0,output(False),[INPUT],strict=False)
        self.assertEqual(len(records),1)
        self.assertFalse(records[0]['passed'])

    def test_missing_duplicate_extra_or_failed_completion_rejected(self):
        good=output()
        for code,out in [(1,good),('timeout',good),(0,''),(0,good+good),
                         (0,good.replace('DATE_DONE:1','DATE_DONE:2')),
                         (0,good.replace('date_0','date_1')),
                         (0,good+'EXTRA\n')]:
            with self.subTest(code=code,out=out),self.assertRaises(ValueError):
                validate_output(code,out,[INPUT])
