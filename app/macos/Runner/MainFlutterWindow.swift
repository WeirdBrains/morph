import Cocoa
import FlutterMacOS

class MainFlutterWindow: NSWindow {
  override func awakeFromNib() {
    let flutterViewController = FlutterViewController()
    let windowFrame = self.frame
    self.contentViewController = flutterViewController
    self.setFrame(windowFrame, display: true)

    RegisterGeneratedPlugins(registry: flutterViewController)
    let messenger = flutterViewController.engine.binaryMessenger
    DefaultBrowser.register(messenger: messenger)
    IncomingLinks.register(messenger: messenger)

    super.awakeFromNib()
  }
}

/// Default-browser status and request through Launch Services. macOS shows
/// its own "use Morph as your default browser?" prompt for the change.
enum DefaultBrowser {
  static let channelName = "org.weirdbrains.morph/default_browser"

  static func register(messenger: FlutterBinaryMessenger) {
    let channel = FlutterMethodChannel(name: channelName, binaryMessenger: messenger)
    channel.setMethodCallHandler { call, result in
      switch call.method {
      case "status":
        result(isDefault())
      case "request":
        request(result)
      default:
        result(FlutterMethodNotImplemented)
      }
    }
  }

  static func isDefault() -> Bool {
    guard let probe = URL(string: "https://example.com"),
      let handler = NSWorkspace.shared.urlForApplication(toOpen: probe)
    else {
      return false
    }
    return handler.standardizedFileURL == Bundle.main.bundleURL.standardizedFileURL
  }

  static func request(_ result: @escaping FlutterResult) {
    if isDefault() {
      result("madeDefault")
      return
    }
    let app = Bundle.main.bundleURL
    NSWorkspace.shared.setDefaultApplication(at: app, toOpenURLsWithScheme: "http") { error in
      guard error == nil else {
        DispatchQueue.main.async { result("declined") }
        return
      }
      NSWorkspace.shared.setDefaultApplication(at: app, toOpenURLsWithScheme: "https") { error in
        DispatchQueue.main.async {
          result(error == nil && isDefault() ? "madeDefault" : "declined")
        }
      }
    }
  }
}

/// Web links macOS opens in Morph (Apple Events), forwarded to Dart.
enum IncomingLinks {
  static let channelName = "org.weirdbrains.morph/links"
  private static var channel: FlutterMethodChannel?
  private static var initial: [String] = []

  static func register(messenger: FlutterBinaryMessenger) {
    let channel = FlutterMethodChannel(name: channelName, binaryMessenger: messenger)
    channel.setMethodCallHandler { call, result in
      if call.method == "initialLink" {
        result(initial.isEmpty ? nil : initial.removeFirst())
        for url in initial {
          channel.invokeMethod("open", arguments: url)
        }
        initial.removeAll()
      } else {
        result(FlutterMethodNotImplemented)
      }
    }
    self.channel = channel
  }

  static func open(_ urls: [URL]) {
    for url in urls where url.scheme == "http" || url.scheme == "https" {
      if let channel = channel {
        channel.invokeMethod("open", arguments: url.absoluteString)
      } else {
        initial.append(url.absoluteString)
      }
    }
  }
}
