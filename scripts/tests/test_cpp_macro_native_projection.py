import pathlib
import hashlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
from cpp_macro_native_projection import project_root, validated_hash_offsets, observed_literal_once_lines


class NativeProjection(unittest.TestCase):
    def test_literal_once_requires_active_physical_callback_and_matching_native_line(self):
        for nl in [b'\n', b'\r\n']:
            original = nl.join([b'// header', b'#pragma once', b'int original;', b''])
            loc = {'nested': False, 'path': 'root.cc', 'offset': original.index(b'pragma'),
                   'buffer_sha256': hashlib.sha256(original).hexdigest()}
            row = {'kind': 'root_pragma', 'hash_pragma': True, 'location': loc}
            lines = observed_literal_once_lines(original, [row], 'root.cc', str)
            self.assertEqual(lines, {2})
            data = nl.join([b'#line 2 "root.cc"', b'#pragma once', b'int original;', b''])
            offsets = {0, data.index(b'#pragma')}
            self.assertEqual(project_root(data, offsets, 'root.cc', str, literal_once_lines=lines),
                             b'int original;' + nl)
            self.assertIn(b'#pragma once', project_root(data, offsets, 'root.cc', str))
            wrong_line = data.replace(b'#line 2', b'#line 3')
            self.assertIn(b'#pragma once', project_root(wrong_line, offsets, 'root.cc', str,
                                                       literal_once_lines=lines))
            repeated = data + data
            with self.assertRaises(ValueError):
                project_root(repeated, offsets | {o + len(data) for o in offsets}, 'root.cc', str,
                             literal_once_lines=lines)
            for change in [{'nested': True}, {'path': 'other.h'}, {'offset': True},
                           {'offset': -1}, {'buffer_sha256': '0' * 64}]:
                with self.assertRaises(ValueError):
                    observed_literal_once_lines(original, [{**row, 'location': {**loc, **change}}],
                                                'root.cc', str)
            self.assertFalse(observed_literal_once_lines(original, [], 'root.cc', str))
            self.assertFalse(observed_literal_once_lines(original, [{**row, 'hash_pragma': False}], 'root.cc', str))
        for original in [b'#pragma ONCE\n', b'__pragma(once)\n', b'_Pragma("once")\n',
                         b'const char *s = "#pragma once";\n', b'#pragma once // comment\n']:
            loc = {'nested': False, 'path': 'root.cc', 'offset': 0,
                   'buffer_sha256': hashlib.sha256(original).hexdigest()}
            self.assertFalse(observed_literal_once_lines(original,
                [{'kind': 'root_pragma', 'hash_pragma': True, 'location': loc}], 'root.cc', str))

    def test_forced_wrapper_restoration_requires_explicit_actual_inputs(self):
        data = b'#line 1 "wrapper.h"\n#pragma external_header(push)\n\n#line 1 "pch.h"\nint pch;\n#line 2 "wrapper.h"\n#pragma external_header(pop)\n#line 1 "unity.cc"\n#line 1 "root.cc"\nint original;\n'
        offsets = {data.index(b'#', start) for start in [0, data.index(b'#pragma'), data.index(b'#line 1 "pch'), data.index(b'#line 2'), data.index(b'#pragma external_header(pop)'), data.index(b'#line 1 "unity'), data.index(b'#line 1 "root')]}
        with self.assertRaises(ValueError): self.project(data, offsets)
        self.assertEqual(project_root(data, offsets, 'root.cc', str,
            forced_inputs=['wrapper.h'], translation_unit='unity.cc'), b'int original;\n')
        for forced, root in [([], 'unity.cc'), (['other.h'], 'unity.cc'), (['wrapper.h'], 'other.cc')]:
            with self.assertRaises(ValueError):
                project_root(data, offsets, 'root.cc', str, forced_inputs=forced, translation_unit=root)
        # An authored matching spelling is retained, never administrative proof.
        self.assertIn(b'#pragma external_header(pop)', project_root(data, offsets, 'wrapper.h', str, True,
            forced_inputs=['wrapper.h'], translation_unit='unity.cc'))

    def test_forced_header_frames_allow_bounded_blank_lines_only(self):
        for nl in [b'\n', b'\r\n']:
            data = nl.join([b'#line 1 "root.cc"', b'#pragma external_header(push)',
                b'', b' \t\v\f', b'#line 1 "pch.h"', b'int pch;',
                b'#pragma external_header(pop)', b'', b'#line 2 "root.cc"', b'int after;', b''])
            self.assertEqual(self.project(data), nl.join([b'', b' \t\v\f', b'int after;', b'']))
            for intervening in [b'int authored;', b'#pragma unknown', b'\0', nl * 65]:
                malformed = data.replace(nl + b' \t\v\f' + nl, nl + intervening + nl)
                with self.assertRaises(ValueError): self.project(malformed)

    def project(self, data, offsets=None, authored=False):
        if offsets is None:
            offsets, offset = set(), 0
            for line in data.splitlines(keepends=True):
                if line.startswith(b'#'):
                    offsets.add(offset)
                offset += len(line)
        return project_root(data, offsets, 'root.cc', str, authored)

    def test_balanced_nested_external_frames_preserve_root_unknown_directives(self):
        for nl in [b'\n', b'\r\n']:
            data = nl.join([b'#line 1 "root.cc"', b'int before;', b'#pragma external_header(push)',
                b'#line 1 "sdk.h"', b'int sdk;', b'#pragma external_header(push)',
                b'#line 1 "nested.h"', b'int nested;', b'#pragma external_header(pop)',
                b'#line 4 "sdk.h"', b'#pragma external_header(pop)', b'#line 3 "root.cc"',
                b'#pragma unknown', b'int after;', b''])
            self.assertEqual(self.project(data), nl.join([b'int before;', b'#pragma unknown', b'int after;', b'']))

    def test_unbalanced_wrong_return_and_unpaired_frames_decline(self):
        for data in [
            b'#line 1 "root.cc"\n#pragma external_header(push)\n#line 1 "sdk.h"\n',
            b'#line 1 "root.cc"\n#pragma external_header(pop)\n#line 1 "sdk.h"\n',
            b'#line 1 "root.cc"\n#pragma external_header(push)\nint after;\n',
            b'#line 1 "root.cc"\n#pragma external_header(push)\n#line 1 "sdk.h"\n#pragma external_header(pop)\n#line 2 "other.cc"\n',
        ]:
            with self.assertRaises(ValueError):
                self.project(data)

    def test_literal_fake_markers_are_payload_at_original_token_boundaries(self):
        data = b'#line 1 "root.cc"\nconst char *s = R"tag(\n#pragma external_header(push)\n#line 1 "sdk.h"\n#pragma external_header(pop)\n#line 2 "root.cc"\n)tag";\n'
        self.assertEqual(self.project(data, {0}), data.split(b'\n', 1)[1])

    def test_authored_names_and_different_pragma_forms_are_not_administrative(self):
        data = b'#line 1 "root.cc"\n#pragma external_header(push)\n#line 1 "sdk.h"\n#pragma external_header(pop)\n#line 2 "root.cc"\nint after;\n'
        self.assertIn(b'#pragma external_header(push)', self.project(data, authored=True))
        for directive in [b'#pragma external_header(unknown)', b'#pragma warning(push)', b'#pragma push_macro("CHECK")']:
            self.assertIn(directive, self.project(b'#line 1 "root.cc"\n'+directive+b'\n'))

    def test_indented_frames_use_hash_token_offset_and_lf_line_boundaries(self):
        data = b'#line 1 "root.cc"\n \t\v\f#pragma external_header(push)\n#line 1 "sdk.h"\n\t#pragma external_header(pop)\n#line 2 "root.cc"\nint after;\n'
        offsets = {data.index(b'#', start) for start in [0, data.index(b' \t'), data.index(b'#line 1 "sdk'), data.index(b'\t#pragma'), data.index(b'#line 2')]}
        self.assertEqual(self.project(data, offsets), b'int after;\n')

    def test_native_hash_packet_requires_exact_buffer_identity_and_bounded_offsets(self):
        data = b'#line 1 "root.cc"\n'
        packet = {'kind': 'native_hash_offsets', 'version': 1, 'bytes': len(data),
                  'sha256': hashlib.sha256(data).hexdigest().upper(), 'offsets': [0]}
        self.assertEqual(validated_hash_offsets(packet, data), {0})
        for update in [{'version': True}, {'bytes': len(data)+1}, {'sha256': None},
                       {'sha256': '0'*64}, {'offsets': [0, 0]}, {'offsets': [True]},
                       {'offsets': [-1]}, {'offsets': [len(data)]}, {'offsets': [1]},
                       {'unexpected': 'field'}]:
            with self.assertRaises(ValueError):
                validated_hash_offsets({**packet, **update}, data)
        with self.assertRaises(ValueError):
            validated_hash_offsets(packet, data+b'int changed;')


if __name__ == '__main__':
    unittest.main()
