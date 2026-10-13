#!/usr/bin/env python3
"""Measure configured native MCP requests against an external, isolated index.

Run in the actual compiler developer environment. No source files are edited.
Use --build with a new index path for a verified cold build, then compare reports
from the same source/config with different binaries. Provider proofs stay fresh.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import queue
import re
import statistics
import subprocess
import threading
import time


def fingerprint(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True).encode()).hexdigest()


def phase_summary(log):
    spans = {'extractionSeconds': ('stream: start', 'stream: absorb tail'),
             'graphLinkSeconds': ('link: table build start', 'link: seal done')}
    starts, samples = {}, {name: [] for name in spans}
    work_counts = []
    for line in log.read_text(encoding='utf-8', errors='replace').splitlines():
        match = re.match(r'\[phase ([0-9.]+)s\] (.*)', line)
        if not match:
            continue
        stamp, label = float(match[1]), match[2]
        counts = re.fullmatch(r'stream: products parsed=(\d+) replayed=(\d+)', label)
        if counts:
            work_counts.append({'parsed': int(counts[1]), 'replayed': int(counts[2])})
        for name, (begin, end) in spans.items():
            if label == begin:
                starts[name] = stamp
            elif label == end and name in starts:
                samples[name].append(stamp - starts.pop(name))
    result = {name: {'samples': len(values), 'median': statistics.median(values)}
              for name, values in samples.items() if values}
    if work_counts:
        result['streamWorkCounts'] = work_counts
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--config", required=True, type=Path)
    parser.add_argument("--index", required=True, type=Path)
    parser.add_argument("--report", required=True, type=Path)
    parser.add_argument("--symbol", required=True)
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--timeout", type=float, default=300)
    parser.add_argument("--build", action="store_true")
    args = parser.parse_args()
    if not 1 <= args.rounds <= 30 or args.timeout <= 0:
        parser.error("rounds must be 1..30 and timeout positive")
    source, index = args.source.resolve(), args.index.resolve()
    if index == source or source in index.parents:
        parser.error("use an external isolated index")
    report = args.report.resolve()
    if report == source or source in report.parents or report == index or index in report.parents:
        parser.error("keep reports outside the source and index")
    inputs = {args.binary.resolve(), args.config.resolve()}
    if any(path in inputs for path in (report, report.with_suffix('.build.log'), report.with_suffix('.mcp.log'))):
        parser.error("report files must not replace inputs")
    if args.build and index.exists():
        parser.error("--build requires a new index directory")
    if not args.build and not (index / "CURRENT").is_file():
        parser.error("existing index must have CURRENT")
    args.report.parent.mkdir(parents=True, exist_ok=True)
    environment = os.environ.copy()
    environment["VORPAL_NO_AUTOWARM"] = "1"
    environment["VORPAL_PHASE_TRACE"] = "1"
    result = {"schema": 1, "binarySha256": hashlib.sha256(args.binary.read_bytes()).hexdigest(),
              "source": str(source), "configSha256": hashlib.sha256(args.config.read_bytes()).hexdigest(),
              "symbol": args.symbol, "rounds": args.rounds, "requests": []}
    result['cachePreparation'] = ('verifiedBuildWithMeasuredBinary' if args.build
                                  else 'existingIndexNoPreparation')
    command = [str(args.binary), "index", str(source), "--out", str(index),
               "--config", str(args.config), "--verify"]
    if args.build:
        started = time.perf_counter()
        with args.report.with_suffix(".build.log").open("wb") as log:
            build = subprocess.run(command, env=environment, stdout=log, stderr=log,
                                   timeout=args.timeout)
        result["coldBuildSeconds"] = time.perf_counter() - started
        if build.returncode:
            raise RuntimeError("verified cold build failed; inspect build log")
        print("cold build", round(result["coldBuildSeconds"], 3), "s", flush=True)
    command = [str(args.binary), "mcp", "--src", str(source), "--index", str(index),
               "--config", str(args.config), "--profile", "full", "--no-watch-rebuild"]
    responses = queue.Queue()
    with args.report.with_suffix(".mcp.log").open("wb") as log:
        launched = time.perf_counter()
        process = subprocess.Popen(command, env=environment, stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=log)

        def reader():
            try:
                for line in process.stdout:
                    responses.put(json.loads(line))
            except Exception as error:
                responses.put(error)
            finally:
                responses.put(EOFError("MCP stdout closed"))

        threading.Thread(target=reader, daemon=True).start()
        number = 0

        def request(method, params):
            nonlocal number
            number += 1
            started = time.perf_counter()
            process.stdin.write((json.dumps({"jsonrpc": "2.0", "id": number,
                                            "method": method, "params": params}) + "\n").encode())
            process.stdin.flush()
            deadline = started + args.timeout
            while True:
                reply = responses.get(timeout=max(0.001, deadline - time.perf_counter()))
                if isinstance(reply, Exception):
                    raise reply
                if reply.get("id") == number:
                    if "error" in reply:
                        raise RuntimeError(reply["error"])
                    return reply["result"], time.perf_counter() - started

        try:
            initialized, _ = request("initialize", {"protocolVersion": "2024-11-05",
                "capabilities": {}, "clientInfo": {"name": "native-mcp-benchmark", "version": "1"}})
            result["version"] = initialized["serverInfo"]["version"]
            result["startupSeconds"] = time.perf_counter() - launched
            process.stdin.write(b'{"jsonrpc":"2.0","method":"notifications/initialized"}\n')
            process.stdin.flush()
            signatures = {}
            for round_number in range(args.rounds):
                for name, arguments in [("health", {}),
                                        ("graph", {"relation": "callees", "name": args.symbol, "format": "lean"}),
                                        ("snippet", {"name": args.symbol, "max_bytes": 4096})]:
                    response, elapsed = request("tools/call", {"name": name, "arguments": arguments})
                    if response.get("isError"):
                        raise RuntimeError("MCP tool failed: " + name)
                    content = response["content"]
                    generation = response.get("structuredContent", {}).get("generation")
                    if not generation:
                        raise RuntimeError("MCP response has no generation: " + name)
                    if result.setdefault('generation', generation) != generation:
                        raise RuntimeError("unchanged requests returned different generations")
                    if name != 'health' and not response['structuredContent'].get('records'):
                        raise RuntimeError("benchmark symbol has no result: " + name)
                    row = {"round": round_number + 1, "tool": name, "seconds": elapsed,
                           "contentSha256": fingerprint(content), "generation": generation}
                    if name == 'health':
                        row['parseHealth'] = content[0]['text'].splitlines()[0]
                    result["requests"].append(row)
                    signature = (row["contentSha256"], generation)
                    if name in signatures and signatures[name] != signature:
                        raise RuntimeError("unchanged requests returned different content/generation: " + name)
                    signatures[name] = signature
                    print("round", round_number + 1, name, round(elapsed, 3), "s", flush=True)
                    args.report.write_text(json.dumps(result, indent=2), encoding="utf-8")
            result["summary"] = {}
            for name in signatures:
                times = [r["seconds"] for r in result["requests"] if r["tool"] == name]
                result["summary"][name] = {"medianSeconds": statistics.median(times),
                    "minSeconds": min(times), "maxSeconds": max(times)}
            result["stableUnchangedAnswers"] = True
        finally:
            process.stdin.close()
            try:
                process.wait(timeout=30)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            process.stdout.close()
            result["exitCode"] = process.returncode
            result['phases'] = phase_summary(args.report.with_suffix('.mcp.log'))
            args.report.write_text(json.dumps(result, indent=2), encoding="utf-8")
        if process.returncode:
            raise RuntimeError("MCP did not exit cleanly")


if __name__ == "__main__":
    main()
