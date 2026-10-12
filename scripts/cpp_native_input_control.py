#!/usr/bin/env python3
"""Genuine Windows whole-process-tree input controls; audit only, no replay."""
import argparse
import json
import ntpath
import os
from pathlib import Path
import shutil
from cpp_macro_native_dependency_audit import run, parse_capture, parse_trace, validate_plan, digest
from cpp_macro_native_lifecycle import read_lifecycle
from cpp_macro_native_input_bytes import capture_inputs


def capture(command, directory, launcher, dll, whole=False, cwd=None):
    directory.mkdir()
    prefix = directory / 'trace'
    environment = dict(os.environ, VORPAL_NATIVE_AUDIT_TRACE=str(prefix))
    argv = [launcher, dll if whole else '-', '60000']
    if whole:
        argv.append('--whole-tree')
    output = run([*argv, '--', *command], cwd or directory, environment, directory, 'native', 60)
    life = read_lifecycle(prefix)
    if not whole:
        return output, life, None, None
    records, coverage = parse_capture(prefix)
    captures = {p['pid']: parse_trace(Path(str(prefix) + f".{p['pid']}.bin").read_bytes())
                for p in life['processes']}
    inputs = capture_inputs(prefix, life, captures)
    (directory / 'inputs.json').write_text(json.dumps(inputs, indent=2) + '\n', encoding='utf-8')
    (directory / 'lifecycle.json').write_text(json.dumps(life, indent=2) + '\n', encoding='utf-8')
    assert coverage['allJobProcessesTraced'], 'new process escaped complete framed debug traces'
    assert life['debuggerMayChangeBehavior'] and not inputs['replayEligible']
    return output, life, coverage, inputs


def summary(life, coverage, inputs):
    reads = [r for rows in inputs['returnedBuffers'].values() for r in rows if r['kind'] == 'readReturn']
    loaders = inputs['loaderFileSnapshots']
    return {'processes': life['uniqueProcesses'], 'images': sorted(ntpath.basename(p['image']) for p in life['processes']),
        'processCoverage': coverage, 'returnedReads': len(reads), 'returnedBytes': sum(r['bytes'] for r in reads),
        'pendingReads': sum('pending' in r['flags'] for r in reads), 'loaderFiles': len(loaders),
        'loaderFileBytes': sum(r['bytes'] for r in loaders), 'unavailableLoaderFiles': sum(bool(r['flags']) for r in loaders),
        'actualMappedPagesCaptured': False, 'ordinaryExecutionInputProof': False,
        'changeAndRestoreExcluded': False, 'replayEligible': False}


def main():
    parser = argparse.ArgumentParser(__doc__)
    for name in ('launcher', 'debugger', 'dll', 'fixture', 'input-fixture', 'directory'):
        parser.add_argument('--' + name, required=True)
    parser.add_argument('--plan')
    parser.add_argument('--source')
    args = parser.parse_args()
    if bool(args.plan) != bool(args.source):
        raise ValueError('native source/plan must be supplied together')
    root = Path(args.directory).resolve()
    root.mkdir(parents=True, exist_ok=False)
    controls = {}
    original = b'S' * len(b'changed native input for ABA\n')
    fixture, input_fixture = str(Path(args.fixture).resolve()), str(Path(args.input_fixture).resolve())
    for mode in ('wide', 'ansi', 'crt', 'burst', 'orphan'):
        directory = root / mode
        directory.mkdir()
        data = directory / 'private input.txt'
        data.write_bytes(original)
        command = [fixture, mode, str(data)]
        ordinary = capture(command, directory / 'ordinary', args.launcher, args.dll)
        whole = capture(command, directory / 'whole', args.debugger, args.dll, True)
        assert ordinary[0] == whole[0] and data.read_bytes() == original
        controls['process-' + mode] = summary(*whole[1:])
    for mode in ('read', 'offset', 'async', 'mapping', 'aba-read', 'aba-mapping', 'sustained-read', 'missing', 'hardlink', 'invalid-offset'):
        directory = root / mode
        directory.mkdir()
        data = directory / 'private input.txt'
        data.write_bytes(original)
        before = data.stat().st_mtime_ns
        outputs = []
        for label, launcher, whole_mode in [('ordinary', args.launcher, False),
                                            ('debug', args.debugger, False), ('whole', args.debugger, True)]:
            data.write_bytes(original)
            os.utime(data, ns=(before, before))
            item = capture([input_fixture, mode, str(data)], directory / label, launcher, args.dll, whole_mode)
            outputs.append(item[0])
        assert len(set(outputs)) == 1, 'debug/instrumentation changed native output'
        life, coverage, inputs = item[1:]
        controls[mode] = summary(life, coverage, inputs)
        source_reads = [r for r in inputs['returnedBuffers'][life['rootPid']]
                        if ntpath.normcase(r['path'].removeprefix('\\\\?\\')) == ntpath.normcase(str(data))]
        wanted = b'changed native input for ABA\n' if mode in ('aba-read', 'aba-mapping', 'sustained-read') else original[3:] if mode == 'offset' else original
        if mode in ('read', 'offset', 'hardlink', 'aba-read', 'sustained-read'):
            assert any(r['kind'] == 'readReturn' and r['sha256'] == digest(wanted) and r['bytes'] == len(wanted)
                       and r['fileId'] != '0'*32 and 'identityUnavailable' not in r['flags'] for r in source_reads)
        if mode in ('mapping', 'aba-mapping'):
            assert any(r['kind'] == 'sectionBoundary' and not r['bytes'] for r in source_reads)
            assert not any(r['kind'] == 'readReturn' and r['sha256'] == digest(wanted) for r in source_reads)
        if mode == 'invalid-offset':
            assert any(r['status'] == 0xc0000005 and not r['bytes'] for r in source_reads)
            controls[mode]['nativePointerFailurePreserved'] = True
        if mode == 'offset':
            assert any(r['requestedOffset'] == 3 and r['sha256'] == digest(wanted) for r in source_reads)
        if mode == 'hardlink':
            alias_reads = [r for r in inputs['returnedBuffers'][life['rootPid']]
                           if r['path'].endswith('.alias') and r['kind'] == 'readReturn']
            assert alias_reads and source_reads
            assert (alias_reads[0]['volume'],alias_reads[0]['fileId']) == (source_reads[0]['volume'],source_reads[0]['fileId'])
            assert alias_reads[0]['handle'] != source_reads[0]['handle']
            controls[mode]['distinctHandlesSamePhysicalIdentity'] = True
        if mode in ('aba-read', 'aba-mapping'):
            assert data.read_bytes() == original and data.stat().st_mtime_ns == before
            assert outputs[-1] == digest(wanted)
            controls[mode]['beforeAfterBytesAndMtimeEqual'] = True
            controls[mode]['nativeOutputContainsTransientBytes'] = True
        if mode == 'sustained-read':
            assert data.read_bytes() != original
            controls[mode]['sustainedChangeVisible'] = True
    if args.plan:
        source = Path(args.source).resolve()
        entries = json.loads(Path(args.plan).read_bytes())
        selected = [e for e in entries if os.path.normcase(os.path.realpath(e['source'])) == os.path.normcase(str(source))]
        assert len(selected) == 1
        entry = selected[0]
        unit = Path(entry.get('translationUnit', entry['source']))
        source_before, unit_before = source.read_bytes(), unit.read_bytes()
        command = [shutil.which('cl.exe'), *validate_plan(entry['args'], unit)]
        outputs = []
        directory = root / 'msvc'
        directory.mkdir()
        for label, launcher, whole_mode in [('ordinary', args.launcher, False),
                                            ('debug', args.debugger, False), ('whole', args.debugger, True)]:
            item = capture(command, directory / label, launcher, args.dll, whole_mode, entry['cwd'])
            outputs.append(item[0])
        assert len(set(outputs)) == 1 and source.read_bytes() == source_before and unit.read_bytes() == unit_before
        controls['msvc'] = summary(*item[1:])
        controls['msvc']['nativeOutputEqual'] = True
        controls['msvc']['sourceAndTranslationUnitUnchanged'] = True
    (root / 'controls.json').write_text(json.dumps(controls, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({'inputControls': len(controls), 'passed': True, 'replayEligible': False}))


if __name__ == '__main__':
    main()
