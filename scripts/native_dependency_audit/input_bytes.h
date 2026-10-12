#pragma once
#include <windows.h>
#include <cstdint>
#include <string>
#include <vector>

// Private local artifacts contain returned file bytes. Never upload these
// sidecars or use them as an OS-enforced completeness/race-exclusion contract.
enum class InputKind : uint32_t { start = 1, readReturn, loaderFile, sectionBoundary, end };
enum : uint32_t { identityUnavailable = 1, inputOverLimit = 2, pendingRead = 4,
  identityChanged = 8, offsetUnavailable = 16, mappingUnobserved = 32,
  bytesUnavailable = 64, inputLoggerFailed = 128 };
#pragma pack(push, 1)
struct InputRecord {
  uint32_t kind, status, flags, pid, thread, pathUnits, byteCount, reserved;
  uint64_t sequence, traceSequence, begin, end, handle, volume, created, offset;
  BYTE fileId[16];
};
#pragma pack(pop)
static_assert(sizeof(InputRecord) == 112);
constexpr char inputMagic[] = "VORPAL-NATIVE-INPUT-BYTES-1\n";
constexpr uint64_t maxInputBytes = 512 * 1024 * 1024;
constexpr DWORD maxInputPayload = 64 * 1024 * 1024;

struct FileIdentity {
  FILE_ID_INFO id{};
  bool known = false;
  explicit FileIdentity(HANDLE handle) {
    known = GetFileType(handle) == FILE_TYPE_DISK &&
      GetFileInformationByHandleEx(handle, FileIdInfo, &id, sizeof(id));
  }
  bool equals(const FileIdentity& other) const {
    return known && other.known && id.VolumeSerialNumber == other.id.VolumeSerialNumber &&
      memcmp(id.FileId.Identifier, other.id.FileId.Identifier, 16) == 0;
  }
};

class InputBytes {
  HANDLE file = INVALID_HANDLE_VALUE;
  SRWLOCK lock = SRWLOCK_INIT;
  uint64_t sequence = 0, bytes = 0;
  bool failed = false;
  bool write(const void* data, DWORD size) {
    DWORD actual = 0;
    return WriteFile(file, data, size, &actual, nullptr) && actual == size;
  }
public:
  bool open(const std::wstring& path) {
    file = CreateFileW(path.c_str(), GENERIC_WRITE, FILE_SHARE_READ, nullptr,
                       CREATE_NEW, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (file == INVALID_HANDLE_VALUE || !write(inputMagic, sizeof(inputMagic) - 1)) return false;
    bytes = sizeof(inputMagic) - 1;
    emit(InputKind::start, 0, 0, GetCurrentProcessId(), nullptr, FileIdentity(nullptr), L"", 0, 0, 0);
    return !failed;
  }
  static uint64_t clock() {
    LARGE_INTEGER value{}; QueryPerformanceCounter(&value); return static_cast<uint64_t>(value.QuadPart);
  }
  static uint64_t creation(HANDLE process) {
    FILETIME a{},b{},c{},d{};
    if (!GetProcessTimes(process,&a,&b,&c,&d)) return 0;
    return (static_cast<uint64_t>(a.dwHighDateTime) << 32) | a.dwLowDateTime;
  }
  static std::wstring pathOf(HANDLE handle) {
    wchar_t path[32768];
    DWORD size = GetFinalPathNameByHandleW(handle,path,ARRAYSIZE(path),FILE_NAME_NORMALIZED | VOLUME_NAME_DOS);
    return size && size < ARRAYSIZE(path) ? std::wstring(path,size) : std::wstring{};
  }
  void emit(InputKind kind, DWORD status, DWORD flags, DWORD pid, HANDLE handle,
      const FileIdentity& id, const std::wstring& path, uint64_t begin,
      uint64_t created, uint64_t offset, const void* data = nullptr, DWORD count = 0,
      uint64_t traceSequence = 0) {
    if (file == INVALID_HANDLE_VALUE) return;
    AcquireSRWLockExclusive(&lock);
    if (count > maxInputPayload) { flags |= inputOverLimit | bytesUnavailable; count = 0; }
    InputRecord record{static_cast<uint32_t>(kind),status,flags,pid,GetCurrentThreadId(),
      static_cast<uint32_t>(path.size()),count,0,++sequence,traceSequence,begin,clock(),
      reinterpret_cast<uint64_t>(handle),id.id.VolumeSerialNumber,created,offset,{}};
    memcpy(record.fileId,id.id.FileId.Identifier,16);
    if (failed) record.flags |= inputLoggerFailed;
    const DWORD pathBytes = record.pathUnits * 2;
    const uint64_t size = sizeof(record) + pathBytes + count;
    if (sequence > 100000 || path.size() > 32768 || bytes + size > maxInputBytes ||
        !write(&record,sizeof(record)) || !write(path.data(),pathBytes) || (count && !write(data,count))) failed = true;
    bytes += size;
    ReleaseSRWLockExclusive(&lock);
  }
  void loader(DWORD pid, HANDLE process, HANDLE handle, uint64_t base) {
    const uint64_t begin = clock();
    FileIdentity before(handle);
    DWORD flags = before.known ? 0 : identityUnavailable;
    std::vector<BYTE> data;
    LARGE_INTEGER size{},zero{};
    FILE_BASIC_INFO initial{},final{};
    if (!handle || handle == INVALID_HANDLE_VALUE ||
        !GetFileSizeEx(handle,&size) || size.QuadPart < 0) flags |= bytesUnavailable;
    else if (size.QuadPart > maxInputPayload) flags |= inputOverLimit | bytesUnavailable;
    else if (!GetFileInformationByHandleEx(handle,FileBasicInfo,&initial,sizeof(initial)) ||
             !SetFilePointerEx(handle,zero,nullptr,FILE_BEGIN)) flags |= bytesUnavailable;
    else {
      data.resize(static_cast<size_t>(size.QuadPart));
      DWORD offset = 0;
      while (offset < data.size()) {
        DWORD got = 0, requested = static_cast<DWORD>(data.size() - offset);
        if (!ReadFile(handle,data.data()+offset,requested,&got,nullptr) || !got) {
          flags |= bytesUnavailable; data.clear(); break;
        }
        offset += got;
      }
      LARGE_INTEGER afterSize{};
      FileIdentity after(handle);
      if (!after.known) flags |= identityUnavailable;
      if ((before.known && after.known && !before.equals(after)) || !GetFileSizeEx(handle,&afterSize) ||
          afterSize.QuadPart != size.QuadPart ||
          !GetFileInformationByHandleEx(handle,FileBasicInfo,&final,sizeof(final)) ||
          initial.LastWriteTime.QuadPart != final.LastWriteTime.QuadPart ||
          initial.ChangeTime.QuadPart != final.ChangeTime.QuadPart) flags |= identityChanged;
    }
    // These are file-handle snapshots at loader events, NOT loaded memory/pages.
    emit(InputKind::loaderFile,0,flags,pid,handle,before,pathOf(handle),begin,
         creation(process),base,data.data(),static_cast<DWORD>(data.size()));
  }
  bool close() {
    if (file == INVALID_HANDLE_VALUE) return true;
    emit(InputKind::end,0,failed ? inputLoggerFailed : 0,GetCurrentProcessId(),nullptr,
         FileIdentity(nullptr),L"",0,0,0);
    if (!FlushFileBuffers(file)) failed = true;
    CloseHandle(file); file = INVALID_HANDLE_VALUE;
    return !failed;
  }
  ~InputBytes() { if (file != INVALID_HANDLE_VALUE) CloseHandle(file); }
};
