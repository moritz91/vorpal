import pathlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
from cpp_macro_native_lifecycle import MAGIC, RECORD, parse_lifecycle, reconcile_traces


def stream(child=True, debug=False):
    rows = []
    def add(kind, pid=0, parent=0, status=0, created=0, image='', flags=0):
        rows.append((kind, pid, parent, status, flags, created, image))
    add(1, 50, status=int(debug))
    add(3)
    add(2, 5, 50, created=500, image='C:\\root.exe')
    add(3, 1, 1, 1)
    add(4, 5, 50, created=500, image='C:\\root.exe')
    if debug:
        add(6, 5, 50, created=500, image='C:\\root.exe')
    if child:
        add(4, 8, 5, created=800, image='C:\\Windows\\System32\\conhost.exe')
        if debug:
            add(6, 8, 5, created=800, image='C:\\Windows\\System32\\conhost.exe')
            add(7, 8, created=800)
        add(5, 8, created=800)
    if debug:
        add(7, 5, created=500)
    add(5, 5, created=500)
    add(3, 2, 2 if child else 1)
    add(9, 5, 2 if child else 1)
    return rows


def encode(rows):
    result = bytearray(MAGIC)
    for i, (kind, pid, parent, status, flags, created, name) in enumerate(rows, 1):
        value = name.encode('utf-16-le')
        result.extend(RECORD.pack(kind, pid, parent, status, flags, len(value) // 2, i, i * 10, created))
        result.extend(value)
    return bytes(result)


class NativeLifecycleTests(unittest.TestCase):
    def test_os_observer_identifies_untraced_console_child_without_name_exemption(self):
        life = parse_lifecycle(encode(stream()))
        self.assertTrue(life['lifetimesReconciled'])
        coverage = reconcile_traces(life, {5})
        self.assertFalse(coverage['allJobProcessesTraced'])
        self.assertEqual(coverage['untracedProcessImages'], ['C:\\Windows\\System32\\conhost.exe'])
        self.assertTrue(reconcile_traces(life, {5, 8})['allObservedProcessesTraced'])

    def test_debug_lifetime_requires_independent_create_and_exit(self):
        life = parse_lifecycle(encode(stream(debug=True)))
        self.assertTrue(life['lifetimesReconciled'])
        self.assertTrue(life['debuggerMayChangeBehavior'])
        self.assertFalse(life['replayEligible'])
        rows = [r for r in stream(debug=True) if not (r[0] == 7 and r[1] == 8)]
        self.assertFalse(parse_lifecycle(encode(rows))['lifetimesReconciled'])

    def test_stopped_process_injection_requires_debug_start_without_name_exemptions(self):
        rows=stream(debug=True)
        inserts=[(10,5,0,0,0,0,''),(10,8,0,0,0,0,'')]
        rows.insert(next(i for i,r in enumerate(rows) if r[0]==6 and r[1]==8),inserts[1])
        rows.insert(next(i for i,r in enumerate(rows) if r[0]==6 and r[1]==5),inserts[0])
        life=parse_lifecycle(encode(rows))
        self.assertEqual(life['stoppedProcessAttachments'],[5,8])
        self.assertFalse(life['replayEligible'])
        bad=list(rows);bad.insert(1,(10,5,0,0,0,0,''))
        with self.assertRaises(ValueError):parse_lifecycle(encode(bad))
        bad=stream(debug=True);bad.append((10,99,0,0,0,0,''))
        with self.assertRaises(ValueError):parse_lifecycle(encode(bad))

    def test_missing_advisory_notification_or_metadata_cannot_certify_coverage(self):
        rows = [r for r in stream() if not (r[0] in (4, 5) and r[1] == 8)]
        self.assertFalse(parse_lifecycle(encode(rows))['lifetimesReconciled'])
        rows = [r for r in stream() if not (r[0] == 4 and r[1] == 8)]
        partial = parse_lifecycle(encode(rows))
        self.assertFalse(partial['lifetimesReconciled'])
        self.assertFalse(next(p for p in partial['processes'] if p['pid'] == 8)['metadataComplete'])
        rows = stream()
        rows[5] = (4, 8, 0, 0, 1, 0, '')
        # Exit metadata is also unknown when no live process handle was retained.
        rows[6] = (5, 8, 0, 0, 1, 0, '')
        self.assertFalse(parse_lifecycle(encode(rows))['lifetimesReconciled'])

    def test_pid_reuse_image_change_and_unrooted_trace_decline(self):
        for field, value in [(5, 501), (6, 'C:\\other.exe')]:
            rows = stream(); changed = list(rows[4]); changed[field] = value; rows[4] = tuple(changed)
            with self.assertRaises(ValueError):
                parse_lifecycle(encode(rows))
        with self.assertRaises(ValueError):
            reconcile_traces(parse_lifecycle(encode(stream())), {5, 99})

    def test_corrupt_truncated_lost_and_duplicate_lifecycle_records_decline(self):
        valid = encode(stream())
        for data in [valid[:-1], valid[1:], valid + b'\0', encode(stream() + [stream()[0]])]:
            with self.assertRaises(ValueError):
                parse_lifecycle(data)
        rows = stream(); bad = list(rows[4]); bad[4] = 2; rows[4] = tuple(bad)
        with self.assertRaises(ValueError):
            parse_lifecycle(encode(rows))

    def test_parent_identity_and_loader_image_lifetime_are_not_guessed(self):
        rows = stream(debug=True)
        changed = list(rows[5]); changed[2] = 999; rows[5] = tuple(changed)
        with self.assertRaises(ValueError):
            parse_lifecycle(encode(rows))
        rows = stream()
        rows.insert(5, (8, 5, 0, 0, 0, 0, 'C:\\early.dll'))
        with self.assertRaises(ValueError):
            parse_lifecycle(encode(rows))


if __name__ == '__main__':
    unittest.main()
