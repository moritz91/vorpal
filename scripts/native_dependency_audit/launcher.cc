#include <windows.h>
#include <detours.h>
#include <string>
#include <vector>
#include "format.h"

// Windows CRT quoting, including backslashes before quotes and before the final
// quote. No shell is involved and the command line is never written to the log.
static std::wstring quote(const wchar_t* argument) {
  std::wstring output = L"\"";
  size_t slashes = 0;
  for (const wchar_t* p = argument; *p; ++p) {
    if (*p == L'\\') { ++slashes; continue; }
    output.append(slashes * (*p == L'"' ? 2 : 1), L'\\');
    slashes = 0;
    if (*p == L'"') output += L'\\';
    output += *p;
  }
  output.append(slashes * 2, L'\\');
  return output + L'"';
}

int wmain(int argc, wchar_t** argv) {
  if (argc < 5) return 120;
  // Usage: launcher DLL TIMEOUT_MS -- ABSOLUTE_COMPILER [arguments...]
  if (std::wstring(argv[3]) != L"--") return 120;
  wchar_t* end;
  unsigned long timeout = wcstoul(argv[2], &end, 10);
  if (*end || timeout < 1 || timeout > 300000) return 120;
  std::string dll;
  for (const wchar_t* p = argv[1]; *p; ++p) {
    if (*p > 127) return 120; // Detours' injection API uses an ANSI DLL path.
    dll += static_cast<char>(*p);
  }
  std::wstring command;
  for (int i = 4; i < argc; ++i) { if (i > 4) command += L' '; command += quote(argv[i]); }
  std::vector<wchar_t> mutableCommand(command.begin(), command.end());
  mutableCommand.push_back(0);
  HANDLE job = CreateJobObjectW(nullptr, nullptr);
  if (!job) return 121;
  JOBOBJECT_EXTENDED_LIMIT_INFORMATION limits{};
  limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
  if (!SetInformationJobObject(job, JobObjectExtendedLimitInformation, &limits, sizeof(limits))) {
    CloseHandle(job); return 121;
  }
  STARTUPINFOW startup{};
  startup.cb = sizeof(startup);
  startup.dwFlags = STARTF_USESTDHANDLES;
  startup.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
  startup.hStdOutput = GetStdHandle(STD_OUTPUT_HANDLE);
  startup.hStdError = GetStdHandle(STD_ERROR_HANDLE);
  PROCESS_INFORMATION process{};
  LPCSTR dlls[] = {dll.c_str()};
  const BOOL created = dll == "-"
    ? CreateProcessW(argv[4], mutableCommand.data(), nullptr, nullptr, TRUE,
        CREATE_SUSPENDED | CREATE_NO_WINDOW, nullptr, nullptr, &startup, &process)
    : DetourCreateProcessWithDllsW(argv[4], mutableCommand.data(), nullptr, nullptr,
        TRUE, CREATE_SUSPENDED | CREATE_NO_WINDOW, nullptr, nullptr, &startup, &process,
        1, dlls, nullptr);
  if (!created) { CloseHandle(job); return 122; }
  if (!AssignProcessToJobObject(job, process.hProcess) || ResumeThread(process.hThread) == static_cast<DWORD>(-1)) {
    TerminateProcess(process.hProcess, 123);
    CloseHandle(process.hThread); CloseHandle(process.hProcess); CloseHandle(job); return 123;
  }
  DWORD result = 124;
  if (WaitForSingleObject(process.hProcess, timeout) == WAIT_OBJECT_0) GetExitCodeProcess(process.hProcess, &result);
  JOBOBJECT_BASIC_ACCOUNTING_INFORMATION accounting{};
  // A frontend may outlive its parent briefly. Wait for this job's descendants,
  // then close only this job; no unrelated processes or persistent tracing service.
  const ULONGLONG waitUntil = GetTickCount64() + 5000;
  do {
    if (!QueryInformationJobObject(job, JobObjectBasicAccountingInformation,
                                  &accounting, sizeof(accounting), nullptr)) { result = 125; break; }
    if (!accounting.ActiveProcesses) break;
    Sleep(10);
  } while (GetTickCount64() < waitUntil);
  if (!QueryInformationJobObject(job, JobObjectBasicAccountingInformation,
                                &accounting, sizeof(accounting), nullptr)) result = 125;
  if (accounting.ActiveProcesses) result = 126;
  TerminateJobObject(job, 126); // Only descendants of this new compiler, never other processes.
  CloseHandle(process.hThread); CloseHandle(process.hProcess); CloseHandle(job);
  wchar_t path[32768];
  DWORD size = GetEnvironmentVariableW(L"VORPAL_NATIVE_AUDIT_TRACE", path, ARRAYSIZE(path));
  if (!size || size >= ARRAYSIZE(path)) return 127;
  std::wstring jobPath = std::wstring(path, size) + L".job.bin";
  HANDLE file = CreateFileW(jobPath.c_str(), GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE,
                           nullptr, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, nullptr);
  if (file == INVALID_HANDLE_VALUE) return 127;
  Record record{static_cast<uint32_t>(Kind::job), result, 0, accounting.TotalProcesses,
                process.dwProcessId, 0, 0, 0, 0, 0};
  DWORD written = 0;
  if (!WriteFile(file, &record, sizeof(record), &written, nullptr) || written != sizeof(record)) result = 127;
  CloseHandle(file);
  return static_cast<int>(result);
}
