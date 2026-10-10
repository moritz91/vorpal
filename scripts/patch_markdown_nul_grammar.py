#!/usr/bin/env python3
"""Classify physical NUL as CommonMark's U+FFFD without changing source bytes.

Run after regenerating either vendored Markdown parser. The hook is inside the
generated lexer loop, so it applies again after each advance, and distinguishes
a real NUL from EOF. It does not alter tokens, spans, ABI or visible node kinds.
"""
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ORIGINAL = "  eof = lexer->eof(lexer);\n"
PATCHED = ORIGINAL + (
    "  // vorpal: CommonMark 2.3 classifies physical NUL as U+FFFD; retain source spans.\n"
    "  if (lookahead == 0 && !eof) lookahead = 0xfffd;\n"
)

for name in ["tree-sitter-markdown", "tree-sitter-markdown-inline"]:
    path = ROOT / "grammars" / "tree-sitter-md" / name / "src" / "parser.c"
    source = path.read_text(encoding="utf-8")
    if source.count(PATCHED) == 1:
        continue
    assert source.count(ORIGINAL) == 1, f"unexpected generated lexer in {path}"
    path.write_bytes(source.replace(ORIGINAL, PATCHED).encode("utf-8"))
