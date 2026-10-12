// Read-only research instrumentation for one launched compiler. This is not a
// sandbox or a completeness certificate, and is never loaded by the MCP provider.
#include <windows.h>
#include <winternl.h>
#include <detours.h>
#include <string>
#include "format.h"
#include "payload.h"
#include "input_bytes.h"

using Create = NTSTATUS (NTAPI *)(PHANDLE, ACCESS_MASK, POBJECT_ATTRIBUTES,
    PIO_STATUS_BLOCK, PLARGE_INTEGER, ULONG, ULONG, ULONG, ULONG, PVOID, ULONG);
using Open = NTSTATUS (NTAPI *)(PHANDLE, ACCESS_MASK, POBJECT_ATTRIBUTES,
    PIO_STATUS_BLOCK, ULONG, ULONG);
using Attributes = NTSTATUS (NTAPI *)(POBJECT_ATTRIBUTES, PVOID);
using Read = NTSTATUS (NTAPI *)(HANDLE, HANDLE, PIO_APC_ROUTINE, PVOID,
    PIO_STATUS_BLOCK, PVOID, ULONG, PLARGE_INTEGER, PULONG);
using Section = NTSTATUS (NTAPI *)(PHANDLE, ACCESS_MASK, POBJECT_ATTRIBUTES,
    PLARGE_INTEGER, ULONG, ULONG, HANDLE);
using Directory = NTSTATUS (NTAPI *)(HANDLE, HANDLE, PIO_APC_ROUTINE, PVOID,
    PIO_STATUS_BLOCK, PVOID, ULONG, FILE_INFORMATION_CLASS, BOOLEAN,
    PUNICODE_STRING, BOOLEAN);
using DirectoryEx = NTSTATUS (NTAPI *)(HANDLE, HANDLE, PIO_APC_ROUTINE, PVOID,
    PIO_STATUS_BLOCK, PVOID, ULONG, FILE_INFORMATION_CLASS, ULONG, PUNICODE_STRING);
using Information = NTSTATUS (NTAPI *)(HANDLE, PIO_STATUS_BLOCK, PVOID, ULONG,
    FILE_INFORMATION_CLASS);

static Create realCreate;
static Open realOpen;
static Attributes realAttributes, realFullAttributes;
static Read realRead;
static Section realSection;
static Directory realDirectory;
static DirectoryEx realDirectoryEx;
static Information realInformation;
static decltype(&CreateProcessW) realProcessW = CreateProcessW;
static decltype(&CreateProcessA) realProcessA = CreateProcessA;
static char dllPath[32768];
static bool debuggerOwned;
static InputBytes returnedBytes;
static uint64_t executionCreated;
static HANDLE logFile = INVALID_HANDLE_VALUE;
static SRWLOCK logLock = SRWLOCK_INIT;
static uint64_t sequence, traceBytes;
static volatile LONG failed;
static thread_local bool inside;

static uint64_t ticks() {
  LARGE_INTEGER value;
  QueryPerformanceCounter(&value);
  return static_cast<uint64_t>(value.QuadPart);
}

static std::wstring handlePath(HANDLE handle) {
  wchar_t path[32768];
  const DWORD size = GetFinalPathNameByHandleW(handle, path, ARRAYSIZE(path),
                                             FILE_NAME_NORMALIZED | VOLUME_NAME_DOS);
  if (!size || size >= ARRAYSIZE(path)) return {};
  return {path, size};
}

static std::wstring objectPath(POBJECT_ATTRIBUTES attributes) {
  if (!attributes || !attributes->ObjectName || !attributes->ObjectName->Buffer
      || (attributes->ObjectName->Length % sizeof(wchar_t))) return {};
  std::wstring name(attributes->ObjectName->Buffer,
                    attributes->ObjectName->Length / sizeof(wchar_t));
  if (attributes->RootDirectory) {
    auto root = handlePath(attributes->RootDirectory);
    if (root.empty() || name.empty() || name.front() == L'\\') return {};
    name = root + L"\\" + name;
  }
  return name;
}

static bool writeBytes(const void* data, DWORD size) {
  DWORD written = 0;
  return WriteFile(logFile, data, size, &written, nullptr) && written == size;
}

static uint64_t emit(Kind kind, NTSTATUS status, uint32_t access, uint64_t begin,
                 const std::wstring& name, const std::wstring& resolved = {}) {
  AcquireSRWLockExclusive(&logLock);
  const DWORD nameBytes = static_cast<DWORD>(name.size() * sizeof(wchar_t));
  const DWORD resolvedBytes = static_cast<DWORD>(resolved.size() * sizeof(wchar_t));
  Record record{static_cast<uint32_t>(kind), static_cast<uint32_t>(status),
      (name.empty() && kind >= Kind::create && kind <= Kind::information)
          ? pathUnavailable : 0u,
      access, GetCurrentThreadId(), nameBytes / 2, resolvedBytes / 2,
      ++sequence, begin, ticks()};
  if (failed) record.flags |= loggerFailed;
  const uint64_t size = sizeof(record) + nameBytes + resolvedBytes;
  if (traceBytes + size > maxTraceBytes || !writeBytes(&record, sizeof(record))
      || !writeBytes(name.data(), nameBytes)
      || !writeBytes(resolved.data(), resolvedBytes)) InterlockedExchange(&failed, 1);
  traceBytes += size;
  ReleaseSRWLockExclusive(&logLock);
  return record.sequence;
}

// Resolve paths while recursion is suppressed, then restore the API's original
// last-error value. A logger failure remains visible at the final record.
struct Scope {
  bool trace;
  DWORD incoming;
  uint64_t begin;
  std::wstring name;
  explicit Scope(POBJECT_ATTRIBUTES attrs) : trace(!inside), incoming(GetLastError()), begin(ticks()) {
    if (trace) { inside = true; try { name = objectPath(attrs); } catch (...) { InterlockedExchange(&failed, 1); } }
    SetLastError(incoming);
  }
  explicit Scope(HANDLE handle) : trace(!inside), incoming(GetLastError()), begin(ticks()) {
    if (trace) { inside = true; try { name = handlePath(handle); } catch (...) { InterlockedExchange(&failed, 1); } }
    SetLastError(incoming);
  }
  uint64_t finish(Kind kind, NTSTATUS status, uint32_t access = 0, HANDLE handle = nullptr, bool release = true) {
    const DWORD outgoing = GetLastError();
    uint64_t emitted = 0;
    if (trace) {
      try { emitted = emit(kind, status, access, begin, name, handle ? handlePath(handle) : std::wstring{}); }
      catch (...) { InterlockedExchange(&failed, 1); }
      if (release) inside = false;
    }
    SetLastError(outgoing);
    return emitted;
  }
};

static NTSTATUS NTAPI probeCreate(PHANDLE file, ACCESS_MASK access, POBJECT_ATTRIBUTES attrs,
    PIO_STATUS_BLOCK io, PLARGE_INTEGER size, ULONG attributes, ULONG share,
    ULONG disposition, ULONG options, PVOID ea, ULONG eaSize) {
  Scope scope(attrs);
  auto status = realCreate(file, access, attrs, io, size, attributes, share, disposition, options, ea, eaSize);
  scope.finish(Kind::create, status, access, status >= 0 ? *file : nullptr);
  return status;
}
static NTSTATUS NTAPI probeOpen(PHANDLE file, ACCESS_MASK access, POBJECT_ATTRIBUTES attrs,
    PIO_STATUS_BLOCK io, ULONG share, ULONG options) {
  Scope scope(attrs);
  auto status = realOpen(file, access, attrs, io, share, options);
  scope.finish(Kind::open, status, access, status >= 0 ? *file : nullptr);
  return status;
}
static NTSTATUS NTAPI probeAttributes(POBJECT_ATTRIBUTES attrs, PVOID output) {
  Scope scope(attrs); auto status = realAttributes(attrs, output);
  scope.finish(Kind::attributes, status); return status;
}
static NTSTATUS NTAPI probeFullAttributes(POBJECT_ATTRIBUTES attrs, PVOID output) {
  Scope scope(attrs); auto status = realFullAttributes(attrs, output);
  scope.finish(Kind::fullAttributes, status); return status;
}
static NTSTATUS NTAPI probeRead(HANDLE file, HANDLE event, PIO_APC_ROUTINE apc, PVOID context,
    PIO_STATUS_BLOCK io, PVOID buffer, ULONG size, PLARGE_INTEGER offset, PULONG key) {
  Scope scope(file);
  FileIdentity before(scope.trace && debuggerOwned ? file : nullptr);
  uint64_t position = UINT64_MAX; // Never inspect an invalid/rejected native pointer.
  SetLastError(scope.incoming);
  auto status = realRead(file,event,apc,context,io,buffer,size,offset,key);
  const DWORD outgoing = GetLastError();
  auto observed = scope.finish(Kind::read,status,size,nullptr,false);
  if (scope.trace && debuggerOwned) try {
    if (status == 0 && offset && offset->QuadPart >= 0) position = static_cast<uint64_t>(offset->QuadPart);
    DWORD flags = before.known ? 0 : identityUnavailable;
    if (position == UINT64_MAX) flags |= offsetUnavailable;
    DWORD count = 0;
    // Never wait, alter APC/event delivery, or read an incomplete async buffer.
    if (status == 0x103) flags |= pendingRead | bytesUnavailable;
    else if (status == 0 && io && io->Status == 0 && io->Information <= size &&
             (buffer || !io->Information)) count = static_cast<DWORD>(io->Information);
    else flags |= bytesUnavailable;
    FileIdentity after(file);
    if (!after.known) flags |= identityUnavailable;
    else if (before.known && !before.equals(after)) flags |= identityChanged;
    returnedBytes.emit(InputKind::readReturn,static_cast<DWORD>(status),flags,GetCurrentProcessId(),
      file,before,scope.name,scope.begin,executionCreated,position,buffer,count,observed);
  } catch (...) { InterlockedExchange(&failed,1); }
  if (scope.trace) inside = false;
  SetLastError(outgoing); return status;
}
static NTSTATUS NTAPI probeSection(PHANDLE section, ACCESS_MASK access, POBJECT_ATTRIBUTES attrs,
    PLARGE_INTEGER size, ULONG protection, ULONG allocation, HANDLE file) {
  Scope scope(file); auto status = realSection(section, access, attrs, size, protection, allocation, file);
  const DWORD outgoing = GetLastError();
  auto observed = scope.finish(Kind::section,status,allocation,nullptr,false);
  if (scope.trace && debuggerOwned) try {
    FileIdentity id(file);
    returnedBytes.emit(InputKind::sectionBoundary,static_cast<DWORD>(status),
      mappingUnobserved | offsetUnavailable | (id.known ? 0 : identityUnavailable),
      GetCurrentProcessId(),file,id,scope.name,scope.begin,executionCreated,UINT64_MAX,nullptr,0,observed);
  } catch (...) { InterlockedExchange(&failed,1); }
  if (scope.trace) inside = false;
  SetLastError(outgoing); return status;
}

// MSVC caches complete directory listings, so an absent __has_include name may
// never reach NtCreateFile. Retain the returned names/attributes, not guessed
// filename probes. Only FileDirectoryInformation's documented layout is decoded.
static void directoryEntries(Scope& scope, NTSTATUS status, PIO_STATUS_BLOCK io,
    PVOID buffer, ULONG size, FILE_INFORMATION_CLASS type, const std::wstring& root) {
  if (!scope.trace || status != 0 || static_cast<ULONG>(type) != 1 || !io || !buffer) return;
  constexpr ULONG nameOffset = 64;
  const auto bytes = static_cast<const BYTE*>(buffer);
  const ULONG_PTR length = io->Information;
  if (length > size) { InterlockedExchange(&failed, 1); return; }
  ULONG offset = 0;
  while (offset < length) {
    if (length - offset < nameOffset) { InterlockedExchange(&failed, 1); return; }
    ULONG next, attributes, nameSize;
    memcpy(&next, bytes + offset, sizeof(next));
    memcpy(&attributes, bytes + offset + 56, sizeof(attributes));
    memcpy(&nameSize, bytes + offset + 60, sizeof(nameSize));
    if (nameSize % 2 || nameSize > length - offset - nameOffset) { InterlockedExchange(&failed, 1); return; }
    std::wstring name(reinterpret_cast<const wchar_t*>(bytes + offset + nameOffset), nameSize / 2);
    emit(Kind::directoryEntry, 0, attributes, scope.begin, root + L"\\" + name);
    if (!next) break;
    if (next < nameOffset + nameSize || next > length - offset) { InterlockedExchange(&failed, 1); return; }
    offset += next;
  }
}
static NTSTATUS NTAPI probeDirectory(HANDLE file, HANDLE event, PIO_APC_ROUTINE apc, PVOID context,
    PIO_STATUS_BLOCK io, PVOID buffer, ULONG size, FILE_INFORMATION_CLASS type,
    BOOLEAN single, PUNICODE_STRING name, BOOLEAN restart) {
  Scope scope(file);
  const auto root = scope.name;
  if (scope.trace && name && name->Buffer) scope.name += L"\\" + std::wstring(name->Buffer, name->Length / sizeof(wchar_t));
  auto status = realDirectory(file, event, apc, context, io, buffer, size, type, single, name, restart);
  const DWORD outgoing = GetLastError();
  directoryEntries(scope, status, io, buffer, size, type, root);
  SetLastError(outgoing);
  scope.finish(Kind::directory, status, static_cast<uint32_t>(type)); return status;
}
static NTSTATUS NTAPI probeDirectoryEx(HANDLE file, HANDLE event, PIO_APC_ROUTINE apc, PVOID context,
    PIO_STATUS_BLOCK io, PVOID buffer, ULONG size, FILE_INFORMATION_CLASS type, ULONG flags,
    PUNICODE_STRING name) {
  Scope scope(file);
  const auto root = scope.name;
  if (scope.trace && name && name->Buffer) scope.name += L"\\" + std::wstring(name->Buffer, name->Length / sizeof(wchar_t));
  auto status = realDirectoryEx(file, event, apc, context, io, buffer, size, type, flags, name);
  const DWORD outgoing = GetLastError();
  directoryEntries(scope, status, io, buffer, size, type, root);
  SetLastError(outgoing);
  scope.finish(Kind::directory, status, static_cast<uint32_t>(type)); return status;
}
static NTSTATUS NTAPI probeInformation(HANDLE file, PIO_STATUS_BLOCK io, PVOID buffer,
    ULONG size, FILE_INFORMATION_CLASS type) {
  Scope scope(file); auto status = realInformation(file, io, buffer, size, type);
  scope.finish(Kind::information, status, static_cast<uint32_t>(type)); return status;
}

// MSVC can isolate its frontend in a child. Propagate only into processes this
// instrumented compiler creates; the launcher's kill-on-close job contains them.
template<class Call> static BOOL child(Call call, DWORD flags, LPPROCESS_INFORMATION process) {
  if (inside) return call(false);
  if (debuggerOwned) return call(false); // Create events inject exactly once, including OS-created children.
  inside = true;
  const uint64_t begin = ticks();
  const BOOL success = call(true);
  const DWORD error = GetLastError();
  std::wstring image;
  if (success) {
    wchar_t path[32768]; DWORD size = ARRAYSIZE(path);
    if (QueryFullProcessImageNameW(process->hProcess, 0, path, &size)) image.assign(path, size);
  }
  emit(Kind::child, success ? 0 : error, success ? process->dwProcessId : 0, begin, image);
  if (success && !(flags & CREATE_SUSPENDED) && ResumeThread(process->hThread) == static_cast<DWORD>(-1)) {
    InterlockedExchange(&failed, 1);
    TerminateProcess(process->hProcess, 123);
  }
  inside = false;
  SetLastError(error);
  return success;
}
static BOOL WINAPI probeProcessW(LPCWSTR application, LPWSTR command, LPSECURITY_ATTRIBUTES pa,
    LPSECURITY_ATTRIBUTES ta, BOOL inherit, DWORD flags, LPVOID env, LPCWSTR cwd,
    LPSTARTUPINFOW startup, LPPROCESS_INFORMATION process) {
  return child([&](bool inject) {
    LPCSTR dlls[] = {dllPath};
    return inject ? DetourCreateProcessWithDllsW(application, command, pa, ta, inherit,
        flags | CREATE_SUSPENDED, env, cwd, startup, process, 1, dlls, realProcessW)
      : realProcessW(application, command, pa, ta, inherit, flags, env, cwd, startup, process);
  }, flags, process);
}
static BOOL WINAPI probeProcessA(LPCSTR application, LPSTR command, LPSECURITY_ATTRIBUTES pa,
    LPSECURITY_ATTRIBUTES ta, BOOL inherit, DWORD flags, LPVOID env, LPCSTR cwd,
    LPSTARTUPINFOA startup, LPPROCESS_INFORMATION process) {
  return child([&](bool inject) {
    LPCSTR dlls[] = {dllPath};
    return inject ? DetourCreateProcessWithDllsA(application, command, pa, ta, inherit,
        flags | CREATE_SUSPENDED, env, cwd, startup, process, 1, dlls, realProcessA)
      : realProcessA(application, command, pa, ta, inherit, flags, env, cwd, startup, process);
  }, flags, process);
}

extern "C" __declspec(dllexport) void AuditMarker() {}

BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, LPVOID) {
  if (DetourIsHelperProcess()) return TRUE;
  inside = true;
  if (reason == DLL_PROCESS_ATTACH) {
    if (!DetourRestoreAfterWith()) return FALSE;
    wchar_t path[32768];
    DWORD payloadSize = 0;
    auto payload = static_cast<const AuditPayload*>(DetourFindPayloadEx(auditPayloadGuid, &payloadSize));
    DWORD size = 0;
    if (payload) {
      if (payloadSize != sizeof(AuditPayload) || payload->version != 1 ||
          !payload->units || payload->units >= ARRAYSIZE(path) - 40 ||
          payload->prefix[payload->units] != 0) return FALSE;
      size = payload->units;
      memcpy(path, payload->prefix, (size + 1) * sizeof(wchar_t));
      debuggerOwned = true;
    } else size = GetEnvironmentVariableW(L"VORPAL_NATIVE_AUDIT_TRACE", path, ARRAYSIZE(path));
    if (!size || size >= ARRAYSIZE(path) - 20) return FALSE;
    std::wstring tracePath = std::wstring(path, size) + L"." + std::to_wstring(GetCurrentProcessId()) + L".bin";
    size = GetModuleFileNameA(instance, dllPath, ARRAYSIZE(dllPath));
    if (!size || size >= ARRAYSIZE(dllPath)) return FALSE;
    logFile = CreateFileW(tracePath.c_str(), GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE,
                         nullptr, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (logFile == INVALID_HANDLE_VALUE || !writeBytes(magic, sizeof(magic) - 1)) return FALSE;
    traceBytes = sizeof(magic) - 1;
    executionCreated = InputBytes::creation(GetCurrentProcess());
    if (debuggerOwned && (!executionCreated || !returnedBytes.open(tracePath.substr(0,tracePath.size()-4) + L".bytes.bin"))) return FALSE;
    HMODULE ntdll = GetModuleHandleW(L"ntdll.dll");
    LONG error = DetourTransactionBegin();
    if (error == NO_ERROR) error = DetourUpdateThread(GetCurrentThread());
#define ATTACH(variable, api, hook) \
    variable = reinterpret_cast<decltype(variable)>(GetProcAddress(ntdll, api)); \
    if (!variable) error = ERROR_PROC_NOT_FOUND; \
    if (error == NO_ERROR) error = DetourAttach(reinterpret_cast<PVOID*>(&variable), hook)
    ATTACH(realCreate, "NtCreateFile", probeCreate);
    ATTACH(realOpen, "NtOpenFile", probeOpen);
    ATTACH(realAttributes, "NtQueryAttributesFile", probeAttributes);
    ATTACH(realFullAttributes, "NtQueryFullAttributesFile", probeFullAttributes);
    ATTACH(realRead, "NtReadFile", probeRead);
    ATTACH(realSection, "NtCreateSection", probeSection);
    ATTACH(realDirectory, "NtQueryDirectoryFile", probeDirectory);
    ATTACH(realDirectoryEx, "NtQueryDirectoryFileEx", probeDirectoryEx);
    ATTACH(realInformation, "NtQueryInformationFile", probeInformation);
#undef ATTACH
    if (error == NO_ERROR) error = DetourAttach(reinterpret_cast<PVOID*>(&realProcessW), probeProcessW);
    if (error == NO_ERROR) error = DetourAttach(reinterpret_cast<PVOID*>(&realProcessA), probeProcessA);
    if (error != NO_ERROR) { DetourTransactionAbort(); CloseHandle(logFile); return FALSE; }
    error = DetourTransactionCommit();
    emit(Kind::start, error, 11, ticks(), {});
    if (error != NO_ERROR) { CloseHandle(logFile); return FALSE; }
  } else if (reason == DLL_PROCESS_DETACH && logFile != INVALID_HANDLE_VALUE) {
    if (!returnedBytes.close()) InterlockedExchange(&failed,1);
    emit(Kind::end, 0, 0, ticks(), {});
    FlushFileBuffers(logFile);
    CloseHandle(logFile);
    logFile = INVALID_HANDLE_VALUE;
  }
  inside = false;
  return TRUE;
}
