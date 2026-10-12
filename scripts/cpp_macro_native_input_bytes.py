"""Bounded local read-buffer/loader-file audit; never a replay contract."""
import hashlib
import ntpath
from pathlib import Path
import struct

MAGIC = b'VORPAL-NATIVE-INPUT-BYTES-1\n'
RECORD = struct.Struct('<8I8Q16s')
MAX_BYTES = 512 * 1024 * 1024
MAX_PAYLOAD = 64 * 1024 * 1024
KINDS = {1: 'start', 2: 'readReturn', 3: 'loaderFile', 4: 'sectionBoundary', 5: 'end'}
FLAGS = {1: 'identityUnavailable', 2: 'overLimit', 4: 'pending', 8: 'identityChanged',
         16: 'offsetUnavailable', 32: 'mappedPagesUnobserved', 64: 'bytesUnavailable'}


def parse_input_bytes(data):
    if len(data) > MAX_BYTES or not data.startswith(MAGIC):
        raise ValueError('invalid or oversized input-byte ledger')
    offset, records = len(MAGIC), []
    while offset < len(data):
        if len(records) >= 100000 or len(data) - offset < RECORD.size:
            raise ValueError('truncated or excessive input-byte records')
        (kind, status, flags, pid, thread, units, size, reserved, sequence, trace,
         begin, end, handle, volume, created, position, file_id) = RECORD.unpack_from(data, offset)
        offset += RECORD.size
        if (kind not in KINDS or flags & ~127 or reserved or not pid or not thread
                or units > 32768 or size > MAX_PAYLOAD or sequence != len(records) + 1
                or end < begin or units * 2 + size > len(data) - offset):
            raise ValueError('invalid/lost input-byte metadata or lengths')
        path = data[offset:offset + units * 2].decode('utf-16-le', errors='surrogatepass')
        offset += units * 2
        if '\0' in path or (kind == 4 and (size or not flags & 32)):
            raise ValueError('invalid input path or fabricated mapping bytes')
        if flags & (2 | 4 | 64) and size:
            raise ValueError('bytes retained for incomplete read')
        if kind == 2 and ((status == 0x103) != bool(flags & 4) or (status != 0 and size)):
            raise ValueError('async/failed read is not a completed return buffer')
        payload = memoryview(data)[offset:offset + size]
        offset += size
        records.append({'kind': KINDS[kind], 'pid': pid, 'thread': thread, 'status': status,
            'flags': [name for bit, name in FLAGS.items() if flags & bit],
            'sequence': sequence, 'traceSequence': trace, 'begin': begin, 'end': end,
            'handle': handle, 'volume': volume, 'fileId': file_id.hex(), 'created': created,
            'path': path, 'imageBase' if kind == 3 else 'requestedOffset': position,
            'bytes': size, 'sha256': hashlib.sha256(payload).hexdigest()})
    if (len(records) < 2 or records[0]['kind'] != 'start' or records[-1]['kind'] != 'end'
            or any(r['kind'] in ('start', 'end') for r in records[1:-1])
            or any(r['flags'] or r['status'] or r['bytes'] or r['path'] for r in (records[0], records[-1]))):
        raise ValueError('input-byte ledger endpoints missing or failed')
    return records


def capture_inputs(prefix, lifecycle, captures):
    """Tie each returned buffer to the exact trace call and observed execution."""
    processes = {p['pid']: p for p in lifecycle['processes']}
    expected_files = {str(prefix) + f'.{pid}.bytes.bin' for pid in captures}
    actual_files = {str(p) for p in prefix.parent.glob(prefix.name + '.*.bytes.bin')}
    if expected_files != actual_files:
        raise ValueError('missing or unowned input-byte sidecar')
    ledgers = {}
    total = 0
    for pid, trace in captures.items():
        path = Path(str(prefix) + f'.{pid}.bytes.bin')
        data = path.read_bytes()
        total += len(data)
        if total > MAX_BYTES:
            raise ValueError('aggregate input-byte limit exceeded')
        records = parse_input_bytes(data)
        if records[0]['pid'] != pid or records[-1]['pid'] != pid:
            raise ValueError('input envelope belongs to another execution')
        indexed = {r['sequence']: r for r in trace}
        seen = set()
        for record in records[1:-1]:
            reference = record['traceSequence']
            observed = indexed.get(reference)
            expected = 'read' if record['kind'] == 'readReturn' else 'section'
            if (record['kind'] not in ('readReturn', 'sectionBoundary') or reference in seen
                    or not observed or observed['kind'] != expected or record['pid'] != pid
                    or record['created'] != processes[pid]['created']
                    or record['status'] != observed['status'] or record['thread'] != observed['thread']
                    or record['path'] != observed['path'] or record['begin'] != observed['begin']
                    or (expected == 'read' and record['bytes'] > observed['access'])):
                raise ValueError('returned buffer has no matching native call/execution')
            seen.add(reference)
        if seen != {r['sequence'] for r in trace if r['kind'] in ('read', 'section')}:
            raise ValueError('native read/section input observation missing')
        ledgers[pid] = records[1:-1]
    data = Path(str(prefix) + '.loader.bin').read_bytes()
    total += len(data)
    if total > MAX_BYTES:
        raise ValueError('aggregate loader/read byte limit exceeded')
    loaders = parse_input_bytes(data)[1:-1]
    events = [r for r in lifecycle['records'] if r['kind'] in ('debugCreate', 'image')]
    if [r['pid'] for r in loaders] != [r['pid'] for r in events]:
        raise ValueError('loader snapshots do not match independent loader/create events')
    for record, event in zip(loaders,events):
        expected_path = ntpath.normcase(event['image'].removeprefix('\\\\?\\'))
        actual_path = ntpath.normcase(record['path'].removeprefix('\\\\?\\'))
        if expected_path and actual_path and expected_path != actual_path:
            raise ValueError('loader snapshot names a different image than the OS event')
        if (record['kind'] != 'loaderFile' or record['pid'] not in processes
                or record['created'] != processes[record['pid']]['created']):
            raise ValueError('loader file does not belong to an observed execution')
    # Snapshotting a loader file is not proof of the bytes actually mapped or
    # consumed; user-mode returned-buffer copies also remain observational.
    return {'version': 1, 'scope': 'separate-debugger-execution-input-audit',
        'returnedBuffers': ledgers, 'loaderFileSnapshots': loaders, 'artifactBytes': total,
        'actualMappedPagesCaptured': False, 'ordinaryExecutionInputProof': False,
        'changeAndRestoreExcluded': False, 'replayEligible': False}
