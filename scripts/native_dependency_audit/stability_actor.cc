#include <windows.h>
#include <winioctl.h>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

// Only the new private fixture file named by this driver's argv is touched.
// READY/OBSERVED/RESTORED form a bounded handshake with the guard-owning process.
static constexpr DWORD sharing = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
static bool write(HANDLE file,const void* bytes,DWORD size,bool async,DWORD& error) {
  OVERLAPPED operation{}; operation.hEvent = async ? CreateEventW(nullptr,TRUE,FALSE,nullptr) : nullptr;
  if (async && !operation.hEvent) { error=GetLastError(); return false; }
  LARGE_INTEGER zero{}; DWORD actual = 0;
  if (!async && !SetFilePointerEx(file,zero,nullptr,FILE_BEGIN)) { error=GetLastError(); return false; }
  BOOL ok=WriteFile(file,bytes,size,&actual,async ? &operation : nullptr);
  error=ok ? 0 : GetLastError();
  if (!ok && async && error==ERROR_IO_PENDING) {
    if (WaitForSingleObject(operation.hEvent,10000)!=WAIT_OBJECT_0) {
      CancelIoEx(file,&operation);
      if (WaitForSingleObject(operation.hEvent,10000)!=WAIT_OBJECT_0) ExitProcess(29);
      error=ERROR_TIMEOUT; CloseHandle(operation.hEvent); return false;
    }
    ok=GetOverlappedResult(file,&operation,&actual,FALSE); error=ok ? 0 : GetLastError();
  }
  if (operation.hEvent) CloseHandle(operation.hEvent);
  if (ok) FlushFileBuffers(file);
  return ok && actual==size;
}
static std::vector<char> read(const std::wstring& path) {
  HANDLE file=CreateFileW(path.c_str(),GENERIC_READ,sharing,nullptr,OPEN_EXISTING,0,nullptr);
  if (file==INVALID_HANDLE_VALUE) return {};
  std::vector<char> data(8192); DWORD size=0;
  if (!ReadFile(file,data.data(),static_cast<DWORD>(data.size()),&size,nullptr)) size=0;
  CloseHandle(file); data.resize(size); return data;
}
static bool command(const char* wanted) {
  char line[32]{}; return fgets(line,sizeof(line),stdin) && strcmp(line,wanted)==0;
}
int wmain(int argc,wchar_t** argv) {
  if (argc==2 && std::wstring(argv[1])==L"--abi") {
    printf("{\"overlapped\":%zu,\"fileId\":%zu,\"oplockInput\":%zu,\"oplockOutput\":%zu,\"requestOplock\":%lu}\n",
      sizeof(OVERLAPPED),sizeof(FILE_ID_INFO),sizeof(REQUEST_OPLOCK_INPUT_BUFFER),sizeof(REQUEST_OPLOCK_OUTPUT_BUFFER),static_cast<DWORD>(FSCTL_REQUEST_OPLOCK));
    return 0;
  }
  if (argc!=3) return 10;
  std::wstring scenario=argv[1], path=argv[2];
  auto original=read(path); if (original.empty() || original.size()>4096) return 11;
  std::vector<char> changed(original.size(),'X');
  HANDLE held=INVALID_HANDLE_VALUE, section=nullptr; char* view=nullptr;
  const bool delayedMap=scenario==L"existing-section-unmapped" || scenario==L"existing-section-readonly-view";
  const bool mapping=delayedMap || scenario==L"existing-map-open" || scenario==L"existing-map-closed" || scenario==L"new-map";
  const bool readAsync=scenario==L"async-read";
  const bool existing=scenario.rfind(L"existing-",0)==0 || readAsync;
  const bool async=scenario==L"existing-async-write";
  auto alias=path+L".alias";
  if (scenario==L"hardlink-write" && !CreateHardLinkW(alias.c_str(),path.c_str(),nullptr)) return 12;
  if (existing) {
    held=CreateFileW(path.c_str(),readAsync ? GENERIC_READ : GENERIC_READ|GENERIC_WRITE,sharing,nullptr,OPEN_EXISTING,
        (async || readAsync) ? FILE_FLAG_OVERLAPPED : FILE_ATTRIBUTE_NORMAL,nullptr);
    if (held==INVALID_HANDLE_VALUE) return 13;
    if (mapping) {
      section=CreateFileMappingW(held,nullptr,PAGE_READWRITE,0,0,nullptr);
      view=section && scenario!=L"existing-section-unmapped" ? static_cast<char*>(MapViewOfFile(section,
          delayedMap ? FILE_MAP_READ : FILE_MAP_READ|FILE_MAP_WRITE,0,0,0)) : nullptr;
      if (!section || (scenario!=L"existing-section-unmapped" && !view)) return 14;
      if (delayedMap) { CloseHandle(held); held=INVALID_HANDLE_VALUE; }
      if (scenario==L"existing-map-closed") { CloseHandle(held); held=INVALID_HANDLE_VALUE; CloseHandle(section); section=nullptr; }
    }
  }
  printf("{\"phase\":\"ready\",\"pid\":%lu,\"writerOpen\":%s,\"viewOpen\":%s,\"sectionOpen\":%s}\n",GetCurrentProcessId(),held!=INVALID_HANDLE_VALUE ? "true":"false",view ? "true":"false",section ? "true":"false"); fflush(stdout);
  bool acted=command("run\n"), mutation=false, changedRead=false, readCompleted=false, readPending=false, sawOriginal=false; DWORD error=0;
  bool renamedFile=false, renamedDirectory=false, optionalCreated=false;
  auto backup=path+L".backup", replacement=path+L".replacement";
  auto slash=path.find_last_of(L"\\/"); if (slash==std::wstring::npos) return 15;
  auto directory=path.substr(0,slash), moved=directory+L".moved";
  auto optional=directory+L"/optional.h";
  if (acted) {
    if (readAsync) {
      std::vector<char> bytes(8192); DWORD amount=0;
      OVERLAPPED operation{}; operation.hEvent=CreateEventW(nullptr,TRUE,FALSE,nullptr);
      if (!operation.hEvent) return 30;
      BOOL ok=ReadFile(held,bytes.data(),static_cast<DWORD>(bytes.size()),nullptr,&operation);
      readPending=!ok && GetLastError()==ERROR_IO_PENDING;
      if (!ok && !readPending) error=GetLastError();
      else {
        if (WaitForSingleObject(operation.hEvent,10000)!=WAIT_OBJECT_0) ExitProcess(29);
        readCompleted=GetOverlappedResult(held,&operation,&amount,FALSE)!=FALSE;
        error=readCompleted ? 0:GetLastError();
        sawOriginal=readCompleted && amount==original.size() && memcmp(bytes.data(),original.data(),amount)==0;
      }
      CloseHandle(operation.hEvent);
    } else if (scenario==L"file-replace") {
      HANDLE alternate=CreateFileW(replacement.c_str(),GENERIC_WRITE,sharing,nullptr,CREATE_NEW,0,nullptr);
      DWORD ignored=0;
      if (alternate==INVALID_HANDLE_VALUE || !write(alternate,changed.data(),static_cast<DWORD>(changed.size()),false,ignored)) return 16;
      CloseHandle(alternate);
      renamedFile=MoveFileExW(path.c_str(),backup.c_str(),MOVEFILE_WRITE_THROUGH)!=FALSE;
      error=renamedFile ? 0:GetLastError();
      mutation=renamedFile && MoveFileExW(replacement.c_str(),path.c_str(),MOVEFILE_WRITE_THROUGH);
      if (renamedFile && !mutation) return 17;
    } else if (scenario==L"directory-rename") {
      renamedDirectory=MoveFileExW(directory.c_str(),moved.c_str(),MOVEFILE_WRITE_THROUGH)!=FALSE;
      error=renamedDirectory ? 0:GetLastError();
      if (renamedDirectory) {
        if (!CreateDirectoryW(directory.c_str(),nullptr)) return 18;
        HANDLE alternate=CreateFileW(path.c_str(),GENERIC_WRITE,sharing,nullptr,CREATE_NEW,0,nullptr);
        if (alternate==INVALID_HANDLE_VALUE || !write(alternate,changed.data(),static_cast<DWORD>(changed.size()),false,error)) return 19;
        CloseHandle(alternate); mutation=true;
      }
    } else if (scenario==L"directory-shadow") {
      HANDLE extra=CreateFileW(optional.c_str(),GENERIC_WRITE,sharing,nullptr,CREATE_NEW,0,nullptr);
      if (extra!=INVALID_HANDLE_VALUE) { optionalCreated=write(extra,changed.data(),static_cast<DWORD>(changed.size()),false,error); CloseHandle(extra); }
      else error=GetLastError();
      mutation=optionalCreated;
    } else if (scenario!=L"stable-read") {
      if (held==INVALID_HANDLE_VALUE && !view && !section) {
        held=CreateFileW((scenario==L"hardlink-write" ? alias:path).c_str(),GENERIC_READ|GENERIC_WRITE,sharing,nullptr,OPEN_EXISTING,0,nullptr);
        if (held==INVALID_HANDLE_VALUE) error=GetLastError();
      }
      if (delayedMap && section) {
        char* writable=static_cast<char*>(MapViewOfFile(section,FILE_MAP_WRITE|FILE_MAP_READ,0,0,0));
        DWORD mapError=writable ? 0:GetLastError();
        if (view) UnmapViewOfFile(view);
        view=writable;
        if (!view) error=mapError;
      }
      if (mapping && !view && held!=INVALID_HANDLE_VALUE) {
        section=CreateFileMappingW(held,nullptr,PAGE_READWRITE,0,0,nullptr);
        view=section ? static_cast<char*>(MapViewOfFile(section,FILE_MAP_WRITE|FILE_MAP_READ,0,0,0)) : nullptr;
        if (!view) error=GetLastError();
      }
      if (view) {
        memcpy(view,changed.data(),changed.size()); FlushViewOfFile(view,changed.size());
        mutation=true; changedRead=memcmp(view,changed.data(),changed.size())==0;
      } else if (held!=INVALID_HANDLE_VALUE) mutation=write(held,changed.data(),static_cast<DWORD>(changed.size()),async,error);
    }
    if (!view && scenario!=L"directory-shadow") changedRead=read(path)==changed;
  }
  printf("{\"phase\":\"observed\",\"acted\":%s,\"mutationSucceeded\":%s,\"mutationError\":%lu,\"consumerSawChanged\":%s,\"readCompleted\":%s,\"readPending\":%s,\"consumerSawOriginal\":%s}\n",acted?"true":"false",mutation?"true":"false",error,changedRead?"true":"false",readCompleted?"true":"false",readPending?"true":"false",sawOriginal?"true":"false"); fflush(stdout);
  if (!command("restore\n")) return 20;
  // The controller observes the attempt before releasing the guard. Directory
  // shadow and range/mapping controls retain it through restoration as well.
  if (view) { if (mutation) { memcpy(view,original.data(),original.size()); FlushViewOfFile(view,original.size()); } UnmapViewOfFile(view); }
  if (section) CloseHandle(section);
  if (held!=INVALID_HANDLE_VALUE) { if (mutation && !mapping && !renamedFile && !renamedDirectory && !optionalCreated) { DWORD ignored=0; if (!write(held,original.data(),static_cast<DWORD>(original.size()),async,ignored)) return 21; } CloseHandle(held); }
  if (renamedFile) { if (!DeleteFileW(path.c_str()) || !MoveFileExW(backup.c_str(),path.c_str(),MOVEFILE_WRITE_THROUGH)) return 22; }
  else if (acted && scenario==L"file-replace" && !DeleteFileW(replacement.c_str())) return 23;
  if (renamedDirectory) { if (!DeleteFileW(path.c_str()) || !RemoveDirectoryW(directory.c_str()) || !MoveFileExW(moved.c_str(),directory.c_str(),MOVEFILE_WRITE_THROUGH)) return 24; }
  if (optionalCreated && !DeleteFileW(optional.c_str())) return 25;
  if (scenario==L"hardlink-write" && !DeleteFileW(alias.c_str())) return 26;
  bool restored=read(path)==original;
  printf("{\"phase\":\"restored\",\"originalBytesRestored\":%s}\n",restored?"true":"false"); fflush(stdout);
  return restored ? 0:27;
}
