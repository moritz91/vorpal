# Markdown control-byte parsing

CommonMark 0.31.2 permits any character sequence and specifies U+FFFD replacement
for physical NUL (section 2.3):
<https://spec.commonmark.org/0.31.2/#insecure-characters>.

The vendored block and inline grammars previously rejected physical NUL because
their generated wildcard lexer transitions exclude code point zero. Each lexer
now classifies a real NUL as U+FFFD after checking `lexer->eof`; EOF is unchanged.
The classification runs after every lexer advance. The source buffer, token
text, byte lengths, authored content and ranges remain unchanged. Vorpal is a
source indexer, not an HTML renderer: replacement-character rendering must not
change physical indexing spans.

Reapply the hook after regenerating either parser:

```sh
python3 scripts/patch_markdown_nul_grammar.py
```

The script is byte-idempotent and rejects an unexpected lexer layout. ABI,
grammar rules, node kinds and schemas are unchanged; only Markdown provenance
changes. The builtin `markdown-physical-nul-v1` behavior revision extends the
structural grammar digest, which alone cannot detect a lexer-only change.
Product replay, injection hosts, the whole-tree manifest stamp and remote parity
use that revision. Frozen pre-fix loose and packed products are migrated even
when source stats are unchanged; unrelated products replay and the resulting
generation equals a scratch build.

Windows/Linux regressions cover UTF-8, LF/CRLF, consecutive NULs, control-byte
fence diagrams, headings following fences, plain/code/HTML/link inline content,
actual EOF and insertion/removal in incremental trees. Inline trees have the
same structure as their U+FFFD equivalents but retain physical source text.
Owned, encoded streaming and Scan-root products agree. The unchanged full Rust
corpus and all 322 stock block corpus cases pass. The stock inline corpus has
eight existing Wiki-link/tag extension mismatches; comparison with the exact
pre-change HEAD shows the same eight failures and no new mismatch.

In the isolated normal Hades MCP, all three affected Markdown guide files are
now parse-clean. With the native generator fixes, total health is four affected
files / 55 diagnostics / 593 diagnostic bytes. Three files are glTF textual
include chunks needing shared context; the other is Catch2. Hades sources,
productive binaries and productive MCP remain
unchanged.
