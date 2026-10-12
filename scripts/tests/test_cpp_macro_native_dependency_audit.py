import pathlib
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
from cpp_macro_native_dependency_audit import (MAGIC, RECORD, parse_trace, parse_capture,
    filesystem_path, snapshot, candidates, ordered_probes, validate_plan, BOUNDARIES)


def record(kind, sequence, name='', resolved='', status=0, flags=0, access=0):
    n, r = name.encode('utf-16-le'), resolved.encode('utf-16-le')
    return RECORD.pack(kind, status, flags, access, 1, len(n) // 2, len(r) // 2,
                       sequence, 10, 11) + n + r


def trace(middle=b''):
    count = 1
    offset = 0
    while offset < len(middle):
        fields = RECORD.unpack_from(middle, offset)
        offset += RECORD.size + 2 * (fields[5] + fields[6])
        count += 1
    return MAGIC + record(1, 1, access=11) + middle + record(10, count + 1)


class NativeDependencyAuditTests(unittest.TestCase):
    def test_original_order_and_status_preserve_failed_search_and_redirect(self):
        data = trace(record(4, 2, '\\??\\C:\\local\\x.h', status=0xC0000034)
                     + record(2, 3, '\\??\\C:\\sdk\\x.h', '\\\\?\\C:\\target\\x.h', access=1))
        rows = parse_trace(data)
        self.assertEqual(candidates(rows), ['C:\\local\\x.h', 'C:\\sdk\\x.h', 'C:\\target\\x.h'])
        self.assertEqual([r['status'] for r in ordered_probes(rows)], [0xC0000034, 0])
        self.assertEqual(rows[2]['resolved'], '\\\\?\\C:\\target\\x.h')

    def test_incomplete_unknown_failed_and_lost_traces_decline(self):
        valid = trace(record(4, 2, '\\??\\C:\\sdk\\x.h'))
        broken = [valid[:-1], valid[1:], valid + b'\0',
                  trace(record(4, 3, 'gap')),
                  trace(record(4, 2, 'lost', flags=2)),
                  trace(record(99, 2, 'unknown')),
                  MAGIC + record(1, 1, status=1, access=11) + record(10, 2),
                  trace(record(4, 2, 'embedded\0nul')),
                  trace(record(1, 2, access=11))]
        for data in broken:
            with self.subTest(data=data[:30]), self.assertRaises(ValueError):
                parse_trace(data)

    def test_job_coverage_is_reported_and_missing_root_declines(self):
        with tempfile.TemporaryDirectory() as directory:
            prefix = pathlib.Path(directory) / 'native'
            pathlib.Path(str(prefix) + '.5.bin').write_bytes(trace())
            job = pathlib.Path(str(prefix) + '.job.bin')
            job.write_bytes(RECORD.pack(12, 0, 0, 2, 5, 0, 0, 0, 0, 0))
            _, coverage = parse_capture(prefix)
            self.assertFalse(coverage['allJobProcessesTraced'])
            job.write_bytes(RECORD.pack(12, 0, 0, 2, 6, 0, 0, 0, 0, 0))
            with self.assertRaises(ValueError):
                parse_capture(prefix)

    def test_paths_do_not_hide_devices_relative_names_wildcards_or_dot_components(self):
        for name in ['relative.h', '\\Device\\ConDrv', 'C:relative.h',
                     '\\??\\C:\\sdk\\..\\x.h', '\\??\\C:\\sdk\\*', '\\??\\C:\\NUL']:
            self.assertIsNone(filesystem_path(name), name)
        self.assertEqual(filesystem_path('\\??\\UNC\\server\\share\\x.h'), '\\\\server\\share\\x.h')

    def test_directory_membership_keeps_a_cached_negative_dependency(self):
        rows = parse_trace(trace(record(14, 2, '\\\\?\\C:\\sdk\\existing.h', access=32)
                                 + record(8, 3, '\\\\?\\C:\\sdk', access=1)))
        self.assertEqual(candidates(rows), ['C:\\sdk'])
        self.assertEqual(ordered_probes(rows)[0]['kind'], 'directoryEntry')
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory)
            before = snapshot([str(path)])
            (path / 'previously-missing.h').write_bytes(b'new')
            self.assertNotEqual(before, snapshot([str(path)]))

    def test_injected_children_must_be_rooted_and_process_ids_are_not_identities(self):
        with tempfile.TemporaryDirectory() as directory:
            prefix = pathlib.Path(directory) / 'tree'
            pathlib.Path(str(prefix) + '.5.bin').write_bytes(trace(record(13, 2, 'C:\\child.exe', access=8)))
            pathlib.Path(str(prefix) + '.8.bin').write_bytes(trace(record(4, 2, '\\??\\C:\\child.h')))
            pathlib.Path(str(prefix) + '.job.bin').write_bytes(RECORD.pack(12, 0, 0, 2, 5, 0, 0, 0, 0, 0))
            rows, coverage = parse_capture(prefix)
            self.assertTrue(coverage['traceCountMatchesJob'])
            self.assertFalse(coverage['allJobProcessesTraced'])
            self.assertEqual(ordered_probes(rows)[0]['access'], 0)
            pathlib.Path(str(prefix) + '.9.bin').write_bytes(trace())
            with self.assertRaises(ValueError):
                parse_capture(prefix)

    def test_byte_edits_and_missing_creation_invalidate_snapshot_without_mtime_change(self):
        import os
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / 'header.h'
            missing = snapshot([str(path)])
            path.write_bytes(b'first')
            first = snapshot([str(path)])
            stat = path.stat()
            path.write_bytes(b'other')
            os.utime(path, ns=(stat.st_atime_ns, stat.st_mtime_ns))
            changed = snapshot([str(path)])
            self.assertNotEqual(first, changed)
            self.assertNotEqual(missing, first)
            path.unlink()
            self.assertEqual(snapshot([str(path)]), missing)

    def test_preprocess_plan_keeps_include_order_and_forced_input_but_rejects_outputs(self):
        unit = str(pathlib.Path('unit.cc').resolve())
        args = ['/nologo', '/Ione', '/Itwo', '/FIforced.h', unit, '/Zs']
        self.assertEqual(validate_plan(args, unit), args[:-1] + ['/E'])
        for option in ['@flags.rsp', '/Foobject.obj', '/sourceDependencies', '/P', '/MP', '/B1alternate.exe']:
            with self.subTest(option=option), self.assertRaises(ValueError):
                validate_plan([option, unit, '/Zs'], unit)
        self.assertGreaterEqual(len(BOUNDARIES), 6)


if __name__ == '__main__':
    unittest.main()
