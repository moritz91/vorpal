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
exact spans. Eighteen parser regressions, unchanged corpus, Clippy, native
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
API or project-configuration opt-in. Default extraction remains unchanged. Set
`ExtractionEnv::cpp_macro_include_roots` to `Some(roots)` or construct an extractor
with `OutlineExtractor::with_cpp_macro_recovery`; an empty root list still allows
local quoted includes. No roots are inferred. For both `vorpal index` and `vorpal
mcp`, a project configuration can opt in without custom language declarations:

```yaml
ruleDirs: []
cppMacroIncludeRoots:
  - tests
  - src
```

Roots retain their order; relative paths use the configuration file's directory,
including an external file supplied with `--config`. `cppMacroIncludeRoots: []`
enables local quoted includes, whereas an absent key keeps recovery disabled.
Multi-project MCP loading carries each project's own roots. Include proof still
declines unknown headers and uncertain definitions; configuring roots does not
assert that their macros are safe or evaluate preprocessor conditions. Evidence
for the uncertain Hades include environment remains a follow-up.

An opt-in MCP daemon performs rebuilds with its retained extraction environment
in process. The existing child-indexer protocol cannot transport this exact proof
configuration, and rediscovering a config under the source directory loses external
`--config` inputs. This bypass applies to explicit, background and reconcile
rebuilds. Default extraction retains child supervision. CLI/MCP regression tests
check roots relative to an external config, empty versus absent roots, explicit
MCP rebuilds and changed external headers that must restore real syntax errors.

Native Windows pack-root inference now recognizes both path separators without
rewriting stored keys or matching ambiguous filename suffixes. Previously a native
Windows index could correctly count parse errors during build while `health` skipped
their unavailable products and reported clean. Health now refuses missing/invalid
products rather than treating uninspected files as healthy. A cross-platform regression
uses clean/damaged files with identical basenames and a damaged product pack.

`cpp_macro_evidence::audit_with_roots` records complete function-like `if`, `try`
and compound-statement replacements, original definition spans, parameter counts
and ordered active intervals. Quoted includes search locally before explicit
ordered roots; angle includes search only those roots. Missing earlier candidates
are dependencies, so newly created local headers invalidate an earlier SDK hit.
Present unreadable candidates stop resolution. Source/header file symlinks,
ancestor-directory symlinks and Windows reparse points (including junctions)
conservatively end proof: canonicalization can choose a different quoted header
than the compiler. Include-root spelling is retained until candidate components
are checked. Opt-in full/live builds reject redirected source roots before the
shared canonicalizer erases their spelling; default indexing retains its existing
root behavior. Use compiler-consistent physical paths for proof-backed recovery.

Redefinitions, undef, unknown directives, unresolved includes, cycles and
depth/file/byte limits end evidence. Opaque include/directive effects also prevent
later definitions from restarting proof: unseen replacement helpers can restore
saved macros or redirect future includes. This boundary also applies to opaque
directives in otherwise admitted conditional groups. Earlier intervals retain their original
boundaries, and the whole-name scanner gate still rejects any later unproved use.
Well-formed #ifdef/#ifndef groups and #if/#elif
conditions composed only of 0, 1, defined operands, parentheses and !/&&/|| preserve
only entering definitions that remain unchanged across every possible branch:
all branch includes are consulted and tracked, and a possible redefine/undef
rejects that name. No definitions originating inside those groups are promoted.
An unconditionally reached top-level `#pragma once` prevents repeated header
execution, including guarded self-includes. Possible visits through unknown
branches are tracked separately: a later include may execute or may be skipped,
so it cannot introduce new proof or mark nested headers as definitely visited.
A conditional pragma is never treated as unconditional. Unguarded cycles still
end proof. Header bytes and missing search candidates remain dependencies;
removing/reinstating `#pragma once` invalidates warm products and normal MCP
rebuilds. Evidence identity v8 includes path-component/opaque-effect guards and rejects
opaque pragma effects. Literal `_Pragma` or
`__pragma` operators in a translation unit or consulted replacement lists disable
its recovery, including operators joined by continuation lines. Replacements and
invocation arguments that reference other observed macros are also declined:
this audit does not prove their expanded token shape. The observed-name set
includes empty replacements, keyword-like names and all possible branches; undef
does not erase that conservative boundary. Literal/comment contents remain inert,
and continuation joining is inspected only for evidence, never to replace source
or spans. Header edits introducing/removing such expansions invalidate recovered
products and normal configured MCP rebuilds. Invoked token-pasting
macros (`##` or `%:%:`) and their transitive replacement wrappers also disable
recovery: they can manufacture a pragma operator that restores hidden saved
definitions. This deliberately declines the entire proof rather than claiming to
expand those macros. Comments, string contents and unused paste definitions do not
supply effects. Header edits introducing/removing these effects invalidate warm
products; regression checks compare the rebuilt graph with a scratch build.

Bare identifiers, macro calls, arithmetic and other unknown #if expressions,
malformed groups and unknown directives still end proof;
conditions are never evaluated. The evidence identity is versioned to invalidate
products built under the previous proof rules. Regression tests cover retained
spans/calls, branch header edit/removal, genuine errors and normal configured MCP
rebuilds on Windows/Linux; warm product replay passes in bucketed and flat layouts.
MSVC accepts the fixture with both defined and undefined platform symbols.
Expression/function-definition,
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

Fourteen recovery tests on Windows/Linux cover spans and arguments, wrong arity,
genuine syntax errors, parser/thread reuse, nested/panic restoration, header edits,
opaque effects and identical owned/streamed/scan-handoff products. Twenty-one
evidence tests pass on Windows and twenty-two on Linux, including directory
redirects and the Unix file-symlink case. Five index regressions check external
header edit/removal, local shadow creation/removal, warm product replay, hinted
live builds, opaque effects, directory redirects and scratch/incremental generation
equality in bucketed and flat formats. Three configured CLI/MCP tests pass on both
platforms. Twenty parser regressions, unchanged corpus, Clippy, native Windows
provenance, MSVC fixtures, resolver evaluations and existing live/cutoff/replay
checks pass. The historical subset audit is recorded below; it must not be confused
with installed MCP health.

Inspect evidence or a recovered tree report without writing an index:

```sh
cargo run -p vorpal-ingest --example macro_audit -- --include-root path/to/includes path/to/file.cc
cargo run -p vorpal-ingest --example macro_audit -- --recover --include-root path/to/includes path/to/file.cc
```

Remaining C++ boundaries in that audit include macro statements without semicolons
and additional SDK annotations/conditional linkage blocks. Third-party C headers forced through the C++ grammar by `*.h` also
remain problematic. Some files are actually incomplete source fragments, such as
an orphan closing brace or a trailing `template <typename T>`; these should remain
reported as parse errors. PowerShell numeric size multipliers now accept the
native parser's case-insensitive spellings, including integer, hex and real
literals. Original literal spans and malformed-expression errors are retained.
The unchanged 139-test PowerShell corpus and Windows/Linux extraction regression
pass. The audited run_per_view_marker_soak.ps1 and
run_runtime_split_screen_live_test.ps1 are now parse-clean. Reproduction uses
scripts/patch_powershell_grammar.py and tree-sitter 0.25.10 generate --abi 15
src/grammar.json in the PowerShell grammar directory. Markdown is unchanged.
Instance-method invocation precedence also preserves complete calls in command
arguments, such as $task.GetAwaiter().GetResult(), rather than splitting the first
argument list away from the member expression. Static invocations are unchanged.
Native PowerShell AST comparison, Windows/Linux span/error regressions and the
unchanged corpus pass. run_editor_split_screen_trace_test.ps1 and
run_save_slot_cold_start_test.ps1 now parse clean too. Native command arguments now
retain standalone whitespace-delimited -- separators, --name=value tokens with
literal or simple variable values, and comma-separated bare-word arrays. The
external scanner validates variable prefixes through their complete lookahead;
incremental edits match a fresh parse. Variable child spans remain original,
double commas and incomplete arguments remain errors, and decrement expressions
are unchanged. All eleven originally damaged PowerShell files now parse clean in
the read-only source audit. Five Windows/Linux extraction regressions, the
unchanged 139-test corpus, native provenance and Clippy pass. These are subset
audit counts, not a new health report from the installed MCP generation.

SDK conventions are also accepted before inline-member declarators, using the
separate inline_method_definition rule. General field/declaration modifiers are
unchanged. Inline methods and constructors are retained as members in extraction,
with their original bodies, call spans, symbol kinds and visibility. A typed
receiver resolves to the inline SDK method in the index regression. The current
text_shaper.cc is parse-clean (previously 20 ERROR nodes / 1266 bytes); Catch2 damage
is unchanged. Eighteen parser regressions, evidence/recovery and replay tests,
Windows/Linux resolver evaluations, native MSVC syntax checking, native provenance,
Clippy and the unchanged 179-test C++ corpus pass. No new visible grammar kinds.
Explicit template instantiations retain a parallel annotation-free parameter
path for unnamed function pointers returning a user-defined type. The original
SDK-annotated declarator path remains available; ordinary parameter and expression
rules are unchanged. Existing function/parameter kinds are aliased, with no new
visible kinds or schema changes. Qualified instantiations retain their types and
spans without inventing runtime calls; genuine following calls remain references.
On identical current test_mocks.h bytes, the previous parser reports 70 ERROR nodes
and 2196 affected bytes, while the new parser reports zero and retains 32 items.
Nineteen C++ parser regressions, evidence/recovery, configured MCP, replay, member
calls, resolver evaluations, native provenance, MSVC and the unchanged corpus
validate this path. The original subset excludes this header and is unchanged.

Primitive type arguments after a comma (for example va_arg(list, int)) retain
explicit type nodes. The first argument slot stays unchanged, preserving ordinary
parenthesized function/member declarations and functional conversions. A C++ call
with a direct explicit type argument is metadata: its callee is suppressed, while
calls in value arguments and enclosing runtime calls retain original spans. An
ordinary same-named function call remains a runtime reference, including after
#undef. The graph regression prevents linking metadata to that function's body.
On identical source, stb_image_write.h drops from five ERROR nodes / 33 bytes to
two / 18; its remaining native-convention declarations are still reported. Catch2
and other original audit paths are unchanged. Twenty parser regressions, three
C++ graph regressions, Windows/Linux proof/replay/configured-MCP checks, MSVC,
Clippy and the unchanged corpus pass. No new visible kinds or schema changes.

The historical 80-source audit now has only 79 paths present: script_system.cc was
removed outside this work, and ui_tests.cc grew. On identical current ui_tests.cc
bytes the baseline and new parser both report 179 ERROR nodes / 106951 bytes;
current original-subset totals are 16 files / 363 nodes / 137675 bytes. These source
changes must not be presented as a parser regression or as installed MCP health.

To inspect a source file without updating an index:

```sh
cargo run -p vorpal-ingest --example parser_audit -- --cpp-headers path/to/file.h
```

Parse-health telemetry now counts both ERROR and zero-length MISSING tokens.
Missing tokens keep their original insertion positions without invented affected
bytes; strict zero-threshold Fail/Exclude policies include them on fresh and
replayed products. Positive thresholds remain affected-byte ratios. Product
format generation 23 re-keys earlier products and the whole-tree fast path, so
missing-only files previously reported as clean cannot replay that diagnosis.
Historical ERROR-only audit totals above do not count MISSING tokens.

Braced parameter defaults (`Options value = {}` / `{value()}`) now retain an
initializer_list in that explicit context instead of inventing a missing type.
Named typed defaults and ordinary call contexts are unchanged. Across all 73
C++ paths reported by the v23 full health audit, this removes 220 MISSING tokens
and makes 46 paths clean with no worsened counts; affected bytes are unchanged.
Catch2, PIX array references and actual semicolonless ASSERT recovery remain
open. The unchanged 179-test corpus, LF/CRLF parser/span regressions, native MSVC,
proof/replay/configured-MCP checks and native provenance validate the extension.

The broader read-only audit checks all 900 C++ paths from the source manifest:
27 retain syntax errors, and no previously clean C++ path gains errors.
This per-parser audit is not a newly installed MCP index or a macro-recovery claim.

PowerShell command parameter names now admit digits after their initial name
character, retaining full spans for `-Sha256` and `-Port18765`. Leading numeric
arguments such as `-256` and decrement expressions keep their existing parsing.
All 35 PowerShell paths in the read-only source manifest are now parse-clean,
including the six missing-only paths exposed by generation 23 telemetry. Native
PowerShell parsing, six Windows/Linux extraction regressions, the unchanged
139-test corpus, Clippy and native provenance validate this narrow lexical fix.
There are no new visible kinds; the installed MCP release remains unchanged.

Directly named pointer-return functions retain a parallel annotation-free
parameter path, reusing the explicit-instantiation parameter productions. This
keeps `Word (&buffer)[size]` as an array/reference declarator rather than an SDK
annotation followed by an invented missing type. SDK-annotated parameters retain
their existing path. General declarators and expression contexts are unchanged;
a broader trial changed parameter-pack interpretation and remains withheld.
PIXEventsLegacy.h loses its two MISSING tokens and retains all 30 extracted
entries. Across the same 900 C++ paths, no other error/byte/item counts change:
26 paths retain 1802 ERROR/MISSING nodes and 137675 affected bytes. The unchanged
179-test corpus, 23 Windows/Linux parser regressions, proof/recovery, configured
MCP, replay in both layouts, resolver/member-call tests, MSVC, Clippy, schema and
native provenance validate this extension. Actual Hades ASSERT recovery remains
unproven; the production MCP installation is unchanged.

The typed function declaration/definition alternative additionally requires a
named parenthesized array-reference parameter. This explicit context avoids an
unrestricted plain-function trial that enlarged Catch2 error spans. Ellipsis is
allowed only at the end of this alternative; MSVC rejects the negative fixture
with ellipsis before the array parameter. General declarator/expression rules
stay unchanged. On the same 900 C++ paths, layered_media.cc becomes parse-clean
and regains its missing function entry; other error/byte/item counts are unchanged.
25 paths retain 1801 ERROR/MISSING nodes and 137675 affected bytes. The unchanged
corpus, 24 Windows/Linux parser regressions, proof/recovery, replay, configured
MCP, member calls, resolver evaluations, schema, provenance and Clippy verify the
path. Broader trials remain withheld.

A completed external whole-index verification at 8c11c24 reports 29 of 1735 paths
with 1878 ERROR/MISSING nodes and 138026 affected bytes through normal MCP health.
That test index does not replace the installed .2 generation, and the metric must
not be compared directly with historical ERROR-only health coverage.

The subsequent verified whole-index build at cf2d722 reports 28 of 1735 paths,
1877 ERROR/MISSING nodes and 138026 damaged bytes through normal MCP health.
It contains 63846 graph nodes, including the restored GatherShadowCascades
function. The initial local build reused an older Cargo CLI executable; verify
the compiled grammar surface before whole-index measurements after regeneration.

Native DLL function declarations have a separate alternative requiring both
an explicit __declspec modifier and a native calling-convention keyword. It
reuses declaration/function kinds in top-level/block contexts, with annotation
metadata preserved. General declaration modifiers remain unchanged. This makes
stb_image_write.h clean and removes two __stdcall errors from stb_image.h.
The 900-path native audit retains every prior item count and otherwise identical
error spans: 24 C++ paths, 1797 ERROR/MISSING nodes, 137639 affected bytes.
Catch2 is unchanged. The 179-test corpus, 25 Windows/Linux parser regressions,
proof/recovery, configured MCP, replay in both layouts, member-call/resolver
tests, native MSVC, Clippy, schema and C++-only provenance checks pass.

Proof-backed invocation spacing now accepts ordinary block/line comments between
the name and opening parenthesis. Name, comment and argument spans remain original;
comments do not become part of the identifier token. The audit and scanner both
use all six C++ ASCII whitespace characters, including vertical tab. The previous
Rust/C difference could miss an out-of-interval invocation while enabling its
name from a later valid use. Comment continuations/trigraphs and unterminated
comments are declined. Missing semicolons, wrong arity and unknown macros retain
errors. Owned, streaming and scan handoff agree; normal configured MCP rebuilds
and header invalidation exercise commented invocations too.

Recovered product identity is v2: scanner-only changes do not necessarily change
the grammar's structural fingerprint. A migration regression seeds a v1
false-clean loose product for an earlier undefined invocation, verifies a fresh
parse exposes its error, then verifies packed replay and scratch convergence.
Its negative control fails with the old v1 identity. Six replay tests pass on
Windows/Linux and Linux flat layout; sixteen recovery tests and the unchanged
evidence/parser suites pass, with native MSVC and Clippy validation.

The latest external candidate index, including the native DLL and invocation
spacing fixes, reports 27 of 1735 paths, 1873 ERROR/MISSING nodes and 137990
affected bytes through normal MCP health. The installed .2 runtime is unchanged;
actual Hades ASSERT recovery after opaque includes remains unproven.

Static/extern variable declarations can retain one complete guarded storage
modifier group without widening general declaration modifiers or selecting a
preprocessor branch. The new conditional_storage_modifier kind preserves its
condition and full original span; the C++ rule schema includes that kind.
Preprocessor condition calls are declaration metadata, while calls in guarded
runtime bodies and ordinary same-named calls retain their original spans.
LF/CRLF regressions cover both paths and genuine malformed declarations.
The 900-path native audit changes only stb_image.h: 33 to 30 ERROR/MISSING
nodes and 47 to 35 affected bytes, with all 326 items retained. Totals are
24 C++ paths, 1794 ERROR/MISSING nodes and 137627 bytes; Catch2 and every other
error/byte/item count stay unchanged. The 179-test corpus, 27 Windows/Linux
parser regressions, proof/recovery, configured MCP, replay in both layouts,
member-call/resolver tests, native MSVC, Clippy, schema and C++ provenance pass.

The verified external candidate including guarded storage declarations reports
27 of 1735 paths, 1870 ERROR/MISSING nodes and 137978 affected bytes through
normal MCP health. Preprocessor-condition calls are absent from runtime edges;
63846 graph nodes are retained. This is a scratch index, not the installed .2.

Abstract member-data pointers now use the existing named-scope production only
in explicit type_descriptor contexts. Their visible declarator kind is reused;
the global abstract-pointer rule remains unchanged. LF/CRLF tests retain alias
and template-specialization spans, scope names, following calls and ::delete[].
The 900-path native audit changes only Catch2: 138 to 137 ERROR/MISSING nodes,
30256 to 30251 bytes, with all 2317 extracted items retained. Totals are 24 paths,
1793 nodes and 137622 bytes. The unchanged 179-test corpus, 28 Windows/Linux
parser regressions, proof/recovery, configured MCP, replay in both layouts,
member-call/resolver tests, native MSVC, Clippy, schema and provenance pass.

Comma expressions are admitted only as complete unevaluated decltype operands.
They retain original spans and the existing exclusion of operand callees from
runtime edges; ordinary same-named calls still retain their original spans.
The 900-path native audit changes only Catch2: 137 to 136 ERROR/MISSING nodes
and 30251 to 30206 bytes, retaining all 2317 items. Totals are 24 C++ paths,
1792 nodes and 137577 affected bytes. All 29 Windows/Linux parser regressions,
the unchanged 179-test corpus, proof/recovery, configured and running MCP,
replay in both layouts, member/resolver tests, native MSVC, Clippy, schema and
provenance checks pass. No new visible kinds or schema changes are introduced.

The verified a4a615c external candidate, before the comma-operand extension,
reports 27 of 1735 paths, 1869 ERROR/MISSING nodes and 137973 affected bytes
through normal MCP health, with 63846 graph nodes retained. The installed .2
runtime remains unchanged.

Watched MCP servers with configured C++ macro recovery also revalidate the
exact consulted source/header inputs before answering quiet queries. External
header edits, removal/recreation, ignored local shadows and redirected paths
cannot rely on source-watch events. Content fingerprints detect same-length
edits with restored timestamps. Extraction and warm replay publish observations
from their actual audits; mixed header contents within one build require a new
epoch. Built-in and configured virtual canaries publish no filesystem inputs.
These observations are ephemeral, contain no macro bindings or parser context,
and do not change product identity or enable recovered-tree caching.

A changed input requests normal revalidation with an uncertain change set;
prior build workers are drained before resetting observations. A mutation still
visible after revalidation returns a retry error instead of certifying a stale
graph. Quiet queries hash the observed inputs but do not reparse unchanged
files. Explicit external-index servers without a source watcher retain their
manual rebuild semantics. Recovery remains opt-in; the installed .2 tunnel
and its external index are unchanged.

The native x86 cpuid helper can retain its explicit brace-delimited MSVC mov
and cpuid instructions. This is a narrow instruction grammar, not an opaque
assembly-body token: named destinations, decimal/identifier sources, opcode
and operand spans are preserved. Immediate operand/comma tokens and horizontal
spacing prevent a missing operand from absorbing the following instruction.
Other assembly forms remain unsupported. The ms_asm_statement and
ms_asm_instruction kinds and their fields are synchronized in the C++ schema.

LF/CRLF tests retain ordinary same-named calls and following functions, reject
cross-line/missing operands and genuine missing semicolons. Native MSVC x86
accepts the positive fixture and rejects the missing-operand negative fixture.
All 30 Windows/Linux parser regressions, unchanged 179-test corpus,
proof/recovery, configured/running MCP, replay in both layouts, member/resolver,
Clippy, provenance and schema checks pass. The native 900-path audit changes
only stb_image.h: 30 to 27 ERROR/MISSING nodes and 35 to 7 bytes, retaining all
326 items. Totals are 24 C++ paths, 1789 nodes and 137549 affected bytes.

Recovery also proves actual arguments in their replacement context, not just
their standalone argument-list syntax and preprocessing arity. An ephemeral
shared template substitutes parameter leaf tokens only in a separate proof
fixture; source bytes, extracted trees and their spans are never rewritten.
Literal/comment/number tokens are protected. Nested parentheses, rather than
template/bracket/brace nesting, determine preprocessing argument commas.
The instantiated replacement must remain exactly one complete statement;
unproven macro-specific grammar extensions do not establish native syntax.
Observed further expansions, ambiguous legacy-MSVC token boundaries and
replacement amplification above 4 MiB decline recovery. One bad invocation
still disables the entire macro name. These are syntax proofs, not C++ type
or control-flow checks.

Product identity v3 invalidates older arity-only false-clean products. LF/CRLF
negative controls cover declaration, goto, assembly and string contexts through
owned, streaming and scan-root extraction. Valid contexts preserve argument-call
spans. Loose-to-packed replay migrations cover both v1 and v2 products; native
MSVC x86 accepts the positive fixture and rejects malformed declaration syntax.
The isolated ac59c50 candidate index verified 1735 files; normal MCP health
reported 27 error-bearing files, 1865 ERROR/MISSING nodes and 137900 affected
bytes. This predates the v3 proof change and is not a deployed generation.

Explicit member operator calls now use the existing operator_name through dot
or arrow field access, alongside existing template methods. Pointer-to-member
operators retain their prior path. Calls retain the complete operator spelling,
receiver evidence and original argument spans. Only exact angle-bearing operator
names receive out-of-class body aliases; ordinary template-specialization names
remain excluded. Typed receivers select their own class body; unknown receivers
do not acquire edges to either candidate class.

All 31 Windows/Linux parser regressions and the unchanged 179-test corpus pass.
Four member-call graph tests and all 14 resolver evaluations retain conservative
edges in bucketed and flat layouts. Native MSVC accepts the operator fixture;
proof/recovery, replay, running/configured MCP, Clippy, provenance, reproduction
and schema checks pass. The native 900-file audit changes only Catch2: 136 to
135 ERROR/MISSING nodes and 30206 to 30195 bytes, retaining all 2317 items.
Totals are 24 C++ paths, 1788 nodes and 137538 affected bytes.

Macro proof identifiers are restricted to canonical ASCII names. Unicode/UCN
or dollar macro names are opaque effect boundaries; noncanonical parameters
and UCN identifier leaves do not enter replacement templates. This avoids
byte-spelling assumptions across preprocessors: the UCN/UTF-8 parameter fixture
is rejected after substitution by Clang but accepted differently by MSVC.
Unicode literal/comment contents and ordinary Unicode argument calls retain
their original spans. MSVC accepts that positive fixture and rejects an actual
invalid expansion through a Unicode macro name. Evidence identity v9 and
product identity v4 invalidate the prior proof policy. Migration tests cover
v1/v2/v3 false-clean products. All 23 evidence and 19 recovery tests pass on
Windows/Linux, with LF/CRLF production-path parity and warm replay checks.

Statement proofs admit only the synthetic outer function definition. Nested
function definitions and local class methods in replacement bodies decline
proof; the permissive C++ grammar alone cannot certify them as statements.
Lambdas remain supported and argument calls keep their original spans. Native
MSVC rejects the nested-function negative fixture with C2601 and accepts the
lambda positive fixture. Evidence v10 and product v5 invalidate prior proofs;
migration coverage includes v4 false-clean products. All 20 recovery tests
retain LF/CRLF extraction-path parity.

Split conditional if scopes now require a complete opener guard with both
branches ending in `if (condition) {`, a shared body, and a complete closer
guard with both branches ending in `} else { ... }`. Only declarations and
expression statements precede the opening if; only expression cleanup precedes
the closing brace. Guard conditions, runtime conditions, branch statements and
original spans remain visible. No branch condition is evaluated. The preproc_*
guard kinds intentionally remain opaque to macro evidence. Both MSVC platform
variants pass; missing semicolons/braces/else/endif stay errors. The unchanged
179-test corpus passes, alongside 32 parser, 20 recovery and 23 Windows/24 Linux
evidence tests, replay/member-call graph tests, normal configured MCP/CLI tests,
Clippy, schema, provenance and idempotent reproduction.
The native 900-file extraction audit changes only interface.cc from one
MISSING node to zero, retaining all 59 items. Totals: 23 damaged C++ files,
1787 ERROR/MISSING nodes and 137538 affected bytes. Owned, streaming and
scan-root handoff encode identically for the LF/CRLF guard fixture.

The read-only `cpp_directive_audit::audit` and `macro_audit --directives` inventory
logical-line directive spans independently of unexpanded C++ declarations.
This inventory neither validates guard groups nor supplies production macro
bindings. It preserves original UTF-8/BOM and LF/CRLF offsets, hides directives
inside literals/comments, retains directive continuations and multiline block
comments, and declines unmodeled token joins, digraphs/trigraphs and unterminated
lexical constructs. Four Windows/Linux tests cover the boundaries; native MSVC
validates the comment/literal fixtures (Clang also accepts the block-comment
continuation). The SDK iostream inventory has 21 spans and test_defines.h six.
The first actual proof boundary is still iostream; adding all available Windows
SDK roots produces zero bindings and 43 consulted paths. No actual Hades ASSERT
recovery is claimed. Independent guard/effect interpretation remains unfinished.

The exact 5fc2df5 external candidate passes index verification and normal MCP
health with 26 damaged files of 1735, 137889 affected bytes. Both its full CI and
9cc14a0's full CI are green. The productive release/tunnel remain unchanged.
The independent directive inventory also matches complete nested textual guard
groups with original header/body/close spans. Duplicate else, elif after else,
unmatched branches and absent endif fail closed. This checks delimiter order,
not condition syntax, macro effects or compiler configuration; it still supplies
no production evidence. Five Windows/Linux tests cover the diagnostic API.

Literal `push_macro("NAME")` / `pop_macro("NAME")` pragmas now conservatively
invalidate only that canonical ASCII name. The audit never restores a saved
definition and prevents later proof restarts for that target; earlier entering
intervals remain intact. Target names are globally potentially expanding, so
arguments or replacement bodies referencing them cannot become false-clean.
Conditional stack metadata is inspected in every branch, including its leaf
preproc_directive metadata. Dynamic/escaped/noncanonical arguments and unknown
pragmas remain opaque. Native MSVC and Clang accept the unrelated-target positive
fixture. Noncanonical undef names decline proof: Clang rejects `#undef
123invalid`, while conforming MSVC warns and proceeds. Product v6/evidence v11
invalidate prior policy; migration tests include v5 telemetry. Header target
edits reparse warm products, and restoration equals scratch in bucketed/flat
layouts. All 32 parser, 21 recovery, 24 Windows/25 Linux evidence tests plus
configured normal MCP/CLI tests pass. The diagnostic guard inventory now consumes
complete directive identifiers and declines uncanonicalized keyword spellings.

Unused pragma-operator replacement lists now have no immediate proof effect.
Their names enter the opaque replacement set alongside token-pasting definitions;
transitive object/function wrappers are closed over before checking ordinary
source tokens in every observed branch/header. Direct operators and invoked
wrappers still decline all recovery. Nothing expands or executes these operators.
Product v7/evidence v12 invalidate the previous policy. Windows/Linux tests cover
unused definitions, aliases, conditional invocations, use after undef, original
argument spans and owned/streaming/scan parity. Warm replay migrates prior v6
telemetry and invalidates external-header wrapper invocations; watched normal MCP
and externally configured CLI tests exercise the same positive and negative cases.
All 32 parser, 22 recovery, 25 Windows/26 Linux evidence and eight replay tests
pass, as do the unchanged 179-test C++ corpus, Clippy and native MSVC/Clang positive
fixture. The actual Hades proof still has zero bindings and 43 consulted paths.
The exact 0615194 candidate separately verifies 1744 paths and reports 26 damaged
files / 152576 bytes through normal MCP. Its source checkout differs from the
earlier 1735-path candidate, so these byte counts are not a parser-regression
comparison. Its full CI is green; no candidate is installed in the live tunnel.

Proven statement macros retained as call_expression nodes now receive original-
span diagnostics when an independent bounded replacement/context fixture proves
the replacement incompatible with its statement, expression or qualified/member
callee slot. Other
observed macro tokens in the surrounding statement decline this diagnosis;
enclosing macros and keyword replacements may change the slot. GNU statement
expressions with enclosing parentheses remain syntactically supported. Existing
surrounding syntax errors are retained without attributing them to the macro.
These temporary fixtures do not replace source or trees and do not type-check
C++. One independently incompatible
use disables that scanner name for the whole translation unit. The tree and
source remain unchanged; ephemeral context diagnostics are counted alongside
ERROR/MISSING nodes in production health/Fail/Exclude policies. Audit reports
expose context_errors explicitly. Proven macro callees create no runtime refs,
including unrecovered ordinary call nodes; real argument calls retain spans.
Ordinary same-named calls after undef remain calls. This is a bounded call-
expression check, not general preprocessing or declaration-macro validation.
The independent context check also covers unrecovered calls followed by a
semicolon: invalid instantiated declaration/goto/if syntax must not become clean
merely because the unexpanded invocation is a valid C++ call statement. One empty
preprocessing argument in F() is distinguished from the zero arguments of a
zero-parameter macro. Whitespace/comment-only arguments keep original spans and
are accepted only when their instantiated replacement is a complete statement.
Multiple or mixed empty comma-separated arguments remain conservatively unsupported.
Scanner-proven statements are also checked against declaration scope. Direct
namespace/linkage compound replacements are incompatible; nested function and
lambda bodies remain valid. Unknown surrounding macro tokens decline this
diagnostic. Other scanner-proven complete statements cannot manufacture an
enclosing function, so multiple invalid uses are checked independently. The
same original-span diagnoses reach watched MCP with background rebuilds on/off.
Product identity v14 invalidates old false-clean telemetry, invented macro
callee rows and v8 false diagnostics for valid enclosing macro expansions;
migration includes v7/v8/v9/v10, with both false errors and false-clean old products.
All 29 recovery, 32 parser, 28 Windows/29 Linux
evidence, thirteen replay, three watched MCP and four configured CLI tests pass on
both platforms (Linux replay also flat). Native MSVC/Clang reject the return-slot
fixture and accept the statement/undef/ordinary-call positive control. Header-only
changes revalidate these diagnostics in live MCP; strict health policies retain
them through packed replay. Changed-target Clippy passes; optional index backends
and unrelated Windows-only all-test warnings are outside the native check scope.
The exact 40dd0d6 candidate verifies 1744 paths and reports 26 damaged files /
152576 bytes, identical to the preceding 0615194 candidate on this source state.
Its full CI is green. No candidate is installed; SDK include proof still remains
conservative, with zero actual Hades ASSERT bindings in the audited test source.
The exact 83be959 candidate separately verifies 1744 paths and reports 26 damaged
files / 152576 bytes through normal MCP; its full CI is green. MSVC/Clang accept
the empty-argument and nested function/lambda controls and reject the direct
namespace statement fixture. No candidate has been installed in the live tunnel.

Entering macro evidence can survive C++ errors caused by unexpanded SDK
declaration macros inside an otherwise intact conditional group. The independent
original-span directive/group inventory must be complete, and every directive's
kind, metadata field text and relative ranges must exactly match an independently
parsed directive-head fixture. Nested guards must have nonexpanding conditions;
extra C++ on a directive line, missing/repeated delimiters and metadata errors
decline proof. Every possible branch still contributes effects; no branch is
selected and no new binding is established from a conditional definition.
Unknown includes, undef/redefinition and opaque pragma effects retain their
existing boundaries. Original header C++ errors remain visible in extraction;
no body/source is masked and no temporary proof tree is banked.
Evidence v13/product v12 invalidate old conservative products as well. A frozen
v11 product migrates to the new proof, while external condition/effect edits
force replay and watched-MCP revalidation with background rebuilding on/off.
LF/CRLF metadata, nested-group and malformed-directive controls pass; owned,
streaming and scan-root extraction agree. Native MSVC and Clang accept both
defined/undefined variants of the guarded SDK declaration fixture. The actual
Hades test still has zero bindings and 43 consulted include candidates with all
available Windows SDK roots. This metadata proof does not admit its expanding
conditions or opaque SDK effects.

Literal MSVC/Clang pack and warning controls can preserve macro evidence:
pack reset/push/pop and literal alignments 1/2/4/8/16; warning push/pop,
literal levels 0..4 and bounded literal 4xxx/5xxx disable lists. Named labels,
alignment/list aliases, operators, token fragments, extra syntax and observed
keyword-like macro definitions decline this proof. Other pragmas retain their
existing conservative boundaries, including target-specific macro-stack effects.
Native MSVC/Clang validate the complete literal fixture; LF/CRLF regression
controls keep macro argument/following-function spans and omit pragma/macro
runtime callees. Evidence v14/product v13 invalidate prior conservative pragma
products. Frozen v12 migration and external-header alias edits pass in packed/
flat replay and normal watched MCP with background rebuilding on/off.
Metadata proofs now inspect only erroneous groups that visit() can actually
admit at the root; nested metadata is checked within that group's proof. An
irreversible opaque environment cannot revive bindings, so it does not repeat
that proof in subsequent SDK headers. Ordinary parsing and header/source
diagnostics remain intact. The checked native Hades audit retains the same 43
include observations and no actual ASSERT binding across this restriction.
The exact 99c197c candidate verifies 1744 paths and reports 26 damaged files /
152576 bytes through normal MCP, with full green CI. It is not installed.

Nonexpanding conditions also admit bounded decimal, hexadecimal, octal and
binary integers up to 2147483647, comparisons and bitwise/logical operators.
The audit checks syntax and operand kinds, without calculating a condition or
selecting a branch. Suffixes, separators, floating/character literals, expanding
operands, arithmetic, shifts and larger integers remain unsupported. Every
possible branch still contributes undef/include effects and cannot introduce
new statement bindings, even for a literal false guard. Independent directive
metadata proofs use the same condition policy. LF/CRLF recovery preserves
original arguments and following calls. MSVC/Clang accept the native fixture
with defined/undefined A. Evidence v15/product v14 reparse conservative v13
products; external header condition/effect changes invalidate packed/flat replay
and watched MCP with background rebuilds enabled/disabled.
The exact 5a7abf3 candidate verifies 1744 paths and reports 26 damaged files /
152576 bytes through normal MCP; its full CI is green. It is not installed.
The exact ab62947 candidate likewise verifies 1744 paths with 26 damaged files /
152576 bytes through normal MCP; its full CI is green after a runner-shutdown
retry. The unchanged local Linux workspace suite also passes. Neither candidate
is installed.

Objective-C++ message expressions have structural local grammar rules only in
an exact positive #ifdef __OBJC__ arm. The head must end at its newline; else/elif
outside that dialect context remain ordinary C++. No condition is evaluated,
source masked or mutable scanner mode added. The objc_message_expression kind
retains original receiver/selector/argument spans. Local namespaces, templates,
class/struct/union bodies, constructors/destructors and directly supported
function statements preserve existing named kinds through aliases. Binary
operands retain the ordinary operator precedence/fields. C++ calls inside
receivers and arguments retain their refs; selectors do not become C++ callees.
This remains bounded dialect support: Objective-C strings, @finally and other
unsupported expression/declaration forms remain errors; C++/CLI is separate.

Local @try/@catch rules preserve complete bodies/handlers using existing
try_statement/catch_clause aliases. Catch requires a nonempty declaration/list
or ellipsis: Clang rejects empty/comment-only parameters. Missing bodies,
handlers, parentheses, semicolons, endif groups and wrong/outside/else guards
retain errors. Ordinary C++ declarations and expression rules are unchanged.
All 35 Windows/Linux parser regressions, proof/recovery tests, configured replay,
bucketed/flat default-tree/product cache tests and normal watched MCP tests pass.
Owned/streaming/scan handoff agree and original call/node spans are checked.
Native Clang Objective-C++ accepts namespace/template/class fixtures and rejects
the missing-semicolon control. The unchanged 179-test corpus, native provenance,
schema and Clippy (including ingest without built-in parsers) pass. Reproduction
is idempotent; only C++ provenance changes to xxh3:f4e6e5432e3cf4b2 (ABI 14),
with the ASCII lexer fast path preserved. No new visible kind is introduced.

The read-only original-source/AST audit includes 902 tracked C++ paths. This
class/template/binary extension changes only Catch2: 118 -> 114 ERROR/MISSING
nodes and 29982 -> 29937 affected bytes, with 3302 observed C++ call expressions
unchanged and all 2317 extracted entries retained. The other 901 source/AST hashes remain identical. The latest verified
external candidate cb50aa8 covers 1744 paths and normal MCP reports 26 damaged
files / 152363 bytes. Its full CI is green after infrastructure cancellations
were rerun. The class extension has not yet been built into that candidate.
The productive release and tunnel remain untouched.

A bounded read-only native MSVC /E /d1PP experiment reports define/undef events,
but omits push_macro/pop_macro operations and the restored macro definition.
The observed expansion restores EPOCH=1 while the directive stream still last
reported EPOCH=2. Such output cannot establish safe original-source intervals
or supply production recovery bindings. /PD's final snapshot has the same
fundamental timeline limitation. No compiler-dump backend is enabled. Existing
Windows-fast/release compilation databases include the actual test translation
unit's flags and search paths; native Windows libclang is unavailable in the
checked tool locations. WSL libclang is not evidence of MSVC preprocessing.
Actual Hades include proof remains conservative; compiler/source/header/config
identity and original expansion locations would require an explicit, validated
compiler-context design before that separate path could recover these macros.
