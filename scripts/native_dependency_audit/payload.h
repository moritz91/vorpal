#pragma once
#include <windows.h>

// Private, bounded audit context copied only into a newly created, stopped
// member of this launcher's job. No environment/command line is inspected.
constexpr GUID auditPayloadGuid = {0x24c84c99, 0xb1a2, 0x4dd3,
    {0x97, 0xac, 0x5a, 0x16, 0xc6, 0xb2, 0xdd, 0x18}};
struct AuditPayload {
  DWORD version = 1;
  DWORD units = 0;
  wchar_t prefix[32768]{};
};
