#pragma once
#include <windows.h>
#include <cstdint>

// Local binary artifacts contain paths, never arguments, environment or file bytes.
// The Python audit strictly bounds lengths and requires start/end/job records.
enum class Kind : uint32_t {
  start = 1, create, open, attributes, fullAttributes, read, section,
  directory, information, end, fault, job, child, directoryEntry
};
enum : uint32_t { pathUnavailable = 1, loggerFailed = 2 };
#pragma pack(push, 1)
struct Record {
  uint32_t kind, status, flags, access, thread, nameUnits, resolvedUnits;
  uint64_t sequence, begin, end;
};
#pragma pack(pop)
static_assert(sizeof(Record) == 52);
constexpr char magic[] = "VORPAL-NATIVE-AUDIT-1\n";
constexpr uint64_t maxTraceBytes = 64 * 1024 * 1024;
