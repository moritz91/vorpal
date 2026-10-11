#!/usr/bin/env python3
"""Read-only Windows/MSVC filesystem-probe audit; never authorizes proof replay.

Runs a trusted preprocess-only plan twice normally and twice under a job-local
Detours observer. Paths/status/order and bracketed byte snapshots are research
evidence, not a dependency-completeness claim. No MCP/provider/cache integration.
"""
import argparse
import hashlib
import json
import ntpath
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import time

MAGIC = b"VORPAL-NATIVE-AUDIT-1\n"
RECORD = struct.Struct("<7I3Q")
MAX_TRACE = 64 * 1024 * 1024
MAX_RECORDS = 100000
MAX_FILE = 64 * 1024 * 1024
MAX_TOTAL = 512 * 1024 * 1024
KINDS = {1: "start", 2: "create", 3: "open", 4: "attributes",
         5: "fullAttributes", 6: "read", 7: "section", 8: "directory",
         9: "information", 10: "end", 11: "fault", 12: "job", 13: "child", 14: "directoryEntry"}
MISSING = {0xC000000F, 0xC0000034, 0xC000003A}  # NO_SUCH_FILE / NAME/PATH_NOT_FOUND
BOUNDARIES = [
    "user-mode hooks do not certify all possible native filesystem access",
    "initial loader inputs before DLL attachment are not observed",
    "registry, device, network and other non-file inputs are not inventoried",
    "only FileDirectoryInformation names/attributes are decoded; other payloads and mapped/read bytes are not captured",
    "before/after snapshots cannot exclude change-and-restore during execution",
    "explicit tool file digests do not certify the complete SDK/resource closure",
]


def digest(data):
    return hashlib.sha256(data).hexdigest()


def identity(value):
    return digest(json.dumps(value, sort_keys=True, ensure_ascii=True,
                             separators=(",", ":")).encode("ascii"))


def parse_trace(data):
    if len(data) > MAX_TRACE or not data.startswith(MAGIC):
        raise ValueError("invalid or oversized native trace")
    records, offset, previous = [], len(MAGIC), 0
    while offset < len(data):
        if len(records) >= MAX_RECORDS or len(data) - offset < RECORD.size:
            raise ValueError("truncated or excessive native records")
        kind, status, flags, access, thread, names, resolved, sequence, begin, end = RECORD.unpack_from(data, offset)
        offset += RECORD.size
        if kind not in KINDS or flags & ~3 or names > 32768 or resolved > 32768:
            raise ValueError("unknown or excessive native record")
        size = 2 * (names + resolved)
        if size > len(data) - offset:
            raise ValueError("truncated native path")
        name = data[offset:offset + 2 * names].decode("utf-16-le", errors="surrogatepass")
        final = data[offset + 2 * names:offset + size].decode("utf-16-le", errors="surrogatepass")
        offset += size
        if "\0" in name or "\0" in final or end < begin:
            raise ValueError("invalid native path or clock")
        if kind == 12 or sequence != previous + 1:
            raise ValueError("native record sequence gap")
        else:
            previous = sequence
        records.append({"kind": KINDS[kind], "status": status, "flags": flags,
                        "access": access, "thread": thread, "sequence": sequence,
                        "begin": begin, "end": end, "path": name, "resolved": final})
    if len(records) < 2 or records[0]["kind"] != "start" or records[-1]["kind"] != "end":
        raise ValueError("native trace lacks lifecycle records")
    if records[0]["status"] or records[0]["access"] != 11 or records[-1]["status"]:
        raise ValueError("native hooks or compiler failed")
    if any(r["flags"] & 2 or r["kind"] == "fault" for r in records):
        raise ValueError("native logger lost observations")
    if any(r["kind"] in ("start", "end", "job") for r in records[1:-1]):
        raise ValueError("duplicate lifecycle record")
    return records


def parse_capture(prefix):
    job = Path(str(prefix) + ".job.bin").read_bytes()
    if len(job) != RECORD.size:
        raise ValueError("invalid native job envelope")
    kind, status, flags, processes, root, names, resolved, sequence, begin, end = RECORD.unpack(job)
    if kind != 12 or status or flags or names or resolved or sequence or begin or end or not 1 <= processes <= 128:
        raise ValueError("native job did not complete within bounds")
    captures, total = {}, 0
    for path in prefix.parent.glob(prefix.name + ".*.bin"):
        pid = path.name[len(prefix.name) + 1:-4]
        if pid == "job":
            continue
        if not pid.isdecimal() or len(captures) >= 128:
            raise ValueError("unknown or excessive process traces")
        data = path.read_bytes()
        total += len(data)
        if total > MAX_TRACE:
            raise ValueError("native process traces exceed aggregate bound")
        captures[int(pid)] = parse_trace(data)
    if root not in captures:
        raise ValueError("root compiler trace missing")
    visited, ordered = set(), []

    def visit(pid):
        if pid in visited:
            raise ValueError("duplicate native child relationship")
        if pid not in captures:
            return
        visited.add(pid)
        label = len(visited)
        for record in captures[pid]:
            ordered.append(dict(record, process=label))
            if record["kind"] == "child" and not record["status"]:
                visit(record["access"])

    visit(root)
    if visited != set(captures):
        raise ValueError("process traces are not rooted in the launched compiler")
    return ordered, {"jobProcesses": processes, "tracedProcesses": len(captures),
                     "allJobProcessesTraced": processes == len(captures)}


def filesystem_path(name):
    """Keep the lexical NT/DOS spelling separately; accept only absolute paths."""
    if name.startswith("\\??\\UNC\\"):
        name = "\\\\" + name[8:]
    elif name.startswith("\\\\?\\UNC\\"):
        name = "\\\\" + name[8:]
    elif name.startswith(("\\??\\", "\\\\?\\")):
        name = name[4:]
    drive, tail = ntpath.splitdrive(name)
    if not drive or not tail.startswith("\\") or "\0" in name:
        return None
    # Special DOS device names and extended paths with dot components are not
    # silently reinterpreted by the Python snapshotter.
    if any(p in (".", "..") for p in tail.split("\\")) or "*" in tail or "?" in tail:
        return None
    if any(p.split(".")[0].upper() in ("NUL", "CON", "PRN", "AUX", "CONIN$", "CONOUT$")
           for p in tail.split("\\") if p):
        return None
    return name


def candidates(records):
    result = []
    for record in records:
        if record["kind"] == "directoryEntry":
            continue  # Listing membership is captured by its parent directory snapshot.
        for field in ("path", "resolved"):
            value = filesystem_path(record[field])
            if value is not None and value not in result:
                result.append(value)
    if len(result) > 4096:
        raise ValueError("native candidate inventory exceeds bound")
    return result


def snapshot(paths):
    result, total = {}, 0
    for spelling in paths:
        path = Path(spelling)
        value = {"resolved": os.path.realpath(path)}
        # Record reparse metadata along the lexical path, including missing
        # ancestors. Canonicalizing only the final file would hide redirects.
        ancestry = []
        for ancestor in [*reversed(path.parents), path]:
            try:
                stat = ancestor.lstat()
                ancestry.append([str(ancestor), stat.st_dev, stat.st_ino,
                                 getattr(stat, "st_reparse_tag", 0)])
            except FileNotFoundError:
                ancestry.append([str(ancestor), "missing"])
            except OSError as error:
                ancestry.append([str(ancestor), "unavailable", getattr(error, "winerror", None) or error.errno])
        value["ancestry"] = ancestry
        try:
            before = path.stat()
            value["fileId"] = [before.st_dev, before.st_ino]
            if path.is_file():
                if before.st_size > MAX_FILE or total + before.st_size > MAX_TOTAL:
                    value["state"] = "over-limit"
                else:
                    with path.open("rb") as stream:
                        content = stream.read(MAX_FILE + 1)
                    total += len(content)
                    if len(content) > MAX_FILE or total > MAX_TOTAL:
                        value["state"] = "over-limit"
                    else:
                        value.update(state="file", size=len(content), sha256=digest(content))
            elif path.is_dir():
                with os.scandir(path) as entries:
                    names = []
                    for entry in entries:
                        metadata = entry.stat(follow_symlinks=False)
                        names.append([entry.name, metadata.st_dev, metadata.st_ino,
                                      getattr(metadata, "st_file_attributes", metadata.st_mode),
                                      getattr(metadata, "st_reparse_tag", 0)])
                        if len(names) > MAX_RECORDS:
                            raise ValueError("directory snapshot exceeds bound")
                value.update(state="directory", entriesIdentity=identity(sorted(names)))
            else:
                value["state"] = "non-regular"
            after = path.stat()
            if (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns) != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns):
                value["state"] = "changed-during-read"
        except FileNotFoundError:
            value["state"] = "missing"
        except OSError as error:
            value.update(state="unavailable", error=getattr(error, "winerror", None) or error.errno)
        result[spelling] = value
    return result


def ordered_probes(records):
    # Thread numbers/times/PIDs are capture metadata, not reusable identities.
    # Completion order is retained; begin/end reveal overlapping calls in raw
    # artifacts, so this report does not invent an ordering across threads.
    result = []
    for record in records:
        if record["kind"] in ("start", "end", "job"):
            continue
        value = {k: record[k] for k in ("kind", "status", "flags", "access", "path", "resolved")}
        value["process"] = record.get("process", 1)
        if value["kind"] == "child":
            value["access"] = 0  # PID is not part of the input identity.
        result.append(value)
    return result


def validate_plan(arguments, unit):
    if (not isinstance(arguments, list) or len(arguments) < 2
            or not all(isinstance(a, str) and "\0" not in a for a in arguments)
            or arguments[-1].lower() != "/zs"
            or os.path.normcase(os.path.realpath(arguments[-2])) != os.path.normcase(os.path.realpath(unit))):
        raise ValueError("one explicit preprocess-only MSVC plan is required")
    for argument in arguments:
        if argument.startswith("@"):
            raise ValueError("response files are not supported")
        if argument.startswith(("/", "-")):
            option = argument.lstrip("/-")
            if option.lower() in ("p", "e", "ep", "c", "ld", "ldd", "link", "mp"):
                raise ValueError("output/multiprocess options are not audit inputs")
            if not option.startswith("FI") and option.lower().startswith(("fo", "fd", "fe", "fp", "fa", "fm", "fr", "fi", "yc", "sourcedependencies", "analyze:log", "b1", "b2")):
                raise ValueError("output/alternate-driver options are not audit inputs")
    return [a for a in arguments if a.lower() != "/zs"] + ["/E"]


def context_identity(compiler, plan, cwd, inputs, environment):
    env = dict(environment)
    env.pop("VORPAL_NATIVE_AUDIT_TRACE", None)
    return identity({"version": 1, "compiler": str(compiler), "cwd": str(cwd),
                     "plan": digest(plan), "environment": env,
                     "explicitFiles": snapshot(inputs)})


def run(command, cwd, environment, directory, label, timeout):
    output, error = directory / (label + ".i"), directory / (label + ".stderr")
    with output.open("xb") as out, error.open("xb") as err:
        process = subprocess.Popen(command, cwd=cwd, env=environment, stdout=out,
                                   stderr=err, stdin=subprocess.DEVNULL)
        deadline = time.monotonic() + timeout + 10
        while process.poll() is None:
            prefix = Path(environment['VORPAL_NATIVE_AUDIT_TRACE'])
            trace_files = list(prefix.parent.glob(prefix.name + '.*.bin'))
            if (time.monotonic() > deadline or output.stat().st_size > 128 * 1024 * 1024
                    or error.stat().st_size > 32 * 1024 * 1024 or len(trace_files) > 129
                    or sum(p.stat().st_size for p in trace_files) > MAX_TRACE):
                process.kill()  # Closing the launcher closes only its own descendant job.
                process.wait()
                raise ValueError("native audit timed out or exceeded output bound")
            time.sleep(0.05)
        status = process.returncode
    if status or output.stat().st_size > 128 * 1024 * 1024:
        raise ValueError("native preprocess failed or exceeded bound; inspect local artifacts")
    return digest(output.read_bytes())


def audit(args):
    if sys.platform != "win32":
        raise ValueError("native MSVC audit requires Windows")
    if os.environ.get("CL") or os.environ.get("_CL_"):
        raise ValueError("implicit CL/_CL_ options are not supported by this audit")
    plan = Path(args.plan).resolve()
    plan_bytes = plan.read_bytes()
    source = Path(args.source).resolve()
    entries = json.loads(plan_bytes)
    selected = [e for e in entries if os.path.normcase(os.path.realpath(e["source"])) == os.path.normcase(str(source))]
    if len(selected) != 1:
        raise ValueError("a unique explicit source plan is required")
    entry = selected[0]
    cwd = Path(entry["cwd"])
    if not cwd.is_absolute():
        raise ValueError("native working directory must be explicit and absolute")
    unit = Path(entry.get("translationUnit", entry["source"])).resolve()
    arguments = validate_plan(entry["args"], unit)
    compiler = shutil.which(args.compiler)
    if not compiler:
        raise ValueError("load the Visual Studio developer environment first")
    compiler = Path(compiler).resolve()
    launcher, dll = Path(args.launcher).resolve(), Path(args.dll).resolve()
    report = Path(args.report).resolve()
    if report == source or report == unit or report.is_relative_to(cwd.resolve()):
        raise ValueError("audit artifacts must live outside the source project")
    if report.exists():
        raise ValueError("audit report must be new")
    directory = report.with_name(report.stem + "-artifacts")
    directory.mkdir(parents=True, exist_ok=False)
    inputs = list(dict.fromkeys(str(p) for p in
        [compiler, launcher, dll, plan, source, unit, Path(__file__).resolve(),
         *[Path(p).resolve() for p in args.identity_input]]))
    if any(not Path(p).is_file() for p in inputs):
        raise ValueError("explicit identity inputs must be physical readable files")
    environment = dict(os.environ)
    before_context = context_identity(compiler, plan_bytes, cwd, inputs, environment)
    def plain(label):
        env = dict(environment)
        env["VORPAL_NATIVE_AUDIT_TRACE"] = str(directory / label)
        return run([str(launcher), "-", str(args.timeout * 1000), "--", str(compiler), *arguments],
                   cwd, env, directory, label, args.timeout)

    plain_before = plain("plain-before")

    def traced(label):
        env = dict(environment)
        trace = directory / label
        env["VORPAL_NATIVE_AUDIT_TRACE"] = str(trace)
        output = run([str(launcher), str(dll), str(args.timeout * 1000), "--", str(compiler), *arguments],
                     cwd, env, directory, label, args.timeout)
        records, coverage = parse_capture(trace)
        return output, records, coverage

    discovery_output, discovery, discovery_coverage = traced("discovery")
    paths = candidates(discovery)
    before = snapshot(paths)
    confirmed_output, confirmed, coverage = traced("confirmed")
    confirmed_paths = candidates(confirmed)
    new_paths = [p for p in confirmed_paths if p not in before]
    after = snapshot(list(dict.fromkeys(paths + confirmed_paths)))
    plain_after = plain("plain-after")
    final = snapshot(list(after))
    after_context = context_identity(compiler, plan.read_bytes(), cwd, inputs, environment)
    probes = ordered_probes(confirmed)
    changes = [p for p in before if before[p] != after[p] or after[p] != final[p]]
    unresolved = sum(bool(p["flags"] & 1) or filesystem_path(p["path"]) is None
                     for p in probes if p["kind"] not in ("directoryEntry", "child"))
    output_equal = len({plain_before, discovery_output, confirmed_output, plain_after}) == 1
    usable_states = {"file", "directory", "missing"}
    unobservable = [p for p, v in final.items() if v["state"] not in usable_states]
    report_data = {
        "version": 1, "scope": "launched-x64-msvc-process-user-mode-filesystem-audit",
        "replayEligible": False, "boundaries": BOUNDARIES,
        "source": str(source), "translationUnit": str(unit),
        "contextBefore": before_context, "contextAfter": after_context,
        "contextStable": before_context == after_context,
        "nativeOutputEqual": output_equal, "nativeOutputSha256": plain_after,
        "traceIntegrity": "framed-start-end-eleven-hooks-per-observed-process",
        "processCoverage": coverage,
        "processCoverageStable": discovery_coverage == coverage,
        "probeOrderEqual": ordered_probes(discovery) == probes,
        "snapshotStable": not changes and not new_paths and not unobservable,
        "newCandidatesDuringConfirmation": new_paths,
        "changedCandidates": changes, "unobservableCandidates": unobservable,
        "unresolvedProbeCount": unresolved,
        "counts": {"probes": len(probes), "candidatePaths": len(final),
                   "failedMissingProbes": sum(p["status"] in MISSING for p in probes),
                   "successfulReadOrMap": sum(p["kind"] in ("read", "section") and p["status"] < 0x80000000 for p in probes)},
        "orderedProbes": probes, "snapshots": final,
        "directoryEntries": [p for p in probes if p["kind"] == "directoryEntry"],
        "auditIdentity": identity({"version": 1, "context": after_context,
                                   "probes": probes, "snapshots": final}),
    }
    with report.open("x", encoding="utf-8") as stream:
        json.dump(report_data, stream, ensure_ascii=True, indent=2)
        stream.write("\n")
    return report_data


def main():
    parser = argparse.ArgumentParser(__doc__)
    for name in ("plan", "source", "launcher", "dll", "report"):
        parser.add_argument("--" + name, required=True)
    parser.add_argument("--compiler", default="cl.exe")
    parser.add_argument("--identity-input", action="append", default=[])
    parser.add_argument("--timeout", type=int, default=60)
    args = parser.parse_args()
    if not 1 <= args.timeout <= 300:
        parser.error("timeout must be between 1 and 300 seconds")
    try:
        report = audit(args)
        print(json.dumps({k: report[k] for k in ("replayEligible", "nativeOutputEqual", "contextStable", "snapshotStable", "counts")}))
    except (ValueError, OSError, KeyError, TypeError) as error:
        print("native dependency audit declined: " + str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
