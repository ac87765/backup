#include <flutter/dart_project.h>
#include <flutter/flutter_view_controller.h>
#include <windows.h>

#include <algorithm>

#include "flutter_window.h"
#include "utils.h"

namespace {

// 单实例：具名 Mutex 的生命周期与进程一致；第二个实例找到已有窗口后退出。
constexpr const wchar_t kSingleInstanceMutex[] =
    L"Local\\EncryptedBackupClient.SingleInstance";
constexpr const wchar_t kWindowClass[] = L"FLUTTER_RUNNER_WIN32_WINDOW";
constexpr const wchar_t kWindowTitle[] = L"\u52A0\u5BC6\u5907\u4EFD";

// 第一个实例可能还在启动：最多等待 2 秒找到它的窗口。
void ActivateExistingWindow() {
  HWND existing = nullptr;
  for (int attempt = 0; attempt < 20 && existing == nullptr; ++attempt) {
    existing = ::FindWindowW(kWindowClass, kWindowTitle);
    if (existing == nullptr) {
      ::Sleep(100);
    }
  }
  if (existing == nullptr) {
    return;
  }
  ::ShowWindow(existing, SW_RESTORE);
  ::SetForegroundWindow(existing);
}

}  // namespace

int APIENTRY wWinMain(_In_ HINSTANCE instance, _In_opt_ HINSTANCE prev,
                      _In_ wchar_t *command_line, _In_ int show_command) {
  // Attach to console when present (e.g., 'flutter run') or create a
  // new console when running with a debugger.
  if (!::AttachConsole(ATTACH_PARENT_PROCESS) && ::IsDebuggerPresent()) {
    CreateAndAttachConsole();
  }

  // Initialize COM, so that it is available for use in the library and/or
  // plugins.
  ::CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);

  HANDLE single_instance =
      ::CreateMutexW(nullptr, TRUE, kSingleInstanceMutex);
  if (single_instance != nullptr && ::GetLastError() == ERROR_ALREADY_EXISTS) {
    ActivateExistingWindow();
    ::CloseHandle(single_instance);
    ::CoUninitialize();
    return EXIT_SUCCESS;
  }

  flutter::DartProject project(L"data");

  std::vector<std::string> command_line_arguments =
      GetCommandLineArguments();
  const bool start_hidden =
      std::find(command_line_arguments.begin(), command_line_arguments.end(),
                "--background") != command_line_arguments.end();

  project.set_dart_entrypoint_arguments(std::move(command_line_arguments));

  FlutterWindow window(project, start_hidden);
  Win32Window::Point origin(10, 10);
  Win32Window::Size size(1280, 800);
  if (!window.Create(L"\u52A0\u5BC6\u5907\u4EFD", origin, size)) {
    if (single_instance != nullptr) {
      ::CloseHandle(single_instance);
    }
    ::CoUninitialize();
    return EXIT_FAILURE;
  }
  window.SetQuitOnClose(true);

  ::MSG msg;
  while (::GetMessage(&msg, nullptr, 0, 0)) {
    ::TranslateMessage(&msg);
    ::DispatchMessage(&msg);
  }

  if (single_instance != nullptr) {
    ::CloseHandle(single_instance);
  }
  ::CoUninitialize();
  return EXIT_SUCCESS;
}
