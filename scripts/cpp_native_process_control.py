#!/usr/bin/env python3
"""Independent Windows process-lifecycle controls; no replay/cache authorization."""
import argparse
import json
import ntpath
import os
from pathlib import Path
import shutil
import subprocess
from types import SimpleNamespace
from cpp_macro_native_dependency_audit import run, parse_capture, validate_plan
from cpp_macro_native_lifecycle import read_lifecycle, decode_records


def main():
    parser = argparse.ArgumentParser(__doc__)
    for name in ('launcher', 'debugger', 'dll', 'fixture', 'directory'):
        parser.add_argument('--' + name, required=True)
    parser.add_argument('--plan')
    parser.add_argument('--source')
    args = parser.parse_args()
    root = Path(args.directory).resolve()
    root.mkdir(parents=True, exist_ok=False)
    data = root / 'input with spaces.txt'
    data.write_bytes(b'unchanged fixture input\n')
    initial = data.read_bytes()
    summaries = {}

    def capture(label, command, debug=False, inject=False, cwd=None):
        directory = root / label
        directory.mkdir()
        prefix = directory / 'process'
        environment = dict(os.environ)
        environment['VORPAL_NATIVE_AUDIT_TRACE'] = str(prefix)
        launcher = args.debugger if debug else args.launcher
        output = run([launcher, args.dll if inject else '-', '30000', '--', *command],
                     cwd or root, environment, directory, 'native', 30)
        life = read_lifecycle(prefix)
        coverage = parse_capture(prefix)[1] if inject else None
        (directory / 'lifecycle.json').write_text(json.dumps(life, indent=2) + '\n', encoding='utf-8')
        summaries[label] = {'mode': life['mode'], 'outputSha256': output,
            'lifetimesReconciled': life['lifetimesReconciled'],
            'uniqueProcesses': life['uniqueProcesses'], 'jobAssociations': life['jobAssociations'],
            'images': sorted(ntpath.basename(p['image']) for p in life['processes']),
            'traceCoverage': coverage, 'replayEligible': False}
        return SimpleNamespace(output=output, life=life, coverage=coverage)

    fixture = str(Path(args.fixture).resolve())
    expected_image = ntpath.normcase(fixture)
    for mode, expected in [('wide', 1), ('ansi', 1), ('crt', 1), ('burst', 8), ('orphan', 1)]:
        command = [fixture, mode, str(data)]
        ordinary = capture(mode + '-ordinary', command)
        observed = capture(mode + '-debug', command, debug=True)
        instrumented = capture(mode + '-injected-debug', command, debug=True, inject=True)
        assert ordinary.output == observed.output == instrumented.output
        for item in (observed, instrumented):
            children = [p for p in item.life['processes'] if p['pid'] != item.life['rootPid']
                        and ntpath.normcase(p['image']) == expected_image]
            assert len(children) == expected, 'debug observer lost fixture executions'
            assert item.life['lifetimesReconciled'], 'independent start/exit/accounting did not reconcile'
        assert instrumented.coverage['lifecycleReconciled']
        # Other observed children (including console hosts) remain in the report;
        # their names never authorize dropping them from filesystem coverage.
        assert data.read_bytes() == initial

    if bool(args.plan) != bool(args.source):
        raise ValueError('native plan/source must be supplied together')
    if args.plan:
        entries = json.loads(Path(args.plan).read_bytes())
        source = Path(args.source).resolve()
        selected = [e for e in entries if os.path.normcase(os.path.realpath(e['source'])) == os.path.normcase(str(source))]
        assert len(selected) == 1
        entry = selected[0]
        unit = entry.get('translationUnit', entry['source'])
        command = [shutil.which('cl.exe'), *validate_plan(entry['args'], unit)]
        before_source, before_unit = source.read_bytes(), Path(unit).read_bytes()
        ordinary = capture('msvc-ordinary', command, cwd=entry['cwd'])
        observed = capture('msvc-debug', command, debug=True, cwd=entry['cwd'])
        instrumented = capture('msvc-injected-debug', command, debug=True, inject=True, cwd=entry['cwd'])
        assert ordinary.output == observed.output == instrumented.output
        assert source.read_bytes() == before_source and Path(unit).read_bytes() == before_unit
        assert observed.life['lifetimesReconciled'] and instrumented.life['lifetimesReconciled']
        # Missing console helper traces are retained when present, with no
        # platform-dependent assumption that every MSVC version creates one.
        summaries['msvc-injected-debug']['independentMissingImages'] = instrumented.coverage['untracedProcessImages']

    # Timeouts must close only this launcher's job, not leave live descendants.
    timeout_dir = root / 'timeout'
    timeout_dir.mkdir()
    environment = dict(os.environ)
    environment['VORPAL_NATIVE_AUDIT_TRACE'] = str(timeout_dir / 'process')
    result = subprocess.run([args.debugger, '-', '100', '--', fixture, 'timeout', str(data)],
        cwd=root, env=environment, stdin=subprocess.DEVNULL, capture_output=True, timeout=15)
    assert result.returncode != 0
    assert Path(environment['VORPAL_NATIVE_AUDIT_TRACE'] + '.job.bin').exists()
    # Verify the original executions have stopped, without killing/reinterpreting
    # a reused PID. Failed metadata and access denial do not prove termination.
    import ctypes
    from ctypes import wintypes
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    kernel.OpenProcess.restype = wintypes.HANDLE
    kernel.GetProcessTimes.argtypes = [wintypes.HANDLE, *([ctypes.POINTER(wintypes.FILETIME)] * 4)]
    kernel.GetProcessTimes.restype = wintypes.BOOL
    kernel.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
    kernel.WaitForSingleObject.restype = wintypes.DWORD
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    kernel.CloseHandle.restype = wintypes.BOOL
    records = decode_records(Path(environment['VORPAL_NATIVE_AUDIT_TRACE'] + '.life.bin').read_bytes())
    identities = {(r['pid'], r['created']) for r in records if r['kind'] in ('root', 'jobAdd', 'debugCreate') and r['created']}
    assert identities
    for pid, created in identities:
        handle = kernel.OpenProcess(0x00100000 | 0x1000, False, pid)
        if not handle:
            assert ctypes.get_last_error() == 87, 'access failure is not process termination evidence'
            continue
        try:
            times = [wintypes.FILETIME() for _ in range(4)]
            assert kernel.GetProcessTimes(handle, *[ctypes.byref(t) for t in times])
            actual = (times[0].dwHighDateTime << 32) | times[0].dwLowDateTime
            assert actual != created or kernel.WaitForSingleObject(handle, 0) == 0, 'original timed-out process remains live'
        finally:
            kernel.CloseHandle(handle)
    summaries['timeout'] = {'declined': True, 'exitCode': result.returncode,
                            'originalExecutionsStopped': True, 'replayEligible': False}

    (root / 'controls.json').write_text(json.dumps(summaries, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({'processControls': len(summaries), 'passed': True, 'replayEligible': False}))


if __name__ == '__main__':
    main()
