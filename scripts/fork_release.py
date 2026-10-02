#!/usr/bin/env python3
"""Synchronize and validate the GitHub-only moritz91 fork release metadata."""
import argparse
import json
from pathlib import Path
import re
import tomllib

ROOT = Path(__file__).resolve().parent.parent
REPOSITORY = "https://github.com/moritz91/vorpal"
VERSION_PATTERN = r"(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\+moritz\.[1-9]\d*"


def write(path, text):
    with path.open("w", encoding="utf-8", newline="\n") as output:
        output.write(text)


def npm_manifests():
    return [ROOT / "npm/package.json", ROOT / "crates/napi/package.json",
            *sorted((ROOT / "npm/platforms").glob("*/package.json")),
            *sorted((ROOT / "crates/napi/npm").glob("*/package.json"))]


def synchronize(version):
    workspace_path = ROOT / "Cargo.toml"
    text = workspace_path.read_text(encoding="utf-8")
    old = tomllib.loads(text)["workspace"]["package"]["version"]
    write(workspace_path, text.replace(f'"{old}"', f'"{version}"')
          .replace("https://github.com/hyper-light/vorpal", REPOSITORY))
    for path in [ROOT / "pyproject.toml", ROOT / "crates/pyo3/pyproject.toml"]:
        text = path.read_text(encoding="utf-8")
        text = re.sub(r'^version = "[^"]+"', f'version = "{version}"', text, count=1, flags=re.M)
        write(path, text.replace("https://github.com/hyper-light/vorpal", REPOSITORY))
    for path in npm_manifests():
        data = json.loads(path.read_text(encoding="utf-8"))
        data.update(version=version, private=True, repository=REPOSITORY)
        for name in data.get("optionalDependencies", {}):
            if name.startswith("@hyper-light/vorpal-"):
                data["optionalDependencies"][name] = version
        write(path, json.dumps(data, indent=2) + "\n")
    lock_path = ROOT / "Cargo.lock"
    sections = lock_path.read_text(encoding="utf-8").split("[[package]]")
    for i, section in enumerate(sections[1:], 1):
        package = tomllib.loads(section)
        if "source" not in package and (package["name"].startswith("vorpal") or package["name"] == "wasm"):
            sections[i] = re.sub(r'^version = "[^"]+"', f'version = "{version}"', section, count=1, flags=re.M)
    write(lock_path, "[[package]]".join(sections))


def check(tag):
    workspace = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]
    version = workspace["package"]["version"]
    errors = []
    def require(condition, message):
        if not condition:
            errors.append(message)
    require(re.fullmatch(VERSION_PATTERN, version), f"Invalid fork version: {version}")
    require(tag is None or tag == f"v{version}", f"Tag {tag} != v{version}")
    require(workspace["package"]["repository"] == REPOSITORY, "Wrong Cargo repository")
    for name, dependency in workspace["dependencies"].items():
        if isinstance(dependency, dict) and "path" in dependency and name.startswith("vorpal"):
            require(dependency["version"] == version, f"Wrong dependency version: {name}")
    for package in tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf-8"))["package"]:
        if "source" not in package and (package["name"].startswith("vorpal") or package["name"] == "wasm"):
            require(package["version"] == version, f"Wrong lock version: {package['name']}")
    for path in [ROOT / "pyproject.toml", ROOT / "crates/pyo3/pyproject.toml"]:
        project = tomllib.loads(path.read_text(encoding="utf-8"))["project"]
        require(project["version"] == version, f"Wrong Python version: {path.relative_to(ROOT)}")
        require(project["urls"]["Repository"] == REPOSITORY, f"Wrong Python repository: {path.relative_to(ROOT)}")
    for path in npm_manifests():
        data = json.loads(path.read_text(encoding="utf-8"))
        require(data["version"] == version and data.get("private") is True,
                f"Wrong npm version or missing private flag: {path.relative_to(ROOT)}")
        require(data["repository"] == REPOSITORY, f"Wrong npm repository: {path.relative_to(ROOT)}")
        for name, value in data.get("optionalDependencies", {}).items():
            if name.startswith("@hyper-light/vorpal-"):
                require(value == version, f"Wrong npm dependency version: {name}")
    if errors:
        raise SystemExit("\n".join(errors))
    print(f"Fork release metadata OK: v{version}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--set-version", help="Synchronize all manifests and Cargo.lock")
    parser.add_argument("--tag", help="Validate the release tag against the manifests")
    args = parser.parse_args()
    if args.set_version:
        if not re.fullmatch(VERSION_PATTERN, args.set_version):
            parser.error("Use <upstream-version>+moritz.<positive-build-number>")
        synchronize(args.set_version)
    check(args.tag)
