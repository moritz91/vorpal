import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from cpp_macro_compiler_provider import physical, plain_windows_path, resolved_path


def ordinary(path):
    return os.path.normcase(str(Path(path).resolve())).removeprefix('\\\\?\\')


class PhysicalPath(unittest.TestCase):
    def test_verbatim_domain_excludes_dos_namespace_and_component_ambiguity(self):
        for path in [r'C:\source\unit.cc', r'z:\space inside\α.cc', r'C:\short~1\unit.cc']:
            self.assertTrue(plain_windows_path(path), path)
        for path in ['C:\\', 'unit.cc', r'C:unit.cc', r'\root\unit.cc',
                     r'\\host\share\unit.cc', r'\\?\C:\root\unit.cc', r'\\.\NUL',
                     r'C:\root\..\unit.cc', r'C:\root\.\unit.cc', r'C:\root\\unit.cc',
                     r'C:\root\unit.', 'C:\\root\\unit ', r'C:\root\unit.cc:stream',
                     r'C:\NUL.cc', r'C:\COM1\unit.cc', r'C:\LPT²\unit.cc',
                     r'C:\CONOUT$\unit.cc', r'C:\ nul.cc', r'C:\NUL .cc',
                     r'C:\root\unit?.cc', 'C:\\root\\control\0.cc',
                     'C:\\root\\control\x1f.cc', 'C:\\' + 'a' * 240]:
            self.assertFalse(plain_windows_path(path), repr(path))

    def test_existing_missing_relative_unicode_and_dot_paths_match_original(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            directory = root / 'space inside α'
            directory.mkdir()
            source = directory / 'unit.cc'
            source.write_bytes(b'int original;\n')
            for path in [source, str(source).replace('\\', '/'), directory,
                         directory / '..' / directory.name / source.name,
                         directory / 'missing.cc', directory / 'missing.',
                         directory / 'missing ', directory / 'missing.cc:stream',
                         root]:
                self.assertEqual(physical(path), ordinary(path))
                self.assertEqual(resolved_path(path), Path(path).resolve())
            # Runner checkouts and their Temp directory can be on different
            # Windows drives. A relative spelling needs a genuine same-drive cwd.
            previous = Path.cwd()
            try:
                os.chdir(root)
                relative = os.path.relpath(source)
                self.assertEqual(physical(relative), ordinary(source))
                self.assertEqual(resolved_path(relative), source.resolve())
            finally:
                os.chdir(previous)

    @unittest.skipUnless(os.name == 'nt', 'Windows native path-resolution control')
    def test_plain_existing_path_uses_one_fresh_native_resolution_per_call(self):
        import ntpath
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / 'unit.cc'
            source.write_bytes(b'int original;\n')
            expected = ordinary(source)
            self.assertTrue(plain_windows_path(str(source)))
            with patch.object(ntpath, '_getfinalpathname', wraps=ntpath._getfinalpathname) as resolve:
                self.assertEqual(physical(source), expected)
                self.assertEqual(resolve.call_count, 1)
                self.assertEqual(physical(source), expected)
                self.assertEqual(resolve.call_count, 2)
            source.unlink()
            self.assertEqual(physical(source), ordinary(source))
            source.write_bytes(b'int replacement;\n')
            self.assertEqual(physical(source), ordinary(source))

    def test_symlink_redirect_removal_and_creation_never_reuse_a_resolution(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            first, second, alias = [root / name for name in ('first.cc', 'second.cc', 'alias.cc')]
            first.write_bytes(b'int first;\n')
            second.write_bytes(b'int second;\n')
            try:
                alias.symlink_to(first)
            except OSError:
                self.skipTest('host does not permit symlink creation')
            self.assertEqual(physical(alias), ordinary(first))
            alias.unlink()
            alias.symlink_to(second)
            self.assertEqual(physical(alias), ordinary(second))
            second.unlink()
            self.assertEqual(physical(alias), ordinary(alias))
            second.write_bytes(b'int recreated;\n')
            self.assertEqual(physical(alias), ordinary(second))
            alias.unlink()
            alias.symlink_to(alias)
            # Python versions differ on non-strict loop handling. Preserve the
            # resolver actually installed on this host rather than inventing one.
            try:
                expected = ordinary(alias)
            except (OSError, RuntimeError) as error:
                with self.assertRaises(type(error)):
                    physical(alias)
            else:
                self.assertEqual(physical(alias), expected)
            alias.unlink()

    @unittest.skipUnless(os.name == 'nt', 'Windows verbatim fallback control')
    def test_ambiguous_reparse_result_and_unavailable_lookup_use_original_resolver(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / 'unit.cc'
            calls = []

            for target in ['\\\\?\\C:\\ambiguous.\\unit.cc',
                           '\\\\?\\UNC\\host\\share\\unit.cc',
                           '\\\\?\\C:\\' + 'a' * 240]:
                calls.clear()

                def ambiguous(file, strict=False):
                    calls.append((str(file), strict))
                    return Path(target) if strict else Path('C:\\ordinary\\unit.cc')

                with patch.object(Path, 'resolve', ambiguous):
                    self.assertEqual(physical(source), os.path.normcase('C:\\ordinary\\unit.cc'))
                self.assertEqual([strict for _, strict in calls], [True, False])
            for error in (PermissionError('private failure'), FileNotFoundError('private failure'),
                          RuntimeError('private failure')):
                calls.clear()

                def unavailable(file, strict=False):
                    calls.append(strict)
                    if strict:
                        raise error
                    return Path('C:\\ordinary\\unit.cc')

                with patch.object(Path, 'resolve', unavailable):
                    self.assertEqual(physical(source), os.path.normcase('C:\\ordinary\\unit.cc'))
                self.assertEqual(calls, [True, False])

    @unittest.skipUnless(os.name == 'nt', 'Windows verbatim reparse control')
    def test_native_ambiguous_symlink_target_keeps_original_namespace_behavior(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            target = Path('\\\\?\\' + str(root / 'target.cc.'))
            alias = root / 'alias.cc'
            target.write_bytes(b'int original;\n')
            try:
                try:
                    alias.symlink_to(target)
                except OSError:
                    self.skipTest('host does not permit symlink creation')
                self.assertEqual(resolved_path(alias), alias.resolve())
                self.assertEqual(physical(alias), ordinary(alias))
            finally:
                if alias.is_symlink():
                    alias.unlink()
                target.unlink()

    @unittest.skipUnless(os.name == 'nt', 'Windows post-resolution check control')
    def test_failed_post_resolution_check_returns_to_original_error_contract(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / 'unit.cc'
            source.write_bytes(b'int original;\n')
            original = Path.resolve
            calls = []

            def tracked(file, strict=False):
                calls.append(strict)
                return original(file, strict=strict)

            with patch.object(Path, 'resolve', tracked), patch.object(Path, 'stat',
                    side_effect=PermissionError('private failure')):
                self.assertEqual(physical(source), ordinary(source))
            self.assertEqual(calls, [True, False, False])


if __name__ == '__main__':
    unittest.main()
