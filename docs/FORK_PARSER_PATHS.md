# Fork parser and Windows path fixes

The fork accepts either separator spelling in graph path suffix selectors, search
filters, graph predicates, dead-code filters, structural `files_of` scopes and
MATCH queries. Stored paths, node identities and case sensitivity are preserved.
Suffixes retain string-suffix semantics, including partial filenames and empty
suffixes. Include resolution separately enforces a path-component boundary.

The resolver also recognizes Windows separators in basenames, module stems,
private Rust module visibility, relative imports and inferred include roots.
This fixes missing Windows edges, rather than just making test labels portable.

## C++ grammar changes

The vendored 0.23.4 grammar carries explicit productions for:

- Qualified type arguments in macro calls, such as `GET_USERDATA(const InputManager, input)`.
- String literals adjacent to format macros, including a final `PRIu64`.
- Quoted and angle-bracket header arguments to `__has_include`.
- `typeid` with either a type or an expression.
- Pointer-member `->*` expressions and allocation of cv-qualified pointer arrays.
- SDK convention/export macros after return types and in parenthesized callback
  declarators, including `F_API`, `F_CALL` and `WINAPI`. Ordinary identifier
  interpretations are preferred when both readings are viable.
- Nested preprocessor branches in initializer lists, retaining alternative branch
  nodes and call spans. Ordinary list entries still require commas.
- Unnamed pointer parameters with defaults and class-scoped pointer fields,
  preserving field names and the existing global `::delete[]` parse.

No source preprocessing or byte rewriting is performed. Definition and reference
spans still refer to the original source. Calls through a pointer-to-member do
not invent an edge to the variable holding that pointer. Bare primitive macro
arguments are deliberately not accepted as general expression arguments: that
would reinterpret valid declarations as function calls.

Reproduce the generated C++ sources with tree-sitter CLI **0.25.10**, retaining ABI 14:

```sh
python scripts/patch_cpp_macro_grammar.py
cd grammars/tree-sitter-cpp
tree-sitter generate --abi 14 src/grammar.json
cd ../..
python scripts/patch_cpp_macro_grammar.py --finalize
cargo test -p vorpal-language --test grammar_provenance -- --ignored regenerate
```

The finalize step restores the existing ASCII lexer fast path. Review provenance
changes; only the C++ grammar's source digest should change. The grammar digest
invalidates cached C++ products on the next index build. Existing indexes must be
rebuilt before querying the newly recovered graph facts.

## Validation and remaining boundaries

On 2026-10-03, a read-only reparse of the 80 C++ files reported as damaged in the
Hades II baseline reduced files containing ERROR nodes from 80 to 40. It used the
repository's `*.h` -> C++ language mapping. `bindings.cc`, `callbacks.cc`, the
affected ECS systems, inspector metadata and profiling core now parse cleanly.
This is a subset audit, not a claim that every repository file has been rescanned
or that the running MCP process has been replaced.

The complete vendored grammar corpus retains its existing expected trees. Focused
tests cover parser recovery, source spans, direct versus indirect calls, suffix
selectors by name/id/external id, query scan and traversal plans, and the MCP
protocol. All 14 published resolver evaluations pass on native Windows, including
the eight cases that previously failed there. Grammar provenance uses portable
repository paths and can now be checked on both operating systems.

The SDK follow-up reduced that subset from 40 to 29 error-bearing files (636 ERROR
nodes, 537882 damaged bytes). All nine audited FMOD headers and both Windows
calling-convention cases now parse without ERROR nodes. The corpus and native
Windows parser regressions pass; invalid fragments still have syntax errors,
including missing-token errors reported by the parser. Header prototypes retain
the existing outline policy; parsing them cleanly does not automatically add
external function definitions to the graph.

The initializer follow-up parses the previously damaged audio backend cleanly and
reduces the subset to 28 error-bearing files (633 ERROR nodes, 537769 damaged
bytes). Tests cover nested `#if`/`#ifdef`/`#elif`/`#elifdef`/`#else`, both branch
calls and following calls, exact call spans, and missing ordinary list commas.

The declaration follow-up fixes `render.h`, `mock_render_device.h` and
`material_library.cc`, reducing the subset to 25 error-bearing files (624 ERROR
nodes, 537663 damaged bytes). Ten parser regressions and the unchanged corpus
pass. Pointer default parameters are added specifically, rather than accepting
every abstract declarator. Member scopes are supported for pointer fields;
globally extending abstract member-pointer types regresses global delete parsing
and remains withheld. Field names are verified in the extracted outline.

The abstract-function follow-up instead confines member-function pointer types
to explicit type descriptors. Both `const` and `noexcept` callback traits parse
cleanly, including `src/script/utils.h`; global `::delete[]` retains its original
parse. Eleven regressions verify source spans, following calls and invalid type
syntax. The unchanged corpus and native Windows checks pass. The audit subset
is now 24 error-bearing files (621 ERROR nodes, 537648 damaged bytes).

The parameter-annotation follow-up accepts SAL-style metadata only in parameter
contexts, retaining original identifier/argument spans and negative precedence
for ordinary declarations. Nested annotation arguments produce no runtime call
references; ordinary calls with the same spelling remain calls. `d3dx12.h` and
`pix3.h` now parse cleanly. The audit subset is 22 error-bearing files (403 ERROR
nodes, 532676 damaged bytes). Twelve parser regressions, the unchanged corpus,
Clippy, native Windows provenance/member calls/resolver evaluations and generated
schema validation pass. The new `sdk_parameter_annotation` kind is in the C++
rule schema. Conditional `else` structures and one pointer-return convention in
PIX headers remain for subsequent work.

Remaining C++ boundaries in that audit include macro statements without semicolons
and additional SDK annotations/conditional linkage blocks. Third-party C headers forced through the C++ grammar by `*.h` also
remain problematic. Some files are actually incomplete source fragments, such as
an orphan closing brace or a trailing `template <typename T>`; these should remain
reported as parse errors. PowerShell and Markdown grammars are unchanged.

To inspect a source file without updating an index:

```sh
cargo run -p vorpal-ingest --example parser_audit -- --cpp-headers path/to/file.h
```
