# Fresh native C++ macro recovery

`cppMacroCompiler` explicitly selects a **trusted local preprocess-only driver**.
Listed translation units use the compiler path. Optional `cppMacroIncludeRoots`
can recover other files independently; a failed compiler capture never falls back
to metadata proof for its translation unit. Defaults remain unchanged.
This is a fresh compiler path, not a dependency-complete incremental cache.
Every listed translation unit is recaptured on extraction; its cached products
never authorize replay. Watched MCP graph/health queries run the environment-aware
pipeline again even when source files and timestamps have not changed. An
authorized source root is required; a frozen external index cannot claim freshness.
This costs native preprocessing on every such query and can be slow on large SDKs.

```yaml
ruleDirs: []
cppMacroCompiler:
  program: C:/Python312/python.exe
  arguments:
    - C:/dev/vorpal/scripts/cpp_macro_compiler_provider.py
    - --plan
    - C:/audit/preprocess-plan.json
    - --observer
    - C:/tools/observer/vorpal_cpp_macro_observer.exe
    - --resource-dir
    - C:/tools/llvm19/lib/clang/19
    - --libclang
    - C:/tools/llvm19/bin/libclang.dll
    - --python-bindings
    - C:/tools/llvm19-python
    - --native-hash-lexer
  directory: C:/dev/project
  translationUnits: [C:/dev/project/tests/main.cc]
  timeoutSeconds: 60
```

Paths, including translation units, resolve against the configuration directory.
Preserve the project's existing language routing (`languageGlobs`); a C++ header
misrouted as C is a different parser input and is not a valid recovery comparison.
Use the original project's Visual Studio developer environment for native MSVC.
The driver does not infer a Windows compiler environment from a Linux/WSL build.
`scripts/cpp_native_mcp.ps1` launches the Windows MCP with an explicit developer
shell path, source authorization, external index and retained configuration. It
preserves UTF-8 stdio when invoked from WSL or a stdio tunnel command:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:/dev/vorpal/scripts/cpp_native_mcp.ps1 `
  -VorpalPath C:/tools/vorpal.exe -SourcePath C:/dev/project `
  -IndexPath C:/cache/project-index -ConfigPath C:/audit/project.json `
  -VsDevShellPath 'C:/Program Files/Microsoft Visual Studio/2022/Community/Common7/Tools/Launch-VsDevShell.ps1'
```

The launcher requires an external index path and propagates the server exit
status. `-NoWatchRebuild` and `-NoAutoWarm` are optional controls for isolated
quiet-query checks; neither implicitly changes the production tunnel profile.
Nothing downloads or installs tools, changes source files, or enables recovery
merely because a name is uppercase. Unknown directives, missing tools, failures,
unsupported expansions and genuine syntax errors retain raw syntax/diagnostics.

The bundled Python driver requires local LLVM 19 CIndex bindings and library,
the matching resource directory, and the physical callback observer built from
`scripts/cpp_macro_observer`. Configure CMake with explicit `LLVM_DIR`, `Clang_DIR`
and `VORPAL_CLANG_DRIVER`; `VORPAL_DIA_GUIDS` can replace an unavailable DIA library
referenced by an imported Windows SDK. These are developer tools, not release assets.
The command plan is a JSON array of `{source, cwd, args, translationUnit?}` with
the original physical source and trusted **preprocessing-only** argument array
ending in the actual translation-unit path and `/Zs`. Without `translationUnit`,
the original source is the translation unit. An explicit `translationUnit` can
select a header or included source inside its genuine compile/Unity context;
`translationUnits` in the configuration still lists the physical files to extract.
The observer selects the actual FileEntry and requires exactly one preprocessing
visit. Missing or repeated physical visits decline capture. Both the selected
source and actual translation-unit bytes are rechecked before returning. Strip
object/PDB/linker/output/dependency-emission flags before producing the plan.
Response files and output-producing options are rejected. Keep original defines,
include order, forced input headers, language and native preprocessor settings.

The driver runs native `/E`, then the physical observer, then native `/E` again.
Independently lexed root token streams must agree; only CRLF inside complete raw
literal payloads may differ. Token boundaries protect fake `#line`/`#pragma` text
inside literals. Balanced MSVC `external_header(push/pop)` diagnostic frames
are omitted from the ephemeral native-output root projection only when each
hash is an actual lexer token and the following physical `#line`
transition matches the full nested frame stack. Indented hashes and LF/CRLF
are handled by physical byte offsets; vertical whitespace is not a line break.
At most 64 intervening ASCII-whitespace lines are permitted. A forced PCH wrapper
may restore the actual TU only in the first frame, with the exact trusted `/FI`
input and translation-unit path. Tokens, extra directives and malformed returns
remain boundaries. Literal `#pragma once` is omitted only at its exact native
line after an active hash-pragma callback verifies the original physical byte
offset and immutable source-buffer digest. Macro operands, `_Pragma`, `__pragma`,
literal payloads and unknown pragmas do not supply this evidence.
Every physical file named by an actual marker is read afresh (at most 2,048
files, 4 MiB per file / 32 MiB total). Missing/unreadable/oversized inputs decline
capture. Any occurrence of `external_header` in those original buffers, including
comments and literal payloads conservatively, disables frame omission. This
keeps authored header/pragma-macro forms as boundaries rather than treating the
spelling alone as generated proof. Malformed/unbalanced frames and actual unknown
native directives decline recovery. Native compiler flags and original source
bytes are unchanged. Source,
definition buffers, native toolchain/environment/plan and native before/after
streams must remain stable. Clang filesystem observations alone never authorize
native replay: a native-only optional header can change MSVC behavior while its
Clang observation is unchanged.

Projection path resolutions are memoized only within one native stream and
rechecked against current redirects before returning. Only punctuation tokens
need hash-spelling queries; literal contents never become directive offsets.
The observer copies filesystem buffers into owned memory before reusing their
digests by SourceManager FileID within one fresh preprocessing action. Neither
memo survives into another stream/action/capture or authorizes product replay.

`--native-hash-lexer` is an explicit optimization requiring a rebuilt matching
observer. Its `--native-hash-offsets` mode raw-lexes the already captured native
stream as C++20/MS extensions, without preprocessing, includes or semantic
parsing. Only actual physical `#` tokens are emitted; the driver verifies the
entire buffer SHA-256, byte length and bounded unique ordered offsets before
projection. CIndex still independently tokenizes the projected root for native
before/after/observed token agreement. Omitting the option retains the CIndex
full-stream path; a failed selected raw-lexer run declines the capture. Nine
stdlib integrity/projection controls run on Windows/Linux CI/release gates.

## Provider protocol v1

The configured program runs directly, with no shell interpolation. Environment
variables `VORPAL_CPP_COMPILER_REQUEST`, `VORPAL_CPP_COMPILER_RESPONSE` and
`VORPAL_CPP_COMPILER_READY` name unique temporary files. The provider **must wait
for READY to exist before spawning compilers**. The launcher owns descendants via
a Windows kill-on-close job or dedicated Unix process group. Requests contain
`version: 1`, `requestId`, physical `path` and exact UTF-8 `source`. The response
must echo all four; an older capture is rejected. Output/stderr are bounded,
timeouts kill the process tree, and a nonzero exit declines recovery. Flat capture
files are bounded during execution (32 MiB for packet/stderr, 128 MiB per native
artifact, 128 files / 512 MiB total); oversized captures terminate the provider.

The response contains:

- `complete`, `volatileInputs`, `nativeContextBefore`, `nativeContextAfter`,
  `observedContext`. Contexts are opaque digests, never environment/secret dumps.
- `nativeBefore`, `nativeAfter`, `observedTokens`: independently captured token
  spelling arrays. Native before/after arrays and native contexts must be equal.
- `nativeDirectives`: actual directives outside literals; currently must be empty.
- `definitions`: `{path, source}` original captured physical buffers.
- `expansions`: nonoverlapping outer physical `{name, start, end, definition}`
  invocation ranges; optional definition anchors are
  `{buffer, nameOffset, end, parameters}` using original byte offsets.
- `expandedNames`: every rooted expansion name, including nested effects.
- `calleeSites`: every physically spelled rooted `{name, start}`, including source
  argument expansions nested inside outer sites. These suppress only observed
  macro callees/definition names, preserving ordinary same-name calls after undef.

The parent independently rechecks current bytes, anchors, argument arity,
replacement syntax and context before enabling exact original scanner positions.
It never parses preprocessed output as the source document, rewrites/masks source,
manufactures expansion owners, or suppresses parser errors globally. Owned,
streaming and Scan handoff share this path; recovered trees/walk snapshots bypass
the incremental cache. Capture identities omit handshake nonces so unchanged
products/generations remain deterministic, but **cannot authorize replay**.

This protocol trusts the explicitly configured producer to capture physical
compiler facts correctly; arbitrary saved JSON is not a production proof. A
dependency-complete native cache and unsupported SDK/directive cases remain work.

Runtime argument edges require an evaluated parameter use in a proven statement
replacement. Ignored/stringified/unevaluated arguments and unsupported expansions
do not supply that evidence. Physical macro-supplied function/type names are not
published as original named owners; ordinary names after undef remain intact.

## Native stringification and token pasting

Fresh native observations also support complete statement replacements using
`#parameter` and identifier token-paste chains (`prefix ## parameter ## suffix`).
This covers the test-call generator that stringifies its label and pastes a
function name. Operator interpretation is confined to the ephemeral syntax proof;
original source, macro/argument spans and product owners remain unchanged.
Pasted targets and stringified text are not published as original runtime calls.
An argument separately used in an evaluated expression retains its original call.

Comments and literals are distinguished from operators. Invalid paste operands,
UCNs, manufactured pragma identifiers, oversized parameter amplification and
missing original do/while semicolons decline recovery. Metadata-only evidence
still treats pasting effects as opaque; it cannot use native-only proof.
Complete function/declaration-list generators use the native role described below.

The application, log and UI test files are clean. Nested SDL argument-macro diagnostics
are handled by the numeric argument evidence described below. The installed
release and productive MCP were not changed.

The genuine CMake UI Unity context now supplies native proofs for all 41
state-storage definitions and 41 declarations in `state.cc` / `state.h`. Fresh
native and observed physical token streams agree. The complete isolated index
and normal source-authorized native MCP report **8 error-bearing files / 158
diagnostics / 951 diagnostic bytes**, compared with the installed `.3` baseline
of 15 / 378 / 951. Both State files are clean. Native controls cover LF/CRLF,
single versus repeated/missing physical visits and a macro-produced pragma whose
token mismatch still declines recovery. Hades and the productive release remain
unchanged; glTF shared-fragment and vendor-generator contexts remain follow-up.

The glTF `gltf_io.cc`, `gltf_accessors.cc`, and `gltf_materials.cc` files are
intentional textual include chunks assembled by `mesh_asset_parse.cc`, including
across template/function boundaries. Their standalone diagnostics do not prove
source corruption. Correct handling requires compilation-context and physical
span provenance. Three Lua guide diagrams, by contrast, contain actual NUL and
control bytes in the original Markdown. Those bytes are not parser generators.

## Fully expanded native statement generators

The provider can additionally emit `observedTokenSites`: an array parallel to
`observedTokens` containing `{offset, fromMacro}` physical root expansion facts.
Older producers can omit it and retain direct replacement proofs. It does not
authorize replay or replace the original document. Native before/after spelling
agreement still covers the complete token stream.

A generator invocation with identifier callback arguments can use its fully
rescanned tokens as an ephemeral complete-block proof. The parent verifies the
physical definition/arity, exact original arguments, one contiguous token slice,
macro origin at the original invocation and a direct original block position.
Authored commas, expression uses and if/loop arms cannot borrow this statement-list
proof. Truncated expansion syntax and invalid token sites preserve raw errors.
Ignored/generated identifiers do not become original runtime edges. Owned,
streaming and Scan handoff use the same parser; native products remain nonreplayable.

The isolated full Hades index now has 11 error-bearing files and 244 diagnostics
(951 diagnostic bytes), down from the installed release's 15/378/951. All six
`ALL_COMPONENT_TYPES(DISPATCH_*)` sites in `registry.cc` are clean. No Hades source
or productive runtime was modified. Physical include/Unity projection for the remaining state-storage files remains separate work.

## Native function-prefix generators

An exact fresh expansion can also supply a complete function prefix followed by
an original authored body, such as `TEST(name) { ... }`. The parent independently
checks the physical definition, invocation arity, original top-level or namespace
position, contiguous macro-origin token slice and a single complete function
prefix. The scanner role is site-specific and cannot be borrowed inside a block.
Numeric and identifier generator arguments preserve their original byte spans.

Only the ephemeral proof contains the expanded function prefix. The original
body is parsed directly and keeps its runtime calls and diagnostics. Its owner
is anonymous: a name created by token pasting is not published as a name authored
in the source. Ordinary same-name definitions after undef remain named owners.
Nested, thread-local and panic-unwinding contexts restore the prior scanner role;
default parsing and metadata-only parsing gain no generator authorization.

The former four input-test diagnostics involved `SDL_BUTTON_LEFT` inside `ASSERT`
arguments, not `TEST` function heads. Function-prefix support alone did not clear
them; the numeric argument evidence below does.

## Numeric object macros inside native statement arguments

The provider may report optional `literalArguments` physical sites, limited to
object-like macros inside an anchored original function-like expansion. Each
site carries the physical definition anchor and is revalidated against current
header/source bytes. The parent requires an original identifier at that span and
a definition consisting of one numeric literal. Other expressions, function-like
effects, overlapping/duplicate sites and stale anchors decline this proof.

Only the ephemeral argument proof substitutes that literal. Original source and
argument spans remain unchanged, and any other expanding token still blocks
argument recovery. The original statement template supplies the evaluated
parameter mask and the original do/while semicolon requirement. Ignored or
unevaluated calls remain omitted; ordinary evaluated argument calls keep their
original ranges. This adds no replay permission.

The fresh isolated Hades index and normal native MCP now have 10 error-bearing
files / 240 diagnostics / 951 diagnostic bytes. The input tests are clean, and
four original `GetMouseButtonState` call references are additionally resolved.
The productive installed release and MCP remain unchanged.

## Complete native function/declaration generators

A fresh anchored invocation may own a complete list of function declarations or
function definitions, including explicit template specializations. The parent
requires a direct original TU/namespace call, exact arity, and one contiguous
fully macro-origin token slice at the invocation offset. The ephemeral token
proof must contain complete function declarations/definitions; expression lists,
truncated syntax, incorrect origins and local/control-arm contexts decline.

The scoped `DeclarationList` role produces `macro_declaration` with only the
original name and arguments. Expanded function names and expanded bodies are
never fabricated as original declarations, and generator arguments do not invent
runtime calls. Following original functions and their diagnostics remain intact.
Owned, streaming and Scan handoff share the proof, and native replay stays disabled.

Windows/Linux regressions, the unchanged full corpus, Clippy, C++ provenance and
schema checks pass. A genuine native MSVC fixture also recovers through normal
MCP; `/Zs` accepts the positive declaration generator and rejects an original
missing semicolon. The source checkout and productive `.3` stay unchanged.

`state.cc` is compiled through CMake's generated `hades-ui` Unity source, explaining
its absence as an individual compiler-database entry. Native physical-file
projection in that genuine context now makes all 41 definitions and 41
declarations clean while retaining their original invocation spans.

### Native case/loop prefixes

`CaseLoopPrefix` admits an original direct switch-body invocation followed by
an authored compound body. Its exact macro-origin token slice must independently
parse as one case label and one complete for-loop prefix whose body is the
appended empty proof block. Extra cases, expanded bodies, truncated prefixes,
incorrect origins and control-arm contexts decline. The original parse aliases
the role to `case_statement` with original name, arguments and body; expanded
labels and loop declarations are not invented as original nodes. Body calls
retain physical spans, and a missing body semicolon stays an error. Owned,
streaming and Scan extraction use the same proof; native products cannot replay.

Native definition endpoints exclude independently parsed trailing comments,
including LF/CRLF and block comments. Literal comment-like payload and opaque
preprocessor `#`/`##` operators are retained, and interior-token or comment
endpoints decline. Syntax leaf endpoints cannot prove complete preprocessor
token coverage and are not used for this check.

The normal isolated MCP now reports 8 error-bearing files / 132 diagnostics /
947 bytes. All 24 `STBI__CASE` sites and both `STBI__IDCT_1D` statement lists
are clean in a separately tested scalar
implementation context (`STB_IMAGE_IMPLEMENTATION`, `STBI_NO_SIMD`,
`STBI_NO_THREAD_LOCALS`, `/X` and explicit native include roots). Native `/Zs`
accepts that original header and the positive prefix fixture, and rejects the
missing-semicolon control. This does not claim SIMD compiler streams agree;
mismatched streams still decline. One stb diagnostic remains. Productive `.3`
and the Hades checkout are unchanged.

Native statement-list arguments can also be simple atoms or subscripts without
calls, side effects or expanding argument names. A raw erroneous assignment may
have absorbed the macro call as its left operand and the following original
identifier as an error; proof requires a separate physical identifier after the
exact invocation and a direct block position. Real operators continuing the call
and control-arm/member contexts do not qualify. The original source is reparsed
under the exact site proof, preserving following assignments and missing
semicolons. Expanded declarations/calls acquire no fabricated original spans.

### Native inline specifiers

The optional `specifierMacros` packet field proves object-like modifiers such as
`stbi_inline`. Only a fresh physical zero-parameter definition containing exactly
`inline`, `__forceinline` or `__inline` qualifies. The parent requires an exact
original identifier span, a non-nested anchored expansion and exactly one matching
macro-origin token. Stale definitions, extra tokens, wrong offsets/origins and
function-like macros decline. The scoped `InlineSpecifier` role uses the existing
`storage_class_specifier` kind, without globally treating identifiers as keywords.
Authored function types, names, bodies, calls and syntax errors retain their spans.
Ordinary same-name calls after undef remain ordinary calls; default parsing has
no specifier authorization. Owned, streaming and Scan agree; native replay stays
disabled. Older providers may omit the field.

The isolated normal native MCP now reports **4 error-bearing files / 55 diagnostics
/ 593 diagnostic bytes**. The explicitly validated scalar stb header is completely
clean. Catch2 and three textual glTF include chunks remain. Productive `.3` and
the Hades checkout are unchanged. Windows/Linux regressions and native MSVC
positive/negative controls pass; differing SIMD compiler streams still decline.

### Native exception prefixes and directive inventory

`controlMacros` optionally supplies fresh zero-parameter object definitions that
are exactly `try` or `catch (...)`. `TryPrefix` and `CatchAllPrefix` consume only
the original identifier in existing `try_statement`/`catch_clause` nodes. A
function-like handler with a literal `catch (parameter)` replacement requires one
fresh parameter, a separately parsed original parameter list and a matching
contiguous macro-origin token slice. `CatchParameterPrefix` keeps that authored
parameter list. An object catch-all creates no fabricated parameter nodes.
Bodies, ordinary calls after undef and real syntax errors preserve their spans;
empty/multiple parameters, changed types, incorrect origins and stale anchors decline.

The observer records literal `__pragma(warning(push/pop))` tokens at the physical
callback because MSVC `/E` retains these operators while LLVM consumes them.
The entire token stream still has to agree. Parameterized/expanding operands,
numeric push levels and other operators are not substituted or discarded.

The native directive inventory is ordered and must be identical before and after
observation. Only literal physical hash-pragma callbacks can supply `pragmaOffsets`.
The parent independently parses those source offsets and admits matching warning
push/pop, one numeric warning-disable code, and optimize with an empty option
string plus on/off. Counts, order and complete directive spellings must match;
unknown native directives, expanded operands, other optimization strings, stale
offsets and missing/duplicate observations decline. Whitespace inside strings
remains significant. Observer-only diagnostic callbacks absent from native `/E`
provide no native directive proof and do not change existing token-agreement behavior.
Every original directive remains in the source tree; this is not source masking.

Windows/Linux production, audit, evidence/recovery, language and replay/resolver
checks pass, along with the unchanged complete corpus and all 179 stock C++ cases.
The explicit native script `scripts/tests/run_cpp_macro_warning_pragmas.py` covers
LF/CRLF positive and negative MSVC syntax/token controls. Catch2's original header
passes native `/Zs`; all 79,049 native and observed token spellings agree in its
explicit MSVC benchmark context, including ten literal warning operators.
Eight try prefixes, seven catch-all prefixes and one typed handler are proven.

The isolated normal MCP now reports **4 error-bearing files / 8 diagnostics /
90 diagnostic bytes**. Catch2 accounts for five diagnostics / 68 bytes; three
glTF textual include chunks account for the remainder. The original sources and
productive `.3` remain unchanged. Catch2's inactive Objective-C annotations,
split conditional handler and remaining unproven contexts need further work;
this measurement does not claim they are supported or error-free.

### Complete native registration declarations

Direct original TU/namespace invocations may now prove complete variable
declarations and nonempty namespace bodies containing complete declarations,
as well as existing function/declaration generators. A literal balanced
`__pragma(warning(push))` / `__pragma(warning(pop))` envelope is classified
around the declaration proof. Its tokens remain in the fully compared native
stream; unpaired or other operators decline. Original source is never rewritten.

Only the original macro name and arguments enter the declaration tree. Generated
registrar names and constructor calls are not authored definitions or runtime
edges. Non-function generators with effectful original call arguments decline
until their runtime use can be proved separately. Wrong original scope, truncated
declarations and real missing semicolons remain errors. Owned, streaming and Scan
agree; native replay stays disabled.

All four `CATCH_REGISTER_REPORTER` invocations in the native Catch2 header are
clean. Windows/Linux production regressions and Clippy pass; the native warning
control script covers 30 LF/CRLF syntax/token cases including registrations. The
isolated normal MCP retains four error-bearing files, with eight diagnostics and
90 affected bytes. Five diagnostics / 68 bytes are in Catch2; the three genuine
glTF include fragments remain. Productive `.3` and Hades source are unchanged.

### Complete conditional C++ catch groups

An existing ordinary or proof-backed C++ try statement accepts a complete
`#if`, `#ifdef` or `#ifndef` group containing one or more complete handlers. The
guard keeps the existing `preproc_if`/`preproc_ifdef` kind and original condition;
handlers keep their original parameter lists and body/call spans. Conditions are
not evaluated. These C++ additions are derived after the Objective-C copies, so
reproduction does not introduce them into Objective-C exception rules.

Empty or unclosed handler groups, unsupported else arms, bare try statements and
authored missing semicolons remain errors. LF/CRLF and UTF-8 tests cover ordinary
and scoped proof-backed try prefixes, original handler spans, following functions
and ordinary runtime calls. All 179 unchanged C++ cases and the complete compiled
corpus pass, as do Windows/Linux extraction/replay/resolver tests, Clippy, native
MSVC controls, provenance and schema checks. Reproduction is byte-identical and
retains the ASCII lexer fast path. No new visible syntax kind is introduced.

The renewed isolated normal MCP reports **4 error-bearing files / 7 diagnostics /
45 diagnostic bytes**. Catch2 retains four diagnostics / 23 bytes: two inactive
Objective-C macro qualifiers and two active warning/no-op macro markers. The three
glTF textual include fragments retain their shared-context boundary. Productive
`.3` and Hades source remain unchanged.
