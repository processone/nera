import unittest
from upstream_cookie_date import adapt, SUITES


class CookieDateAdapterTests(unittest.TestCase):
    def cookie_source(self):
        names = list(SUITES['cow_cookie']['generators']) + SUITES['cow_cookie']['direct']
        return '-module(cow_cookie).\n' + '\n'.join(n+'() -> sentinel = sentinel.' for n in names)

    def test_cookie_test_bodies_are_unchanged(self):
        original = self.cookie_source()
        adapted, removed = adapt(original, 'cow_cookie')
        self.assertIn(original.split('\n', 1)[1], adapted)
        self.assertEqual(removed, [])
        self.assertIn('54 = length(Tests)', adapted)
        self.assertIn('fun setcookie_max_age_test/0', adapted)
        self.assertIn('    F(),', adapted)

    def test_inventory_change_rejected(self):
        with self.assertRaises(ValueError):
            adapt(self.cookie_source()+'\nextra_test() -> ok.', 'cow_cookie')
        with self.assertRaises(ValueError):
            adapt('-module(cow_date).\n', 'cow_date')

    def test_date_keeps_all_functional_bodies(self):
        tests = [n+'() -> [{"test", fun() -> sentinel = sentinel end}].'
                 for n in SUITES['cow_date']['generators']]
        groups = []
        for i, test in enumerate(tests):
            props = 'day_name() -> dummy.\nprop_http_date() -> dummy.\n' if i == 0 else ''
            horses = '\n'.join(f'horse_g{i}_{j}() -> horse:repeat(1, ok).' for j in range(3))
            groups.append('-ifdef(TEST).\n'+props+test+'\n'+horses+'\n-endif.')
        adapted, removed = adapt('-module(cow_date).\n'+'\n'.join(groups), 'cow_date')
        for test in tests:
            self.assertIn(test, adapted)
        self.assertEqual(len(removed), 9)
        self.assertNotIn('horse:repeat', adapted)
        self.assertNotIn('prop_http_date()', adapted)
        self.assertIn('7 = length(Tests)', adapted)


if __name__ == '__main__':
    unittest.main()
