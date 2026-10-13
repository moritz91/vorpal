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

## Native compiler dispatch, 2026-10-13

The `.8` timeline identifies delayed dispatch rather than an insufficient worker
count. In the two repeated profiled requests, the twenty native contexts start
across 7.81-7.88 seconds; `ui_tests.cc` starts after about seven seconds and is
the last capture to finish. Each capture still executes both native passes and
the independent observer. Registry/UI/Lua and state contexts dominate processing.
The profiler now records monotonic capture/subprocess start clocks and Python
CPU time, using only hashed source identities and the existing bounded function
summary. Failed captures emit no report. Profiling adds overhead; these clocks
are diagnostic evidence rather than ordinary response latency.

The spilled index stream gives already-admitted, explicitly configured native
compiler files a separate bounded priority work lane. Canonical entry admission,
source-byte reservations, sequence numbers, in-shard application and shard
absorption order stay unchanged. Only dispatch among admitted entries changes.
This avoids starving an unadmitted canonical prefix under a small byte budget:
workers can still drain ordinary admitted work when the priority lane is empty.
Default extraction keeps its existing FIFO lane, and small trees retain their
serial path. The hint is neither a cost estimate nor a freshness/replay proof.

All twenty contexts start within 0.50-0.53 seconds in repeated candidate profiles,
with all twenty providers overlapping. The tail moves to Registry; individual
native subprocesses sometimes take longer under the increased concurrency.
An unprofiled worker sweep confirms that merely reducing the global worker count
does not help on this 64-logical-CPU host: the eight-worker tool requests take
27-28 seconds, sixteen workers take 19-20 seconds, and four take 44-45 seconds,
against approximately 17 seconds with the unchanged default. These are local
measurements, not a portable choice of worker count. No worker override or
provider/SDK configuration change is installed.

Three alternating unprofiled pairs per tool use published `.8` and the final
locally built candidate, identical native provider/configuration and separate
index copies. The first request is retained in the results:

| Tool | Published `.8` median (range) | Candidate median (range) | Median paired saving |
| --- | ---: | ---: | ---: |
| Health | 15.444 s (15.118-17.152) | 15.200 s (12.564-19.856) | 1.952 s |
| Graph | 17.063 s (14.576-17.543) | 12.693 s (12.401-14.519) | 2.544 s |
| Snippet | 16.220 s (14.618-16.873) | 12.504 s (12.398-14.619) | 2.254 s |

Paired savings are medians of individual differences, not differences of column
medians. The first candidate health request is slower (19.856 s against 15.444 s),
so this is a repeated-request improvement, not a startup-latency guarantee.
Timing drift remains visible in both variants. All 106 semantic graph/span files,
graph/snippet answers and original diagnostics match exactly; only an explicitly
displayed extraction identity may differ with provenance. The same two inactive
Catch2 diagnoses / 13 bytes remain visible. Native compiler proofs are still
fresh on every request; none are reused to obtain these timings.

An independent startup control launches a new MCP process and uses a fresh
index copy for each measurement. Reversing baseline/candidate process order
confirms the tradeoff: first health takes 15.917/16.217 seconds on `.8` against
17.831/19.816 seconds on the candidate. The immediately repeated health takes
16.741/16.825 seconds against 14.492/14.435 seconds. Answers remain identical.
The extra 1.9-3.6 seconds on initial health must be considered before deployment;
the candidate has not replaced the productive `.8` service.

Tests compare priority/FIFO graph bytes across parsed/replayed/skipped entries,
mixed/all/empty priority sets and deliberately starved one-byte admission. Both
channel closure orders drain, and failures in either lane return no partial
result. The compiler-outage regression now includes 120 ordinary files to exercise
the parallel lanes rather than only the small-tree shortcut. Native optional
header creation, same-timestamp edits/restoration and removal, plus missing
compiler/observer/library/provider and timeout controls, preserve the existing
failure/restoration contract. Both native passes, original input rereads, token
agreement, process containment and product/tree-cache gates remain unchanged.

Final-binary checks compare all 106 semantic files against the installed `.8`
index, paired indices, a new verified cold build and the restored outage index.
They match exactly. The isolated outage preserves all 124 committed
graph/span/product artifacts and `CURRENT`; restoring the genuine compiler
restores the original answers. Native Windows and Linux stream/compiler/replay
regressions, the unchanged full grammar corpus, 76 Python controls, library
Clippy on Windows, all-target Clippy on Linux and Actionlint pass. The connected
productive MCP and tunnel also pass health checks with the same two original
Catch2 spans, `201224..201229` and `203143..203151`.

Private timeline, worker-sweep, paired benchmark and native fixture records are
under `C:/Users/morit/.codex/tmp/vorpal-scheduling8`. The productive `.8` binary,
provider, index and service stay unchanged; Hades is read-only.

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

## Capture-local buffer and token processing, 2026-10-13

Profiling published `.7` across a verified cold build and three ordinary tools
produces 80 successful captures of the same 20 genuine MSVC source contexts.
Registry, UI, Lua and Unity contexts dominate. Repeated JSON decoding of native
marker spellings, hashing the already immutable source/definition bytes and
retrieving each LLVM token spelling twice add avoidable processing work.

The candidate reuses decoded marker strings only inside one projection, keyed
by their exact quoted bytes. Its 2,048-entry bound stops retaining new spellings
without dropping or changing their projection. Every marker resolution and
redirect check still runs. The source digest is computed once from the capture's
immutable original bytes. Each definition site still resolves and reads its
header anew: only a complete byte-equal read can reuse that capture's digest and
decoded definition text. Changed bytes or observer identities abort; final
source/header/plan/tool rechecks remain in place. A new capture starts empty.

Root lexing retrieves each foreign spelling and location once, retaining the
same comment exclusion, literal payload, directive lines and explicit Clang
diagnostic treatment. Both complete native preprocessing runs, the independent
observer and all physical input reads remain mandatory. No compiler observation,
product, graph or filesystem answer is reused between requests.

The two 80-capture profiles have exactly 76,328 file-read calls each. Digest
calls drop from 96,972 to 53,932, JSON decode calls from 2,770,504 to 1,933,408,
and LLVM spelling accesses from 2,991,240 to 1,484,913. Root lexer inclusive time
drops from 37.81 to 30.80 seconds over these captures. Inclusive phases overlap;
their savings must not be summed, and profiler results are not response times.

An unprofiled paired run uses the installed `.7` binary, compiler, observer,
SDK, plan and unchanged Hades sources. The two private configurations differ
only in provider location/bytes. Both clients have external verified indices;
each of the nine baseline/candidate pairs alternates which client runs first.

| Operation | `.7` median (range) | Candidate median (range) | Median paired saving |
| --- | ---: | ---: | ---: |
| Health | 17.523 s (15.034–17.578) | 17.336 s (14.770–17.515) | 0.187 s |
| Graph | 17.279 s (16.768–17.816) | 17.088 s (14.794–17.281) | 0.535 s |
| Snippet | 17.417 s (17.342–17.562) | 17.059 s (15.550–17.244) | 0.503 s |

Paired savings are medians of individual differences, not differences of the
column medians. Ranges overlap and earlier consecutive blocks drifted by about
a second; this is a small local improvement, not a large or cross-machine
latency guarantee. Full fresh native validation still dominates requests.

All 106 stored semantic artifacts (node/span families, edges, signatures,
evidence, usage, names, dataflow, reach and physical source contexts) match the
fresh `.7` baseline exactly. Graph/snippet payloads and original diagnostic
spans match; only the displayed health extraction identity changes with provider
provenance. The same two inactive Catch2 diagnoses / 13 bytes remain visible.
The 74 Python controls pass on Windows/Linux, including same-timestamp header
edits, missing headers, independent capture identities, bounded escaped marker
decoding and raw-literal/directive roles. A private 5,000-stream LF/CRLF
differential comparison retains exact output/declines and resolution call order.

Five real MSVC/LLVM CLI/MCP failure controls retain the committed generation and
all graph/span bytes through missing compiler, observer, library, provider and
timeout cases, then restore exact answers. Native-only optional header creation,
same-timestamp edit/restoration and removal invalidate recovery correctly.
Coordinated header changes between native runs retain diagnostics; an inconsistent
definition read during projection fails closed and keeps CURRENT. Restoration
recovers the original generation. All 38 native LF/CRLF syntax/token controls pass.

Reports and private paired/native harnesses are under
`C:/Users/morit/.codex/tmp/vorpal-latency7-*`. The installed `.7` provider,
configuration, index and productive tunnel are unchanged; Hades is read-only.

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
