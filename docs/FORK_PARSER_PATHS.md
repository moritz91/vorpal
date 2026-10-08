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
Independently instantiated replacements also prove whether an adjacent else can
bind to a dangling if. A distinct hidden scanner token retains this syntax class
for every use of a name; mixed classes decline the whole name. Original name and
argument spans remain fields of macro_statement, whose optional alternative is
an existing else_clause. Open-if statements do not absorb source semicolons.
Literal adjacent orphan else / extra-semicolon cases receive original-span
context diagnostics only in independently proven, nonexpanding contexts; closed
statements can still be consequences of an enclosing ordinary if.

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
This remains bounded dialect support: Objective-C selector/protocol expressions, @finally and other
unsupported expression/declaration forms remain errors; C++/CLI is separate.

Local @try/@catch rules preserve complete bodies/handlers using existing
try_statement/catch_clause aliases. Catch requires a nonempty declaration/list
or ellipsis: Clang rejects empty/comment-only parameters. Missing bodies,
handlers, parentheses, semicolons, endif groups and wrong/outside/else guards
retain errors. Ordinary C++ declarations and expression rules are unchanged.
All 37 Windows/Linux parser regressions, proof/recovery tests, configured replay,
bucketed/flat default-tree/product cache tests and normal watched MCP tests pass.
Owned/streaming/scan handoff agree and original call/node spans are checked.
Native Clang Objective-C++ accepts namespace/template/class fixtures and rejects
the missing-semicolon control. The unchanged 179-test corpus, native provenance,
schema and Clippy (including ingest without built-in parsers) pass. Reproduction
is idempotent; only C++ provenance changes to xxh3:c2ff3e4cb148e39b (ABI 14),
with the ASCII lexer fast path preserved. No new visible kind is introduced.

The read-only original-source/AST audit includes 902 tracked C++ paths. This
class/template/binary extension changes only Catch2: 118 -> 114 ERROR/MISSING
nodes and 29982 -> 29937 affected bytes, with 3302 observed C++ call expressions
unchanged and all 2317 extracted entries retained. The other 901 source/AST hashes remain identical. The latest verified
external candidate 4259b77 covers 1744 paths and normal MCP reports 26 damaged
files / 152318 bytes (2014 ERROR/MISSING/context diagnostics). Its complete CI is
green. This exact candidate includes the guarded class extension. The later
6397a66 candidate has the same health counts with corrected member-template
outline rows; its full CI is green. Neither candidate is installed.
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

C++ member-template outlines now distinguish typed methods from constructors.
A return-type field admits ordinary identifier declarators used in template
methods/prototypes; constructors/destructors remain type-less. Searches through
pointer/return declarators stop at parameter lists, so a function-pointer field
with a named function parameter cannot invent a method named after that parameter.
Function-pointer-returning methods retain their names. Access walks only enclosing
template wrappers to the class body, preserving the nearest label and class/
struct defaults without inheriting labels from an outer class.
Windows/Linux outline suites and 35 parser regressions retain member kinds,
access and spans; bucketed/flat replay retains those rows and source-label edits
match scratch generations. Typed calls resolve to the appropriate owner, while
ambiguous untyped calls remain unresolved. All 14 resolver evaluations, existing
incremental/live/stamp tests, native MSVC syntax controls and workspace Clippy
pass. Grammar/provenance are unchanged; edited bundled rules change the existing
source-derived product identity, invalidating older rows.

Equals initializers in supported Objective-C method bodies and fields now retain
the ordinary declaration/init_declarator/field_declaration aliases and fields.
The field's default_value and local declaration's value keep original message
and nested-call spans. Namespace/type/template declaration rules are unchanged,
and direct parenthesized initialization remains outside this bounded extension.
A broader local declaration/argument-list variant passed the unchanged corpus
but produced a large Catch2 root error in the original-source audit; it was
withheld. The narrowed variant changes only Catch2 among 902 tracked C++ paths:
114 -> 112 ERROR/MISSING nodes, 29937 -> 29916 affected bytes, 3302 C++ call
expressions and all 2317 extracted entries retained. The other 901 AST hashes
remain identical; stb_image.h retains 27 diagnostics / 7 bytes / 326 entries.
Windows/Linux parser, proof/recovery, bucketed/flat replay, default-tree health
and normal watched MCP regressions pass. LF/CRLF controls retain missing
semicolons, wrong/outside/else guards and malformed messages as errors. Native
Clang accepts the initializer fixture and rejects its missing-semicolon control.
The unchanged corpus, provenance, generated schema and workspace/no-builtins
Clippy pass. This grammar change introduces no new visible kind.

Complete Objective-C protocol blocks and plain object-string constants have
local structural kinds under the exact positive guard. Protocol methods retain
return_type, selector, parameter_type and parameter fields; optional/required
sections and a mandatory end delimiter remain explicit. Keyword tokens split
from @ reject protocolFixture/optionally/ending prefixes. Missing delimiters,
method types/parameters and semicolons remain errors. Object strings retain
ordinary quoted value/content spans; C++ wide/UTF/raw prefixes are not admitted.
Only keyword messages admit complete comma-separated variadic arguments, each
with its original argument field. Protocol declarations/selectors do not invent
C++ items/callees; native type checking and other dialect forms remain separate.

The original 902-path source/AST audit changes only Catch2: 112 -> 86 ERROR/
MISSING nodes and 29916 -> 29664 affected bytes, with message nodes 10 -> 19.
Comparison against the caae3ce persisted product retains all 2317 prior item
names/kinds/spans and all 3231 prior call-reference names/spans. The restored
Catch namespace is the only additional item (2318 total); annotationName.c_str()
and testCaseName.c_str() are the two additional genuine call refs (3233 total).
Observed AST call expressions increase 3302 -> 3304. stb_image.h stays at 27
nodes / 7 bytes / 326 entries, and the other 901 AST hashes remain identical.
All 37 Windows/Linux parser regressions, evidence/recovery, default-tree and
bucketed/flat product replay, and normal watched MCP tests pass. Cache/MCP
controls reject protocol keyword prefixes after edits and restore clean health.
Native Clang accepts the protocol/string/variadic fixture and rejects invalid
keyword/string prefixes and a comma tail after a unary selector. The unchanged
corpus, schema, only-C++ provenance, idempotent reproduction and workspace/
no-builtins Clippy pass. New visible kinds/fields are synchronized with
schemas/cpp_rule.json; original C++ expression rules remain unchanged.
The exact caae3ce candidate verifies 1744 paths and reports 26 damaged files /
152297 bytes (2012 diagnostics); its complete CI is green. It is not installed.

Selector metadata is now structural under the same exact positive Objective-C
guard. Unary names and complete named keyword selectors retain their original
selector fields, with no C++ call_expression inside the metadata. Ordinary
selector(real()) calls before, within and after the guard keep original call
spans. Empty/malformed argument lists, keyword prefixes, missing semicolons and
wrong/outside/else guards remain errors. Other selector forms remain unsupported.
The 902-path original-source audit changes only Catch2: 86 -> 84 ERROR/MISSING
nodes, 29664 -> 29662 bytes, and 3304 -> 3302 AST C++ call expressions. Comparing
the persisted 959790b product retains all 2318 item names/kinds/spans and removes
only the two false selector call-reference rows (3233 -> 3231); genuine call refs
are unchanged. The other 901 AST hashes match. All 38 Windows/Linux parser
regressions, evidence/recovery, bucketed/flat replay, default-tree health and
normal watched MCP tests pass; prefix edits produce damaged health and restoration
returns clean health. Native Clang accepts selector metadata and rejects the
keyword-prefix control. The unchanged corpus, schema, only-C++ provenance,
idempotent reproduction and workspace/no-builtins Clippy pass.
The exact 959790b candidate verifies 1744 paths / 64023 nodes and normal MCP
reports 26 damaged files / 152045 bytes (1986 diagnostics); its full CI is green.
This is an external candidate, not an installed release.

Named dotted fields rooted in a guarded message retain the existing
field_expression kind and argument/operator/field spans, including chained
named fields. Ordinary C++ field/member/template expressions are unchanged.
Only the message-rooted dotted form is added; missing names, dots, delimiters,
semicolons and wrong/outside/else guards remain errors. The 902-path audit changes
only Catch2: 84 -> 81 ERROR/MISSING nodes and 29662 -> 29635 affected bytes, with
3302 AST calls unchanged. Comparing the persisted 09080eb product retains all
2318 item names/kinds/spans and all 3231 call-reference names/spans. The other
901 AST hashes match. All 39 Windows/Linux parser regressions, evidence/recovery,
bucketed/flat replay and normal watched MCP tests pass; malformed field edits
produce damaged health and restoration is clean. Native Clang accepts the
message/field/chained-field fixture and rejects its missing-semicolon control.
The unchanged corpus, provenance, generated schema, idempotent reproduction and
workspace/no-builtins Clippy pass. No new visible kind is introduced.
The 09080eb external candidate verifies 1744 paths / 64023 nodes and normal MCP
reports 26 damaged files / 152043 bytes (1984 diagnostics). Its complete CI is
green.

Guarded constructor member-initializer argument lists now preserve structural
Objective-C messages through the ordinary field_initializer/list and
argument_list aliases. Constructor names, access and original spans remain;
namespace/type declarations are unchanged. Missing initializer parentheses,
message delimiters, member names and class semicolons stay errors. The 902-path
audit changes only Catch2: 81 -> 77 ERROR/MISSING nodes, 29635 -> 29601 affected
bytes and 19 -> 21 message nodes, with 3302 AST C++ calls unchanged. Comparison
against the persisted f594e31 product retains all 2318 item names/kinds/spans,
1630 member names/kinds/access/spans and 3231 call-reference names/spans. All
other 901 AST hashes match. All 40 Windows/Linux parser regressions, evidence/
recovery, bucketed/flat replay, default-tree health and normal watched MCP tests
pass; malformed constructor edits revalidate health and restoration is clean.
Native Clang accepts the constructor fixture and rejects its missing-parenthesis
control. The unchanged corpus, schema, only-C++ provenance, idempotent reproduction
and workspace/no-builtins Clippy pass; no new visible kind is introduced.
The f594e31 external candidate verifies 1744 paths / 64023 nodes and normal MCP
reports 26 damaged files / 152016 bytes (1981 diagnostics). Its full CI is
green. Productive binaries, release and tunnel are unchanged.

Guarded dialect declaration scopes now preserve ordinary C++ top-level fragment
rules without admitting new standalone Objective-C messages, message-bearing
calls/returns or exception statements. File/namespace bodies and their nested
conditional groups remain declaration contexts; functions/methods retain dialect
statement contexts through nested preprocessing groups, including local equals
initializers. A native Clang namespace control is rejected; the previous candidate
incorrectly reported that message clean. LF/CRLF regressions retain following
functions and owned/streaming/scan parity for both clean and erroneous source.
All 902 original source/AST hashes remain identical. Comparison with the aba00d1
persisted Catch2 product retains all 2318 item names/kinds/spans, 1630 member
names/kinds/access/spans and 3231 call-reference names/spans. All 41 Windows/Linux
parser regressions, evidence/recovery, bucketed/flat replay, default-tree health,
normal watched MCP tests, unchanged corpus, only-C++ provenance, schema,
idempotent reproduction and workspace/no-builtins Clippy pass. No new visible
kind is introduced. This does not turn tree-sitter into a compiler/type checker.
The exact aba00d1 candidate verifies 1744 paths / 64023 nodes and normal MCP
reports 26 damaged files / 151982 bytes (1977 diagnostics); full CI is green.
The exact 35ca69b candidate has the same verified normal-MCP counts and full
green CI (37470128426). With unchanged source, its changed grammar identity
reparses the previous false-clean namespace-message product: normal MCP retains
the syntax error and following function; warm replay and scratch generations
match. The migration fixture remains outside both checkouts.


An exact #ifndef __OBJC__ with a complete #else arm now supplies the guarded
Objective-C context only in that else arm. The first arm remains ordinary C++;
separate declaration/body rules retain preproc_ifdef and preproc_else aliases,
original guard/alternative spans and function bodies through nested groups.
Wrong/prefix names, elif alternatives, extra else-line tokens, messages in the
first arm or directly in namespace scope, and missing semicolons/guards remain
errors. Native Clang accepts both language variants of the positive fixture and
rejects the C++ first-arm and Objective-C namespace negatives. No condition is
evaluated and no new visible kind is introduced.
The 902-path source/AST audit changes only Catch2's guard structure; its 77
ERROR/MISSING nodes, 29601 damaged bytes, 21 messages and 3302 C++ calls are
unchanged. Comparison against the persisted 35ca69b product retains all 2318 item
names/kinds/spans, 1630 member names/kinds/access/spans and 3231 call-reference
names/spans. All other 901 AST hashes match. All 42 Windows/Linux parser
regressions, evidence/recovery, bucketed/flat replay and normal watched MCP tests
pass. Guard edits and malformed else lines revalidate health; restoration is
clean. The unchanged corpus, schema, idempotent reproduction, only-C++ provenance
and workspace/no-builtins Clippy pass. The generated C parser grows from
43702756 to 47815016 bytes (18920 to 20076 states); generation adds no conflicts.
This extension does not repair Catch2's remaining split-function main prefix.

A read-only native MSVC /E /d1PP invocation using the existing Windows-fast
compile database successfully preprocesses the unchanged binary serialization
test translation unit. It records ASSERT's actual two-parameter if/throw
replacement and the TEST/TEST_CALL forms. Source, database and invocation hashes
and output remain in external audit artifacts. This establishes the selected
compiler experiment, not source-interval recovery evidence: the macro-stack
trace limitation remains, and no compiler backend or productive binary is changed.


The dangling-if recovery regression fixes a false-clean native-valid case:
CHECK(value()) else after(); previously lost the real after() call by parsing
else as a declaration type. Nested if/else and trailing loop-body if replacements
retain nearest-if binding, original argument/else calls and following functions.
Complete if/else, compound and try replacements cannot acquire an orphan else;
extra source semicolons, missing else bodies/semicolons and ordinary unproven
calls remain damaged. Opaque prefix/keyword macros decline the new diagnostic.
The typed borrowed scanner context restores across nested parses, panic and
threads; default parsing clears both token families. No new visible kind or
source transformation is introduced. Evidence v16 / product v15 invalidate prior
proofs, alongside the changed C++ grammar identity.
All 42 parser, 33 recovery, 28 Windows / 29 Linux evidence tests, 14 replay tests
in bucketed/flat formats, member-call/default-tree-health tests and normal MCP
with background rebuild on/off pass. MCP verifies both original callees after
same-size external-header edits with restored timestamps and restoration. Native
MSVC accepts open/nested/outer-if positives (LF/CRLF) and rejects orphan/extra-
semicolon/missing-semicolon controls. All 902 original source/AST hashes match
with an empty scanner context, and the unchanged corpus, generated schema,
only-C++ provenance, idempotent reproduction and workspace/no-builtins Clippy
pass. The generated C parser grows from 47815016 to 48027890 bytes (20076 to
20110 states), with no new generation conflicts. This does not establish actual
Hades ASSERT evidence past opaque SDK includes or change installed binaries.

The native /d1PP macro-stack control also shifts reported source lines after
push/pop; restored definitions are absent from its timeline. Plain /E retains
this bounded control's lines and actual expanded values, but user #line tokens,
raw strings, dependency snapshots and compiler/environment identity still need
independent proof before any compiler-backed recovery could be banked.


The full CI for 39b933e exposed a deferred-persistence MCP race: a freshly
sealed graph could carry the correct edges before its evidence family was
committed, so a navigation response omitted original call-site lines. Edge
navigation (callers/callees/references/importers/implementors/type_users) now
drains the served generation's commit and attaches its exact evidence before
answering. Node and pattern navigation still serve the in-memory graph without
waiting. A channel-held real PendingPersist regression reproduces the missing
sites before the fix and retains original lines for callers and callees after
it, without depending on a fast filesystem. Windows/Linux MCP unit, macro
freshness and live-differential tests pass; Linux's complete protocol suite and
MCP Clippy also pass. The wider native Windows protocol suite exposed six path,
scope and Unix-specific assertion failures; these are separate follow-up
findings, not a green complete native protocol result. No grammar/product
format or productive installation is changed by the persistence fix.

The exact 39b933e candidate retains 1744 files / 64023 nodes and normal MCP
reports 26 damaged files / 1977 ERROR/MISSING/context diagnostics / 151982 bytes.
Two unchanged-source binary migrations from 5340a72 invalidate old false-clean
macro products: the valid dangling-if fixture recovers run -> after, while the
closed-statement/orphan-else fixture reports 19 original damaged bytes. Both
warm replay and scratch generation agree; source hashes stay unchanged. These
external candidate indexes remain separate from the productive release.


Native Windows MCP scope matching now recognizes both platform separators in
segment-exact directory/file admission and the sorted FileTable intervals.
Filesystem roots keep their separator; prefix siblings remain excluded.
Neighbour proximity and query-radius directories use the same native boundaries.
Structured records factor a shared absolute directory by slicing its original
spelling, preserving lossless base + relative-path reconstruction for drive and
verbatim Windows paths. Relative/mixed/different-drive pages remain unfactored.
Client workspace roots decode file URLs (including percent-encoded spaces, #
and Unicode) with url::Url, then compare canonical Path components rather than
slash-only string prefixes. Non-file or invalid URLs do not become local paths;
manual scopes still take precedence over client roots.
All 20 MCP protocol tests now pass on both Windows and Linux, as do 12 MCP unit
tests, three macro-header freshness cases, the live/scratch differential, 39
index unit cases and the path-prefix/scoped-oracle regressions. Native tests
cover file/directory/root row-vs-range parity, original path reconstruction and
escaped roots with an excluded prefix sibling. CI/release Windows gates run the
complete MCP protocol/unit suite and the scope/native-record unit regressions.
Linux workspace Clippy -D warnings, native MCP Clippy and Actionlint pass;
whole-workspace native Clippy still hits the pre-existing non-MSVC jemalloc
build-tool boundary, so it is not reported green. No graph/product identity,
parser grammar, installed binary or productive tunnel is changed by this fix.

The exact 3224ee8 candidate verifies 1744 files / 64023 nodes and normal MCP
retains 26 damaged files / 1977 ERROR/MISSING/context diagnostics / 151982 bytes.
Its complete CI is green (37519854318), including the full Windows MCP protocol
and native scope/record gates. The productive release remains unchanged.

C++/CLI handle declarators now have a bounded structural path in the positive
arm of exact #if defined(_MANAGED) and #ifdef _MANAGED groups. Additional direct
function definitions require a handle parameter or return; template static
return calls and struct specializations require a handle type argument. Existing
namespace/template/body/preprocessor kinds remain aliases; named underlying
rules prevent inherited template-name fields leaking into the callee lookup.
Only managed_handle_declarator and abstract_managed_handle_declarator are new
visible kinds. Original conditions/spans remain, without evaluating a guard.
Global C++ declarators, XOR, outer else/elif arms, arbitrary unmanaged guards and
unsupported dialect forms remain unchanged. Missing semicolons/braces/guards,
extra guard tokens and malformed type arguments retain syntax errors.
The 902-path original-source/AST audit changes only Catch2: 77 -> 71 ERROR/MISSING
nodes and 29601 -> 29600 damaged bytes, with 21 Objective-C messages and 3302 C++
AST calls unchanged. Compared with the exact 3224ee8 persisted product, all 2318
old items, 1630 members and 3231 call names/spans survive; the two recovered
functions are clrReferenceToString and stringify. The other 901 AST hashes are
identical. Guard edits and parser reuse converge with fresh parses. LF/CRLF
owned, streaming and scan-root tests retain the original static callee and
public member spans. Native MSVC /clr accepts both newline fixtures and rejects
the missing-semicolon control; ordinary C++ with forced _MANAGED rejects T^.
All 43 parser, 33 recovery, 28 Windows / 29 Linux evidence tests, bucketed/flat
replay, member-call/health tests, normal watched MCP and all 14 native Windows
resolver evaluations pass. The unchanged corpus, complete schema generation,
only-C++ native/Linux provenance, idempotent reproduction, workspace/no-builtins
Linux Clippy and native ingest Clippy pass. Seven explicit generation conflicts
are local to the dialect paths; the generated parser grows from 48027890 to
50544515 bytes and from 20110 to 20671 states. This does not establish Hades ASSERT
evidence past opaque includes, fix every C++/CLI form or install a candidate.

Two complete #if / #else function heads can now share one original trailing
body in declaration scope, including ordinary namespace/preprocessor containers
and the ordinary arm of the exact inverse Objective-C guard. The three visible
kinds are conditional_function_definition, conditional_function_prefix and
conditional_function_body. Prefixes end at their actual opening brace; the body
starts after the complete conditional group and ends at its actual closing
brace. Neither fragment aliases a fictitious complete function or compound
statement. Outline functions retain only their original header spans, and shared
body references are attributed to both alternatives before binder/type dedup,
with separate parameter types and unchanged call sites. Complete shared bodies
keep local declarations out of the top-level outline; damaged bodies retain
ordinary recovery traversal. Header fragments receive no complete-body clone
sketch. Custom outlines that omit a head decline shared-body attribution.

Macro proof recognizes the explicit shared-body statement context only after a
complete, nonexpanding head condition and ordinary all-branch effect checks.
Expanding conditions, unknown body includes, undef, damaged directives and
unproven invocations still decline proof. Evidence identity v17 / opt-in product
identity v16 invalidate older proof products. This adds no compiler branch
selection, default macro-name assumption or actual Hades ASSERT evidence past
opaque includes. Other split-head forms (#ifdef, elif, arbitrary body macros)
remain unsupported, and local type inference can remain conservative.

The 902-path original-source/AST comparison changes only Catch2: 71 -> 70
ERROR/MISSING nodes and 29600 -> 878 damaged bytes. All 21 Objective-C messages
and 3302 C++ AST calls survive; the other 901 AST hashes are identical. Exact
be124d6 product comparison retains 1630 members and all 3231 unique call
names/spans. Its main item becomes its original header fragment and wmain is
added (2320 -> 2321 entries); shared calls retain both owners. No existing
signature text disappears. The parser grows from 50544515 to 53144626 bytes,
20671 -> 21181 states, with 13 scoped generation conflicts. LF/CRLF head/body
edits and parser reuse converge with fresh parses. Native MSVC accepts both
wide/narrow and newline variants and rejects missing-semicolon controls.
All 45 parser, 34 recovery, 28 Windows / 29 Linux evidence tests, bucketed/flat
replay, member-call/health, normal watched MCP, native resolver evaluations and
outline suites pass. The unchanged corpus, actual full schema generation,
C++-only native/Linux provenance, idempotent reproduction, Linux workspace and
no-builtins Clippy and native ingest Clippy pass. The productive binary, index
and tunnel are unchanged.

The exact a369d55 external candidate verifies 1744 files / 64026 nodes. Normal
MCP health reports 26 damaged files / 1970 ERROR/MISSING/context diagnostics /
123259 bytes, compared with be124d6's 151981 bytes. An opt-in normal MCP fixture
also resolves shared/value callees from both wide/narrow function fragments and
returns both callers with distinct IDs and the original macro-invocation line.
Its four watched-MCP regressions pass on Windows/Linux: LF/CRLF, background
rebuild on/off and same-length external-header edits with restored timestamps
invalidate the proof, then restoration recovers both original sites. MCP Clippy
passes on both platforms. No productive installation is changed.

Unqualified explicit operator calls now have a separate low-precedence
call_expression alternative with negative dynamic precedence. Operator names
are not promoted to general expressions. Two explicit conflicts preserve
ordinary function declarators, including the unchanged alternative-token
corpus. Intact calls retain their original operator/argument spans and owners.
A damaged operator argument list can instead be a recovered typed declaration;
that ambiguous callee and nested argument-call rows decline runtime attribution,
while the original errors remain visible. LF/CRLF regression fixtures cover
both the typed signature and a nested parameter function, with owned/streaming/
scan-root parity. Intact nested argument calls and following functions survive.

The 902-file source/AST audit changes only Catch2: 70 -> 68 ERROR/MISSING nodes
and 878 -> 850 damaged bytes; its 21 Objective-C messages remain. The AST has
3312 call nodes, including one damaged signature, but that signature creates no
runtime edge. Exact a369d55 product comparison retains 2321 item entries, 1630
members and all 3231 previous unique call names/spans, adding nine genuine
operator calls (3240 total). No previous reference row or signature text is
removed. Two namespace ends recover their actual closing braces; item spans
are therefore not all identical. The other 901 AST hashes are identical.
The parser grows from 53144626 to 53307007 bytes / 21181 -> 21191 states; there
are no new visible kinds. All 47 parser, 34 recovery, 28 Windows / 29 Linux
evidence tests, bucketed/flat replay, member-call/health, four watched MCP tests
and 14 native resolver evaluations pass. Native MSVC accepts LF/CRLF operator
fixtures and rejects malformed arguments/missing semicolons. The unchanged
corpus, full schema generation, C++-only native/Linux provenance, idempotence,
Linux workspace/no-builtins Clippy and native ingest Clippy pass. No productive
binary, index or tunnel is changed.

The exact 0e32e33 external candidate verifies 1744 files / 64026 nodes; normal
MCP reports 26 damaged files / 1968 ERROR/MISSING/context diagnostics / 123231
bytes. Full CI 37584424866 is green. The productive release remains unchanged.

Declaration-scoped inverse __OBJC__ guards now admit primitive-return function
definitions in the complete else arm, with a mandatory function declarator and
existing function_definition/compound_statement aliases. Nested Objective-C
preprocessor bodies retain message metadata and original spans. A general
specifiers prototype incorrectly accepted namespace bodies and is withheld;
namespace-as-type, malformed bodies/guards and unsupported custom returns
retain errors. There are no new visible kinds. Four explicit generation
conflicts preserve the existing declaration/expression alternatives.

The independent metadata proof for the exact inverse guard supplies an empty
else arm only in its ephemeral syntax fixture: the original directive/name
and the entire original guard inventory must still match, and every actual
branch undergoes ordinary effect checks. No input or banked tree is rewritten,
no branch is selected and no global metadata check is loosened. Evidence v18 /
opt-in product v17 invalidate old proof products. A LF/CRLF recovery regression
retains both shared C++ owners plus the Objective-C owner and exact argument
calls without CHECK/sink/message-selector runtime edges. Unknown/expanding
conditions, missing includes, undef, extra guard tokens and missing closing
groups decline proof. Default extraction still leaves unproven calls damaged.
This does not establish Hades ASSERT evidence beyond opaque SDK includes.

The 902-file production AST audit changes only Catch2: 68 -> 64 ERROR/MISSING
nodes and 850 -> 804 damaged bytes, retaining 3312 C++ AST calls and recognizing
21 -> 24 Objective-C messages. The other 901 AST hashes are identical. Product
comparison retains 2321 entries / 1630 members and every existing call/name/span;
there are no reference changes beyond the preceding nine genuine operator
calls. The generated parser grows from 53307007 to 53397795 bytes / 21191 ->
21221 states. All 48 parser, 35 recovery, 28 Windows / 29 Linux evidence tests,
bucketed/flat replay, member-call/health, watched MCP and 14 native resolver
evaluations pass. Linux Clang accepts both nested Objective-C guard variants
and rejects syntax controls; native MSVC accepts both ordinary C++ alternatives
and LF/CRLF forms. Unchanged corpus, full schema generation, C++-only native/
Linux provenance, idempotence and Clippy gates pass. No productive binary or
tunnel is changed.
The exact 39c8f5e external candidate verifies 1744 files / 64026 nodes;
normal MCP health reports 26 damaged files / 1964 ERROR/MISSING/context
diagnostics / 123185 bytes. Full current-head CI 37587681437 is green.

Inline preceding friend now wraps the original friend declaration alternatives,
with one storage-class/friend conflict. The keyword remains declaration syntax,
not a fictitious return type. All existing visible kinds are unchanged. The
902-file production AST audit changes only Catch2: 64 -> 62 ERROR/MISSING nodes
and 804 -> 780 bytes, with all 24 messages / 3312 C++ AST calls retained; the
other 901 hashes match. The exact 0e32e33 product comparison retains 2321 items,
1630 members and all 3240 call names/spans, removing only two bogus Type refs
named friend. Named friend prototypes still expose the pre-existing member
classification limitation; free-function outline attribution is a separate
follow-up. This grammar repair does not claim to resolve that limitation.
The parser grows from 53397795 to 53596112 bytes / 21221 -> 21329 states.
All 49 parser, 35 recovery, 28 Windows / 29 Linux evidence tests, bucketed/flat
replay, member-call/health, four watched MCP tests and 14 native resolver
checks pass. Native MSVC accepts LF/CRLF inline-friend definitions/declarations
and rejects missing semicolons, malformed parameters, extra prefixes and
missing braces. Unchanged corpus, full schema generation, C++-only native/Linux
provenance, idempotent reproduction and Clippy pass. No productive binary,
index, tunnel or Hades source is changed.

Friend definitions now have namespace-function outline entries reached by the
existing nested-item pass, with immediate friend-parent matching. Ordinary
function rules exclude these direct friend children to prevent duplicates;
member rules exclude friend subtrees. Bare friend prototypes remain declarations,
not fictitious class methods/constructors. Local records/methods inside friend
bodies do not leak into the granting class. Pointer-return and template friends,
friend constructor declarations, ordinary methods, LF/CRLF spans and nested
local records have outline regressions. The bundled rule digest invalidates old
products. C++ now needs the existing full-tree nested-item pass, so its default
walk-snapshot fast path declines reuse; ordinary parse-tree caching remains
available and configured recovered trees retain their existing cache bypass.

The 902-file product audit changes only Catch2: 2321 -> 2337 items and 1630 ->
1626 members. Sixteen actual friend definitions are recovered and four bogus
member prototypes removed. All original item names/spans, runtime call names,
sites and multiplicities survive; 21 call sites move to their proper free-function
owners. Parser health is unchanged by these outline rules. A production recovery
fixture retains exact macro argument sites and friend owners through owned,
streaming and scan-root extraction, without macro-callee/replacement calls.
Five watched MCP tests pass on Windows/Linux; the friend case tests both newline
styles and background rebuild settings, same-length header changes with restored
timestamps and restored proofs, with original displayed sites and both edge
navigation directions. Full outline suites, 49 parser / 36 recovery tests,
evidence, bucketed/flat replay, member-call/health, 14 native resolver checks,
Linux workspace/no-builtins and native ingest/MCP Clippy pass. Native MSVC accepts
the complete pointer/template/local-record outline fixtures with LF/CRLF.
Windows CI now also runs c_family_outline_rules (the release gate already does);
Actionlint passes. The grammar, schema and provenance are unchanged since b6d53dc.
No productive binary, index or tunnel is installed by this change.


The exact 290e5d1 external candidate verifies 1744 files / 64040 nodes. Its
normal MCP health is 26 damaged files / 1962 ERROR/MISSING/context diagnostics /
123161 bytes; full CI 37644140751 is green. The productive .2 server is unchanged.

Return-only conditional logical suffixes now preserve the original left value,
complete #if/#ifdef/#ifndef groups and each logical right operand. The grammar
requires real directive line endings and the final semicolon. No else/elif arms,
general expressions, branch evaluation or source rewriting are introduced.
The generated conditional_logical_expression kind and preproc operator/right
fields are reflected in the C++ schema/provenance. Reproduction removes its own
prior return alternative before deriving Objective-C returns, keeping that
separate dialect boundary stable. The parser is 53770909 bytes / 21347 states;
Cpp source digest is xxh3:501fa1831f6bac8b.

Ordinary root functions may retain entering statement-macro evidence only when
every directive belongs to a complete, clean logical suffix group. An independent
original-span inventory must exactly match both directives and group boundaries;
conditions must not expand macros. Unknown includes, define/undef mutations,
malformed groups, expanding guards and namespace containers still decline proof.
Evidence v19 / opt-in product v18 invalidate old proof products. Other function
metadata remains opaque. No Hades ASSERT success is claimed across opaque includes.

The 902-file source AST comparison changes only Catch2: 62 -> 60 ERROR/MISSING
nodes / 780 -> 763 bytes, retaining all 24 Objective-C messages and 3312 C++ AST
calls. Every other AST hash matches. Product comparison with exact 290e5d1 retains
2337 items, 1626 members and all 3240 unique runtime call names/spans; full
reference rows and outline signatures have no changes. Native MSVC accepts LF/CRLF
fixtures with the guard defined and undefined and rejects the active malformed
suffix. Windows/Linux parser (50), recovery (38), evidence (28/29), owned/streaming/
scan parity, bucketed/flat replay, member-call/health, 14 native resolver checks,
corpus, schema generation, provenance, reproduction and Clippy gates are checked.
Six watched MCP tests include this return/macro combination with original sites
for every guarded operand and argument call. Both newline styles and background
rebuild settings invalidate same-length external header edits with restored
timestamps, then restore the proven sites. No productive binary/index/tunnel is
installed by this change.


The exact 8f4017c external candidate verifies 1744 files / 64040 nodes, with
26 damaged files / 1960 ERROR/MISSING/context diagnostics / 123144 bytes.
Full CI 37648351839 is green; the productive .2 server is unchanged.

C++ call extraction now declines only an ambiguous recovered callee head: a
callee containing ERROR/MISSING nodes, or a bare name immediately after a loose
type fragment in an ERROR context (or a single-type-leaf ERROR sibling).
Only actual AST comment nodes and whitespace can separate the latter fragments.
Other ordinary/static/member/operator calls inside unrelated errors remain;
argument calls are traversed with their original spans. No error node is hidden
and no source is rewritten. An unexpanded ASSERT followed by a member call can
otherwise create a false member callee spanning both statements. In the 902-file
product comparison against exact 8f4017c, 207 such ambiguous references disappear
in 12 damaged files, including two Catch2 constructor variables misread as calls.
Every item/member, diagnostic count/span total, near-clone sketch and retained
reference row/multiplicity matches. This does not claim macro recovery across
opaque Hades includes or recovery of those damaged statements.

Reference capture v1 is folded into extraction_identity, including the manifest
stamp used by whole-tree replay. Legacy products migrate once even with unchanged
source and grammar. Grammar generation and the clone token seed stay unchanged.
The legacy 32-byte identity contract has a migration regression. Parser tests
(51) retain owned/streaming/scan-root parity. A seventh normal watched-MCP test
checks both newline styles and rebuild settings: invalid same-length external
header edits with restored timestamps keep syntax errors and suppress the fused
callee; restored statement proof returns the actual member call and its original
site. Argument and following calls retain their original sites throughout.
The paired conditional-return branch draft remains external and withheld until
its full-file extraction effects are independently validated.


The exact 26bc03f external candidate keeps 1744 files / 64040 nodes,
26 damaged files / 1960 ERROR/MISSING/context diagnostics / 123144 bytes,
and 3101 near-clone pairs. Full CI 37654953140 is green.

Proof-backed statement recovery now also accepts complete do/while replacements.
A standard wrapper without its final semicolon is completed only in the ephemeral
proof template, and every original invocation must supply its own literal
semicolon after independently checked spacing/comments. No original source or
banked tree is rewritten. A missing terminator blocks the entire macro name for
the offset-free scanner. A replacement already containing its own terminator
needs no source semicolon. Only do/while receives this completion; expression,
while/for, partial body and function-generating replacements remain unsupported.

The adjacent-else proof consumes exactly the first required do/while terminator;
any additional source semicolon remains an empty statement that breaks else
attachment. Expression/qualified invocations and genuinely missing terminators
retain their original syntax/context diagnostics. Evidence v20 / opt-in product
v19 invalidate old proof products, while default grammar/rule identities remain
unchanged. Original argument calls and following functions survive owned,
streaming and scan-root handoff without macro-callee/replacement edges. An eighth
watched-MCP regression checks both newline styles and background settings: a
same-length external header edit with restored timestamps changes the semicolon
contract, reports the incompatible else, and restoring the header restores proof.

Native libclang 18.1.1 experiments are external read-only audits, not a production
compiler backend. Detailed preprocessing records omit the restored invocation
after literal and wrapped pop_macro, and selected-out invocations have no record.
Physical source ranges survive user line directives, and immutable compiler
header buffers survive later disk edits, but these facts do not establish complete
macro coverage. Cursor statement ranges do not provide the missing definition
anchor. No compiler records activate recovery or replace MSVC evidence. Actual
Hades ASSERT proof remains bounded by opaque transitive SDK includes.

Windows/Linux parser (51), recovery (39), evidence (28/29), bucketed/flat
replay, member-call/health and eight watched-MCP tests pass. Fourteen native
resolver evaluations, native ingest/MCP Clippy and Linux workspace/no-builtins
Clippy pass. MSVC /Zs accepts the LF/CRLF caller-terminated and definition-
terminated fixtures and rejects missing/extra terminators and malformed loops.
The unchanged 179-case corpus is green. The exact 26bc03f 902-file product
comparison has no default product changes, including all item/member/reference
rows, diagnostics and clone sketches. Grammar, schema and provenance are unchanged.
No productive binary, index, tunnel or Hades source is modified.


Bare member constructor names now require their nearest enclosing class/struct/
union name. Explicit and partial specializations compare the template's name
without its argument list, retaining the injected class name. Destructor and
qualified declarator shapes remain unchanged. A declaration macro such as
DECLARE_STORAGE(T) therefore cannot become a foreign constructor. Matching stops
at the nearest owner, so an outer class cannot validate an inner foreign head.
This is an outline correction, not additional macro recovery or error suppression.
The bundled rule digest invalidates products extracted with the older rule.

Windows/Linux outline (14), parser/product (53), recovery (39), evidence (28/29),
bucketed/flat replay, member-call/health and eight watched-MCP regressions pass.
Fourteen native resolver checks, native ingest and Linux workspace/no-builtins
Clippy pass. MSVC /Zs LF/CRLF controls retain ordinary, qualified and specialized
constructors and reject a foreign constructor and missing call semicolon. All
902 current C++ products are byte-identical to exact 690343f after normalizing
stat/extraction identity fields: no definitions, reference owners, diagnostics,
parameter/return facts, requests or clone sketches change. Grammar, schema and
provenance are unchanged. Broader removal of unresolved free function heads is
withheld: their bodies need anonymous scope barriers before local classes and
receiver types can safely survive without a synthetic function owner.

A read-only native LLVM 19.1.7 audit of binary_serialization_tests.cc resolves all
27 original ASSERT/TEST invocations with zero compiler errors under adapted MSVC
flags. This is not production proof or equivalence with MSVC. TEST generates a
function using token paste and remains unsupported. Detailed preprocessing records
still omit restored uses after literal/wrapped pop_macro; a wrapped __has_include
query also hides the underlying missing-file observation. Complete invocation and
macro-stack coverage, native compiler configuration identity and immutable found/
missing/shadow/query dependency observations are required before a compiler backend
could activate recovery. Neither the temporary compiler nor these records changes
normal MCP behavior. Hades and the productive .2 installation remain untouched.


Qualified class definitions retain the terminal injected constructor name:
Outer::Inner compares Inner, and ns::Box<T>::Nested compares Nested. Searching
qualified names stops before template arguments and excludes template names used
as qualification prefixes, so Box/T cannot validate a foreign constructor of
Nested. Windows/Linux outline (15), parser/product (53), watched MCP, replay,
Clippy and native MSVC LF/CRLF controls pass. Constructor-body calls retain their
exact member owners and original spans. The renewed 902-file complete-product
comparison remains byte-identical after stat/extraction identity normalization.
An external dependent non-type template-parameter rule is unnecessary: production
already parses its isolated valid controls correctly, and adding the rule changes
none of the 902 full-file ASTs. Those Catch2 errors follow unresolved macro prefixes;
the duplicate template rule is withheld. Grammar/schema/provenance are unchanged.


Complete while, for/range-for and switch statement replacements now share the
existing proof-backed statement scanner. Their entire bodies must be present in
the replacement; caller-supplied bodies, case/loop prefixes and missing inner
semicolons remain unsupported. Only do/while receives an independently proven
caller terminator. Nested dangling ifs retain their native else attachment; an
extra source semicolon still breaks it. Replacement-only calls do not become
runtime edges, and argument/following calls keep their original spans through
owned, streaming and scan-root extraction. Evidence v21 / opt-in product v20
invalidate old recovery products without changing default identities.

Windows/Linux evidence (29/30), recovery (40), parser/product (53), outline (15),
nine watched-MCP tests, bucketed/flat replay, native resolver and Clippy gates
pass. Watched MCP catches same-length external header edits with restored
timestamps that remove the control body, and restoring proof restores clean
health and the original caller sites. MSVC /Zs LF/CRLF controls accept all complete
forms and reject expression uses, extra else-breaking semicolons and genuinely
missing call semicolons. The unchanged 179-case grammar corpus passes; all 902
existing complete C++ products remain identical after stat/extraction identity
normalization. Grammar, schema and provenance are unchanged.

The exact ac32d65 candidate index verifies 1744 files / 64040 nodes, 26 damaged
files / 1960 ERROR/MISSING/context diagnostics / 123144 bytes; its full CI
37695647104 is green. The first actual ASSERT evidence boundary with native SDK
roots is the original iostream include guard: its _STL_COMPILER_PREPROCESSOR
condition and named SDK pragma operands are not nonexpanding/literal proofs.
All 21 original directives were independently inventoried during a read-only
audit. Unknown effects remain opaque; this control-statement extension neither
repairs that boundary nor claims an actual Hades ASSERT recovery. Hades and the
productive .2 installation remain untouched.


Bare returnless C++ function heads now keep an anonymous traversal boundary
instead of becoming named functions. Declaration-generating macro names and
parameter-shaped generated names supply neither function entities nor type-use
proof. Qualified constructors/destructors and conversion operators retain their
existing rules. Anonymous boundaries survive outline filters and swallow recovery,
so nested local classes/aliases cannot become public definitions by removing the
outer name. Local binding and type-use dedup domains remain separate between
anonymous bodies and the file. Hidden local type names cannot resolve to a
same-spelled global type; anonymous method fields/parameters/returns never become
file facts. Original argument/body calls retain their spans and use the existing
containing owner, without inventing an anonymous graph entity. Reference capture
v2 and typefacts v6 invalidate older products; no grammar/schema/provenance change.

Windows/Linux outline (17), parser/product (57), evidence (29/30), recovery (40),
ten watched MCP regressions, bucketed/flat replay/member-call/health, fourteen
native resolver checks and Clippy gates pass. Default cached C++ reparses match
fresh products after prefix/body edits, removal and conversion to a real named
function; owned/streaming/scan products also agree for LF/CRLF. Snapshot tests
preserve and shift the lexical dedup anchor. The unchanged 179-case grammar corpus
and native MSVC positive/negative controls pass. A complete-product comparison of
all 902 original C++ files against the verified v53 candidate changes 149 files:
2424 false function items, 4357 unproven reference rows and 1886 false function
sketches disappear; 42806 reference owners move to existing containing scopes and
1306 receiver facts conservatively change. All retained definitions/members,
syntax diagnostics/spans/cuts/requests and retained clone sketches are unchanged.
Additional original type leaves survive where previously distinct anonymous
scopes lost their dedup domains. This is an extraction correction, not recovery
of actual Hades ASSERT proofs. The SDK evidence boundary remains opaque.

External index freshness now takes an explicit launcher-authorized source root:
`vorpal mcp --src <source> --index <external-index> --config <config>` and the
library's with_profile_env_rebuild_source/serve_stdio_source_opts entry points.
Registered projects pass their enrolled source root to the same server path.
No root is inferred from index data or an arbitrary index parent. Missing explicit
roots never fall back to another layout root; missing/non-directory CLI roots
fail before serving. --src conflicts with --projects, and defaults to serving
<source>/.vorpal/index when --index is absent. Config selection stays --config or
launch-directory discovery, not implicit rediscovery under --src. The retained
environment and existing proof-freshness/rebuild logic remain authoritative.

Windows/Linux pass thirteen macro-freshness MCP tests, six actual CLI/MCP-process
config tests, existing watcher/project routing tests and the explicit-root refusal
unit. External index tests cover default and recovered extraction, LF/CRLF,
background rebuild on/off, anonymous-to-real function transitions and original
caller sites. Header-only edits with restored timestamps change running MCP
health without an index tool call; restoring proof recovers clean health. Registry
routing uses its enrolled root for external indexes. Native CLI/MCP and Linux
workspace/default/no-builtins Clippy pass. No parser, product identity, schema,
provenance or installed runtime changes in this source-root follow-up. Productive
.2 binaries/index/tunnel and the read-only Hades checkout remain intact.

Macro effect/substitution token scans now index the union of protected original
literal/comment/definition spans instead of repeatedly searching every span for
each byte or node. Empty proof environments keep the already parsed original tree
and complete dependency observations; they cannot activate a scanner or context
diagnostic, so a second empty-context parse is unnecessary. No proof cache,
source rewrite, new grammar kind or extraction identity is introduced.

The complete evidence records (definitions/templates, intervals, all observed
macro names, ordered roots and consulted dependency digests) remain identical
across all 902 original C++ files with the configured native SDK roots. Under the
concurrent audit load, that comparison takes 1077761 ms before / 526024 ms after;
a separate dense 8192-literal/comment fixture takes 24549 ms / 625 ms with the same
evidence hash. These are debug audit measurements, not release latency promises.
Windows/Linux evidence, recovery, parser/product, watched MCP, replay and Clippy
checks pass. A linear span oracle covers unsorted/nested/duplicate ranges. A dense
LF/CRLF source with missing include proof keeps its byte-identical raw product;
creating the header still changes dependency identity and restores original-site
recovery. Grammar/schema/provenance and all conservative proof boundaries remain
unchanged. The separate exact 236d860 full CI is green; its verified normal MCP
candidate reports 1744 files / 61631 nodes / 26 damaged files / 1960 diagnostics /
123144 damaged bytes / 2848 clone pairs. The productive .2 runtime is untouched.

Watched MCP refresh consumes the initial dirty marker even before the first graph
exists. A successful first request therefore leaves quiet requests on freshness
validation instead of rebuilding again with zero captured paths. Events arriving
during a build still re-arm the marker. Windows/Linux regressions cover default
and opt-in recovery with both layout and external indexes, repeated quiet queries
and an explicitly re-armed dirty marker. No runtime installation changes.

Quiet MCP proof-freshness checks snapshot the consulted input fingerprints, then
read them in parallel without retaining the observations mutex across Rayon work.
Each check still validates contents, missing candidates and redirected paths; no
proof or cross-query filesystem cache is added. A monotonic observation revision
rejects results spanning refresh epochs, new inputs or conflicting observations,
even if a new epoch restores the same input map. Revision exhaustion fails closed.
Identical repeat observations do not invalidate an otherwise consistent snapshot.
Five deterministic unit controls cover content/redirect/missing changes, concurrent
publication and refresh, incomplete epochs and revision exhaustion. Windows/Linux
normal MCP/header-replay/CLI regression suites and Clippy gates pass.

Native DLL prototypes with literal language linkage now use a bounded declaration
alternative: extern string literal, __declspec, primitive return, reserved native
calling convention and directly named function. Existing linkage_specification,
declaration and function_declarator aliases preserve the original structure.
No recursive declarator or general SDK modifier extension is enabled.
Declaration-scoped preprocessor copies now prefer their declaration context;
penalizing every nested group could turn a later local error into a whole-file
ERROR after an otherwise valid DLL prototype was repaired.

Three LF/CRLF regressions preserve native modifiers, include guards, split heads,
namespace/function owners and exact runtime call sites. Replacement text creates
no calls, functions or clone sketches. Missing terminators/parameters and genuinely
unclosed guards retain syntax errors after the valid prototype. All 60 parser
regressions pass on Windows/Linux, alongside evidence/recovery, outline, replay,
MCP/CLI, fourteen resolver checks and native/Linux Clippy. The unchanged 179-case
corpus, schema and both provenance checks pass. Native MSVC accepts positive
LF/CRLF normal/wide/Objective-C controls and the full original Catch2 runner header;
missing-semicolon and incomplete-guard controls are rejected.

The complete-product audit of all 902 original C++ files against verified v54
changes only Catch2 after normalizing file stamps, extraction identity and the
sketch bytes seeded by the grammar generation. Catch2 diagnostics fall from 60 /
763 bytes to 56 / 580 bytes, with no root ERROR. Three false function items are
removed; two real functions, two namespaces and two macros are restored. Two
replacement-only calls disappear; two real calls retain original spans with their
correct owners. One false macro clone sketch disappears; retained sketch owners
and shingle sizes remain identical. The outer include guard becomes one root
child again. The native DebugBreak void return fact is restored; other return
facts, retained entries/members/parameters, representative error spans, requests
and swallow diagnostics remain unchanged. The remaining 901 products are
byte-identical after the stated normalization. Only the C++ provenance digest
changes; schema and visible node kinds remain unchanged. Actual Hades ASSERT
proofs remain blocked by opaque SDK includes. The productive .2 runtime is intact.

Complete conditional return values now have an explicit conditional_return_expression:
zero or more complete if/ifdef logical-prefix groups followed by a complete if/ifdef
value group with a mandatory else operand. Original branch conditions, operands and
operator spans survive; conditions are never selected or evaluated. Every group
requires directive line endings, and the caller return still requires its semicolon.
The new visible kind is synchronized in node types and the C++ rule schema; only the
C++ provenance entry changes. No general expression or macro-prefix extension.

LF/CRLF controls cover if/ifdef/ifndef, with/without a logical prefix, namespace end
spans, original argument/callee sites and owners, and equal owned/streaming/scan
products. Empty/missing arms, missing directives/operators/terminators and ordinary
missing semicolons remain errors. Native MSVC accepts all four branch combinations
on both newline styles and rejects the malformed controls. All 61 parser tests,
evidence/recovery, outline, index/replay/resolver, normal MCP/CLI, corpus/schema,
provenance and native/Linux workspace/default/no-builtins Clippy gates pass.

Against verified v55, all 902 complete original C++ products change only Catch2
(after stamp/grammar/sketch-seed normalization). Diagnostics fall from 56 / 580 bytes
to 52 / 571 bytes. The real useColourOnPlatform function and its 47-shingle sketch
are restored; its surrounding namespace gains the correct final 25 source bytes.
Two existing calls retain every original site/argument/form and move from that
namespace to the restored function. Every other complete item/member, reference,
parameter, return fact, cut, representative error span, request, swallow and retained
sketch owner/shingle count is unchanged. The remaining 901 products are identical
after normalization. This does not widen opaque SDK macro proof or modify Hades.

The exact 8d8975b full CI (37789029641) is green on all three jobs. Its separately
pinned external --verify candidate and two normal source-aware MCP health requests
report 61630 nodes, 26 of 1744 damaged files, 1952 diagnostics, 122952 damaged bytes
and 2846 clone pairs. Both health responses/generations agree; the trace contains
one full pipeline. Initial/quiet debug query times are 201.707 s / 4.334 s under
the concurrent audit load, not release latency guarantees. Productive .2 remains
untouched.

Proof-backed recovery now indexes protected spans for the original invocation
scan and the independent argument-boundary/arity/splitting scans. A sweep records
the first original covering span at each boundary, preserving DFS precedence,
the original end offset and the literal/comment flag even across overlaps. Lookup
uses binary search; no proof decision, source, dependency, identity, scanner or
serialized product changes. A linear oracle covers unsorted/nested/duplicate,
empty/adjacent spans, mixed flags and usize endpoint boundaries on Windows/Linux.

Pinned before/after release probes exercise 8192 literals/comments outside a
macro invocation and inside one argument, with both LF/CRLF. All complete audit
reports and encoded products remain byte-identical across both pairs of runs.
The repeated paired audit measurements are 1076/1124 -> 415/310 ms outside and
827/1012 -> 126/139 ms inside (LF/CRLF); timings are audit observations rather than
MCP latency promises. Windows/Linux parser/evidence/recovery/freshness, index
replay/member-call/health/fourteen resolver checks, normal MCP/CLI and native/Linux
workspace/default/no-builtins Clippy gates pass. The grammar, schema and provenance
remain unchanged by this recovery-only optimization. Opaque native SDK boundaries
still decline actual Hades ASSERT proof.


Conditional return value groups now retain entering statement-macro proofs across
ordinary functions. The proof recognizes the existing return-only prefix/value
kind as well as logical suffixes. Each admitted group must match the independent
original directive inventory exactly, including every header, optional else and
final endif. Only nonexpanding conditions qualify; neither branch is selected.
Extra/missing directives, expanding guards, macro mutations, includes and named
pragmas still decline proof. Missing ordinary return semicolons retain diagnostics
even when unaffected macro statements can be proven.

Evidence v22 / opt-in product v21 invalidate earlier declined products. The frozen
v21-evidence/v20-product replay fixture reparses old macro errors, then warm packed
products replay and match a scratch generation. Owned, streaming and scan-root
products agree, preserving both branch calls and macro-argument original spans.
No macro-callee or replacement-only runtime edges appear. A fourteenth watched
MCP regression verifies both newline styles/background settings and original
callee sites after same-size external-header edits with restored timestamps.
Windows/Linux 61 parser, 42 recovery, 29/30 evidence, 15 replay, member-call/health,
14 resolver, MCP/CLI and Clippy gates pass; Linux default/no-builtins workspace
Clippy and the unchanged 179-case corpus/provenance checks pass. Native MSVC accepts
all eight prefix/choice/newline combinations and rejects missing return terminators
and empty selected operands. Grammar, schema and provenance are unchanged. This
bounded repair does not establish Hades ASSERT proof across opaque SDK includes.


The exact protected-span optimization 2d10760 passes all three CI jobs in run
37791336237. A renewed native LLVM 19 token-location check confirms that restored
macro uses have neither a preprocessing cursor nor a token annotation supplying
the missing definition anchor. This is explained by LLVM 19's
[PreprocessingRecord implementation](https://github.com/llvm/llvm-project/blob/llvmorg-19.1.7/clang/lib/Lex/PreprocessingRecord.cpp):
undef removes the definition mapping; expansion records require that mapping,
and nested expansions are intentionally omitted. A future compiler observer needs
actual expansion callbacks and complete filesystem/query observations, rather
than inferring those missing records from statement shapes. No compiler backend
or productive runtime is activated by this investigation.

The configured recovery comparison against verified v56 retains all 902 original
C++ products byte-for-byte after only source stat and extraction-identity fields
are normalized. Clone sketches also match without normalization. The existing
Hades definitions, references, owners, facts and full diagnostics do not change.

The exact 3eaad6c normal source-aware MCP candidate retains the verified v56
generation and complete health: 61630 nodes, 26 of 1744 damaged files, 1952
diagnostics and 122952 damaged bytes. Initial/quiet debug requests take 194.877 s /
3.336 s under the audit load and the trace contains one full pipeline. These are
candidate measurements, not productive runtime or release latency claims.

Its full CI (37793677565) passes Windows and encoder jobs, but Linux exposes two
watched-MCP test teardown races: detached scope/posting warmers can still access
the index after Server drops. Shutdown now retains both startup/commit scope
warmer handles, reaps finished handles during serving, stops the watcher, cancels
optional warming and joins all owned background jobs without invoking successor-
spawning adoption paths. Existing persistence/canonicalization draining is
preserved. A gated nine-worker regression fails before the repair and passes
after it; it verifies that every final artifact write completes before drop
returns, including a deadline-forced warm's cancellation. The existing pending-
call-site regression now requires immediate directory cleanup to succeed.
Grammar, extraction identities, products, source and the productive .2 remain
unchanged by this lifecycle repair.
The full MCP test suite passes on Windows/Linux with automatic warming enabled,
including all fifteen library controls and fourteen watched macro cases. A
separate fourteen-thread watched-suite run, six CLI/MCP configuration tests,
native CLI/MCP Clippy and Linux default/no-builtins workspace Clippy also pass.

The exact shutdown repair 16d3d67 passes all three jobs in CI 37808950360.

Literal `#pragma push_macro("NAME")` now preserves only an entering proven
definition. A save does not change the definition; complete nonexpanding
conditional groups can therefore retain it without selecting a branch. Every
stack target remains ineligible for a later proof restart, and `pop_macro` still
ends its binding rather than claiming restoration, even after a known save.
Malformed/nonliteral operations and pragma-name macros decline proof. Native
MSVC expands an aliased `push_macro` into `pop_macro` in both preprocessor modes;
an apparent save must not hide the missing semicolon after a restored expression.
The literal distinction follows Microsoft's
[push_macro](https://learn.microsoft.com/en-us/cpp/preprocessor/push-macro?view=msvc-170)
and [pop_macro](https://learn.microsoft.com/en-us/cpp/preprocessor/pop-macro?view=msvc-170)
contracts, with the compiler alias counterexample checked separately.

Evidence v23 / opt-in product v22 invalidate older declined products. Frozen
evidence-v21/product-v20 and evidence-v22/product-v21 replay controls both force
reparsing, then match warm and scratch generations. LF/CRLF owned, streaming and
scan handoffs retain identical encoded products, original definition/argument
spans and following function/call sites. No macro-callee/replacement-only runtime
edges are introduced. The fifteenth watched-MCP case swaps an external save for
a pop with identical byte length and restored timestamp, observes the resulting
diagnostics, and restores the original sites when the save is restored.
Windows/Linux parser (61), evidence (30/31), recovery (43), replay (15),
member-call/health, fourteen resolver cases, MCP/CLI and Clippy checks pass,
including Linux default/no-builtins workspace Clippy and flat replay. The
unchanged 179-case corpus/provenance checks pass. Native MSVC accepts all twelve
direct/conditional/zero-condition/newline save controls and rejects all twelve
missing ordinary semicolons. Both native preprocessor modes also reject the
restored-expression alias counterexample. All 902 configured original C++
products remain byte-identical against verified v57 after only source stat and
extraction-identity normalization; complete clone sketches match unchanged.
Grammar, schema, provenance and the productive .2 runtime are unchanged.

An external read-only LLVM 18/19 callback experiment records actual macro
expansions, including restored definitions and nested uses omitted by detailed
preprocessing records. Six LF/CRLF controls per version observe missing/present/
removed optional headers, wrapped `__has_include`, original physical offsets
despite `#line`, and missing earlier search candidates before a later hit. The
same six fixtures pass native MSVC syntax checks. A preprocess-only LLVM 19
invocation adapted from Hades' native compile command records 21 ASSERT and six
TEST uses in `binary_serialization_tests.cc`; all 27 original name/range/
definition/header-digest records agree with the separate native Windows LLVM 19
audit. Hades source bytes remain unchanged. This does not establish native MSVC
equivalence or a complete compiler/filesystem/environment cache contract. The
observer is outside the repository and supplies no production recovery proofs,
parser roots or products. Opaque SDK boundaries still decline Hades ASSERT proof.
The same original Hades translation unit also passes native MSVC `/Zs` with its
recorded compile definitions/include roots and the Visual Studio environment;
output/multiprocess/debug-write flags are removed. This confirms that this
specific source is valid for its native invocation, not that LLVM and MSVC select
identical macro environments or that compiler-backed production recovery is safe.

The exact literal-save proof repair 5dc47fb passes all three full CI jobs in
37813761964. All sixteen remaining damaged C++ translation units with native
compile commands pass read-only MSVC `/Zs`, including `ui_tests.cc`; four other
source files have no matching command. This separates valid native source from
parser limitations without changing the source or suppressing diagnostics.

The native Windows LLVM 19 callback observer now also records its effective cc1
arguments/predefines, volatile builtin expansions, physical read-buffer digests,
failed searches, directory membership, redirects and filesystem metadata. Its
controls distinguish optional-header creation/removal and same-size content edits
with restored timestamps. Defines, language mode and MSVC compatibility settings
change the observed compiler context. These are read-only experiments, not a
complete or reusable dependency contract. Native MSVC `/E` and the observer emit
the same 622 main-file token spellings for `binary_serialization_tests.cc` under
its recorded command. Equality for that selected file does not establish general
MSVC equivalence, including dependency searches in compiler-specific SDK branches.
An explicit counterexample confirms that limitation: an optional header behind
`defined(_MSC_VER) && !defined(__clang__)` changes CHECK from a complete block to
an expression. With the directory timestamp restored, the complete observed
Clang filesystem/context/token trace remains identical across absent/created/
removed states; MSVC's expanded tokens differ and `/Zs` rejects the created
state's missing semicolon. MSVC returns to the original tokens/valid syntax after
removal. A Clang observation ledger alone therefore cannot authorize reuse of an
MSVC-targeted proof, even after one matching native token comparison. Native
preprocessing must be rerun or its own complete dependencies validated.

`cpp_macro_compiler_audit::audit` now brings physical callback observations into
the existing independent recovery proof without copied parser implementations.
Its input binds the exact source/path, captured definition buffers, physical byte
anchors, parameter counts and all main-file expansion names, including nested
effects. Invalid anchors/overlaps, conflicting buffers, stale sources, unfinished
observations, volatile inputs and resource limits decline the report. Unsupported
definitions or unobserved/invalid invocations cannot enable an offset-free name.
Preprocessor operators are conservatively tracked even without macro callbacks.
Function names expanded by the compiler are not reported as original functions.

This API returns only diagnostics, original macro/call/member spans and original
function names: no root, product, dependency identity or ExtractionEnv setting.
Normal production extraction still uses the existing metadata proof. The compiler
audit does not validate compiler selection, process environment, filesystems or
cache reuse, and cannot activate SDK-backed recovery in normal MCP operation.
A separate adapter verifies the original native callback buffer digests before
using this API on Windows and Linux: all 21 ASSERT diagnoses disappear, all 19
free argument calls and `first.empty()` retain original spans, and stale source
bytes decline. Pasted/function-generating TEST definitions remain unsupported;
no expanded function owners or graph improvements are claimed. Hades and the
productive `.2` remain unchanged.

Twelve compiler-audit regressions cover restored/unsupported definitions,
stale/incomplete/volatile observations, malformed anchors, conflicting buffers,
resource limits, nested effects, real missing semicolons/argument errors,
unobserved invocations, original member spans, generated function names and
thread/nested scanner-context restoration. Windows/Linux parser (61), evidence
(30/31), recovery (43), freshness/directive, bucketed/flat replay (15), member-call,
health, fourteen resolver, fifteen watched-MCP and six CLI configuration cases
pass. Native ingest Clippy and Linux default/no-builtins workspace Clippy pass.
The unchanged 179-case corpus, provenance and Actionlint checks pass. All 902
configured original C++ products remain identical after only source stat/identity
normalization; complete clone sketches match without normalization. Grammar,
schema, provenance and production extraction identities are unchanged.

The compiler observation audit also handles LLVM's implicit variadic parameter:
unsupported signatures decline only their definition before comparing supported
parameter counts. An unrelated valid ASSERT proof is retained. Actual observed
macro callee sites are omitted from the report, including member spellings;
argument calls and ordinary same-name calls after undef retain original spans.
The native Windows/Linux sixteen-source report agrees exactly: seven files are
parse-clean (132 statements), log_tests.cc recovers 37 ASSERT statements while
retaining other errors, and ui_tests.cc recovers 967 ASSERT statements while
retaining other errors. Input-manager proof still declines the entire ASSERT
name, and six observations are incomplete or differ from native preprocessing.
These are report-only results, not production graphs or normal SDK recovery.
The twelve controls and ingest Clippy pass on both platforms. All three full CI
jobs for 121c8ad are green (37821305016). Production identities/runtime are unchanged.

Compiler reports now scope proof to exact original invocation byte positions,
including the syntax class at each site. The vendored runtime supplies a private
byte-offset capability through a borrowed callback; only that runtime reads its
own Lexer layout. TSLexer ABI, generated parser, visible kinds and serialized
scanner state remain unchanged. Empty/name-wide/site contexts replace and restore
both TLS pointers together through nesting, unwinding and parser reuse. Original
UTF-8/BOM/CRLF/comment spans and absolute included-range offsets are tested.
Standalone upstream CLI compilation has no dependency on the private symbol.

Independent statement sites survive another unsupported, unobserved or invalid
same-name occurrence. Invalid arguments, ordinary missing semicolons and
incompatible expression contexts retain original diagnostics; mixed closed/open
if replacements retain the else binding for their respective positions. Report
invocation spans exclude the source semicolon/else clause belonging to the full
statement node. This precise mode is limited to the compiler report: normal
extraction retains the existing metadata/name-wide proof and production identities.

Four scanner controls and fourteen compiler-report controls pass on Windows/Linux,
as do the unchanged production parser/evidence/recovery/replay and resolver checks,
Clippy, provenance and all 179 corpus cases in the stock CLI and Rust harness.
Only the C++ provenance entry changes; no schema kinds are introduced. The two
full sixteen-source reports agree, now recovering 103 independent ASSERT sites in
input_manager_tests.cc while preserving its remaining errors; the previous seven
clean reports and log/UI results remain. SDK-backed production compiler selection,
native preprocessing and dependency validation are still required. No report is
turned into a production tree, product or normal MCP SDK recovery proof.
