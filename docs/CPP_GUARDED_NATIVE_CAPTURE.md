# Guarded genuine native capture prototype (report only)

`scripts/cpp_macro_guarded_capture.py` combines actual Windows/MSVC contexts,
observed input inventories, read pins, directory R oplocks and a sticky private
staging barrier. It builds fresh external indices with the installed binary and
trusted native configuration. Hades sources, production configuration/index and
tunnel are never modified. This is a trusted-command research controller, not a
compiler filesystem sandbox or production cache gate.

**`replayEligible`, `ordinaryInputCompleteness`, `publicationAuthorized` and
`atomicPublicationRaceClosed` remain false.** A stable private observation window
is not a complete native input or atomic publication contract.

## Inventory and capture lifecycle

The controller builds a verified fresh baseline and reads only the versioned
`VMAN` path inventory. Framing, exact count, UTF-8, absolute paths, duplicate
spellings, limits and trailing bytes are checked. Its timestamps are never
replay authorization. Extended DOS `\\?\` prefixes are treated as path spelling;
junctions/symlinks are not canonicalized away to assert equivalent namespaces.
Every indexed file must stay inside the explicitly supplied source root.

Each configured source must have one explicit preprocessing plan entry. The
native trace launcher runs that plan in its own bounded kill-on-close job.
Implicit `CL`/`_CL_`, output-producing flags and alternate/response-file drivers
are declined. The compiler's complete framed traces supply **partial** candidate
observations. Declined jobs and incomplete descendant traces are retained with
their real statuses, PIDs, creation times and executable names. Successful root
output or an intact compiler trace never repairs a failed whole job.

The inventory adds all indexed source files, actual observed paths and missing
candidates, explicit compiler/provider/configuration/plan/tool files, ordered
include roots, working directories and existing parents. Unknown file states,
reparse ancestry and incomplete discoveries stay explicit. This still does not
close every provider import, loader/resource input, empty/ignored source-walk
directory, native access path or registry/device/network dependency. The held
MCP execution is not independently retraced as a complete new input inventory.

The controller snapshot budget is explicitly 256 MiB per file and 512 MiB in
aggregate, with at most 8,192 paths. This accommodates the released 114-MB image;
the older dependency audit retains its default 64-MiB file budget. Unknown or
over-budget inputs are not silently removed to obtain an eligible result.

Directory R oplocks are acquired before read pins. Both remain held through a
fresh verified native index build, private MCP health/graph/snippet observations,
complete semantic-file comparison and the private staging attempt. At each
phase, byte/path/ancestry snapshots are compared and directory breaks are
latched. Acquisition failures prevent a guarded capture. A diagnostic run may
continue after a decline, but its candidate remains rejected; private files and
responses are observations only. Tool/configuration/plan/environment identities
are checked without publishing environment values.

## Private staging and its publication boundary

`StagingGate` rejects a candidate before writing it if any prior checkpoint has
failed. It checks again after writing the **private, unpublished** candidate.
Restoration never clears a prior decline. The candidate file itself carries
`publicationAuthorized: false` and `replayEligible: false`.

`scripts/cpp_native_capture_control.py` coordinates the native actor and actual
MSVC preprocessing in six external cases:

1. Stable source/header input under held guards.
2. Pre-existing writer: acquisition fails, and no candidate is staged.
3. Later ordinary write: read pin blocks it and native output stays equal.
4. Native-only optional-header creation before staging: rejected before writing.
5. Creation between the pre-staging check and private write: the post-check
   rejects the quarantined candidate, and fresh native output independently
   demonstrates the changed `__has_include` decision.
6. Creation after the final staging check: the late lease break demonstrates why
   that check is not an atomic publication transaction. Release-time validation
   declines; production publication was never authorized.

The controls independently retain every native job status. If the compiler
exits cleanly while a descendant outlives the five-second post-root drain, status
126 stays `jobClosureDeclined: true`. Its scoped stdout is used only for private
file-guard observations, never as a complete native capture. A compiler failure,
root identity/lifecycle mismatch or other job error fails the control. This is
not an executable-name exemption or a repaired process inventory.

Every private header is restored, and fresh observed output after restoration
must match the original baseline. The positive cases cannot turn into replay approval.
CI runs all six controls after the existing 56 native stability controls and
uploads only their small summary. Framing/decline/budget/cleanup controls also run
on Windows and Linux. Failed asynchronous cancellation retains its buffer owner
as described in [the stability audit](CPP_NATIVE_STABILITY_AUDIT.md).

## Reproduce in the genuine Windows/MSVC environment

Build the pinned native helpers as described in
[the dependency audit](CPP_NATIVE_DEPENDENCY_AUDIT.md). Load the VS developer
shell and use a new output directory outside the source project:

```powershell
python C:/dev/vorpal/scripts/cpp_macro_guarded_capture.py `
  --binary C:/Users/morit/.local/bin/vorpal.exe `
  --source C:/dev/hades-II `
  --config C:/Users/morit/.local/share/vorpal/native-moritz5/project.json `
  --launcher C:/audit/native-dependency-build/vorpal_native_audit.exe `
  --dll C:/audit/native-dependency-build/vorpal_native_probe64.dll `
  --directory C:/audit/new-genuine-guarded-capture
```

The default graph symbol is `ReadQuatAccessor`. The native root gets ten
seconds by default (`--discovery-timeout` accepts 1..60); the regular capture
bound is 120 seconds (`--timeout` accepts 1..300). A separate five-second descendant drain follows root completion. Timeout/partial
observations remain declined, including a process that outlives the compiler. No already
running process is attached to or killed. Only each new scoped job is terminated
on its bound; no worker is exempted by executable name.

The genuine Hades configuration contains 20 source contexts / 19 distinct TUs
and 1,744 indexed physical files. The first complete comparison has exact
fresh-before/held-guard/fresh-after stored graph/span and health/graph/snippet
equality. All twenty scoped discoveries observed a complete compiler trace but
failed final job closure with status 126: `vctip.exe` remained alongside the
compiler/console-host execution. The controller preserves this additional
process and rejects the discoveries. It also detects breaks in shared root,
System32 and Temp directory leases during the genuine capture. These are not
silently excluded as unrelated activity.

Matching graph bytes do not establish that the changed shared namespaces were
irrelevant to every possible native decision. The prototype therefore refuses
candidate staging and leaves productive native captures mandatory. Registry,
loader/mapping, complete tool/provider inputs, newly consulted accesses and an
atomic capture/publication protocol still need an authoritative contract before
production reuse.

## Final local validation (2026-10-12)

The bounded final inventory has 2,416 protected known files, 247 directory leases
and 545 actual missing candidates, with no over-budget/unavailable file states.
Guard acquisition succeeds. All 88 semantic index files and health/graph/snippet
payloads match across fresh-before, held-guard diagnostic and fresh-after runs;
the binary, configuration, plan, controller script and environment identities
stay unchanged. All twenty discoveries remain declined with status 126. Snapshot
changes already visible before the native phase and later directory breaks
prevent private staging. Breaks include the MSVC/Python tool directories, the
shared development directory, System32 and Temp; these observations are not
attributed to a particular writer or exempted.

All six native staging controls and 54 Python controls on Windows/Linux pass.
Their positive results certify only the tested file-guard observations. Hades and
the installed .5 service remain unchanged; no result becomes a cache key or an
atomic production-publication contract.
