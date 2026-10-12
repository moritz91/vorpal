#include <windows.h>
#include <process.h>
#include <string>
#include <vector>
#include "command_line.h"

static bool spawn(bool ansi, const wchar_t* image, const wchar_t* file,
                   const wchar_t* mode, PROCESS_INFORMATION& child) {
  std::wstring command = quoteArgument(image) + L" " + quoteArgument(mode) + L" " + quoteArgument(file);
  std::vector<wchar_t> wide(command.begin(), command.end()); wide.push_back(0);
  STARTUPINFOW startup{}; startup.cb = sizeof(startup);
  startup.dwFlags = STARTF_USESTDHANDLES;
  startup.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
  startup.hStdOutput = GetStdHandle(STD_OUTPUT_HANDLE);
  startup.hStdError = GetStdHandle(STD_ERROR_HANDLE);
  if (!ansi) return CreateProcessW(image, wide.data(), nullptr, nullptr, TRUE,
      CREATE_NO_WINDOW, nullptr, nullptr, &startup, &child) != FALSE;
  auto narrow = [](const wchar_t* text) {
    int size = WideCharToMultiByte(CP_ACP, 0, text, -1, nullptr, 0, nullptr, nullptr);
    std::vector<char> bytes(size);
    if (size) WideCharToMultiByte(CP_ACP, 0, text, -1, bytes.data(), size, nullptr, nullptr);
    return bytes;
  };
  auto executable = narrow(image), arguments = narrow(command.c_str());
  STARTUPINFOA sa{}; sa.cb = sizeof(sa); sa.dwFlags = startup.dwFlags;
  sa.hStdInput = startup.hStdInput; sa.hStdOutput = startup.hStdOutput; sa.hStdError = startup.hStdError;
  return !executable.empty() && !arguments.empty() && CreateProcessA(executable.data(), arguments.data(),
      nullptr, nullptr, TRUE, CREATE_NO_WINDOW, nullptr, nullptr, &sa, &child) != FALSE;
}

int wmain(int argc, wchar_t** argv) {
  if (argc != 3) return 10;
  std::wstring mode = argv[1];
  if (mode == L"leaf" || mode == L"linger") {
    HANDLE file = CreateFileW(argv[2], GENERIC_READ, FILE_SHARE_READ, nullptr,
                             OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (file == INVALID_HANDLE_VALUE) return 11;
    char bytes[16]; DWORD read = 0;
    BOOL ok = ReadFile(file, bytes, sizeof(bytes), &read, nullptr);
    CloseHandle(file);
    if (mode == L"linger") Sleep(80);
    return ok && read ? 0 : 12;
  }
  if (mode == L"timeout") { Sleep(60000); return 0; }
  if (mode != L"wide" && mode != L"ansi" && mode != L"crt" && mode != L"burst" && mode != L"orphan") return 10;
  wchar_t image[32768];
  DWORD size = GetModuleFileNameW(nullptr, image, ARRAYSIZE(image));
  if (!size || size >= ARRAYSIZE(image)) return 13;
  if (mode == L"crt") {
    // MS CRT spawn joins argv with spaces; preserve this physical file argument.
    auto quotedImage = quoteArgument(image), quotedFile = quoteArgument(argv[2]);
    const wchar_t* arguments[] = {quotedImage.c_str(), L"leaf", quotedFile.c_str(), nullptr};
    return _wspawnv(_P_WAIT, image, arguments) == 0 ? 0 : 14;
  }
  const int count = mode == L"burst" ? 8 : 1;
  std::vector<PROCESS_INFORMATION> children;
  for (int i = 0; i < count; ++i) {
    PROCESS_INFORMATION child{};
    if (!spawn(mode == L"ansi" || (mode == L"burst" && (i % 2)), image, argv[2],
               mode == L"orphan" ? L"linger" : L"leaf", child)) return 15;
    children.push_back(child);
  }
  int result = 0;
  for (const auto& child : children) {
    if (mode != L"orphan") {
      DWORD code = 0;
      if (WaitForSingleObject(child.hProcess, 10000) != WAIT_OBJECT_0 ||
          !GetExitCodeProcess(child.hProcess, &code) || code) result = 16;
    }
    CloseHandle(child.hThread); CloseHandle(child.hProcess);
  }
  return result;
}
