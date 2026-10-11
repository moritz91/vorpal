#!/usr/bin/env python3
"""Real MSVC controls for the report-only native dependency audit.

Creates fresh isolated fixtures outside the repository. Native source bytes stay
fixed while only fixture headers/search shadows change. No product/cache enabled.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import shutil
from types import SimpleNamespace
from unittest.mock import patch
import cpp_macro_native_dependency_audit as module


SOURCE = b'''int external_value();
#if defined(_MSC_VER) && !defined(__clang__)
# if __has_include("optional.h")
#  include "optional.h"
# endif
#endif
#ifndef AUDIT_VALUE
#define AUDIT_VALUE 0
#endif
int value = AUDIT_VALUE;
'''


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--launcher', required=True)
    parser.add_argument('--dll', required=True)
    parser.add_argument('--directory', required=True)
    args = parser.parse_args()
    root = Path(args.directory).resolve()
    root.mkdir(parents=True, exist_ok=False)
    source_dir, sdk, later, reports = (root / p for p in ('src', 'sdk', 'later', 'reports'))
    for path in (source_dir, sdk, later, reports):
        path.mkdir()
    source = source_dir / 'probe.cc'
    source.write_bytes(SOURCE)
    plan = root / 'plan.json'
    entry = {'source': str(source), 'cwd': str(source_dir), 'args':
             ['/nologo', '/std:c++20', '/Zc:preprocessor', '/I' + str(sdk),
              '/I' + str(later), str(source), '/Zs']}
    plan.write_text(json.dumps([entry]), encoding='utf-8')
    options = SimpleNamespace(plan=str(plan), source=str(source), launcher=args.launcher,
                              dll=args.dll, compiler='cl.exe', identity_input=[], timeout=30)
    results = {}

    def capture(label):
        options.report = str(reports / (label + '.json'))
        result = module.audit(options)
        assert result['replayEligible'] is False and result['boundaries']
        results[label] = result
        return result

    def path_snapshot(result, path):
        key = os.path.normcase(str(path))
        matches = [v for p, v in result['snapshots'].items() if os.path.normcase(p) == key]
        assert matches, 'native candidate was not observed: ' + str(path)
        return matches[0]

    def listed(result, path):
        key = os.path.normcase(str(path))
        return any(os.path.normcase(module.filesystem_path(p['path']) or '') == key
                   for p in result['directoryEntries'])

    optional = sdk / 'optional.h'
    absent = capture('absent')
    assert absent['nativeOutputEqual'] and absent['contextStable']
    assert not listed(absent, optional)
    assert path_snapshot(absent, sdk)['state'] == 'directory'

    def source_dependencies(label):
        # Microsoft's list is a useful successful-include control, not a list of
        # unsuccessful candidates or directory-cache inputs. /Zs emits no object.
        output = reports / (label + '-source-dependencies.json')
        environment = dict(os.environ)
        environment['VORPAL_NATIVE_AUDIT_TRACE'] = str(reports / (label + '-dependency-job'))
        result = subprocess.run([args.launcher, '-', '30000', '--', shutil.which('cl.exe'),
                                 *entry['args'], '/sourceDependencies', str(output)],
                                cwd=source_dir, env=environment, capture_output=True, timeout=40)
        assert result.returncode == 0 and output.exists(), 'MSVC sourceDependencies control failed'
        return json.loads(output.read_bytes())['Data']['Includes']

    assert not source_dependencies('absent')
    optional.write_bytes(b'#define AUDIT_VALUE 1\n')
    created = capture('created')
    assert listed(created, optional) and created['nativeOutputEqual']
    assert created['nativeOutputSha256'] != absent['nativeOutputSha256']
    assert any(os.path.normcase(p) == os.path.normcase(str(optional)) for p in source_dependencies('created'))
    stamp = optional.stat()
    optional.write_bytes(b'#define AUDIT_VALUE 2\n')
    os.utime(optional, ns=(stamp.st_atime_ns, stamp.st_mtime_ns))
    edited = capture('same-mtime-edited')
    assert edited['nativeOutputEqual'] and optional.stat().st_mtime_ns == stamp.st_mtime_ns
    assert path_snapshot(created, optional)['sha256'] != path_snapshot(edited, optional)['sha256']
    assert created['nativeOutputSha256'] != edited['nativeOutputSha256']
    optional.unlink()
    removed = capture('removed')
    assert not listed(removed, optional) and removed['nativeOutputEqual']
    assert removed['nativeOutputSha256'] == absent['nativeOutputSha256']

    # The later include root supplies the name; a new local file shadows it.
    fallback = later / 'optional.h'
    fallback.write_bytes(b'#define AUDIT_VALUE 3\n')
    root_hit = capture('later-root')
    shadow = source_dir / 'optional.h'
    shadow.write_bytes(b'#define AUDIT_VALUE 4\n')
    shadowed = capture('local-shadow')
    assert root_hit['nativeOutputEqual'] and shadowed['nativeOutputEqual']
    assert path_snapshot(root_hit, fallback)['state'] == 'file'
    assert path_snapshot(shadowed, shadow)['state'] == 'file'
    assert root_hit['nativeOutputSha256'] != shadowed['nativeOutputSha256']
    shadow.unlink()
    unshadowed = capture('shadow-removed')
    assert unshadowed['nativeOutputSha256'] == root_hit['nativeOutputSha256']

    # Deterministic real native mutation inside the discovery/confirmation bracket.
    # This detects a sustained concurrent change, not an ABA completeness proof.
    original_run = module.run

    def changed_run(command, cwd, environment, directory, label, timeout):
        if label == 'confirmed':
            original_stat = fallback.stat()
            fallback.write_bytes(b'#define AUDIT_VALUE 5\n')
            os.utime(fallback, ns=(original_stat.st_atime_ns, original_stat.st_mtime_ns))
        return original_run(command, cwd, environment, directory, label, timeout)

    with patch.object(module, 'run', changed_run):
        concurrent = capture('changed-during-bracket')
    assert not concurrent['snapshotStable'] and not concurrent['nativeOutputEqual']
    assert any(os.path.normcase(p) == os.path.normcase(str(fallback)) for p in concurrent['changedCandidates'])

    # Genuine /FI input participates in the same native read inventory.
    forced = root / 'forced.h'
    forced.write_bytes(b'#define FORCED_VALUE 17\n')
    entry['args'].insert(-2, '/FI' + str(forced))
    plan.write_text(json.dumps([entry]), encoding='utf-8')
    forced_case = capture('forced-input')
    assert forced_case['nativeOutputEqual'] and path_snapshot(forced_case, forced)['state'] == 'file'

    # Junctions are genuine lexical redirects, and require no symlink privilege.
    target_a, target_b, junction = root / 'target-a', root / 'target-b', root / 'redirect'
    target_a.mkdir(); target_b.mkdir()
    for target in (target_a, target_b):
        (target / 'optional.h').write_bytes(b'#define AUDIT_VALUE 6\n')
    fallback.unlink()
    entry['args'].insert(3, '/I' + str(junction))
    plan.write_text(json.dumps([entry]), encoding='utf-8')

    def link(target):
        command = "New-Item -ItemType Junction -Path '" + str(junction).replace("'", "''") + "' -Target '" + str(target).replace("'", "''") + "' | Out-Null"
        subprocess.run(['powershell.exe', '-NoProfile', '-Command', command], check=True)

    link(target_a)
    redirect_a = capture('redirect-a')
    os.rmdir(junction)  # Removes only the verified junction, not its target tree.
    link(target_b)
    redirect_b = capture('redirect-b')
    assert redirect_a['nativeOutputEqual'] and redirect_b['nativeOutputEqual']
    assert path_snapshot(redirect_a, junction / 'optional.h')['resolved'] != path_snapshot(redirect_b, junction / 'optional.h')['resolved']
    assert (target_a / 'optional.h').exists() and (target_b / 'optional.h').exists()
    assert source.read_bytes() == SOURCE

    summary = {label: {k: r[k] for k in ('replayEligible', 'nativeOutputEqual', 'contextStable',
               'snapshotStable', 'processCoverage', 'counts')} for label, r in results.items()}
    (root / 'controls.json').write_text(json.dumps(summary, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({'nativeControls': len(results), 'passed': True, 'replayEligible': False}))


if __name__ == '__main__':
    main()
