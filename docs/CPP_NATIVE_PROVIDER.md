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
The command plan is a JSON array of `{source, cwd, args}` with the original physical
source and trusted **preprocessing-only** argument array ending in `/Zs`. Strip
object/PDB/linker/output/dependency-emission flags before producing the plan.
Response files and output-producing options are rejected. Keep original defines,
include order, forced input headers, language and native preprocessor settings.

The driver runs native `/E`, then the physical observer, then native `/E` again.
Independently lexed root token streams must agree; only CRLF inside complete raw
literal payloads may differ. Token boundaries protect fake `#line`/`#pragma` text
inside literals. Balanced MSVC `external_header(push/pop)` diagnostic frames
are omitted from the ephemeral native-output root projection only when each
hash is an actual lexer token and the immediately following physical `#line`
transition matches the full nested frame stack. Indented hashes and LF/CRLF
are handled by physical byte offsets; vertical whitespace is not a line break.
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
full-stream path; a failed selected raw-lexer run declines the capture. Six
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
Declaration-list generators still require separate context support.

The application, log and UI test files are clean. Four nested SDL argument-macro diagnostics
remain in the input tests; native-only proofs do not conceal them. The installed
release and productive MCP were not changed.

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
or productive runtime was modified. Template-specialization declaration generation remains separate work.

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

The four input-test diagnostics involve `SDL_BUTTON_LEFT` inside `ASSERT`
arguments, not `TEST` function heads. They require separate safe argument-expansion
evidence; function-prefix support alone is not reported as clearing them.
