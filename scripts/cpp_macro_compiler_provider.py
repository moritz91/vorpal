#!/usr/bin/env python3
"""Fresh MSVC/LLVM19 provider. Requires a trusted preprocess-only command plan.

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
from cpp_macro_native_projection import project_root, validated_hash_offsets, observed_literal_once_lines


def digest(data):
    return hashlib.sha256(data).hexdigest()


def physical(path):
    return os.path.normcase(str(Path(path).resolve())).removeprefix("\\\\?\\")


def load(path):
    return json.loads(Path(path).read_bytes())


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--plan", required=True)
    parser.add_argument("--observer", required=True)
    parser.add_argument("--resource-dir", required=True)
    parser.add_argument("--libclang", required=True)
    parser.add_argument("--python-bindings", required=True)
    parser.add_argument("--compiler", default="cl.exe")
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
    assert arguments[-1].lower() == "/zs"
    assert len(arguments) >= 2 and physical(arguments[-2]) == physical(translation_unit)
    for argument in arguments:
        option = argument.lstrip("/-")
        lowered = option.lower()
        assert not argument.startswith("@"), "response files are not preprocessing plans"
        if argument.startswith(("/", "-")):
            assert lowered not in ["p", "e", "ep", "c", "ld", "ldd", "link"]
            if not option.startswith("FI"):  # uppercase FI is a forced input header
                assert not lowered.startswith(("fo", "fd", "fe", "fp", "fa", "fm", "fr", "fi", "yc", "sourcedependencies", "analyze:log"))

    native_args = [a for a in arguments if a.lower() != "/zs"] + ["/E"]
    forced_inputs = [a[3:] for a in arguments if a.startswith(("/FI", "-FI"))]
    assert all(forced_inputs), "forced input paths must be explicit joined arguments"
    compiler = shutil.which(args.compiler)
    if not compiler:
        raise RuntimeError("native compiler unavailable; load the VS environment")
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
                 "observer": digest(Path(args.observer).read_bytes()),
                 "libclang": digest(Path(args.libclang).read_bytes()),
                 "plan": digest(Path(args.plan).read_bytes()),
                 "arguments": native_args, "cwd": entry["cwd"],
                 "physicalSource": physical(source),
                 "environment": dict(os.environ), "msvcVersion": args.msvc_version,
                 "resourceDir": str(Path(args.resource_dir).resolve())}
        # Per-capture paths are handshake metadata, not toolchain inputs.
        for key in ["VORPAL_CPP_COMPILER_REQUEST", "VORPAL_CPP_COMPILER_RESPONSE", "VORPAL_CPP_COMPILER_READY"]:
            value["environment"].pop(key, None)
        return digest(json.dumps(value, sort_keys=True).encode())

    def run(command, output, error):
        with output.open("wb") as out, error.open("wb") as err:
            result = subprocess.run(command, cwd=entry["cwd"], stdout=out, stderr=err, check=False)
        if result.returncode or output.stat().st_size > 128 * 1024 * 1024:
            raise RuntimeError("preprocess-only compiler run failed or exceeded limit")

    def native_tokens(file, label):
        data = file.read_bytes()
        index = cindex.Index.create()
        # A fake #line inside a raw string is payload. Lex the complete native
        # output BEFORE interpreting markers, so physical token boundaries govern.
        if args.native_hash_lexer:
            hashes = scratch / (label + "-hashes.json")
            run([args.observer, "--native-hash-offsets", str(file)], hashes,
                scratch / (label + "-hashes.stderr"))
            directive_offsets = validated_hash_offsets(load(hashes), data)
        else:
            virtual = str(scratch / (label + "-full.cc"))
            tu = index.parse(virtual, args=["-std=c++20", "-fms-extensions"],
                             unsaved_files=[(virtual, data.decode("utf-8"))],
                             options=cindex.TranslationUnit.PARSE_SKIP_FUNCTION_BODIES)
            tokens = tu.get_tokens(extent=tu.get_extent(virtual, (0, len(data))))
            directive_offsets = {t.location.offset for t in tokens
                                 if t.kind == cindex.TokenKind.PUNCTUATION and t.spelling == "#"}
        inputs, resolved_paths = set(), {}

        def input_path(path):
            spelling = str(path)
            if spelling not in resolved_paths:
                resolved_paths[spelling] = physical(path)
            resolved = resolved_paths[spelling]
            inputs.add(resolved)
            return resolved

        projected = project_root(data, directive_offsets, source, input_path,
                                 forced_inputs=forced_inputs, translation_unit=translation_unit,
                                 literal_once_lines=literal_once_lines)
        # A matching frame can also be authored in a header, or emitted through
        # a pragma macro. Inspect every physical file named by an actual marker,
        # including strings/comments conservatively; no name allowlist is proof.
        if len(inputs) > 2048:
            raise RuntimeError("native frame input count exceeds limit")
        total, authored = 0, False
        for path in inputs:
            with Path(path).open('rb') as stream:
                content = stream.read(4 * 1024 * 1024 + 1)
            total += len(content)
            if len(content) > 4 * 1024 * 1024 or total > 32 * 1024 * 1024:
                raise RuntimeError("native frame input bytes exceed limit")
            authored |= b'external_header' in content
        # This memo exists only inside one native stream projection. Recheck all
        # spellings before returning; no canonical path or header bytes survive
        # into the next stream/capture or authorize product replay.
        if any(physical(path) != resolved for path, resolved in resolved_paths.items()):
            raise RuntimeError("native input redirect changed during projection")
        if authored:
            projected = project_root(data, directive_offsets, source, physical, True,
                                     forced_inputs=forced_inputs, translation_unit=translation_unit,
                                     literal_once_lines=literal_once_lines)
        text = projected.decode("utf-8")
        virtual = str(scratch / (label + "-root.cc"))
        tu = index.parse(virtual, args=["-std=c++20", "-fms-extensions"], unsaved_files=[(virtual, text)])
        tokens = [t for t in tu.get_tokens(extent=tu.get_extent(virtual, (0, len(text.encode()))))
                  if t.kind != cindex.TokenKind.COMMENT]
        lines = text.split('\n')
        directives = {t.location.line for t in tokens if t.spelling == "#"
                      and not lines[t.location.line-1][:t.location.column-1].strip()}
        return ([t.spelling for t in tokens if t.location.line not in directives],
                [lines[line-1].strip() for line in sorted(directives)])

    before_context = context()
    before_file = scratch / "native-before.i"
    run([str(compiler), *native_args], before_file, scratch / "native-before.stderr")
    observer_file = scratch / "observer.jsonl"
    selection = ["--physical-source", str(source)] if included_source else []
    run([args.observer, *selection, *arguments, "-fms-compatibility-version=" + args.msvc_version,
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
    before, directives = native_tokens(before_file, "before")
    after, after_directives = native_tokens(after_file, "after")
    if directives != after_directives:
        raise RuntimeError("native directive inventory changed during capture")
    pragma_offsets = []
    for row in rows:
        if row.get("kind") != "root_pragma" or row.get("hash_pragma") is not True:
            continue
        loc = row["location"]
        if loc.get("nested") is not False or physical(loc.get("path", "")) != physical(source):
            continue
        assert loc["buffer_sha256"].lower() == digest(original)
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
    definitions, buffers, expansions, callees, literal_arguments, specifier_macros, control_macros = [], {}, [], {}, [], [], []
    for record in records:
        begin, end = record["begin"], record.get("end_exclusive", {})
        if begin["nested"] or not begin.get("path") or not end.get("path"):
            continue
        if physical(begin["path"]) != physical(source) or physical(end["path"]) != physical(source):
            continue
        assert begin["buffer_sha256"].lower() == end["buffer_sha256"].lower() == digest(original)
        start, stop = begin["offset"], end["offset"]
        assert 0 <= start < stop <= len(original)
        assert original[start:start+len(record["name"])].decode() == record["name"]
        callees[start] = {"name": record["name"], "start": start}
        definition, definition_end = record.get("definition", {}), record.get("definition_end_exclusive", {})
        anchor = None
        literal = (not record.get("function_like") and len(record.get("replacement_tokens", [])) == 1
                   and record["replacement_tokens"][0][:1].isdigit())
        specifier = (not record.get("function_like") and record.get("replacement_tokens") in
                     [["inline"], ["__forceinline"], ["__inline"]])
        control = (not record.get("function_like") and record.get("replacement_tokens") in
                   [["try"], ["catch", "(", "...", ")"]])
        if (record.get("function_like") or literal or specifier or control) and definition.get("path") and definition_end.get("path"):
            header = Path(definition["path"]).resolve()
            if header.is_file() and Path(definition_end["path"]).resolve() == header:
                data = header.read_bytes()
                assert definition["buffer_sha256"].lower() == definition_end["buffer_sha256"].lower() == digest(data)
                key = os.path.normcase(str(header))
                if key not in buffers:
                    buffers[key] = len(definitions)
                    definitions.append({"path": str(header), "source": data.decode("utf-8")})
                assert definitions[buffers[key]]["source"].encode() == data
                anchor = {"buffer": buffers[key], "nameOffset": definition["offset"],
                          "end": definition_end["offset"], "parameters": record["parameters"]}
        if literal and anchor is not None:
            literal_arguments.append({"name": record["name"], "start": start, "end": stop, "definition": anchor})
        if specifier and anchor is not None:
            specifier_macros.append({"name": record["name"], "start": start, "end": stop, "definition": anchor})
        if control and anchor is not None:
            control_macros.append({"name": record["name"], "start": start, "end": stop, "definition": anchor})
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
              "expandedNames": sorted({r["name"] for r in records}), "calleeSites": list(callees.values())}
    encoded = json.dumps(packet, ensure_ascii=False).encode()
    if len(encoded) > 32 * 1024 * 1024:
        raise RuntimeError("observation packet exceeds limit")
    Path(os.environ["VORPAL_CPP_COMPILER_RESPONSE"]).write_bytes(encoded)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        # Detailed compiler stderr remains in the capture scratch directory;
        # do not expose arguments/environment or secrets in protocol diagnostics.
        print("native C++ observation declined", file=sys.stderr)
        raise SystemExit(1)
