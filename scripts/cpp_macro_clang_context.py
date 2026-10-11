"""Bounded preprocessing plans and physical line markers for native Clang.

This is a separate opt-in dialect. It never interprets GNU/Clang markers as MSVC
warning frames, and never resolves virtual predefine buffers as physical files.
"""
import hashlib
import json
from pathlib import Path
import re


def clang_plan(arguments, translation_unit, physical):
    if (len(arguments) < 2 or arguments[-1] != '-fsyntax-only'
            or physical(arguments[-2]) != physical(translation_unit)):
        raise ValueError('a syntax-only plan with the actual final input is required')
    arguments = list(arguments[:-2])
    paired = {'-x', '-target', '-isysroot', '-resource-dir', '-I', '-isystem',
              '-iquote', '-F', '-iframework', '-include', '-D', '-U'}
    flags = {'-fobjc-arc', '-fno-objc-arc', '-fblocks', '-fno-modules',
             '-fno-implicit-modules', '-fno-implicit-module-maps', '-pthread'}
    joined = ('-std=', '--target=', '-stdlib=', '-fobjc-runtime=',
              '-mmacosx-version-min=', '-resource-dir=')
    forced, sdk, languages = [], [], []
    index = 0
    while index < len(arguments):
        option = arguments[index]
        if option in paired:
            index += 1
            if index >= len(arguments) or not arguments[index] or arguments[index].startswith('@'):
                raise ValueError('missing explicit plan operand')
            value = arguments[index]
            if option == '-include':
                forced.append(value)
            if option == '-isysroot':
                sdk.append(value)
            if option == '-x':
                languages.append(value)
        elif option in flags or option.startswith(joined):
            pass
        elif (option.startswith(('-I', '-F', '-D', '-U')) and len(option) > 2
              or re.fullmatch(r'-W(?:no-)?[A-Za-z0-9-]+', option)):
            pass
        else:
            # In particular: no response/config files, -Xclang, plugins, modules,
            # dependency/output options, alternate drivers or arbitrary inputs.
            raise ValueError('unsupported native Clang preprocessing option')
        index += 1
    if languages != ['objective-c++'] or len(sdk) != 1 or not Path(sdk[0]).is_absolute():
        raise ValueError('an explicit Objective-C++ language and SDK are required')
    return arguments + [str(translation_unit), '-E'], forced, Path(sdk[0])


def sdk_identity(sdk):
    sdk = Path(sdk).resolve(strict=True)
    if not (sdk / 'System/Library/Frameworks/Foundation.framework/Headers/Foundation.h').is_file():
        raise ValueError('the selected SDK does not contain Apple Foundation')
    settings = sdk / 'SDKSettings.json'
    data = settings.read_bytes()
    value = json.loads(data)
    if not re.fullmatch(r'macosx(?:\d+(?:\.\d+)*)?', value.get('CanonicalName', '')):
        raise ValueError('a genuine macOS SDK is required')
    return {'path': str(sdk), 'settings': hashlib.sha256(data).hexdigest()}


def project_clang_root(data, hash_offsets, source, input_path):
    """Keep original root output; validate every physical/virtual marker.

    Hash boundaries come from independent full-stream lexing. Marker flags are
    strictly Clang's enter/return/system/extern-C flags. Unknown directives stay
    in the root stream for the parent to accept or decline explicitly.
    """
    current, active, stack, output = None, False, [], []
    offset = 0
    lines = data.split(b'\n')
    for index, part in enumerate(lines):
        line = part + (b'\n' if index < len(lines) - 1 else b'')
        indent = len(line) - len(line.lstrip(b' \t\v\f\r'))
        if offset + indent in hash_offsets:
            marker = re.fullmatch(rb'[ \t\v\f]*#[ \t]+(\d+)[ \t]+("(?:[^"\\]|\\.)*")((?:[ \t]+[1-4])*)[ \t]*\r?\n?', line)
            if marker:
                spelling = json.loads(marker[2])
                flags = [int(v) for v in marker[3].split()]
                if flags != sorted(set(flags)) or 1 in flags and 2 in flags:
                    raise ValueError('malformed native Clang marker flags')
                virtual = spelling in ('<built-in>', '<command line>')
                if spelling.startswith('<') and not virtual:
                    raise ValueError('unknown virtual native input')
                target = None if virtual else input_path(spelling)
                if 1 in flags:
                    stack.append(current)
                elif 2 in flags:
                    if not stack or stack.pop() != target:
                        raise ValueError('native Clang include return differs')
                current = target
                active = target == input_path(str(source))
                offset += len(line)
                continue
            if re.match(rb'[ \t\v\f]*#[ \t]+\d', line):
                raise ValueError('malformed native Clang line marker')
        if active:
            output.append(line)
        offset += len(line)
    if stack:
        raise ValueError('native Clang include markers are incomplete')
    return b''.join(output)
