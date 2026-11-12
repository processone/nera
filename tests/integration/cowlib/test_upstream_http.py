import unittest
from upstream_http import adapt_source, functional_inventory, passed, property_closure


class HttpAdapterTests(unittest.TestCase):
    def test_inventory_has_direct_and_generator_tests(self):
        self.assertEqual(functional_inventory('a_test() -> ok.\nb_test_() -> [].\n'),
                         [('a_test', False), ('b_test_', True)])

    def test_only_successful_exact_completion_passes(self):
        self.assertTrue(passed(0, 'COW_HTTP_UPSTREAM:3\n', 3))
        for code, output in [(1, 'COW_HTTP_UPSTREAM:3\n'), (0, ''),
                             (0, 'COW_HTTP_UPSTREAM:2\n'),
                             (0, 'COW_HTTP_UPSTREAM:3\nextra\n')]:
            self.assertFalse(passed(code, output, 3))

    def test_original_test_and_production_spans_preserved(self):
        body = 'sample_test_() ->\n    [{<<"x">>, fun() -> expected = parse(<<"x">>) end}].\n'
        production = 'parse(X) -> X.\n'
        source = ('-module(cow_http_hd).\n' + production + '-ifdef(TEST).\n'
                  '-include_lib("proper/include/proper.hrl").\n'
                  'generator() -> ?LET(X, integer(), X).\n' + body +
                  'prop_x() -> ?FORALL(X, generator(), X =:= X).\n'
                  'horse_x() -> horse:repeat(1, ok).\n-endif.\n')
        result, removed = adapt_source(source, 'cow_http_hd', properties=False)
        self.assertIn(body, result)
        self.assertIn(production, result)
        self.assertNotIn('horse:repeat', result)
        self.assertNotIn('?LET', result)
        self.assertNotIn('prop_x()', result)
        self.assertEqual(set(removed), {'generator', 'prop_x', 'horse_x'})

    def test_property_predicate_is_preserved(self):
        prop = 'prop_x() -> ?FORALL(X, generator(), X =:= X).\n'
        source = ('-module(cow_http_hd).\n-ifdef(TEST).\n'
                  '-include_lib("proper/include/proper.hrl").\n'
                  'generator() -> ?LET(X, integer(), X).\n' + prop + '-endif.\n')
        result, _ = adapt_source(source, 'cow_http_hd', properties=True, property_envelope='closure')
        self.assertIn('fun(X) -> X =:= X end.', result)
        self.assertNotIn('?LET', result)

    def test_nested_conditionals_rejected(self):
        with self.assertRaises(ValueError):
            adapt_source('-ifdef(TEST).\n-ifdef(OTHER).\n-endif.\n-endif.\n', 'cow_http')

    def test_property_block_and_tuple_binding_preserved(self):
        predicate = '\n    begin A = B, A =:= B end\n'
        span = 'prop_x() -> ?FORALL({A, B}, vector(1, type()),' + predicate + ').\n'
        self.assertEqual(property_closure(span), 'prop_x() -> fun({A, B}) ->' + predicate + ' end.\n')

    def test_property_envelope_change_rejected(self):
        with self.assertRaises(ValueError):
            property_closure('prop_x() -> true.')

    def test_macro_envelope_preserves_forall_and_begin_predicate(self):
        prop = 'prop_x() -> ?FORALL(X, generator(), begin Y = X, Y =:= X end).\n'
        source = ('-module(cow_http_hd).\n-ifdef(TEST).\n'
                  '-include_lib("proper/include/proper.hrl").\n'
                  'generator() -> integer().\n' + prop + '-endif.\n')
        result, _ = adapt_source(source, 'cow_http_hd', properties=True)
        self.assertIn(prop, result)
        self.assertIn('-define(FORALL(Variable, Generator, Property), fun(Variable) -> Property end).', result)
        self.assertNotIn('generator() ->', result)
