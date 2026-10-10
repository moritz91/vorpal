"""Native MSVC/LLVM controls for literal warning-operator observation.

Run from a Visual Studio developer shell with an explicit trusted provider config.
No tools are downloaded, sources outside --out are read-only, and no index is built.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess


CASES = [
    ("literal", "__pragma(warning(push))\nvoid run() {}\n__pragma(warning(pop))\n", True, True),
    ("object", "#define BEGIN __pragma(warning(push))\n#define END __pragma(warning(pop))\nBEGIN\nvoid run() {}\nEND\n", True, True),
    ("nested", "#define BEGIN __pragma(warning(push))\n#define END __pragma(warning(pop))\n#define WHOLE BEGIN void run() {} END\nWHOLE\n", True, True),
    ("literal_payload", 'const char* text = "__pragma(warning(push))";\n/* __pragma(warning(pop)) */\nvoid run() {}\n', True, True),
    ("parameter", "#define WARN(op) __pragma(warning(op))\nWARN(push)\nvoid run() {}\nWARN(pop)\n", True, False),
    ("expanded_operand", "#define OP push\n#define WARN __pragma(warning(OP))\nWARN\nvoid run() {}\n__pragma(warning(pop))\n", True, False),
    ("numeric_level", "__pragma(warning(push, 4))\nvoid run() {}\n__pragma(warning(pop))\n", True, False),
    ("disable", "__pragma(warning(disable: 4100))\nvoid run() {}\n", True, False),
    ("missing_semicolon", "void call();\nvoid run() { __pragma(warning(push)) call() }\n", False, True),
    ("hash_warning", '#pragma warning(push)\n#pragma warning(disable: 4180) // original comment\n#pragma optimize("", off)\nvoid run() {}\n#pragma optimize("", on)\n#pragma warning(pop)\n', True, True),
    ("try_handler", '#define ENTER try\n#define HANDLE catch (...)\nvoid call();\nvoid run() { ENTER { call(); } HANDLE { call(); } }\n', True, True),
    ("typed_handler", '#define ENTER try\n#define HANDLE(type_1) catch (type_1)\nstruct Token {};\nvoid call();\nvoid run() { ENTER { throw Token{}; } HANDLE /* original */ (Token&) { call(); } }\n', True, True),
    ("try_missing_semicolon", '#define ENTER try\n#define HANDLE catch (...)\nvoid call();\nvoid run() { ENTER { call() } HANDLE { call(); } }\n', False, True),
    ("observer_only_diagnostic", '#if defined(__clang__)\n#pragma clang diagnostic push\n#pragma ide diagnostic ignored "inspection"\n#pragma clang diagnostic pop\n#endif\nvoid run() {}\n', True, True),
]


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--config", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    command = json.loads(args.config.read_bytes())["cppMacroCompiler"]
    compiler = shutil.which("cl.exe")
    assert compiler, "Load the Visual Studio developer environment first"
    args.out.mkdir(parents=True, exist_ok=True)
    cases = [(name + suffix, text.replace("\n", newline), syntax_ok, agreement)
             for suffix, newline in [("-lf", "\n"), ("-crlf", "\r\n")]
             for name, text, syntax_ok, agreement in CASES]
    for name, text, syntax_ok, agreement in cases:
        directory = args.out.resolve() / name
        directory.mkdir(exist_ok=True)
        source = directory / "proof.cc"
        source.write_bytes(text.encode())
        flags = ["/nologo", "/TP", "/std:c++20", "/EHsc", "/Zc:preprocessor",
                 "/Zc:__cplusplus", "/U__clang__", "/U__GNUC__", str(source), "/Zs"]
        if name.startswith("observer_only_diagnostic"):
            flags.remove("/U__clang__")
        syntax = subprocess.run([compiler, *flags], capture_output=True)
        (directory / "syntax.log").write_bytes(syntax.stdout + syntax.stderr)
        assert (syntax.returncode == 0) == syntax_ok, (name, "native syntax control")
        plan = directory / "plan.json"
        plan.write_bytes(json.dumps([{"source": str(source), "cwd": str(directory), "args": flags}]).encode())
        request = directory / "request.json"
        request.write_bytes(json.dumps({"version": 1, "requestId": name,
                                       "path": str(source), "source": text}).encode())
        response = directory / "response.json"
        ready = directory / "ready"
        ready.write_bytes(b"")
        env = dict(os.environ, VORPAL_CPP_COMPILER_REQUEST=str(request),
                   VORPAL_CPP_COMPILER_RESPONSE=str(response), VORPAL_CPP_COMPILER_READY=str(ready))
        arguments = list(command["arguments"])
        arguments[arguments.index("--plan") + 1] = str(plan)
        observed = subprocess.run([command["program"], *arguments], cwd=command["directory"],
                                  env=env, capture_output=True, timeout=60)
        (directory / "provider.log").write_bytes(observed.stdout + observed.stderr)
        assert observed.returncode == 0, (name, "observer failed; inspect local provider.log")
        packet = json.loads(response.read_bytes())
        assert packet["complete"] and packet["nativeBefore"] == packet["nativeAfter"]
        assert (packet["nativeBefore"] == packet["observedTokens"]) == agreement, (name, "token agreement")
        if name.startswith(("hash_warning", "observer_only_diagnostic")):
            assert len(packet["pragmaOffsets"]) == len(packet["nativeDirectives"])
        assert source.read_bytes() == text.encode(), (name, "original source changed")
        print(f"{name}: native syntax={syntax_ok}, full token agreement={agreement}", flush=True)


if __name__ == "__main__":
    main()
