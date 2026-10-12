#!/usr/bin/env python3
"""Package exact committed native provider sources without host tools/configuration."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parent.parent
FILES = (
    "scripts/cpp_macro_compiler_provider.py",
    "scripts/cpp_macro_native_projection.py",
    "scripts/cpp_macro_clang_context.py",
    "scripts/cpp_native_mcp.ps1",
    "scripts/cpp_macro_observer/observer.cc",
    "scripts/cpp_macro_observer/CMakeLists.txt",
    "docs/CPP_NATIVE_PROVIDER.md",
    "docs/CPP_NATIVE_MCP_LATENCY.md",
    "LICENSE",
)


def bundle(output, contents, version, revision):
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise ValueError("expected an exact commit identity")
    if not re.fullmatch(r"\d+\.\d+\.\d+\+moritz\.[1-9]\d*", version):
        raise ValueError("expected a fork release version")
    if set(contents) != set(FILES) or any(not isinstance(v, bytes) for v in contents.values()):
        raise ValueError("expected the exact provider source inventory")
    manifest = {"schema": 1, "version": version, "revision": revision,
                "repository": "https://github.com/moritz91/vorpal",
                "files": {name: {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
                          for name, data in sorted(contents.items())},
                "externalPrerequisites": ["Python 3.11+", "LLVM 19 observer/CIndex library and bindings",
                                          "Authoritative native compiler, SDK and explicit preprocessing plan"]}
    entries = dict(contents)
    entries["manifest.json"] = (json.dumps(manifest, indent=2, sort_keys=True)+"\n").encode()
    entries["README.txt"] = ("Vorpal native provider " + version + "\n"
        "Verify this ZIP against the release SHA256SUMS, then verify member digests in manifest.json.\n"
        "This package contains committed provider/observer sources, not compiler/SDK/library binaries.\n"
        "Keep existing native tools pinned and use a new versioned directory/configuration.\n"
        "Update cppMacroCompiler.arguments to the packaged Python provider and explicit local tools.\n"
        "Validate a new external index before switching MCP; preserve the prior binary/config/index.\n"
        "Native compiler captures stay mandatory on every query; this package enables no proof cache.\n").encode()
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("xb") as stream:
        with zipfile.ZipFile(stream, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
            for name, data in sorted(entries.items()):
                info = zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0))
                info.create_system = 3
                info.external_attr = 0o100644 << 16
                info.compress_type = zipfile.ZIP_DEFLATED
                archive.writestr(info, data, compresslevel=9)
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--revision")
    args = parser.parse_args()
    revision = args.revision or subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        parser.error("use an exact 40-character commit identity")
    def committed(name):
        return subprocess.check_output(["git", "show", revision+":"+name], cwd=ROOT)
    version = tomllib.loads(committed("Cargo.toml").decode())["workspace"]["package"]["version"]
    bundle(args.output, {name: committed(name) for name in FILES}, version, revision)
    print("Committed native provider package:", version, revision)


if __name__ == "__main__":
    main()
