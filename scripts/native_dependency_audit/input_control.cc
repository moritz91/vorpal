#include <windows.h>
#include <winternl.h>
#include <string>
#include <vector>
#include <cstring>

// This fixture mutates ONLY the explicit private test file supplied by the
// control driver. It is never used with a source/native preprocessing plan.
static constexpr char changed[] = "changed native input for ABA\n";
static bool replace(HANDLE file, const char* data, DWORD size) {
  LARGE_INTEGER zero{}; DWORD written = 0;
  LARGE_INTEGER current{};
  return GetFileSizeEx(file,&current) && SetFilePointerEx(file,zero,nullptr,FILE_BEGIN) &&
    WriteFile(file,data,size,&written,nullptr) && written == size && (current.QuadPart == size || SetEndOfFile(file)) && FlushFileBuffers(file);
}
struct Mutation { HANDLE file, go, done; bool ok = false; };
static DWORD WINAPI mutate(void* context) {
  auto& state = *static_cast<Mutation*>(context);
  if (WaitForSingleObject(state.go,10000) == WAIT_OBJECT_0)
    state.ok = replace(state.file,changed,sizeof(changed)-1);
  SetEvent(state.done); return 0;
}
int wmain(int argc, wchar_t** argv) {
  if (argc != 3) return 10;
  std::wstring mode = argv[1];
  const bool mapping = mode == L"mapping" || mode == L"aba-mapping";
  const bool mutateFile = mode == L"aba-read" || mode == L"aba-mapping" || mode == L"sustained-read";
  if (mode == L"missing") {
    HANDLE missing = CreateFileW((std::wstring(argv[2])+L".missing").c_str(),GENERIC_READ,
        FILE_SHARE_READ,nullptr,OPEN_EXISTING,0,nullptr);
    if (missing != INVALID_HANDLE_VALUE) { CloseHandle(missing); return 11; }
    return GetLastError() == ERROR_FILE_NOT_FOUND ? 0 : 12;
  }
  HANDLE file = CreateFileW(argv[2],GENERIC_READ | (mutateFile ? GENERIC_WRITE : 0),
    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,nullptr,OPEN_EXISTING,
    mode == L"async" ? FILE_FLAG_OVERLAPPED : FILE_ATTRIBUTE_NORMAL,nullptr);
  if (file == INVALID_HANDLE_VALUE) return 13;
  FILETIME creation{},access{},write{};
  if (!GetFileTime(file,&creation,&access,&write)) return 14;
  std::vector<char> original(4096); DWORD count = 0;
  if (mutateFile) {
    if (!ReadFile(file,original.data(),static_cast<DWORD>(original.size()),&count,nullptr)) return 15;
    original.resize(count);
  }
  HANDLE section = nullptr; const char* view = nullptr;
  if (mapping) {
    section = CreateFileMappingW(file,nullptr,PAGE_READONLY,0,0,nullptr);
    view = section ? static_cast<const char*>(MapViewOfFile(section,FILE_MAP_READ,0,0,0)) : nullptr;
    if (!view) return 16;
  }
  if (mutateFile) {
    Mutation state{file,CreateEventW(nullptr,FALSE,FALSE,nullptr),CreateEventW(nullptr,FALSE,FALSE,nullptr)};
    HANDLE thread = CreateThread(nullptr,0,mutate,&state,0,nullptr);
    if (!state.go || !state.done || !thread) return 17;
    SetEvent(state.go);
    if (WaitForSingleObject(state.done,10000) != WAIT_OBJECT_0 || !state.ok ||
        WaitForSingleObject(thread,10000) != WAIT_OBJECT_0) return 18;
    CloseHandle(thread); CloseHandle(state.go); CloseHandle(state.done);
  }
  char buffer[4096]{};
  if (mapping) {
    count = mutateFile ? sizeof(changed)-1 : 16;
    memcpy(buffer,view,count);
  } else if (mode == L"offset" || mode == L"invalid-offset") {
    using NativeRead = NTSTATUS (NTAPI *)(HANDLE,HANDLE,PIO_APC_ROUTINE,PVOID,PIO_STATUS_BLOCK,PVOID,ULONG,PLARGE_INTEGER,PULONG);
    auto read = reinterpret_cast<NativeRead>(GetProcAddress(GetModuleHandleW(L"ntdll.dll"),"NtReadFile"));
    IO_STATUS_BLOCK io{}; LARGE_INTEGER position{}; position.QuadPart = 3;
    if (!read) return 24;
    auto status = read(file,nullptr,nullptr,nullptr,&io,buffer,sizeof(buffer),
      mode == L"invalid-offset" ? reinterpret_cast<PLARGE_INTEGER>(0x1000) : &position,nullptr);
    if (mode == L"invalid-offset") {
      CloseHandle(file); return static_cast<DWORD>(status) == 0xc0000005 ? 0 : 28;
    }
    if (status != 0 || io.Information > sizeof(buffer)) return 24;
    count = static_cast<DWORD>(io.Information);
  } else if (mode == L"hardlink") {
    auto link = std::wstring(argv[2]) + L".alias";
    if (!CreateHardLinkW(link.c_str(),argv[2],nullptr)) return 25;
    HANDLE alias = CreateFileW(link.c_str(),GENERIC_READ,FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,nullptr,OPEN_EXISTING,FILE_ATTRIBUTE_NORMAL,nullptr);
    DWORD originalCount = 0;
    if (alias == INVALID_HANDLE_VALUE || !ReadFile(file,buffer,sizeof(buffer),&originalCount,nullptr) ||
        !ReadFile(alias,buffer,sizeof(buffer),&count,nullptr) || count != originalCount) return 26;
    CloseHandle(alias);
    if (!DeleteFileW(link.c_str())) return 27;
  } else if (mode == L"async") {
    OVERLAPPED async{}; async.hEvent = CreateEventW(nullptr,TRUE,FALSE,nullptr);
    BOOL ok = ReadFile(file,buffer,sizeof(buffer),nullptr,&async);
    if (!ok && GetLastError() != ERROR_IO_PENDING) return 19;
    if (!GetOverlappedResult(file,&async,&count,TRUE)) return 20;
    CloseHandle(async.hEvent);
  } else {
    LARGE_INTEGER position{}; position.QuadPart = mode == L"offset" ? 3 : 0;
    if (!SetFilePointerEx(file,position,nullptr,FILE_BEGIN) ||
        !ReadFile(file,buffer,sizeof(buffer),&count,nullptr)) return 21;
  }
  if (mapping) { UnmapViewOfFile(view); CloseHandle(section); }
  if (mutateFile && mode != L"sustained-read") {
    if (!replace(file,original.data(),static_cast<DWORD>(original.size())) ||
        !SetFileTime(file,&creation,&access,&write)) return 22;
  }
  DWORD written = 0;
  if (!WriteFile(GetStdHandle(STD_OUTPUT_HANDLE),buffer,count,&written,nullptr) || written != count) return 23;
  CloseHandle(file); return 0;
}
