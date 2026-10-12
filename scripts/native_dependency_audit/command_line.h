#pragma once
#include <string>

// Windows CRT quoting; no shell and no command line in the audit artifacts.
inline std::wstring quoteArgument(const wchar_t* argument) {
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
