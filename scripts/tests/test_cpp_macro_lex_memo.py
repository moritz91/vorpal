import pathlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
from cpp_macro_native_projection import CaptureLexMemo, project_root


class CaptureLexMemoTests(unittest.TestCase):
    def test_equal_bytes_reuse_immutable_lex_results_only_inside_one_capture(self):
        calls = []
        hashes = {0}
        spellings, directives = ['int', 'original', ';'], []
        memo = CaptureLexMemo()
        data = b'#line 1 "root.cc"\nint original;\n'

        def lex_hashes():
            calls.append('hashes')
            return hashes

        def lex_root():
            calls.append('root')
            return spellings, directives

        first = memo.hash_offsets(data, lex_hashes)
        root = memo.root_tokens(b'int original;\n', lex_root)
        hashes.clear()
        spellings.clear()
        self.assertEqual(memo.hash_offsets(bytes(bytearray(data)), lex_hashes), first)
        self.assertEqual(memo.root_tokens(b'int original;\n', lex_root), root)
        self.assertEqual(first, frozenset({0}))
        self.assertEqual(root, (('int', 'original', ';'), ()))
        self.assertEqual(calls, ['hashes', 'root'])
        CaptureLexMemo().hash_offsets(data, lex_hashes)
        self.assertEqual(calls, ['hashes', 'root', 'hashes'])

    def test_every_byte_matters_and_only_last_result_is_retained(self):
        for domain in ['hash_offsets', 'root_tokens']:
            memo = CaptureLexMemo()
            calls = []

            def lex():
                calls.append(1)
                return {0} if domain == 'hash_offsets' else (['token'], [])

            data = b'#line 1 "root.cc"\nR"(payload)"\n'
            for changed in [data.replace(b'root', b'else'), data.replace(b'payload', b'changed'),
                            data.replace(b'\n', b'\r\n'), data + b'\0', data[:-1]]:
                getattr(memo, domain)(data, lex)
                getattr(memo, domain)(changed, lex)
                getattr(memo, domain)(data, lex)
            self.assertEqual(len(calls), 11)

    def test_failed_lexing_is_not_cached(self):
        memo = CaptureLexMemo()
        for domain in ['hash_offsets', 'root_tokens']:
            def broken():
                raise ValueError('uncertain lexer')

            with self.assertRaises(ValueError):
                getattr(memo, domain)(b'input', broken)
            value = {0} if domain == 'hash_offsets' else (['valid'], [])
            result = getattr(memo, domain)(b'input', lambda: value)
            self.assertTrue(result)

    def test_marker_resolution_is_fresh_even_when_lexed_bytes_are_equal(self):
        memo = CaptureLexMemo()
        data = b'#line 1 "root.cc"\nint original;\n'
        visits = []
        mapping = {'root.cc': 'root.cc'}

        def physical(path):
            visits.append(path)
            return mapping.get(path, path)

        offsets = memo.hash_offsets(data, lambda: {0})
        self.assertEqual(project_root(data, offsets, 'root.cc', physical), b'int original;\n')
        before = len(visits)
        mapping['root.cc'] = 'redirected.cc'
        offsets = memo.hash_offsets(data, lambda: self.fail('identical buffer was re-lexed'))
        self.assertEqual(project_root(data, offsets, 'root.cc', physical), b'int original;\n')
        self.assertGreater(len(visits), before)


if __name__ == '__main__':
    unittest.main()
