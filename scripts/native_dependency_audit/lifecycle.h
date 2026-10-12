#pragma once
#include <windows.h>
#include <tlhelp32.h>
#include <cstdint>
#include <map>
#include <set>
#include <string>

// Independent OS observations, never command lines/environment/debug strings.
// Completion-port messages are advisory. Debug events belong to a separate
// launch whose behavior must be compared with the ordinary compiler run.
enum class LifeKind : uint32_t {
  start = 1, root, jobSample, jobAdd, jobExit, debugCreate, debugExit, image, end
};
#pragma pack(push, 1)
struct LifeRecord {
  uint32_t kind, pid, parent, status, flags, nameUnits;
  uint64_t sequence, ticks, created;
};
#pragma pack(pop)
static_assert(sizeof(LifeRecord) == 48);
constexpr char lifeMagic[] = "VORPAL-NATIVE-LIFECYCLE-1\n";

class Lifecycle {
  HANDLE file = INVALID_HANDLE_VALUE;
  HANDLE port = nullptr;
  HANDLE job = nullptr;
  std::map<DWORD, HANDLE> held;
  std::set<DWORD> initialBreakpoints;
  uint64_t sequence = 0, bytes = 0;
  bool failed = false;
  bool debug;

  static uint64_t createdAt(HANDLE process) {
    FILETIME created{}, exited{}, kernel{}, user{};
    if (!GetProcessTimes(process, &created, &exited, &kernel, &user)) return 0;
    return (static_cast<uint64_t>(created.dwHighDateTime) << 32) | created.dwLowDateTime;
  }
  static std::wstring imagePath(HANDLE process) {
    wchar_t path[32768]; DWORD size = ARRAYSIZE(path);
    if (!QueryFullProcessImageNameW(process, 0, path, &size)) return {};
    return {path, size};
  }
  static DWORD parentOf(DWORD pid) {
    HANDLE snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if (snapshot == INVALID_HANDLE_VALUE) return 0;
    PROCESSENTRY32W entry{}; entry.dwSize = sizeof(entry);
    DWORD parent = 0;
    if (Process32FirstW(snapshot, &entry)) do {
      if (entry.th32ProcessID == pid) { parent = entry.th32ParentProcessID; break; }
    } while (Process32NextW(snapshot, &entry));
    CloseHandle(snapshot);
    return parent;
  }
  bool write(const void* data, DWORD size) {
    DWORD written = 0;
    return WriteFile(file, data, size, &written, nullptr) && written == size;
  }
  HANDLE retain(DWORD pid, HANDLE offered = nullptr) {
    auto known = held.find(pid);
    if (known != held.end()) return known->second;
    if (held.size() >= 128) { failed = true; return nullptr; }
    HANDLE process = nullptr;
    if (offered) {
      if (!DuplicateHandle(GetCurrentProcess(), offered, GetCurrentProcess(),
                           &process, 0, FALSE, DUPLICATE_SAME_ACCESS)) return nullptr;
    } else {
      process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, FALSE, pid);
    }
    if (!process) return nullptr;
    BOOL belongs = FALSE;
    if (!IsProcessInJob(process, job, &belongs) || !belongs || !createdAt(process)) {
      CloseHandle(process); return nullptr;
    }
    held.emplace(pid, process); // Prevent PID reuse while these records are compared.
    return process;
  }

public:
  std::set<DWORD> liveDebug;
  explicit Lifecycle(bool debugMode) : debug(debugMode) {}
  ~Lifecycle() {
    for (const auto& entry : held) CloseHandle(entry.second);
    if (port) CloseHandle(port);
    if (file != INVALID_HANDLE_VALUE) CloseHandle(file);
  }
  void emit(LifeKind kind, DWORD pid, DWORD parent, DWORD status, DWORD flags,
            uint64_t created, const std::wstring& name = {}) {
    LARGE_INTEGER clock{}; QueryPerformanceCounter(&clock);
    LifeRecord record{static_cast<uint32_t>(kind), pid, parent, status, flags,
      static_cast<uint32_t>(name.size()), ++sequence, static_cast<uint64_t>(clock.QuadPart), created};
    if (failed) record.flags |= 2;
    const DWORD nameBytes = record.nameUnits * 2;
    if (name.size() > 32768 || sequence > 16384 || bytes + sizeof(record) + nameBytes > 16 * 1024 * 1024
        || !write(&record, sizeof(record)) || !write(name.data(), nameBytes)) failed = true;
    bytes += sizeof(record) + nameBytes;
  }
  bool attach(HANDLE ownedJob) {
    job = ownedJob;
    wchar_t prefix[32768];
    DWORD size = GetEnvironmentVariableW(L"VORPAL_NATIVE_AUDIT_TRACE", prefix, ARRAYSIZE(prefix));
    if (!size || size >= ARRAYSIZE(prefix) - 10) return false;
    auto path = std::wstring(prefix, size) + L".life.bin";
    file = CreateFileW(path.c_str(), GENERIC_WRITE, FILE_SHARE_READ, nullptr,
                       CREATE_NEW, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (file == INVALID_HANDLE_VALUE || !write(lifeMagic, sizeof(lifeMagic) - 1)) return false;
    port = CreateIoCompletionPort(INVALID_HANDLE_VALUE, nullptr, 0, 1);
    if (!port) return false;
    JOBOBJECT_ASSOCIATE_COMPLETION_PORT association{job, port};
    if (!SetInformationJobObject(job, JobObjectAssociateCompletionPortInformation,
                                 &association, sizeof(association))) return false;
    emit(LifeKind::start, GetCurrentProcessId(), 0, debug ? 1 : 0, 0, 0);
    sample(0);
    return !failed;
  }
  void root(DWORD pid, HANDLE process) {
    HANDLE stable = retain(pid, process);
    emit(LifeKind::root, pid, GetCurrentProcessId(), 0, stable ? 0 : 1,
         stable ? createdAt(stable) : 0, stable ? imagePath(stable) : std::wstring{});
    sample(1);
  }
  void sample(DWORD phase) {
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION info{};
    if (!QueryInformationJobObject(job, JobObjectBasicAccountingInformation,
                                  &info, sizeof(info), nullptr)) { failed = true; return; }
    // pid = phase; parent = TotalProcesses; status = ActiveProcesses;
    // created = TotalTerminatedProcesses. This is accounting, not a PID record.
    emit(LifeKind::jobSample, phase, info.TotalProcesses, info.ActiveProcesses, 0,
         info.TotalTerminatedProcesses);
  }
  void pump() {
    DWORD message; ULONG_PTR key; LPOVERLAPPED value;
    while (GetQueuedCompletionStatus(port, &message, &key, &value, 0)) {
      if (key != reinterpret_cast<ULONG_PTR>(job)) { failed = true; continue; }
      DWORD pid = static_cast<DWORD>(reinterpret_cast<ULONG_PTR>(value));
      if (message == JOB_OBJECT_MSG_NEW_PROCESS) {
        HANDLE process = retain(pid);
        emit(LifeKind::jobAdd, pid, process ? parentOf(pid) : 0, 0, process ? 0 : 1,
             process ? createdAt(process) : 0, process ? imagePath(process) : std::wstring{});
      } else if (message == JOB_OBJECT_MSG_EXIT_PROCESS || message == JOB_OBJECT_MSG_ABNORMAL_EXIT_PROCESS) {
        auto known = held.find(pid);
        DWORD code = 0;
        HANDLE process = known == held.end() ? nullptr : known->second;
        const bool valid = process && GetExitCodeProcess(process, &code) && code != STILL_ACTIVE;
        emit(LifeKind::jobExit, pid, 0, code, valid ? 0 : 1, process ? createdAt(process) : 0);
      }
    }
  }
  bool debugEvent(const DEBUG_EVENT& event, DWORD& rootExit, DWORD rootPid) {
    DWORD continuation = DBG_CONTINUE;
    switch (event.dwDebugEventCode) {
      case CREATE_PROCESS_DEBUG_EVENT: {
        HANDLE process = retain(event.dwProcessId, event.u.CreateProcessInfo.hProcess);
        liveDebug.insert(event.dwProcessId);
        emit(LifeKind::debugCreate, event.dwProcessId, process ? parentOf(event.dwProcessId) : 0, 0,
             process ? 0 : 1, process ? createdAt(process) : 0,
             process ? imagePath(process) : std::wstring{});
        if (event.u.CreateProcessInfo.hFile) CloseHandle(event.u.CreateProcessInfo.hFile);
        break;
      }
      case EXIT_PROCESS_DEBUG_EVENT: {
        auto found = held.find(event.dwProcessId);
        HANDLE process = found == held.end() ? nullptr : found->second;
        emit(LifeKind::debugExit, event.dwProcessId, 0, event.u.ExitProcess.dwExitCode,
             process ? 0 : 1, process ? createdAt(process) : 0);
        liveDebug.erase(event.dwProcessId);
        if (event.dwProcessId == rootPid) rootExit = event.u.ExitProcess.dwExitCode;
        break;
      }
      case LOAD_DLL_DEBUG_EVENT: {
        wchar_t path[32768];
        DWORD size = event.u.LoadDll.hFile ? GetFinalPathNameByHandleW(event.u.LoadDll.hFile,
            path, ARRAYSIZE(path), FILE_NAME_NORMALIZED | VOLUME_NAME_DOS) : 0;
        emit(LifeKind::image, event.dwProcessId, 0, 0, size && size < ARRAYSIZE(path) ? 0 : 1, 0,
             size && size < ARRAYSIZE(path) ? std::wstring(path, size) : std::wstring{});
        if (event.u.LoadDll.hFile) CloseHandle(event.u.LoadDll.hFile);
        break;
      }
      case EXCEPTION_DEBUG_EVENT:
        if (!(event.u.Exception.dwFirstChance &&
              event.u.Exception.ExceptionRecord.ExceptionCode == EXCEPTION_BREAKPOINT &&
              initialBreakpoints.insert(event.dwProcessId).second)) continuation = DBG_EXCEPTION_NOT_HANDLED;
        break;
      case RIP_EVENT: failed = true; break;
      default: break;
    }
    // Windows owns debug hProcess/hThread handles until Continue(EXIT_*).
    // Our duplicated process handles remain valid for lifetime reconciliation.
    if (!ContinueDebugEvent(event.dwProcessId, event.dwThreadId, continuation)) failed = true;
    return !failed;
  }
  bool finish(DWORD rootPid, DWORD result, DWORD associations) {
    pump(); sample(2);
    emit(LifeKind::end, rootPid, associations, result, failed ? 2 : 0, 0);
    if (!FlushFileBuffers(file)) failed = true;
    return !failed;
  }
};
