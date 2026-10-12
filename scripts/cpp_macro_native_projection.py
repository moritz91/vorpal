"""Token-boundary projection of MSVC /E output, never of the source document.

MSVC emits balanced external-header warning frames at actual #line transitions.
Only that exact administrative shape is omitted. Unknown pragmas, authored
external_header identifiers, malformed transitions and literal payload remain
conservative boundaries. Caller-provided offsets come from lexing the full output.
"""
import json
import hashlib
import re


class CaptureLexMemo:
    """Two immutable lexer results, scoped to ONE native before/after capture.

    Full byte equality is required, including markers, literals and line endings.
    This stores no physical path resolution, header reads, compiler context,
    observation packet or proof. Those are independently repeated by the caller.
    A new provider invocation always starts empty. Exceptions never populate it.
    """
    def __init__(self):
        self._hashes = None
        self._tokens = None

    def hash_offsets(self, data, lex):
        if self._hashes is None or self._hashes[0] != data:
            result = frozenset(lex())
            self._hashes = (data, result)
        return self._hashes[1]

    def root_tokens(self, data, lex):
        if self._tokens is None or self._tokens[0] != data:
            spellings, directives = lex()
            self._tokens = (data, tuple(spellings), tuple(directives))
        return self._tokens[1:]


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
    # Visit only token-proven physical directive lines. Byte slices keep all
    # other lines, including raw-literal payload and CRLF, exactly as captured.
    # Decode marker syntax once; physical() still runs in each projection pass
    # and each administrative lookahead. No resolved path survives this call.
    events, markers = [], {}
    for offset in sorted(directive_offsets):
        if not 0 <= offset < len(data):
            continue
        start = data.rfind(b'\n', 0, offset) + 1
        if data[start:offset].strip(b' \t\v\f\r'):
            continue
        stop = data.find(b'\n', offset)
        stop = len(data) if stop < 0 else stop + 1
        line = data[start:stop]
        match = re.fullmatch(rb'#(?:line)?\s+(\d+)\s+(".*?")\s*', line.strip(b' \t\v\f\r\n'))
        if match:
            markers[start] = (int(match[1]), json.loads(match[2]))
        events.append((start, stop, line))

    def marker(start):
        value = markers.get(start)
        return physical(value[1]) if value is not None else None

    def frame_target(stop):
        # The original frame admits at most 64 intervening blank physical
        # lines. Tokens, unknown directives and literal payload stop lookahead.
        for _ in range(65):
            if stop >= len(data):
                return None
            end = data.find(b'\n', stop)
            end = len(data) if end < 0 else end + 1
            if data[stop:end].strip(b' \t\v\f\r\n'):
                return marker(stop)
            stop = end
        return None

    forced = {physical(path) for path in forced_inputs}
    native_root = physical(translation_unit) if translation_unit is not None else None
    current, stack, administrative = None, [], set()
    physical_line, cursor = 0, 0
    omitted_once = set()
    first_frame = True
    for start, stop, line in events:
        physical_line += data.count(b'\n', cursor, start) + 1
        cursor = stop
        path = marker(start)
        if path is not None:
            current = path
            physical_line = markers[start][0] - 1
            continue
        if (current == physical(source)
                and line.strip(b' \t\v\f\r\n') == b'#pragma once'
                and physical_line in literal_once_lines):
            if physical_line in omitted_once:
                raise ValueError('literal once line was projected more than once')
            omitted_once.add(physical_line)
            administrative.add(start)
            continue
        if authored_external_header:
            continue
        control = line.strip(b' \t\v\f\r\n')
        if control not in [b'#pragma external_header(push)', b'#pragma external_header(pop)']:
            continue
        target = frame_target(stop)
        if target is None or current is None or target == current:
            raise ValueError('external-header frame is not a file transition')
        if control.endswith(b'(push)'):
            if target == physical(source):
                raise ValueError('external-header frame cannot enter the source root')
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
        administrative.add(start)
        first_frame = False
    if stack:
        raise ValueError('external-header frames are incomplete')

    active, root, cursor = False, [], 0
    for start, stop, _ in events:
        if active:
            root.append(data[cursor:start])
        path = marker(start)
        if path is not None:
            active = path == physical(source)
        elif active and start not in administrative:
            root.append(data[start:stop])
        cursor = stop
    if active:
        root.append(data[cursor:])
    return b''.join(root)
