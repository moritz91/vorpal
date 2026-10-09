"""Token-boundary projection of MSVC /E output, never of the source document.

MSVC emits balanced external-header warning frames at actual #line transitions.
Only that exact administrative shape is omitted. Unknown pragmas, authored
external_header identifiers, malformed transitions and literal payload remain
conservative boundaries. Caller-provided offsets come from lexing the full output.
"""
import json
import hashlib
import re


def validated_hash_offsets(observation, data):
    if (not isinstance(observation, dict)
            or set(observation) != {'kind', 'version', 'bytes', 'sha256', 'offsets'}
            or observation['kind'] != 'native_hash_offsets'
            or type(observation['version']) is not int or observation['version'] != 1
            or type(observation['bytes']) is not int or observation['bytes'] != len(data)
            or not isinstance(observation['sha256'], str)
            or observation['sha256'].lower() != hashlib.sha256(data).hexdigest()):
        raise ValueError('native hash lexer buffer identity differs')
    offsets = observation['offsets']
    if (not isinstance(offsets, list) or len(offsets) > 1048576
            or any(type(offset) is not int or not 0 <= offset < len(data)
                   or data[offset] != ord('#') for offset in offsets)
            or offsets != sorted(set(offsets))):
        raise ValueError('native hash lexer offsets are invalid')
    return set(offsets)


def project_root(data, directive_offsets, source, physical, authored_external_header=False):
    parts = data.split(b'\n')
    lines = [part + b'\n' for part in parts[:-1]]
    if parts[-1]:
        lines.append(parts[-1])
    offsets, offset = [], 0
    for line in lines:
        offsets.append(offset)
        offset += len(line)

    def directive(index):
        if index >= len(lines):
            return False
        prefix = len(lines[index]) - len(lines[index].lstrip(b' \t\v\f\r'))
        return offsets[index] + prefix in directive_offsets

    def marker(index):
        if not directive(index):
            return None
        match = re.fullmatch(rb'#(?:line)?\s+\d+\s+(".*?")\s*', lines[index].strip(b' \t\v\f\r\n'))
        return physical(json.loads(match[1])) if match else None

    current, stack, administrative = None, [], set()
    for index, line in enumerate(lines):
        path = marker(index)
        if path is not None:
            current = path
            continue
        if not directive(index) or authored_external_header:
            continue
        control = line.strip(b' \t\v\f\r\n')
        if control not in [b'#pragma external_header(push)', b'#pragma external_header(pop)']:
            continue
        target = marker(index + 1)
        if target is None or current is None or target == current:
            raise ValueError('external-header frame is not a file transition')
        if control.endswith(b'(push)'):
            if target == physical(source):
                raise ValueError('external-header frame cannot enter the source root')
            stack.append((current, target))
        else:
            if not stack or stack.pop() != (target, current):
                raise ValueError('external-header frame restoration differs')
        administrative.add(index)
    if stack:
        raise ValueError('external-header frames are incomplete')

    active, root = False, []
    for index, line in enumerate(lines):
        path = marker(index)
        if path is not None:
            active = path == physical(source)
        elif active and index not in administrative:
            root.append(line)
    return b''.join(root)
