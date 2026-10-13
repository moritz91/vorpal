from pathlib import Path
from types import SimpleNamespace
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from cpp_macro_compiler_provider import root_token_inventory


class Token:
    def __init__(self, spelling, line, column, comment=False):
        self.kind = 'comment' if comment else 'token'
        self.value = spelling
        self.position = SimpleNamespace(line=line, column=column)
        self.spelling_reads = self.location_reads = 0

    @property
    def spelling(self):
        self.spelling_reads += 1
        return self.value

    @property
    def location(self):
        self.location_reads += 1
        return self.position


class RootTokens(unittest.TestCase):
    def test_directives_comments_and_raw_literal_hashes_keep_their_roles(self):
        lines = ['\t#pragma unknown\r', 'int x = # value;', 'R"tag(\n#pragma payload\n)tag";']
        tokens = [Token('#',1,2), Token('pragma',1,3), Token('unknown',1,10),
                  Token('int',2,1), Token('#',2,9), Token('value',2,11),
                  Token('/* ignored #pragma */',2,18,True),
                  Token('R"tag(\n#pragma payload\n)tag"',3,1)]
        spellings, directives = root_token_inventory(iter(tokens), lines, 'comment')
        self.assertEqual(directives, ['#pragma unknown'])
        self.assertEqual(spellings, ['int','#','value','R"tag(\n#pragma payload\n)tag"'])
        for token in tokens:
            self.assertEqual((token.spelling_reads,token.location_reads),
                             (0,0) if token.kind == 'comment' else (1,1))

    def test_clang_diagnostic_tokens_and_unknown_directive_inventory_stay_distinct(self):
        lines = ['#pragma clang diagnostic push', '#pragma unknown', '#pragma clang diagnostic pop']
        def tokens():
            for line,text in enumerate(lines,1):
                column = 1
                for word in text.replace('#','# ').split():
                    yield Token(word,line,column)
                    column += len(word)+1
        msvc = root_token_inventory(tokens(),lines,'comment')
        clang = root_token_inventory(tokens(),lines,'comment',True)
        self.assertEqual(msvc, ([],lines))
        self.assertEqual(clang, (['#','pragma','clang','diagnostic','push',
                                  '#','pragma','clang','diagnostic','pop'],['#pragma unknown']))


if __name__ == '__main__':
    unittest.main()
