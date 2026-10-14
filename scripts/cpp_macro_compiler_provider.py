#!/usr/bin/env python3
"""Fresh MSVC or explicit macOS Clang/LLVM19 preprocessing provider.

Runs native PP before AND after the physical observer. No retained observations,
line-based source rewriting, final macro dumps or dependency-cache assumptions.
The launcher owns this process tree and its timeout. LLVM/python dependencies are
explicit local paths; this script does not download tools or alter the checkout.
"""
import argparse
from bisect import bisect_right
import hashlib
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import sys
import time
from cpp_macro_native_projection import CaptureLexMemo, project_root, validated_hash_offsets, observed_literal_once_lines
from cpp_macro_clang_context import clang_plan, project_clang_root, sdk_identity, clang_diagnostic_line


def digest(data):
    return hashlib.sha256(data).hexdigest()


_WINDOWS_COMPONENT_SPECIAL = re.compile(r'[<>:"/|?*\x00-\x1f]')


def plain_windows_path(path):
    """An absolute drive path whose components have no DOS/verbatim ambiguity.

    UNC/device namespaces, drive-relative paths, dot components, stream names,
    DOS devices and trailing dots/spaces must keep pathlib's ordinary resolver.
    The same check applies to the result: a reparse point can target one of those
    names even when its input spelling is plain.
    """
    if (not 3 < len(path) < 240 or path[1:3] != ':\\'
            or not ('A' <= path[0] <= 'Z' or 'a' <= path[0] <= 'z')):
        return False
    for component in path[3:].split('\\'):
        if (not component or component in ('.', '..') or component[0] == ' '
                or component[-1] in ' .'
                or _WINDOWS_COMPONENT_SPECIAL.search(component)):
            return False
        stem = component.split('.', 1)[0].rstrip(' ').upper()
        if (stem in ('CON', 'PRN', 'AUX', 'NUL', 'CONIN$', 'CONOUT$')
                or (len(stem) == 4 and stem[:3] in ('COM', 'LPT')
                    and stem[3] in '123456789¹²³')):
            return False
    return True


def resolved_path(path):
    file = Path(path)
    spelling = str(file)
    if os.name == 'nt' and plain_windows_path(spelling):
        # CPython's ordinary Windows realpath resolves once, then resolves again
        # to check whether stripping \\?\ changes DOS interpretation. For this
        # narrow domain an explicitly verbatim lookup needs only one fresh OS
        # resolution. No resolved path is cached, even within one capture.
        # Missing/inaccessible inputs and ambiguous reparse targets fall back.
        try:
            resolved = str(Path('\\\\?\\' + spelling).resolve(strict=True))
            plain = resolved.removeprefix('\\\\?\\')
            if plain_windows_path(plain):
                result = Path(plain)
                # Keep the ordinary resolver's post-resolution filesystem check.
                # Any uncertainty returns to that resolver's full error behavior.
                result.stat()
                return result
        except (OSError, RuntimeError):
            pass
    return file.resolve()


def physical(path):
    return os.path.normcase(str(resolved_path(path))).removeprefix("\\\\?\\")


def load(path):
    return json.loads(Path(path).read_bytes())


def definition_buffer(header, observed_sha256, end_sha256, definitions, buffers):
    """Fresh read on every invocation; reuse only a matching immutable byte identity.

    The tables belong to one capture. No file read, resolution or final input
    recheck is memoized. A changed read or observer identity aborts the capture.
    """
    data = header.read_bytes()
    key = os.path.normcase(str(header))
    previous = buffers.get(key)
    if previous is None:
        identity = digest(data)
        number = len(definitions)
    else:
        number, original, identity = previous
        assert data == original
    assert observed_sha256.lower() == end_sha256.lower() == identity
    if previous is None:
        definitions.append({"path": str(header), "source": data.decode("utf-8")})
        buffers[key] = (number, data, identity)
    return number


def root_token_inventory(tokens, lines, comment_kind, clang_mode=False):
    """Read each foreign token spelling/location once; preserve directive roles."""
    values, directives = [], set()
    for token in tokens:
        if token.kind == comment_kind:
            continue
        spelling = token.spelling
        location = token.location
        line = location.line
        values.append((spelling, line))
        if spelling == "#" and not lines[line-1][:location.column-1].strip():
            directives.add(line)
    if clang_mode:
        directives = {line for line in directives
                      if not clang_diagnostic_line(lines[line-1].strip())}
    return ([spelling for spelling, line in values if line not in directives],
            [lines[line-1].strip() for line in sorted(directives)])


class ProofDeclined(Exception):
    """A completed capture cannot establish the bounded source projection."""


class ToolUnavailable(Exception):
    """The configured native toolchain cannot be used."""


def provider_exit(action):
    """Sanitized exit contract shared by direct and profiling launchers.

    65 is an explicit proof decline; 69 means required tools are unavailable.
    Every other failure is operational. Never expose exception/command/stderr
    payloads, which can contain source, arguments or environment secrets.
    """
    try:
        action()
        return 0
    except ProofDeclined:
        print("native C++ projection proof declined", file=sys.stderr)
        return 65
    except (ToolUnavailable, OSError, ImportError):
        print("native C++ toolchain unavailable", file=sys.stderr)
        return 69
    except Exception:
        print("native C++ capture failed", file=sys.stderr)
        return 1


def proven_projection(projector, *args, **kwargs):
    try:
        return projector(*args, **kwargs)
    except ValueError:
        raise ProofDeclined() from None


def run_capture_command(command, output, error, cwd):
    with output.open("wb") as out, error.open("wb") as err:
        result = subprocess.run(command, cwd=cwd, stdout=out, stderr=err, check=False)
    # A failed native invocation is not evidence that a raw-parser graph is a
    # valid replacement (e.g. missing SDK/environment). Preserve the old index.
    if result.returncode or output.stat().st_size > 128 * 1024 * 1024:
        raise RuntimeError("native capture command failed or exceeded limit")


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--plan", required=True)
    parser.add_argument("--observer", required=True)
    parser.add_argument("--resource-dir", required=True)
    parser.add_argument("--libclang", required=True)
    parser.add_argument("--python-bindings", required=True)
    parser.add_argument("--compiler", default="cl.exe")
    parser.add_argument("--driver-mode", choices=["msvc", "clang"], default="msvc")
    parser.add_argument("--msvc-version", default="19.44")
    parser.add_argument("--native-hash-lexer", action="store_true",
                        help="use the matching observer's raw lexer for full-stream hash offsets")
    args = parser.parse_args()
    ready = Path(os.environ["VORPAL_CPP_COMPILER_READY"])
    deadline = time.monotonic() + 10
    while not ready.exists():
        if time.monotonic() >= deadline:
            raise RuntimeError("compiler launch was not authorized")
        time.sleep(0.01)
    request_path = Path(os.environ["VORPAL_CPP_COMPILER_REQUEST"])
    request = load(request_path)
    assert request["version"] == 1
    source = Path(request["path"])
    original = source.read_bytes()
    assert original.decode("utf-8") == request["source"]
    original_digest = digest(original)
    plan_bytes = Path(args.plan).read_bytes()
    entries = json.loads(plan_bytes)
    selected = [p for p in entries if physical(p["source"]) == physical(source)]
    if len(selected) != 1:
        raise RuntimeError("a unique explicit native command is required")
    entry = selected[0]
    translation_unit = Path(entry.get("translationUnit", entry["source"]))
    included_source = physical(translation_unit) != physical(source)
    translation_unit_bytes = translation_unit.read_bytes()
    # Plans are supplied by the trusted launcher and contain only preprocessing
    # arguments plus the original source. Never accept an arbitrary compile line.
    arguments = entry["args"]
    if args.driver_mode == "clang":
        if sys.platform != "darwin":
            raise RuntimeError("the Foundation provider requires a native macOS host")
        native_args, forced_inputs, sdk = clang_plan(arguments, translation_unit, physical)
        native_args.append("-resource-dir=" + args.resource_dir)
        sdk_before = sdk_identity(sdk)
    else:
        assert arguments[-1].lower() == "/zs"
    assert len(arguments) >= 2 and physical(arguments[-2]) == physical(translation_unit)
    for argument in arguments if args.driver_mode == "msvc" else []:
        option = argument.lstrip("/-")
        lowered = option.lower()
        assert not argument.startswith("@"), "response files are not preprocessing plans"
        if argument.startswith(("/", "-")):
            assert lowered not in ["p", "e", "ep", "c", "ld", "ldd", "link"]
            if not option.startswith("FI"):  # uppercase FI is a forced input header
                assert not lowered.startswith(("fo", "fd", "fe", "fp", "fa", "fm", "fr", "fi", "yc", "sourcedependencies", "analyze:log"))

    if args.driver_mode == "msvc":
        native_args = [a for a in arguments if a.lower() != "/zs"] + ["/E"]
        forced_inputs = [a[3:] for a in arguments if a.startswith(("/FI", "-FI"))]
    assert all(forced_inputs), "forced input paths must be explicit joined arguments"
    compiler = shutil.which(args.compiler)
    if not compiler:
        raise ToolUnavailable()
    compiler = Path(compiler).resolve()
    sys.path.insert(0, str(Path(args.python_bindings).resolve()))
    from clang import cindex
    cindex.Config.set_library_file(str(Path(args.libclang).resolve()))
    scratch = request_path.parent

    def context():
        # Only digests leave this process. Environment values and flags can carry
        # secrets; they are never emitted as diagnostics or packet fields.
        value = {"compiler": digest(compiler.read_bytes()),
                 "provider": digest(Path(__file__).read_bytes()),
                 "projection": digest(Path(__file__).with_name('cpp_macro_native_projection.py').read_bytes()),
                 "clangContext": digest(Path(__file__).with_name('cpp_macro_clang_context.py').read_bytes()),
                 "observer": digest(Path(args.observer).read_bytes()),
                 "libclang": digest(Path(args.libclang).read_bytes()),
                 "plan": digest(Path(args.plan).read_bytes()),
                 "arguments": native_args, "cwd": entry["cwd"],
                 "physicalSource": physical(source),
                 "environment": dict(os.environ), "msvcVersion": args.msvc_version,
                 "resourceDir": str(Path(args.resource_dir).resolve()),
                 "driverMode": args.driver_mode}
        if args.driver_mode == "clang":
            value["sdk"] = sdk_identity(sdk)
        # Per-capture paths are handshake metadata, not toolchain inputs.
        for key in ["VORPAL_CPP_COMPILER_REQUEST", "VORPAL_CPP_COMPILER_RESPONSE", "VORPAL_CPP_COMPILER_READY"]:
            value["environment"].pop(key, None)
        return digest(json.dumps(value, sort_keys=True).encode())

    def run(command, output, error):
        run_capture_command(command, output, error, entry["cwd"])

    # Pure lexical work can repeat within the bracketed capture. This memo never
    # crosses a request and cannot certify include dependencies or product replay.
    lex_memo = CaptureLexMemo()

    def native_tokens(file, label):
        data = file.read_bytes()

        def lex_hashes():
            # A fake #line inside a raw string is payload. Lex the complete native
            # output BEFORE interpreting markers, so physical boundaries govern.
            if args.native_hash_lexer:
                hashes = scratch / (label + "-hashes.json")
                run([args.observer, "--native-hash-offsets", str(file)], hashes,
                    scratch / (label + "-hashes.stderr"))
                return validated_hash_offsets(load(hashes), data)
            index = cindex.Index.create()
            virtual = str(scratch / (label + "-full.cc"))
            tu = index.parse(virtual, args=["-std=c++20", "-fms-extensions"],
                             unsaved_files=[(virtual, data.decode("utf-8"))],
                             options=cindex.TranslationUnit.PARSE_SKIP_FUNCTION_BODIES)
            tokens = tu.get_tokens(extent=tu.get_extent(virtual, (0, len(data))))
            return {t.location.offset for t in tokens
                    if t.kind == cindex.TokenKind.PUNCTUATION and t.spelling == "#"}

        directive_offsets = lex_memo.hash_offsets(data, lex_hashes)
        inputs, resolved_paths = set(), {}

        def input_path(path):
            spelling = str(path)
            if spelling not in resolved_paths:
                resolved_paths[spelling] = physical(path)
            resolved = resolved_paths[spelling]
            inputs.add(resolved)
            return resolved

        if args.driver_mode == "clang":
            projected = proven_projection(project_clang_root, data, directive_offsets, source, input_path)
        else:
            projected = proven_projection(project_root, data, directive_offsets, source, input_path,
                                     forced_inputs=forced_inputs, translation_unit=translation_unit,
                                     literal_once_lines=literal_once_lines)
        # A matching frame can also be authored in a header, or emitted through
        # a pragma macro. Inspect every physical file named by an actual marker,
        # including strings/comments conservatively; no name allowlist is proof.
        if len(inputs) > 2048:
            raise RuntimeError("native frame input count exceeds limit")
        total, authored, input_digests = 0, False, {}
        for path in inputs:
            with Path(path).open('rb') as stream:
                content = stream.read(4 * 1024 * 1024 + 1)
            total += len(content)
            if len(content) > 4 * 1024 * 1024 or total > 32 * 1024 * 1024:
                raise RuntimeError("native frame input bytes exceed limit")
            authored |= b'external_header' in content
            input_digests[path] = digest(content)
        # This memo exists only inside one native stream projection. Recheck all
        # spellings before returning; no canonical path or header bytes survive
        # into the next stream/capture or authorize product replay.
        if any(physical(path) != resolved for path, resolved in resolved_paths.items()):
            raise RuntimeError("native input redirect changed during projection")
        if authored and args.driver_mode == "msvc":
            projected = proven_projection(project_root, data, directive_offsets, source, physical, True,
                                     forced_inputs=forced_inputs, translation_unit=translation_unit,
                                     literal_once_lines=literal_once_lines)
        def lex_root():
            text = projected.decode("utf-8")
            index = cindex.Index.create()
            virtual = str(scratch / (label + "-root.cc"))
            tu = index.parse(virtual, args=["-std=c++20", "-fms-extensions"], unsaved_files=[(virtual, text)])
            return root_token_inventory(
                tu.get_tokens(extent=tu.get_extent(virtual, (0, len(projected)))),
                text.split('\n'), cindex.TokenKind.COMMENT, args.driver_mode == "clang")

        spellings, directives = lex_memo.root_tokens(projected, lex_root)
        return spellings, directives, input_digests

    before_context = context()
    before_file = scratch / "native-before.i"
    run([str(compiler), *native_args], before_file, scratch / "native-before.stderr")
    if args.driver_mode == "clang":
        before, directives, before_inputs = native_tokens(before_file, "before")
    observer_file = scratch / "observer.jsonl"
    selection = ["--physical-source", str(source)] if included_source else []
    driver_args = (["--native-driver-mode", "g++"] if args.driver_mode == "clang"
                   else ["-fms-compatibility-version=" + args.msvc_version])
    run([args.observer, *selection, *driver_args, *arguments,
         "-resource-dir=" + args.resource_dir], observer_file, scratch / "observer.stderr")
    rows = [json.loads(line) for line in observer_file.read_bytes().splitlines()]
    literal_once_lines = observed_literal_once_lines(original, rows, source, physical)
    if included_source:
        visits = [r for r in rows if r["kind"] == "physical_root_visit"]
        assert len(visits) == 1 and visits[0]["visit"] == 1
        assert physical(visits[0]["path"]) == physical(source)
        assert all(physical(r["path"]) == physical(source)
                   for r in rows if r["kind"] == "root_token")
    after_file = scratch / "native-after.i"
    run([str(compiler), *native_args], after_file, scratch / "native-after.stderr")
    after_context = context()
    if args.driver_mode == "msvc":
        before, directives, before_inputs = native_tokens(before_file, "before")
    after, after_directives, after_inputs = native_tokens(after_file, "after")
    if directives != after_directives:
        raise RuntimeError("native directive inventory changed during capture")
    if args.driver_mode == "clang":
        assert sdk_before == sdk_identity(sdk) and before_inputs == after_inputs
        assert all(digest(Path(p).read_bytes()) == d for p, d in after_inputs.items())
        # SDK identity, search plan and freshly read physical inputs identify this
        # capture. They never authorize replay without another native capture.
        before_context = digest((before_context + json.dumps(before_inputs, sort_keys=True)).encode())
        after_context = digest((after_context + json.dumps(after_inputs, sort_keys=True)).encode())
    pragma_offsets = []
    for row in rows:
        if row.get("kind") != "root_pragma" or row.get("hash_pragma") is not True:
            continue
        loc = row["location"]
        if loc.get("nested") is not False or physical(loc.get("path", "")) != physical(source):
            continue
        assert loc["buffer_sha256"].lower() == original_digest
        offset = loc["offset"]
        assert type(offset) is int and 0 <= offset < len(original)
        start = original.rfind(b"\n", 0, offset) + 1
        if original.count(b"\n", 0, start) + 1 in literal_once_lines:
            continue
        end = original.find(b"\n", start)
        line = original[start:end if end >= 0 else len(original)]
        if not re.match(rb"[ \t]*#[ \t]*pragma[ \t]+(?:warning|optimize)\b", line):
            # Other callbacks cannot prove this bounded native inventory. If an
            # unknown directive survives native /E, the parent still declines.
            continue
        # The parent independently parses and validates the complete directive.
        pragma_offsets.append(start + len(original[start:offset].split(b"#", 1)[0]))
    records = [r for r in rows if r["kind"] == "macro"]
    definitions, buffers, expansions, callees, literal_arguments, specifier_macros, control_macros, annotation_macros = [], {}, [], {}, [], [], [], []
    for record in records:
        begin, end = record["begin"], record.get("end_exclusive", {})
        if begin["nested"] or not begin.get("path") or not end.get("path"):
            continue
        if physical(begin["path"]) != physical(source) or physical(end["path"]) != physical(source):
            continue
        assert begin["buffer_sha256"].lower() == end["buffer_sha256"].lower() == original_digest
        start, stop = begin["offset"], end["offset"]
        assert 0 <= start < stop <= len(original)
        assert original[start:start+len(record["name"])].decode() == record["name"]
        callees[start] = {"name": record["name"], "start": start}
        definition, definition_end = record.get("definition", {}), record.get("definition_end_exclusive", {})
        anchor = None
        literal = (not record.get("function_like") and len(record.get("replacement_tokens", [])) == 1
                   and record["replacement_tokens"][0][:1].isdigit())
        specifier = (not record.get("function_like") and record.get("replacement_tokens") in
                     [["inline"], ["__forceinline"], ["__inline"], ["__strong"], ["__weak"],
                      ["__unsafe_unretained"], ["__autoreleasing"]])
        control = (not record.get("function_like") and record.get("replacement_tokens") in
                   [["try"], ["catch", "(", "...", ")"]])
        annotation = (not record.get("function_like") and record.get("replacement_tokens") in
                      [[], ["__pragma", "(", "warning", "(", "push", ")", ")"],
                       ["__pragma", "(", "warning", "(", "pop", ")", ")"]])
        if (record.get("function_like") or literal or specifier or control or annotation) and definition.get("path") and definition_end.get("path"):
            header = resolved_path(definition["path"])
            if header.is_file() and resolved_path(definition_end["path"]) == header:
                number = definition_buffer(header, definition["buffer_sha256"],
                                           definition_end["buffer_sha256"], definitions, buffers)
                anchor = {"buffer": number, "nameOffset": definition["offset"],
                          "end": definition_end["offset"], "parameters": record["parameters"]}
        if literal and anchor is not None:
            literal_arguments.append({"name": record["name"], "start": start, "end": stop, "definition": anchor})
        if specifier and anchor is not None:
            specifier_macros.append({"name": record["name"], "start": start, "end": stop, "definition": anchor})
        if control and anchor is not None:
            control_macros.append({"name": record["name"], "start": start, "end": stop, "definition": anchor})
        if annotation and anchor is not None:
            annotation_macros.append({"name": record["name"], "start": start, "end": stop, "definition": anchor})
        expansions.append({"name": record["name"], "start": start, "end": stop,
                           "definition": anchor if record.get("function_like") else None})
    outer = []
    for record in sorted(expansions, key=lambda r: (r["start"], -r["end"])):
        if outer and record["start"] < outer[-1]["end"]:
            assert record["end"] <= outer[-1]["end"]
        else:
            outer.append(record)
    outer_starts = [r["start"] for r in outer]
    def in_argument(record):
        index = bisect_right(outer_starts, record["start"]) - 1
        return (index >= 0 and outer[index]["definition"] is not None
                and outer[index]["start"] < record["start"]
                and record["end"] <= outer[index]["end"])
    literal_arguments = [r for r in literal_arguments if in_argument(r)]
    outer_sites = {(r["name"], r["start"], r["end"]) for r in outer if r["definition"] is None}
    specifier_macros = [r for r in specifier_macros if (r["name"], r["start"], r["end"]) in outer_sites]
    control_macros = [r for r in control_macros if (r["name"], r["start"], r["end"]) in outer_sites]
    annotation_macros = [r for r in annotation_macros if (r["name"], r["start"], r["end"]) in outer_sites]
    assert source.read_bytes() == original and Path(args.plan).read_bytes() == plan_bytes
    assert translation_unit.read_bytes() == translation_unit_bytes
    assert all(Path(b["path"]).read_bytes() == b["source"].encode() for b in definitions)
    packet = {"version": 1, "requestId": request["requestId"], "path": request["path"], "source": request["source"],
              "complete": any(r["kind"] == "complete" and r["success"] for r in rows),
              "volatileInputs": any(r["kind"] == "volatile_macro" for r in rows),
              "nativeContextBefore": before_context, "nativeContextAfter": after_context,
              "observedContext": digest(json.dumps([r for r in rows if r["kind"] == "compiler_context"], sort_keys=True).encode()),
              "nativeBefore": before, "nativeAfter": after,
              "observedTokens": [r["spelling"] for r in rows if r["kind"] == "root_token"],
              "observedTokenSites": [{"offset": r["offset"], "fromMacro": r["from_macro"]}
                                     for r in rows if r["kind"] == "root_token"],
              "nativeDirectives": directives,
              "pragmaOffsets": pragma_offsets,
              "definitions": definitions, "expansions": outer, "literalArguments": literal_arguments,
              "specifierMacros": specifier_macros,
              "controlMacros": control_macros,
              "annotationMacros": annotation_macros,
              "expandedNames": sorted({r["name"] for r in records}), "calleeSites": list(callees.values())}
    encoded = json.dumps(packet, ensure_ascii=False).encode()
    if len(encoded) > 32 * 1024 * 1024:
        raise RuntimeError("observation packet exceeds limit")
    Path(os.environ["VORPAL_CPP_COMPILER_RESPONSE"]).write_bytes(encoded)


if __name__ == "__main__":
    raise SystemExit(provider_exit(main))
