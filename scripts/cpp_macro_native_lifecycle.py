"""Strict report-only reader for independent Windows process-lifecycle evidence."""
import ntpath
from pathlib import Path
import struct

MAGIC = b'VORPAL-NATIVE-LIFECYCLE-1\n'
RECORD = struct.Struct('<6I3Q')
KINDS = {1: 'start', 2: 'root', 3: 'jobSample', 4: 'jobAdd', 5: 'jobExit',
         6: 'debugCreate', 7: 'debugExit', 8: 'image', 9: 'end'}
MAX_BYTES = 16 * 1024 * 1024


def decode_records(data):
    """Bounded framing only; usable to inspect declined/timeout artifacts."""
    if not data.startswith(MAGIC) or len(data) > MAX_BYTES:
        raise ValueError('invalid or oversized lifecycle trace')
    offset, records, clock = len(MAGIC), [], 0
    while offset < len(data):
        if len(records) >= 16384 or len(data) - offset < RECORD.size:
            raise ValueError('truncated or excessive lifecycle records')
        kind, pid, parent, status, flags, units, sequence, ticks, created = RECORD.unpack_from(data, offset)
        offset += RECORD.size
        if kind not in KINDS or flags & ~3 or flags & 2 or units > 32768 or units * 2 > len(data) - offset:
            raise ValueError('unknown, lost or oversized lifecycle record')
        name = data[offset:offset + units * 2].decode('utf-16-le', errors='surrogatepass')
        offset += units * 2
        if '\0' in name or sequence != len(records) + 1 or ticks < clock:
            raise ValueError('invalid lifecycle sequence, path or clock')
        clock = ticks
        records.append(dict(kind=KINDS[kind], pid=pid, parent=parent, status=status,
                            flags=flags, sequence=sequence, ticks=ticks, created=created, image=name))
    return records


def parse_lifecycle(data):
    records = decode_records(data)
    if len(records) < 6 or records[0]['kind'] != 'start' or records[-1]['kind'] != 'end':
        raise ValueError('lifecycle endpoints missing')
    if (records[0]['status'] not in (0, 1) or records[0]['flags'] or not records[0]['pid']
            or records[-1]['status'] or records[-1]['flags']):
        raise ValueError('lifecycle observer or compiler failed')
    roots = [r for r in records if r['kind'] == 'root']
    samples = [r for r in records if r['kind'] == 'jobSample']
    if (len(roots) != 1 or [s['pid'] for s in samples] != [0, 1, 2]
            or any(r['kind'] in ('start', 'end') for r in records[1:-1])):
        raise ValueError('lifecycle root/accounting sequence invalid')
    root = roots[0]
    if (not root['pid'] or root['status'] or root['flags'] or not root['created'] or not root['image']
            or root['parent'] != records[0]['pid'] or root['pid'] != records[-1]['pid']
            or any(s['flags'] for s in samples) or samples[0]['parent'] or samples[0]['status']
            or samples[1]['parent'] < 1 or samples[1]['status'] < 1
            or samples[-1]['status'] or samples[-1]['created']
            or samples[-1]['parent'] != records[-1]['parent']
            or not 1 <= samples[-1]['parent'] <= 128):
        raise ValueError('lifecycle root or terminal accounting invalid')
    processes = {}
    debug = records[0]['status'] == 1
    for record in records:
        kind, pid = record['kind'], record['pid']
        if kind in ('root', 'jobAdd', 'debugCreate'):
            if not pid:
                raise ValueError('zero process identifier')
            if pid not in processes:
                if len(processes) >= 128:
                    raise ValueError('too many lifecycle processes')
                processes[pid] = {'pid': pid, 'created': record['created'],
                    'parent': record['parent'], 'image': record['image'],
                    'metadataComplete': not record['flags'] and bool(record['created']) and bool(record['image']),
                    'jobStarts': [], 'jobExits': [], 'debugStarts': [], 'debugExits': []}
            process = processes[pid]
            if record['created'] and process['created'] and process['created'] != record['created']:
                raise ValueError('process identifier reused during capture')
            if record['image'] and process['image'] and ntpath.normcase(record['image']) != ntpath.normcase(process['image']):
                raise ValueError('process image identity changed')
            if record['parent'] and process['parent'] and record['parent'] != process['parent']:
                raise ValueError('process ancestry changed')
            # Unknown advisory metadata is not repaired with a later PID lookup.
            process['metadataComplete'] &= not record['flags'] and bool(record['created']) and bool(record['image'])
            if kind == 'jobAdd':
                process['jobStarts'].append(record['sequence'])
            elif kind == 'debugCreate':
                if not debug or process['debugStarts']:
                    raise ValueError('duplicate or unexpected debug create')
                process['debugStarts'].append(record['sequence'])
        elif kind in ('jobExit', 'debugExit'):
            if pid not in processes:
                if kind == 'debugExit' or not pid:
                    raise ValueError('debug exit without observed start')
                # An advisory create notification may have been missed. Retain
                # the exit as incomplete evidence, without inventing its start.
                processes[pid] = {'pid': pid, 'created': record['created'],
                    'parent': 0, 'image': '', 'metadataComplete': False,
                    'jobStarts': [], 'jobExits': [], 'debugStarts': [], 'debugExits': []}
            process = processes[pid]
            if record['created'] and process['created'] != record['created']:
                raise ValueError('exit has different process identity')
            starts = process['debugStarts' if kind == 'debugExit' else 'jobStarts']
            exits = process['debugExits' if kind == 'debugExit' else 'jobExits']
            if exits or (kind == 'debugExit' and (not starts or not debug)):
                raise ValueError('duplicate or unordered process exit')
            process['metadataComplete'] &= not record['flags'] and bool(record['created'])
            process['metadataComplete'] &= bool(starts)
            exits.append(record['sequence'])
            process['exitCode'] = record['status']
        elif kind == 'image' and (not debug or pid not in processes
                                 or not processes[pid]['debugStarts'] or processes[pid]['debugExits']):
            raise ValueError('image outside debug process lifetime')
    if any(p['parent'] not in processes and p['pid'] != root['pid'] for p in processes.values()):
        rooted = False
    else:
        rooted = True
        for process in processes.values():
            visited, current = set(), process['pid']
            while current != root['pid']:
                if current in visited:
                    raise ValueError('cycle in process ancestry')
                visited.add(current)
                current = processes[current]['parent']
    job_starts = sum(len(p['jobStarts']) for p in processes.values())
    complete = (rooted and job_starts == samples[-1]['parent'] and
                all(p['metadataComplete'] and p['jobStarts'] and p['jobExits'] for p in processes.values()))
    if debug:
        complete &= all(len(p['debugStarts']) == len(p['debugExits']) == 1 for p in processes.values())
    return {'mode': 'debug' if debug else 'normal', 'rootPid': root['pid'],
            'jobAssociations': samples[-1]['parent'], 'observedJobAssociations': job_starts,
            'uniqueProcesses': len(processes), 'lifetimesReconciled': bool(complete),
            'processes': list(processes.values()), 'records': records,
            'debuggerMayChangeBehavior': debug, 'replayEligible': False}


def read_lifecycle(prefix):
    return parse_lifecycle(Path(str(prefix) + '.life.bin').read_bytes())


def reconcile_traces(lifecycle, trace_pids):
    known = {p['pid']: p for p in lifecycle['processes']}
    if not set(trace_pids) <= set(known):
        raise ValueError('file trace has no independently observed process')
    missing = [p for p in lifecycle['processes'] if p['pid'] not in trace_pids]
    return {'jobProcesses': lifecycle['jobAssociations'], 'tracedProcesses': len(trace_pids),
        'uniqueObservedProcesses': lifecycle['uniqueProcesses'],
        'lifecycleReconciled': lifecycle['lifetimesReconciled'],
        'observerMode': lifecycle['mode'],
        'untracedProcessImages': sorted({p['image'] for p in missing}),
        'allObservedProcessesTraced': not missing,
        'allJobProcessesTraced': not missing and lifecycle['lifetimesReconciled']}
