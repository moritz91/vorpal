# MSVC native dependency audit (report only)

`scripts/cpp_macro_native_dependency_audit.py` investigates the actual native
filesystem inputs that a future replay contract would have to cover. It runs
one explicit trusted MSVC preprocessing plan twice without instrumentation and
twice with a local Windows probe. Source files are read, never rewritten. No
provider packet, product, tree or MCP graph is replayed or authorized by the
report: **`replayEligible` is always false**.

The compiler's ordinary include list is a useful positive control, not the
complete dependency contract. Microsoft's [`/sourceDependencies` documentation](https://learn.microsoft.com/en-us/cpp/build/reference/sourcedependencies?view=msvc-170)
describes consumed headers/PCH/modules. Our genuine native-only optional-header
control confirms an absent header is absent from that report, while its include
directories still affect the native decision.

## Native observations

The optional x64 Windows helper uses a locally built, pinned Microsoft Detours
4.0.1 checkout (`e4bfd6b03e50de46b47abfbd1e46b384f0c5f833`, MIT). The
[documented suspended-process injection API](https://github.com/microsoft/Detours/wiki/DetourCreateProcessWithDlls)
loads our DLL into the new compiler's in-memory imports. Compiler/tool binaries
on disk remain untouched; nothing is installed into other running processes.
An unnamed kill-on-close job owns only the newly launched process and its
descendants. Baseline runs use that same launcher/job with injection disabled.
No administrator token, persistent tracing service or machine-wide capture is
required. The existing production tunnel and provider are unaffected.

The DLL records `NtCreateFile`, `NtOpenFile`, `NtQueryAttributesFile`,
`NtQueryFullAttributesFile`, `NtReadFile`, `NtCreateSection`,
`NtQueryDirectoryFile`, `NtQueryDirectoryFileEx` and `NtQueryInformationFile`.
It also propagates instrumentation through the two observed `CreateProcess`
APIs. Records retain lexical object names, successful handle resolutions,
status/access, thread numbers and call start/end clocks. Completion sequence is
not claimed as a total call order across threads. Per-process traces and job
accounting must agree before process coverage can even be claimed; missing
coverage remains an explicit report field. Framing, consecutive sequence numbers,
eleven installed hooks, normal start/end, bounded paths, logger failures and
process-tree relationships are checked. An independent lifecycle ledger now
records normal job messages before the root resumes; a separate debugger launch
supplies process create/exit and loader-image events. Corrupt/incomplete traces decline the
audit instead of silently dropping events.

**MSVC 19.44 caches include-directory listings.** In the absent `__has_include`
control there is no individual OS probe naming `optional.h`; the native compiler
enumerates the source directory, ordered explicit roots and developer-shell
include directories, then answers from that inventory. Intercepting file opens
alone would miss the negative dependency. For `FileDirectoryInformation`, the
probe also retains returned entry names and attributes. Other directory payload
formats remain boundaries. The report keeps actual missing OS probes separately;
these can include compiler-internal filenames and are not all header searches.
It never fabricates a missing per-name probe from source text.

## Bracketed snapshots and context

The discovery trace selects lexical/resolved candidates. Each candidate is
snapshotted before a second native trace, after it and after the final baseline
run. Files use complete bounded SHA-256 bytes and physical file IDs. Directories
use sorted membership/attributes/IDs/reparse tags. Missing paths and unavailable
states are distinct. Lexical ancestors retain identity/reparse metadata so a
junction redirect is not hidden by final-path canonicalization. Newly consulted
candidates, changes within the bracket, unknown devices, missing hook coverage
and resource bounds stay visible. No timestamp-only equality is accepted.

The versioned context digest covers the explicit plan bytes, original source and
TU, lexical cwd, compiler, helper executable/DLL, audit script, explicit
`--identity-input` files and complete environment (except the audit log prefix).
Only hashes leave that context calculation; flags/environment values and source
bytes are not included in the JSON report. Explicit identities can include the
provider/projection scripts, configuration, observer and libclang, but this does
not certify the complete Python/compiler/SDK/resource closure.

`auditIdentity` identifies the observations/snapshots in this report. It is not a
product identity or a stable replay key. Compiler-generated ephemeral probe names
may legitimately change between captures; they are reported, not allowlisted.
Raw local artifacts contain native output/stderr and filesystem paths. CI uploads
only the small control summary, not those potentially private buffers.

## Build and run on Windows

Prepare the trusted Detours checkout outside the source project, at the exact
commit above. In a native VS developer shell, build its `src` with `nmake`, then:

```powershell
cmake -S C:/dev/vorpal/scripts/native_dependency_audit `
  -B C:/audit/native-dependency-build -G Ninja -DCMAKE_BUILD_TYPE=Release `
  -DDETOURS_ROOT=C:/audit/Detours-4.0.1
cmake --build C:/audit/native-dependency-build
python C:/dev/vorpal/scripts/cpp_macro_native_dependency_audit.py `
  --plan C:/audit/preprocess-plan.json --source C:/project/tests/main.cc `
  --launcher C:/audit/native-dependency-build/vorpal_native_audit.exe `
  --dll C:/audit/native-dependency-build/vorpal_native_probe64.dll `
  --identity-input C:/audit/project.json --report C:/audit/new-report.json
```

The plan is the same explicit MSVC `{source, cwd, args, translationUnit?}` shape
as the fresh provider; arguments end in the original TU and `/Zs`. Include/define
order and joined `/FI` inputs are preserved. The audit substitutes `/E` for `/Zs`.
Output-producing flags, response files, alternate frontend drivers, `/MP` and
implicit `CL`/`_CL_` options are declined. Paths are explicit; the Detours DLL
path must be ASCII. Reports/artifacts must be new and outside the source cwd.
Output, input bytes, trace/path counts and time are bounded. The audit is a
trusted-command research tool, not a filesystem-write sandbox.

## Independent process lifetimes

The two-process/one-trace discrepancy has been identified on native Windows/MSVC
19.44, including the genuine Hades Catch2 context: `cl.exe` creates a
`C:/Windows/System32/conhost.exe` child. The ordinary job ledger and the separate
debugger capture both identify its executable, parent, creation time and exit.
This is a real untraced process, not a counter normalization to fix by subtracting
one. **Console-host names never exempt a process from dependency coverage.**

The ordinary launcher associates its job with a completion port while the job is
empty, retains live handles and checks job membership before recording process
metadata. PID plus creation time identifies an execution; handles are held until
reconciliation to prevent PID reuse. Windows documents that
[job completion messages are advisory and can be missed](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_associate_completion_port).
Missing notifications/metadata remain incomplete observations even when final
process counts happen to match. [TotalProcesses](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_accounting_information)
is retained as raw accounting; it is not substituted for observed lifetimes.

`vorpal_native_process_audit.exe` is the same scoped launcher built with
[DEBUG_PROCESS](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags)
for a **separate explicit control execution**. It handles independent Windows
create/exit events while the affected process is stopped, records initial loader
image paths, and closes OS image handles. The lifecycle collector does not
inspect command lines, debug strings, environment or process memory; optional
Detours injection remains the separate in-memory import instrumentation described
above. Only each process's first loader
breakpoint is consumed; other exceptions pass back to Windows. The root and its
descendants stay in the owned kill-on-close job. No already running process is
attached to and no elevated token or persistent debugger is installed.

Debugging can affect timing/behavior, and Windows allows a child to establish a
new debug chain. Debug observations are therefore never merged into an ordinary
run as if they belonged to that same capture. Output must match ordinary native
output; agreeing outputs do not certify complete inputs. Job/debug create/exit,
parent identities and final accounting are independently reconciled within each
launch. Unknown identities and missing lifetimes retain false coverage. Early
image paths are evidence, not mapped-byte or SDK-closure proofs.

`scripts/cpp_native_process_control.py --launcher ... --debugger ... --dll ...
--fixture ... --directory ... [--plan ... --source ...]` exercises native
`CreateProcessW`, `CreateProcessA`, CRT `_wspawnv`, eight short-lived parallel
children and a child outliving its parent in ordinary, debug and injected-debug
launches. A timeout must be declined. With the genuine plan it also compares all
three native compiler outputs and unchanged physical source/TU bytes. The Windows
CI job runs these controls in addition to the eleven dependency transitions.

`scripts/cpp_native_dependency_control.py --launcher ... --dll ... --directory ...`
creates a fresh isolated fixture and runs eleven real MSVC cases: absent/native-only
created optional header, same-mtime edit, removal, later-root hit, local shadow
creation/removal, sustained bracket mutation, forced input and two junction targets.
It checks original source bytes, native baseline equivalence, successful native
reads, actual directory membership, changed SHA-256 and redirect identity. The
mutation case must report unequal native output/unstable snapshots. The successful
`/sourceDependencies` report is independently compared for absent/created cases.
Python framing/snapshot controls also run on Windows and Linux. The Windows CI
job builds pinned Detours and our `/W4 /WX` helper before running the native cases.

## Separate whole-process-tree input audit

An optional `--debugger C:/audit/native-dependency-build/vorpal_native_process_audit.exe`
argument adds a **separate execution** to the dependency audit. The ordinary
run's probes/coverage and bracket remain unchanged. Its JSON report places the
new observations only in `separateDebuggerInputAudit`; neither ledger authorizes
proof/product/graph reuse. Native output is compared with the ordinary run.

The debug helper's explicit `--whole-tree` mode receives each
[process-create event before user-mode execution](https://learn.microsoft.com/en-us/windows/win32/debug/debugging-events).
It checks membership in its own new job before updating that stopped process's
imports with the pinned Detours implementation. A bounded private
[Detours payload](https://github.com/microsoft/Detours/wiki/DetourCopyPayloadToProcess)
carries only the local trace prefix and version. This also covers OS-created
console hosts whose environment does not inherit the prefix. The DLL's ordinary
CreateProcess propagation is disabled in this mode to avoid double injection.
No running process is attached to, disk executable is modified, administrator
token requested, or machine-wide collector installed. An injection failure
terminates the new job and declines the capture.

Each independently observed execution must have one attachment event, a framed
filesystem trace and a byte sidecar. PID plus process creation time links each
sidecar to its execution. Completed `NtReadFile` buffers are copied inside the
hook before its calling thread resumes and tied to the exact probe sequence,
thread, native status, requested length, lexical/resolved path, original handle,
volume serial number and 128-bit file ID. Explicit nonnegative offsets are
recorded; shared current-position requests remain `offsetUnavailable`.
A raw handle number is never substituted for physical file identity. Synchronous
failure, missing identity, changed identity, byte limits and asynchronous
`STATUS_PENDING` remain visible. The observer neither waits for asynchronous
completion nor reads its unfinished buffer. Other threads can still mutate a
shared buffer; this user-mode copy is observational, not an atomic kernel-read
or application-consumption contract.

CREATE_PROCESS and LOAD_DLL file handles are snapshotted before the debug event
continues. Full bounded file bytes, physical IDs, process identities and image
bases are retained; size/change metadata is rechecked on that same handle.
**These are loader-file snapshots, not the bytes of loaded memory/pages.** Earlier
failed loader searches, KnownDll/resource policies and every future mapped-page
access are not certified by these snapshots. Successful file-section creation
is recorded as `sectionBoundary`, with no invented read bytes. The independent
loader/create event inventory must match the snapshot inventory.

Raw `.loader.bin` and `.PID.bytes.bin` sidecars may contain private file bytes
and paths. They are new local artifacts, bounded to 64 MiB per payload and 512 MiB
per complete input capture. Framing, sizes, identities, missing/extra sidecars and
probe references are checked; corruption/logger loss declines. CI uploads only
the small `controls.json` summaries, never these buffers or full input reports.

`scripts/cpp_native_input_control.py --launcher ... --debugger ... --dll ...
--fixture ... --input-fixture ... --directory ... [--plan ... --source ...]`
creates private external fixture files and runs five complete process-tree
controls (wide/ANSI/CRT launch, eight simultaneous short-lived children, and an
outliving child), plus ten input cases: ordinary/explicit-offset reads, rejected native pointers, pending
asynchronous I/O, mapped access, coordinated change-and-restore through reads
and through mapped memory, sustained change, failed missing access, and two
handles/hardlink names for one physical file. The genuine native plan adds a
sixteenth control. Source/TU bytes never change in that control.

The ABA controls coordinate a real writer thread, reader and restoration using
Windows events. Before/after bytes and LastWriteTime are equal, while native
output contains the transient changed bytes. The read-buffer sidecar observes
those transient bytes; the mapping case intentionally retains no fabricated
mapped/read-page payload. This demonstrates the before/after-snapshot blind
spot; it **does not** exclude concurrent mutation or enable replay.

Native Windows/MSVC 19.44 validation in the genuine Hades Catch2 context yields
identical ordinary/debug/whole-tree native output and unchanged original source.
The separate whole-tree execution has two framed traces (`cl.exe`, `conhost.exe`),
671 completed read observations with 12,335,459 captured bytes, and 51 loader-file
snapshots with 62,200,008 bytes. These are per-execution observations, not a tool
closure or production-MCP input proof. `actualMappedPagesCaptured`,
`ordinaryExecutionInputProof`, `changeAndRestoreExcluded` and `replayEligible`
remain false. Default audit/MCP operation stays unchanged.

## Remaining requirements before replay

Local Windows/MSVC 19.44 controls pass all eleven cases. The unchanged genuine
Hades Catch2 context yields equal normal/instrumented native output and stable
explicit context hashes: 4,814 observations, 457 candidate paths, 21 missing OS
probes and 689 successful read/map observations. These are observation counts,
not unique headers or parser diagnoses. Hades and the installed `.5` MCP remain
unchanged. Independent follow-up observations now identify the second process
as the console host. Its ordinary-run filesystem trace remains absent; that
report retains false process coverage. The explicit separate debugger input
audit now captures framed probes from both executions, as described below. Some compiler-internal missing names also change between
discovery and confirmation, so the complete candidate bracket is not stable.
Neither discrepancy is suppressed to manufacture an eligible snapshot.

This audit exposes these boundaries; passing controls does not remove them:

- User-mode hooks are not an OS-enforced completeness contract. Other syscall,
  child-launch or filesystem paths can bypass the selected exports. Observed
  process counts that do not match job accounting must stay ineligible.
- Initial loader inputs before attachment, registry/device/network inputs,
  unsupported directory formats, metadata and complete tool/resource trees are
  not comprehensively covered.
- Actual mapped/read bytes are not retained. Before/after snapshots detect a
  sustained change but cannot exclude a change-and-restore while the compiler runs.
  Read identity and race exclusion need an authoritative contract before serving
  or banking reused proof/graphs.
- All relevant failed searches, enumeration membership, redirects and tool inputs
  must be cheaply revalidated between requests; unknown cases still recapture.

The next cache work must close those boundaries and demonstrate equality against
fresh complete graphs/spans/diagnoses, including native-only optional headers and
genuine split-source contexts. Production remains on mandatory fresh native
captures. See [the latency measurements](CPP_NATIVE_MCP_LATENCY.md).
