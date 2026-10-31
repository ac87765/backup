import Cocoa
import FlutterMacOS
import ServiceManagement

class MainFlutterWindow: NSWindow {
  override func awakeFromNib() {
    let flutterViewController = FlutterViewController()
    let windowFrame = self.frame
    self.contentViewController = flutterViewController
    self.setFrame(windowFrame, display: true)

    RegisterGeneratedPlugins(registry: flutterViewController)
    registerLaunchAtStartupChannel(messenger: flutterViewController.engine.binaryMessenger)

    super.awakeFromNib()

    // 开机启动（--background）只进菜单栏，不显示主窗口。
    if ProcessInfo.processInfo.arguments.contains("--background") {
      self.orderOut(nil)
    }
  }

  /// `launch_at_startup` 包在 macOS 上通过名为 `launch_at_startup` 的通道调用宿主实现
  /// （见该包 README）。这里用系统自带的 SMAppService（macOS 13+）登记 Login Item，
  /// 不引入第三方 Swift 包。macOS 12 上报告未开启，开启时返回错误由界面提示。
  private func registerLaunchAtStartupChannel(messenger: FlutterBinaryMessenger) {
    let channel = FlutterMethodChannel(name: "launch_at_startup", binaryMessenger: messenger)
    channel.setMethodCallHandler { call, result in
      switch call.method {
      case "launchAtStartupIsEnabled":
        if #available(macOS 13.0, *) {
          result(SMAppService.mainApp.status == .enabled)
        } else {
          result(false)
        }
      case "launchAtStartupSetEnabled":
        guard
          let arguments = call.arguments as? [String: Any],
          let enabled = arguments["setEnabledValue"] as? Bool
        else {
          result(FlutterError(code: "invalid_arguments", message: "缺少开机启动参数", details: nil))
          return
        }
        guard #available(macOS 13.0, *) else {
          result(FlutterError(code: "unsupported", message: "开机自动启动需要 macOS 13 或更高版本", details: nil))
          return
        }
        do {
          if enabled {
            try SMAppService.mainApp.register()
          } else {
            try SMAppService.mainApp.unregister()
          }
          result(nil)
        } catch {
          result(FlutterError(code: "launch_at_login_failed", message: "无法更改开机自动启动设置", details: nil))
        }
      default:
        result(FlutterMethodNotImplemented)
      }
    }
  }
}
