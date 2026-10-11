#!/usr/bin/env python3
"""Create a separate, genuine macOS Foundation context for Catch2 v2.13.9.

Requires a real macOS host/SDK, Apple Clang, and a matching LLVM 19 observer.
The public upstream header is checksum-pinned. Its CRLF mirror must match the
existing Hades physical header byte-for-byte; no production source is modified.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import time
import urllib.request

from cpp_macro_clang_context import sdk_identity

UPSTREAM = 'https://raw.githubusercontent.com/catchorg/Catch2/v2.13.9/single_include/catch2/catch.hpp'
LF_SHA256 = '27da57c7a06d09be8dd81fab7246b79e7892b6ae7e4e49ba8631f1d5a955e3fc'
PHYSICAL_SHA256 = 'ed84f023b21d49f1e337a70601e2af205767eec0004ec5624f5e8340d4b0f7bb'


def run(command, directory, log):
    with log.open('wb') as output:
        result = subprocess.run(command, cwd=directory, stdout=output, stderr=subprocess.STDOUT)
    if result.returncode:
        print(log.read_text(errors='replace'))
        raise RuntimeError(f'Foundation control failed: {log.name}')


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--llvm-root', type=Path, required=True)
    parser.add_argument('--observer', type=Path, required=True)
    parser.add_argument('--directory', type=Path, required=True)
    args = parser.parse_args()
    if platform.system() != 'Darwin':
        raise RuntimeError('this control requires a genuine macOS host')
    root, llvm, observer = args.directory.resolve(), args.llvm_root.resolve(), args.observer.resolve()
    root.mkdir(parents=True, exist_ok=True)
    sdk = Path(subprocess.check_output(['xcrun', '--sdk', 'macosx', '--show-sdk-path'], text=True).strip()).resolve()
    identity = sdk_identity(sdk)
    apple_clang = subprocess.check_output(['xcrun', '--find', 'clang++'], text=True).strip()
    clang = llvm / 'bin/clang++'
    resource = subprocess.check_output([str(clang), '-print-resource-dir'], text=True).strip()
    bindings = sorted(llvm.glob('lib/python*/site-packages/clang/cindex.py'))
    if not bindings:
        raise RuntimeError('matching LLVM Python bindings unavailable')
    binding_root = bindings[-1].parents[1]
    for attempt in range(3):
        try:
            with urllib.request.urlopen(UPSTREAM, timeout=15) as response:
                upstream = response.read(4 * 1024 * 1024 + 1)
            break
        except (OSError, urllib.error.URLError):
            if attempt == 2:
                raise
            time.sleep(2)
    if hashlib.sha256(upstream).hexdigest() != LF_SHA256:
        raise RuntimeError('upstream Catch2 identity changed')
    physical = upstream.replace(b'\n', b'\r\n')
    if hashlib.sha256(physical).hexdigest() != PHYSICAL_SHA256:
        raise RuntimeError('Catch2 physical mirror identity differs')
    catch = root / 'catch.hpp'
    catch.write_bytes(physical)
    wrapper = root / 'catch-foundation.mm'
    wrapper.write_text('#define CATCH_CONFIG_RUNNER\n#include "catch.hpp"\n'
                       'int main() { @autoreleasepool { NSString* s = @"Foundation"; '
                       'return [s length] == 10 ? 0 : 1; } }\n')
    header = root / 'ownership.h'
    header.write_text('#import <Foundation/Foundation.h>\n'
                      '#if __has_feature(objc_arc)\n#define CHECK __unsafe_unretained\n'
                      '#define HANDLE __strong\n#else\n#define CHECK\n#define HANDLE\n#endif\n')
    project = root / 'project'
    project.mkdir(exist_ok=True)
    probe = project / 'ownership.cc'
    probe.write_text('#include "ownership.h"\n#ifdef __OBJC__\n'
                     'struct Holder { NSString* HANDLE stored; };\n'
                     'Class* run() { Class* classes = (CHECK Class*)malloc(sizeof(Class)); return classes; }\n'
                     '#endif\n#undef CHECK\n#undef HANDLE\n'
                     'void CHECK(); void HANDLE();\nvoid ordinary() { CHECK(); HANDLE(); }\n')
    cases = []
    repo = Path(__file__).resolve().parents[1]
    for name, ownership in [('arc', '-fobjc-arc'), ('mrc', '-fno-objc-arc')]:
        flags = ['-x', 'objective-c++', '-std=c++17', '-isysroot', str(sdk),
                 ownership, '-fblocks', '-fno-modules', '-fno-implicit-modules',
                 '-fno-implicit-module-maps', '-I', str(root)]
        executable = root / f'foundation-{name}'
        run([apple_clang, *flags, str(wrapper), '-framework', 'Foundation',
             '-o', str(executable)], root, root / f'apple-{name}.log')
        run([str(executable)], root, root / f'runtime-{name}.log')
        run([str(clang), *flags, str(wrapper), '-fsyntax-only'], root,
            root / f'llvm-{name}.log')
        plan = root / f'plan-{name}.json'
        run([str(clang), *flags, str(probe), '-fsyntax-only'], root, root / f'probe-{name}.log')
        plan.write_text(json.dumps([
            {'source': str(catch), 'translationUnit': str(wrapper),
             'cwd': str(root), 'args': flags + [str(wrapper), '-fsyntax-only']},
            {'source': str(probe), 'cwd': str(root), 'args': flags + [str(probe), '-fsyntax-only']}]))
        command = {'program': sys.executable, 'directory': str(root),
                   'translationUnits': [str(catch), str(probe)], 'timeoutSeconds': 120,
                   'arguments': [str(repo / 'scripts/cpp_macro_compiler_provider.py'),
                       '--driver-mode', 'clang', '--compiler', str(clang),
                       '--plan', str(plan), '--observer', str(observer),
                       '--resource-dir', resource, '--libclang', str(llvm / 'lib/libclang.dylib'),
                       '--python-bindings', str(binding_root), '--native-hash-lexer']}
        cases.append({'name': name, 'path': str(catch), 'probePath': str(probe),
                      'headerPath': str(header), 'command': command})
        # Retain real capture artifacts for investigating native stream/context
        # disagreement. Rust subsequently launches its own fresh handshake.
        capture = root / f'capture-{name}'
        capture.mkdir(exist_ok=True)
        request = capture / 'request.json'
        request.write_text(json.dumps({'version': 1, 'requestId': name,
                                      'path': str(catch), 'source': physical.decode()}))
        ready, response = capture / 'ready', capture / 'response.json'
        ready.touch()
        env = dict(os.environ, VORPAL_CPP_COMPILER_REQUEST=str(request),
                   VORPAL_CPP_COMPILER_RESPONSE=str(response), VORPAL_CPP_COMPILER_READY=str(ready))
        with (root / f'provider-{name}.log').open('wb') as output:
            result = subprocess.run([sys.executable, *command['arguments']], cwd=root,
                                    env=env, stdout=output, stderr=subprocess.STDOUT)
        if result.returncode:
            for error in sorted(capture.glob('*.stderr')):
                print(error.name, error.read_text(errors='replace')[-4000:])
            raise RuntimeError(f'fresh {name} observation declined; inspect capture artifacts')
        packet = json.loads(response.read_bytes())
        (root / f'context-{name}.json').write_text(json.dumps({
            'nativeContextBefore': packet['nativeContextBefore'],
            'nativeContextAfter': packet['nativeContextAfter'],
            'observedContext': packet['observedContext'],
            'nativeTokenCount': len(packet['nativeBefore']),
            'observedTokenCount': len(packet['observedTokens']),
            'nativeDirectives': packet['nativeDirectives'],
            'specifierMacros': [s for s in packet['specifierMacros'] if s['name'].startswith('CATCH_ARC') or s['name'].startswith('CATCH_UNSAFE')],
            'annotationMacros': [s for s in packet['annotationMacros'] if s['name'].startswith('CATCH_ARC') or s['name'].startswith('CATCH_UNSAFE')],
        }))
        if packet['nativeBefore'] != packet['nativeAfter'] or packet['nativeBefore'] != packet['observedTokens']:
            for index, (native, observed) in enumerate(zip(packet['nativeBefore'], packet['observedTokens'])):
                if native != observed:
                    print('first native/observer difference:', index, repr(native), repr(observed))
                    break
            raise RuntimeError(f'native {name} token streams differ')
        aliases = [s for field in ['specifierMacros', 'annotationMacros']
                   for s in packet[field] if s['name'] in ['CATCH_ARC_STRONG', 'CATCH_UNSAFE_UNRETAINED']]
        if {s['start'] for s in aliases} != {201200, 203126}:
            raise RuntimeError(f'native {name} ownership sites differ: {aliases}')
        print(f'{name}: Apple build/runtime, LLVM syntax and three native streams agree; '
              f'{len(aliases)} physical ownership sites')
    manifest = root / 'cases.json'
    manifest.write_text(json.dumps(cases))
    (root / 'sdk-identity.json').write_text(json.dumps(identity))
    if 'GITHUB_ENV' in os.environ:
        with Path(os.environ['GITHUB_ENV']).open('a') as output:
            output.write(f'VORPAL_TEST_OBJC_FOUNDATION_CASES={manifest}\n')
    print(f'Foundation cases: {manifest}')


if __name__ == '__main__':
    main()
