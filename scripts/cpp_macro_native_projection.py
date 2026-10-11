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


def observed_literal_once_lines(original, rows, source, physical):
    """Only active, literal physical #pragma once lines can be administrative.

    _Pragma/__pragma and macro-spelled operands do not supply this evidence.
    SourceManager's immutable buffer identity and byte offset bind the callback
    to the unchanged original, independently of native line marker spellings.
    """
    result = set()
    for row in rows:
        if row.get('kind') != 'root_pragma' or row.get('hash_pragma') is not True:
            continue
        loc = row['location']
        offset = loc.get('offset')
        if (loc.get('nested') is not False or not loc.get('path')
                or physical(loc['path']) != physical(source)
                or loc.get('buffer_sha256', '').lower() != hashlib.sha256(original).hexdigest()
                or type(offset) is not int or not 0 <= offset < len(original)):
            raise ValueError('pragma physical source identity differs')
        start = original.rfind(b'\n', 0, offset) + 1
        end = original.find(b'\n', offset)
        if end < 0:
            end = len(original)
        line = original[start:end]
        if re.fullmatch(rb'[ \t]*#[ \t]*pragma[ \t]+once[ \t]*\r?', line):
            result.add(original.count(b'\n', 0, start) + 1)
    return result


def project_root(data, directive_offsets, source, physical, authored_external_header=False,
                 forced_inputs=(), translation_unit=None, literal_once_lines=()):
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

    def frame_target(index):
        # Native /FI precompiled-header inputs can emit blank physical lines
        # between the administrative pragma and #line transition. Permit only
        # bounded ASCII-whitespace lines, never tokens, directives or payload.
        next_index = index + 1
        limit = min(len(lines), next_index + 65)
        while next_index < limit and not lines[next_index].strip(b' \t\v\f\r\n'):
            next_index += 1
        return marker(next_index) if next_index < limit else None

    forced = {physical(path) for path in forced_inputs}
    native_root = physical(translation_unit) if translation_unit is not None else None
    current, stack, administrative = None, [], set()
    physical_line = 0
    omitted_once = set()
    first_frame = True
    for index, line in enumerate(lines):
        path = marker(index)
        if path is not None:
            current = path
            physical_line = int(re.match(rb'#(?:line)?\s+(\d+)', line.lstrip(b' \t\v\f\r'))[1]) - 1
            continue
        physical_line += 1
        if (directive(index) and current == physical(source)
                and line.strip(b' \t\v\f\r\n') == b'#pragma once'
                and physical_line in literal_once_lines):
            if physical_line in omitted_once:
                raise ValueError('literal once line was projected more than once')
            omitted_once.add(physical_line)
            administrative.add(index)
            continue
        if not directive(index) or authored_external_header:
            continue
        control = line.strip(b' \t\v\f\r\n')
        if control not in [b'#pragma external_header(push)', b'#pragma external_header(pop)']:
            continue
        target = frame_target(index)
        if target is None or current is None or target == current:
            raise ValueError('external-header frame is not a file transition')
        if control.endswith(b'(push)'):
            if target == physical(source):
                raise ValueError('external-header frame cannot enter the source root')
            # /FI's first administrative frame begins in the generated wrapper,
            # enters its header, returns to the wrapper, then restores the actual
            # TU. Admit that shape only with an explicit trusted forced input.
            forced_frame = first_frame and not stack and current in forced
            stack.append((current, target, forced_frame))
        else:
            if not stack:
                raise ValueError('external-header frame restoration differs')
            entering, included, forced_frame = stack.pop()
            ordinary = (entering, included) == (target, current)
            forced_return = (forced_frame and not stack and current == entering
                             and target == native_root and native_root is not None)
            if not ordinary and not forced_return:
                raise ValueError('external-header frame restoration differs')
        administrative.add(index)
        first_frame = False
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
