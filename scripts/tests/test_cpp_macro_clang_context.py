import pathlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
from cpp_macro_clang_context import clang_plan, project_clang_root


class ClangContext(unittest.TestCase):
    def project(self, data, hashes=None):
        if hashes is None:
            hashes, offset = set(), 0
            for line in data.split(b'\n'):
                if line.lstrip(b' \t').startswith(b'#'):
                    hashes.add(offset + len(line) - len(line.lstrip(b' \t')))
                offset += len(line) + 1
        paths = set()
        def physical(path):
            paths.add(path)
            return path
        result = project_clang_root(data, hashes, 'root.hpp', physical)
        self.assertNotIn('<built-in>', paths)
        self.assertNotIn('<command line>', paths)
        return result

    def test_nested_system_and_virtual_markers_preserve_root_bytes(self):
        for nl in [b'\n', b'\r\n']:
            data = nl.join([b'# 1 "wrapper.mm"', b'# 1 "<built-in>" 1',
                b'# 1 "<built-in>" 3', b'# 1 "<command line>" 1',
                b'# 1 "<built-in>" 2', b'# 1 "wrapper.mm" 2',
                b'# 1 "root.hpp" 1', b'Class * original;',
                b'# 1 "Foundation.h" 1 3', b'@class NSString;',
                b'# 5 "root.hpp" 2', b'NSString * __strong stored;',
                b'#pragma unknown', b'# 3 "wrapper.mm" 2', b'wrapper();', b''])
            self.assertEqual(self.project(data), nl.join([b'Class * original;',
                b'NSString * __strong stored;', b'#pragma unknown', b'']))

    def test_literal_markers_and_vertical_whitespace_are_payload(self):
        data = b'# 1 "root.hpp"\nconst char *s = R"x(\n# 1 "fake.hpp" 1\n)x";\nint\vvalue;\n'
        self.assertEqual(self.project(data, {0}), data.split(b'\n', 1)[1])

    def test_invalid_flags_returns_virtual_inputs_and_incomplete_frames_decline(self):
        for marker in [b'# 1 "sdk.h" 2', b'# 1 "sdk.h" 1 2',
                       b'# 1 "sdk.h" 3 1', b'# 1 "sdk.h" 1 1',
                       b'# 1 "sdk.h" 5', b'# 1 "<unknown>"',
                       b'# 1 "sdk.h" 1']:
            with self.assertRaises(ValueError):
                self.project(b'# 1 "root.hpp"\n' + marker + b'\n')

    def test_only_explicit_bounded_syntax_plans_are_accepted(self):
        sdk_path = str(pathlib.Path.cwd() / 'sdk')
        plan = ['-x', 'objective-c++', '-std=c++17', '-isysroot', sdk_path,
                '-fobjc-arc', '-fno-modules', '-include', '/prefix.h',
                '-DCONTEXT=1', '/wrapper.mm', '-fsyntax-only']
        native, forced, sdk = clang_plan(plan, '/wrapper.mm', str)
        self.assertEqual(native[-2:], ['/wrapper.mm', '-E'])
        self.assertEqual(forced, ['/prefix.h'])
        self.assertEqual(str(sdk), sdk_path)
        for extra in ['-c', '-o', '-MF', '-MMD', '-Xclang', '-load',
                      '--config=evil', '-fmodules', '@flags.rsp', 'other.mm',
                      '--driver-mode=cl', '-save-temps']:
            with self.assertRaises(ValueError):
                clang_plan([extra] + plan, '/wrapper.mm', str)
        for invalid in [plan[:-1], plan[:-2] + ['/other.mm', '-fsyntax-only'],
                        [v if v != 'objective-c++' else 'c++' for v in plan],
                        [v if v != sdk_path else 'relative-sdk' for v in plan]]:
            with self.assertRaises(ValueError):
                clang_plan(invalid, '/wrapper.mm', str)


if __name__ == '__main__':
    unittest.main()
