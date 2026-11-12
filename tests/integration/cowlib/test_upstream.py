import unittest
from upstream import adapt_source, native_passed, GENERATORS

class AdapterTests(unittest.TestCase):
    def test_native_requires_marker_and_success(self):
        self.assertFalse(native_passed(0, '', 1))
        self.assertFalse(native_passed(1, 'COW_QS_TEST:1\n', 1))
        self.assertFalse(native_passed(0, 'COW_QS_TEST:2\n', 1))
        self.assertTrue(native_passed(0, 'COW_QS_TEST:1\n', 1))
    def test_inventory_change_rejected(self):
        with self.assertRaises(ValueError): adapt_source('-module(cow_qs).\n')
    def test_only_benchmarks_removed(self):
        tests = '\n'.join(n+'() -> [{"case", fun() -> sentinel = sentinel end}].' for n in GENERATORS)
        sections = []
        for group, count in enumerate((4,4,5,4)):
            horse = '\n'.join(f'horse_g{group}_{i}() -> horse:repeat(1, ok).' for i in range(count))
            sections.append('-ifdef(TEST).\n'+horse+'\n-endif.')
        source = '-module(cow_qs).\n'+tests+'\n'+'\n'.join(sections)
        adapted, removed = adapt_source(source)
        self.assertIn(tests, adapted)
        self.assertNotIn('horse:repeat', adapted)
        self.assertEqual(len(removed), 17)
        self.assertIn('    F(),', adapted)
    def test_functional_test_in_benchmark_region_rejected(self):
        tests = '\n'.join(n+'() -> [].' for n in GENERATORS)
        source = '-module(cow_qs).\n'+tests+'\nhorse_x() -> ok.\nextra_test_() -> [].\n-endif.'
        with self.assertRaises(ValueError): adapt_source(source)
    def test_extra_output_rejected(self):
        self.assertFalse(native_passed(0, 'COW_QS_TEST:1\nextra\n', 1))

if __name__ == '__main__': unittest.main()
