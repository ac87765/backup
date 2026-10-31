import Cocoa
import FlutterMacOS

class MainFlutterWindow: NSWindow {
  override func awakeFromNib() {
    let flutterViewController = FlutterViewController()
    let windowFrame = self.frame
    self.contentViewController = flutterViewController
    self.setFrame(windowFrame, display: true)

    RegisterGeneratedPlugins(registry: flutterViewController)

    super.awakeFromNib()

    // 开机启动（--background）只进菜单栏，不显示主窗口。
    if ProcessInfo.processInfo.arguments.contains("--background") {
      self.orderOut(nil)
    }
  }
}
