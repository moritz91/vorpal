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

The conditional-prefix follow-up models SDK `#if`/`#ifdef`/`#ifndef` groups
whose runtime `if`/`else` prefix ends directly before `#endif`, with the final
statement shared outside the directive. The exact boundary is required; ordinary
dangling `else` remains an error. `conditional_if_statement` retains separate
preprocessor and runtime condition fields and an `else_clause` spanning the
original directive boundary. Both bodies and following calls keep exact source
spans. All three PIXEvents headers now parse cleanly, reducing the subset to 19
error-bearing files (389 ERROR nodes, 532620 damaged bytes). Thirteen parser
regressions, the unchanged corpus, Clippy, native Windows checks and schema
validation pass. A broad pointer-return calling-convention extension remains
withheld: it changes ordinary template functions with array-reference parameters.
Their shape is retained in the regression fixture; corpus expectations are intact.

The pointer-convention follow-up requires a directly named function after an
SDK convention following the pointer star. It reuses `function_declarator` via
an alias and keeps recursive declarators on their original path. This avoids
the withheld draft's array-reference template regression. `pix3_win.h` is now
clean alongside the other four audited PIX headers. The subset is 18 error-bearing
files (388 ERROR nodes, 532614 damaged bytes). Fourteen parser regressions,
unchanged corpus, Clippy and native Windows checks pass. No new schema kinds
are introduced. Calling conventions on more elaborate declarator shapes still
need separate context-specific treatment.

The conditional-linkage follow-up admits independently guarded opening and
closing `extern "C"` braces, including nested guards as used by NanoSVG. Complete
opening and closing groups are required; unclosed linkage blocks remain errors.
`conditional_linkage_specification`, `conditional_linkage_open` and
`conditional_linkage_close` retain the original guard conditions, source spans
and contained declarations. The parser models their structure without evaluating
preprocessor conditions. Both NanoSVG headers now parse cleanly; `stb_image.h`
retains twelve small errors spanning 107 bytes instead of a large root error.
The audit subset is 16 error-bearing files (382 ERROR nodes, 147319 damaged bytes),
with original element counts retained in these headers. Fifteen parser
regressions, unchanged corpus, Clippy, native Windows checks and schema validation
pass. The new node kinds and opening/closing guard fields are in the C++ schema.

The macro-replacement follow-up supports multiple replacement-list fragments
separated by block comments in object-like and function-like definitions,
including LF and CRLF continuations. Existing `preproc_arg` and comment spans
remain intact; replacement-list calls and synthetic function text are opaque,
rather than becoming runtime graph facts. Sixteen parser regressions, unchanged
corpus, Clippy and native Windows checks pass, and MSVC accepts the continued
comment fixture. The audit remains at 16 error-bearing files, with 362 ERROR
nodes and 137703 damaged bytes. Catch2 drops from 124 to 107 ERROR nodes and
39883 to 30309 damaged bytes; `stb_image.h` drops from twelve to nine errors.
No new schema kinds are introduced.

The decltype-base follow-up is isolated from the withheld calling-convention
change: class base lists accept `decltype` in their explicit type context.
Catch2 improves by one ERROR node and 53 bytes, without the large root error
of the combined draft. Calls nested inside `decltype` operands are unevaluated
and do not become runtime graph edges; ordinary calls with the same name retain
exact spans. Seventeen parser regressions, unchanged corpus, Clippy, native
Windows checks, schema validation and an MSVC base-specifier fixture pass.
The subset remains 16 error-bearing files, with 361 ERROR nodes and 137650
damaged bytes. No new visible node kinds are introduced.

Semicolonless statement macros need definition/include evidence. The audited
`ASSERT` definitions in the two test-framework headers expand to a complete
`if` statement, whereas an ordinary same-named function must still require a
semicolon. Their recovery must also invalidate cached products when defining
headers change; macro argument calls retain their own source spans and macro
names must not invent runtime callees. The broader built-in calling-convention part of the combined draft
remains withheld because it enlarges the Catch2 error region, despite passing
isolated regressions and the unchanged corpus. Catch2 has ordinary CRLF bytes;
diagnosis must inspect bytes instead of newline-translating console output.

Proof-backed statement recovery is now available through an explicit extraction
API opt-in. Default extraction and CLI configuration remain unchanged. Set
`ExtractionEnv::cpp_macro_include_roots` to `Some(roots)` or construct an extractor
with `OutlineExtractor::with_cpp_macro_recovery`; an empty root list still allows
local quoted includes. No roots are inferred. Production CLI/configuration wiring
is still a follow-up, as is evidence for the uncertain Hades include environment.

`cpp_macro_evidence::audit_with_roots` records complete function-like `if`, `try`
and compound-statement replacements, original definition spans, parameter counts
and ordered active intervals. Quoted includes search locally before explicit
ordered roots; angle includes search only those roots. Missing earlier candidates
are dependencies, so newly created local headers invalidate an earlier SDK hit.
Present unreadable candidates stop resolution. File symlinks conservatively end
proof because canonicalization can change their quoted-include directory.

Redefinitions, undef, unknown directives, conditional groups, unresolved includes,
cycles and depth/file/byte limits end evidence. Expression/function-definition,
variadic and duplicate-parameter macros, stringification/pasting and comments
between the macro name and opening parenthesis remain unsupported. The audit does
not evaluate conditional branches or implement a full C++ preprocessor. Hades
binary_serialization_tests.cc still has no active ASSERT binding after uncertain
transitive includes, even with tests/src roots; no actual Hades recovery is claimed.

The scanner has no public byte offset. Every invocation of an enabled name is
independently checked against its definition interval and valid argument syntax;
preprocessor parenthesis rules determine its parameter count. Any unproven use
rejects that name for the whole source. ASCII identifiers and whitespace agree
between Rust and C. The borrowed context is thread-local, synchronous, nested and
panic-safe, and never serialized in trees. An empty context keeps ordinary parsing.
`macro_statement` preserves original name/argument spans without inventing a call
edge to the macro name; argument calls and following functions are retained.

Owned and streaming extraction share one proof-backed parse. Scan-root handoff
reparses under the configured proof rather than banking a context-free tree.
Recovered C++ parses bypass incremental tree/walk caches and drain pending reuse
state: those caches have no dependency identity yet. Product identity includes
versioned recovery configuration, ordered roots, consulted header bytes and missing
candidates. Replay recomputes dependencies from current source/header bytes using
the same extractor. Whole-tree stat reuse and stamp-only/scoped compose shortcuts
are disabled under this opt-in; unrelated products can still replay through the
per-file identity gate. Custom environments bypass default-only live overlay lanes.
Callers outside the index pipeline must use the extractor's dependency-aware
`extraction_identity_for_path`, rather than the free grammar/rules helper.

Seven recovery tests on Windows/Linux cover spans and arguments, wrong arity,
genuine syntax errors, parser/thread reuse, nested/panic restoration, header edits
and identical owned/streamed/scan-handoff products. Ten evidence tests pass on both
platforms, plus a Unix symlink regression. An index regression checks external
header edit/removal, local shadow creation/removal, warm product replay, hinted live
builds and scratch/incremental generation equality in bucketed and flat formats.
Seventeen parser regressions, unchanged corpus, Clippy, native Windows provenance,
MSVC fixtures, resolver evaluations and existing live/cutoff/replay tests pass.
The original 80-file default-parser audit remains 16 error-bearing files, 361 ERROR
nodes and 137650 affected bytes.

Inspect evidence or a recovered tree report without writing an index:

```sh
cargo run -p vorpal-ingest --example macro_audit -- --include-root path/to/includes path/to/file.cc
cargo run -p vorpal-ingest --example macro_audit -- --recover --include-root path/to/includes path/to/file.cc
```

Remaining C++ boundaries in that audit include macro statements without semicolons
and additional SDK annotations/conditional linkage blocks. Third-party C headers forced through the C++ grammar by `*.h` also
remain problematic. Some files are actually incomplete source fragments, such as
an orphan closing brace or a trailing `template <typename T>`; these should remain
reported as parse errors. PowerShell and Markdown grammars are unchanged.

To inspect a source file without updating an index:

```sh
cargo run -p vorpal-ingest --example parser_audit -- --cpp-headers path/to/file.h
```
