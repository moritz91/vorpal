# Native MCP latency measurements

Use `scripts/benchmark_cpp_native_mcp.py` in the real compiler developer
environment. It builds a new external index with `--verify`, measures the stdio
initialization handshake and repeated `health`, `graph` and `snippet` requests,
then closes the MCP process. Source files and the productive index are never
edited. The native provider, include roots and textual include contexts come
from the supplied configuration. Native captures remain mandatory on each
query; watcher rebuilding and automatic warming are disabled to make the work
attributable to the request.

```powershell
& 'C:/PROGRA~1/Microsoft Visual Studio/2022/Community/Common7/Tools/Launch-VsDevShell.ps1' -Arch amd64 -HostArch amd64 -SkipAutomaticLocation | Out-Null
python scripts/benchmark_cpp_native_mcp.py `
  --binary C:/Users/morit/.local/bin/vorpal.exe `
  --source C:/dev/hades-II `
  --config C:/Users/morit/.local/share/vorpal/native-moritz5/project.json `
  --index C:/Users/morit/.codex/tmp/native-latency-new-index `
  --report C:/Users/morit/.codex/tmp/native-latency.json `
  --symbol ReadQuatAccessor --build --rounds 3
```

`--build` refuses an existing index, and all runs require an external index.
Omit `--build` to measure an existing isolated index. The JSON report contains
binary/configuration digests, the server's version, each request's monotonic
duration, content digest and generation, medians/ranges, and extraction/link
phase medians from the accompanying `VORPAL_PHASE_TRACE` log. Nonempty symbol
results and unchanged answers/generations are checked. The initialization time
is only the protocol handshake: the first tool response includes graph loading
and its synchronous native revalidation. The report does not contain compiler
flags, environment values or full source snippets.

For a provider-only comparison use the same released binary, an isolated copy
of the configuration pointing to the candidate provider, and a new external
index. Keep the compiler, plan, observer, SDK, source and environment fixed.
Compare the complete stored node families, graph edges/confidences, source
contexts, names and dataflow with the freshly built baseline. A provider/config
change legitimately changes proof identities and generation IDs.

## Capture-local lexical reuse

The provider retains at most one full-output hash-offset result and one
projected-root token/directive result **inside a single native capture**.
Reuse requires exact complete bytes, including markers, line endings and raw
literal payload. Results are immutable; a changed buffer is lexed independently
and exceptions do not populate the memo. Physical directive lines are classified
once from the original full-stream lexer offsets for both projection passes.

Each native stream still resolves its physical marker paths independently,
reads every frame input again, checks authored markers and redirects, and runs
the complete projection. Both native preprocessing runs, the independent
observer, compiler/toolchain/environment identities, definition/source checks,
directive inventory and token agreement stay in place. Nothing survives into
the next request. No native observation packet, parse tree, extraction product
or graph receives a new replay gate.

## Requirements for reuse between requests

The ordinary observer's include list is insufficient: MSVC-only branches can
consult headers and negative `__has_include` candidates unseen by Clang. An
authoritative native dependency contract must cover positive reads, missing
search candidates in order, shadows/redirects, forced headers, compiler/resource
inputs, provider/observer/library bytes, working directory, configuration and
environment. Edits with restored timestamps, header creation/removal and changes
outside the source watcher must invalidate it. Validation must also bracket
concurrent changes before serving or banking a graph.

Until that contract is available, cached native proofs cannot be trusted from
timestamps, watcher silence or a prior dependency digest. The capture-local
optimization supplies measured savings without changing this boundary. Future
dependency-aware replay must demonstrate the same definitions, original spans,
graph edges and diagnoses as a fresh complete build, including the native-only
optional-header and genuine split-source controls.

## Windows Hades measurement, 2026-10-11

The next [report-only MSVC dependency audit](CPP_NATIVE_DEPENDENCY_AUDIT.md)
observes native file probes and directory inventories, with original bytes and
negative states bracketed. It exposes remaining completeness/race boundaries;
its reports never authorize reuse between requests.

Both runs use published `0.10.2+moritz.5`, 20 explicit MSVC source contexts,
the same genuine textual include group and 1,744 unchanged physical files.
Only the candidate provider path changes in the isolated configuration. Three
ordinary requests per tool are measured after a fresh verified build.

| Operation | Installed provider | Candidate provider |
| --- | ---: | ---: |
| Verified cold build | 13.20 s | 12.07 s |
| Health median | 16.36 s | 15.60 s |
| Callees graph median | 16.22 s | 15.69 s |
| Multipart snippet median | 16.07 s | 15.31 s |
| Extraction phase median (9 requests) | 15.32 s | 14.55 s |
| Graph linking phase median (9 requests) | 0.044 s | 0.047 s |

This is a modest 3–5% end-to-end median reduction in one local comparison, not
a cross-machine rate guarantee. Health ranges overlap (baseline 16.34–16.96 s,
candidate 15.21–16.34 s); graph ranges are 16.10–16.35 s versus 15.27–15.90 s,
and snippet ranges 16.06–16.30 s versus 15.28–15.99 s. Repeated requests still
perform full fresh compiler revalidation. A separately instrumented Catch2
capture reduces lexer calls from two to one while both physical projections,
both toolchain context checks and all marker resolutions remain in place.

The candidate and cold baseline have exactly equal stored node-family bytes,
all 148,881 graph edge/confidence triples (including approximate similarities),
`names.idx`, `source_contexts.json` and `dataflow.bin`. They retain 61,627 nodes
and the same two inactive Catch2 diagnoses / 13 bytes. Generation IDs differ
because provider/configuration identities are part of proof provenance.

A real native-only optional-header MCP control passes creation, same-mtime
edit/restoration and removal while the source bytes and directory timestamp stay
unchanged. The changed expression replacement remains diagnosed; removal restores
the initial generation. The installed binary/configuration and productive tunnel
are unchanged. Local reports and phase logs are kept outside both repositories.


## Capture phase profiling and sparse MSVC projection, 2026-10-12

`scripts/profile_cpp_native_provider.py` is an opt-in profiling wrapper for a
private copy of the compiler configuration. Leave `program` pointing at the
same Python interpreter and prepend these arguments to the original provider
script and arguments:

```text
C:/dev/vorpal/scripts/profile_cpp_native_provider.py
--output-dir C:/Users/morit/.codex/tmp/native-phase-profiles
<original provider script and all its original arguments>
```

Run the ordinary benchmark against an external index in the actual VS developer
environment. The Rust launcher still owns the provider job, ready handshake and
timeout. Reports are created only after successful captures, with unique capture
filenames, hashed source-context identities and at most 90 function timing rows.
File basenames, call counts and timings are retained; compiler arguments,
environment values, absolute paths and source contents are not emitted. Separate
subprocess timings identify both native passes, the observer and hash lexer.
Function timings are inclusive and overlap; do not sum them as disjoint phases.
Profiling itself adds overhead. Always disable the wrapper for latency comparisons.
Neither profiles nor their timing identities authorize replay.

Sixty successful profiled captures per variant cover the same twenty genuine
MSVC source contexts across health/graph/snippet requests. The median combined
`project_root` cost per capture drops from 1.203 s to 0.421 s; native stream
processing drops from 2.196 s to 1.467 s. Native before/after subprocess medians
remain about 0.31–0.35 s each, observer 0.37 s, hash lexer 0.041 s and toolchain
context checks 0.17 s. These are profiler observations, not end-to-end savings.
The previous Registry capture made roughly 1.9 million marker checks, mostly
visiting SDK lines which contribute no root output.

The MSVC projector now visits only physical directive lines identified by the
independent full-stream hash lexer and copies intervening bytes directly. Marker
syntax is decoded once inside a projection, but actual physical path resolution
still runs in both passes and administrative lookahead. Each native stream is
projected independently; every physical frame input is reread and redirect
checks still run. The native before/after compiler runs, observer, token
agreement, definition-buffer validation and toolchain checks are unchanged.
No native capture, file read or path result is reused between requests.

This changes only processing of captured native output, never the original
source. LF/CRLF, Unicode/raw literals, token-proven fake marker boundaries,
unknown directives, physical pragma-once line numbers, forced-header restoration
and the exact 64-blank-line frame limit are retained. Invalid frames continue to
decline. Five new projection regressions cover these boundaries, and one covers
bounded profiling/path privacy. All 60 Python controls pass on Windows/Linux.
An additional private comparison of 5,000 deterministic LF/CRLF streams has
exactly equal output bytes and decline reasons against the preceding projector.

Unprofiled, consecutive genuine Hades runs use published `.5`, identical compiler,
observer, plan, source and environment, and new external verified indices. Only
the private provider location/projector bytes change. Each tool is measured
three times after its cold build:

| Operation | Previous projector | Sparse projector |
| --- | ---: | ---: |
| Verified cold build | 12.321 s | 11.712 s |
| Health median (range) | 15.866 s (15.577–15.874) | 14.851 s (14.755–14.900) |
| Graph median (range) | 15.742 s (15.717–15.767) | 14.741 s (14.628–14.877) |
| Snippet median (range) | 15.573 s (15.542–15.587) | 14.848 s (14.779–15.028) |
| Extraction median, nine requests | 14.772 s | 13.889 s |
| Graph-link median, nine requests | 0.046 s | 0.044 s |

This local comparison gives 4.7–6.4% lower tool medians, approximately 0.7–1.0 s
per request; it is not a cross-machine guarantee. All 88 semantic index files,
including full node/span families, graph edge/confidence bytes, names, contexts
and dataflow, match exactly. Graph/snippet payloads match exactly. Health retains
the same file, diagnostics and error spans: two inactive Catch2 diagnoses / 13
bytes in 1,744 files. Only its explicitly displayed per-product `extraction-id`
differs, as expected when provider configuration/projector provenance changes;
comparison separates that exact field and retains the original reports.

A private native-only optional-header MCP fixture passes creation, same-mtime
edit/restoration and removal with unchanged source bytes and restored directory
timestamps. Two additional private controls write a real macro header from a
second thread, with its timestamp restored: after the first native run, and
during the first root projection. Both decline recovery and retain the original
syntax diagnostic. Restoring the header and taking another complete fresh
capture restores the clean graph and original generation. These deterministic
observed mutations do not claim exhaustive OS input coverage or close the
atomic-publication boundary documented by the guarded native capture audit.

Private reports/fixtures are under `C:/Users/morit/.codex/tmp/vorpal-sparse-projection`,
`vorpal-sparse-native-quiet` and `vorpal-sparse-concurrent`. Hades, the installed
`.5` binary/configuration/index and productive MCP tunnel remain unchanged.
