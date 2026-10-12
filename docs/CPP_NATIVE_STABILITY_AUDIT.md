# Native Windows input-stability canaries (report only)

`scripts/cpp_native_stability_control.py` tests whether explicit Windows guards
block changes to private input files or force a capture to decline. It launches
one new ordinary native actor per case through the existing kill-on-close job.
Only newly created external fixtures are mutated. No Hades source, installed
provider, production configuration, MCP index or replay policy is changed.
**Every result retains `replayEligible: false` and
`ordinaryInputCompleteness: false`.**

## Mechanisms and native controls

| Mode | Mechanism | Tested boundary |
| --- | --- | --- |
| `none` | No guard handle | Counterfactual: the actor can change its private fixture. |
| `range` | Shared `LockFileEx` over the full byte range | Ordinary writes fail; writable mappings can still change consumed bytes. |
| `share` | `GENERIC_READ` with only `FILE_SHARE_READ` | Existing writable handles/sections prevent acquisition; later conflicting opens/replacements fail. |
| `share-directory` | Read pins plus a directory R oplock | Optional-header creation can proceed, but the directory break stays visible after deletion/restoration. |

Four modes exercise fourteen real native cases, for **56 controls**: stable
read, later and pre-existing writes, pre-existing asynchronous write, writable
views with open and closed file handles, later writable mapping, hardlink write,
file replacement, directory shadow creation, directory rename/recreation,
pre-existing writable sections without a view or with only a read-only view,
and asynchronous read. The delayed-section cases create a writable view only
*after* guard acquisition. An asynchronous read uses the actual Windows
completion result and checks the bytes consumed under the guard.

The actor pauses at `READY`, attempts the operation only after the controller
acquires its guard, and pauses again at `OBSERVED`. Physical file ID, path bytes
and content hashes are inspected at these coordinated boundaries. Restoration
must preserve the original physical identity and bytes. These snapshots are
fixture observations, **not an atomic production snapshot implementation**.
Range/mapping and directory-shadow cases retain the guard through restoration.
The native SDK structure sizes are checked against the Python ctypes ABI;
protocol records require complete typed fields and bounded line lengths/waits.

On the local Windows/MSVC 19.44 run, ordinary writes under the range lock fail
with `ERROR_LOCK_VIOLATION` (33), including the pre-existing asynchronous writer.
Every tested writable-mapping variant still changes the input. This matches the
[LockFileEx contract](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-lockfileex):
byte locks do not prevent access through mapped views. Equal before/after bytes
therefore cannot exclude transient changes consumed by the compiler.

Read pins reject pre-existing writable handles, views and sections with
`ERROR_SHARING_VIOLATION` (32), including a section with no view and one with
only a read-only view after its file handle was closed. Later write/map/hardlink
opens and file replacement fail in the tested cases. The
[CreateFile sharing contract](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew)
explains these conflicts. Acquisition failure remains a decline; unchanged
snapshot bytes do not turn that failure into a successful guard.

A pin on the existing file does **not** protect a missing optional include
candidate: the actor can create a sibling header while the pinned file's ID and
bytes remain equal. The directory R oplock reports a break that stays latched
through create/delete restoration. Directory R oplocks are
[advisory and require overlapped I/O](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_request_oplock);
this test detects the namespace change, rather than claiming it was prevented.
No production publication race is closed by a final event check.

Cancellation must complete before the request, OVERLAPPED and output buffers
are released. A cancellation failure or timeout aborts the harness and retains
the buffer owner until a successful drain or process exit. Protocol errors,
unknown operation errors, failed acquisition, changed identity/bytes and lease
breaks all decline the canary. The strongest positive disposition is only
`private-fixture-stable-only`; it is never proof or cache authorization.

The independent ordinary job ledger is preserved. A missing console-host parent
identity can still leave `lifetimesReconciled: false`; successful file controls
do not repair or normalize that incomplete process inventory.

## Fresh compiler and graph comparison

Optional `--binary` and `--native-config` arguments add a **57th control**.
It copies the compiler-provider invocation from a trusted configuration but
creates a new explicit MSVC plan, source/header pair, configuration and three
external indices. The installed binary/configuration are only read.

Fresh-before, guarded and fresh-after captures use the genuine native provider.
The guarded capture holds read pins on both known fixture files and a directory
R oplock throughout compilation, index verification and MCP requests. It checks
unchanged source/header bytes, physical identities and an unbroken lease. The
stored nodes, edges, signatures, evidence, contexts and span-bearing graph files
must match by content, as must MCP health/diagnosis, call graph and snippet
payloads. The local run compares 87 semantic files and reports exact equality.
All three captures remain fresh; this control performs no provider replay.

This covers **only the private known source/header pair**. It does not lock or
certify the compiler/SDK/tool/registry closure. Identical complete fixture graph
payloads do not authorize banking a native proof.

## Reproduce on native Windows

Build the pinned Detours checkout and native helpers as described in
[the dependency audit](CPP_NATIVE_DEPENDENCY_AUDIT.md). In the same native VS
developer shell, use a new external output directory:

```powershell
python C:/dev/vorpal/scripts/cpp_native_stability_control.py `
  --launcher C:/audit/native-dependency-build/vorpal_native_audit.exe `
  --actor C:/audit/native-dependency-build/vorpal_native_stability_actor.exe `
  --directory C:/audit/new-stability-controls
```

For the additional native graph comparison, append `--binary C:/tools/vorpal.exe
--native-config C:/audit/trusted-native-project.json`. Both options are required
together. The trusted configuration must already describe a working native MSVC
provider. This is a trusted-command audit, not a compiler filesystem sandbox.
The normal CI Windows job runs all 56 OS controls and uploads only their small
`controls.json` summary, not raw fixture/compiler artifacts.

Python protocol/decline controls run on Windows and Linux. Native guard results
require Windows/MSVC; a Linux run cannot stand in for these controls. No
administrator privileges, global tracing, driver/service installation or
attachment to old processes is used.

## Remaining production boundary

Complete ordinary-execution input coverage remains open: loader/mapped-page
consumption, native negative searches and enumerations, redirects and ancestors,
metadata, tools/resources, registry/devices and remote inputs. A guarded fixture
is not that inventory. Advisory directory breaks also need a conservative
capture/publication protocol before any reuse could be justified. Production
continues to require fresh native captures. See
[the process/read-buffer audit](CPP_NATIVE_DEPENDENCY_AUDIT.md) and
[the latency measurements](CPP_NATIVE_MCP_LATENCY.md).

[The guarded genuine capture prototype](CPP_GUARDED_NATIVE_CAPTURE.md) applies
these mechanisms to the configured Hades/MSVC contexts and retains rejected
whole-job discoveries and private staging/publication-gap controls.
