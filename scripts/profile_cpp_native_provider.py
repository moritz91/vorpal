#!/usr/bin/env python3
"""Opt-in, capture-local provider profiler for an external benchmark config.

Forward provider arguments unchanged. Only successful captures emit a bounded
profile; no source, compiler arguments or environment values enter the report.
Profiling adds overhead: compare unprofiled MCP runs for actual latency claims.
The normal Rust launcher still owns this subprocess tree and its timeout.
"""
import argparse
import cProfile
import hashlib
import importlib.util
import json
import ntpath
import os
from pathlib import Path
import pstats
import re
import sys
import subprocess
import time


def summarize(stats):
    rows = [{"file": ntpath.basename(os.path.basename(file)), "line": line,
             "function": function, "calls": calls, "selfSeconds": own,
             "inclusiveSeconds": cumulative}
            for (file, line, function), (_, calls, own, cumulative, _) in stats.items()]
    return sorted(rows, key=lambda row: row["inclusiveSeconds"], reverse=True)[:90]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("provider", type=Path)
    parser.add_argument("arguments", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    request_path = Path(os.environ["VORPAL_CPP_COMPILER_REQUEST"]).resolve()
    request = json.loads(request_path.read_bytes())
    request_id = request["requestId"]
    if not isinstance(request_id, str) or not re.fullmatch(r"[A-Za-z0-9_-]{1,128}", request_id):
        raise ValueError("invalid profiling request identity")
    output = args.output_dir.resolve()
    for root in [request_path.parent, Path(request["path"]).resolve().parent]:
        if output == root or root in output.parents:
            raise ValueError("use an external profiling directory")
    provider = args.provider.resolve()
    sys.path.insert(0, str(provider.parent))
    spec = importlib.util.spec_from_file_location("vorpal_profiled_provider", provider)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    observer = Path(args.arguments[args.arguments.index("--observer") + 1]).resolve()
    native_runs, subprocesses = 0, []
    original_run = subprocess.run
    def measured_run(command, *positional, **keywords):
        nonlocal native_runs
        if "--native-hash-offsets" in command:
            phase = "nativeHashLexer"
        elif Path(command[0]).resolve() == observer:
            phase = "observer"
        else:
            phase = "nativeBefore" if native_runs == 0 else "nativeAfter"
            native_runs += 1
        begin = time.perf_counter()
        result = original_run(command, *positional, **keywords)
        subprocesses.append({"phase": phase, "seconds": time.perf_counter() - begin})
        return result
    profiler = cProfile.Profile()
    argv = sys.argv
    try:
        sys.argv = [str(provider), *args.arguments]
        subprocess.run = measured_run
        started = time.perf_counter()
        profiler.runcall(module.main)
        elapsed = time.perf_counter() - started
    finally:
        sys.argv = argv
        subprocess.run = original_run
    report = {"schema": 1, "profiledSeconds": elapsed,
              "sourceContextSha256": hashlib.sha256(request["path"].encode()).hexdigest(),
              "subprocesses": subprocesses,
              "functions": summarize(pstats.Stats(profiler).stats)}
    output.mkdir(parents=True, exist_ok=True)
    with (output / (request_id + ".json")).open("x", encoding="utf-8") as stream:
        json.dump(report, stream)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        print("native provider profiling declined", file=sys.stderr)
        raise SystemExit(1)
