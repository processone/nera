import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[3]/'scripts'))
from prepare_erlang_dependency import prepare_dependency


class PreparationTests(unittest.TestCase):
    def test_runs_original_parse_transform_without_modifying_source(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root/'dependency'; (source/'src').mkdir(parents=True)
            beam = root/'beam'; beam.mkdir()
            transform = beam/'add_answer.erl'
            transform.write_text('''-module(add_answer).
-export([parse_transform/2]).
parse_transform(Forms, _) ->
 {ok, Tokens, _} = erl_scan:string("answer() -> 42."),
 {ok, Function} = erl_parse:parse_form(Tokens),
 [F || F <- Forms, element(1,F) =/= eof] ++ [Function].
''')
            subprocess.run(['erlc', '-o', str(beam), str(transform)], check=True, capture_output=True)
            original = source/'src'/'prepared_probe.erl'
            text = '-module(prepared_probe). -compile({parse_transform,add_answer}). -export([answer/0]).\n'
            original.write_text(text)
            output = root/'prepared'
            manifest = prepare_dependency(source, [beam], output)
            prepared = output/'src'/'prepared_probe.erl'
            self.assertTrue(prepared.is_file())
            self.assertIn('answer() ->', prepared.read_text())
            self.assertEqual(original.read_text(), text)
            self.assertEqual(len(manifest['sources']), 1)
            subprocess.run(['erlc', '-o', str(beam), str(prepared)], check=True, capture_output=True)
            subprocess.run(['erl', '-noshell', '-pa', str(beam), '-eval',
                            '42 = prepared_probe:answer(), halt().'], check=True, capture_output=True)
            self.assertEqual(json.loads((output/'manifest.json').read_text()), manifest)
            with self.assertRaises(ValueError):
                prepare_dependency(source, [beam], output)


if __name__ == '__main__':
    unittest.main()
