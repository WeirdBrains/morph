import Cocoa
import CryptoKit
import FlutterMacOS
import WebKit

/// Prepares a per-profile `WKWebsiteDataStore` for the next `WKWebView`
/// created by flutter_inappwebview. macOS 14+ only; earlier releases keep
/// the default store.
public class MorphWebViewInAppPlugin: NSObject, FlutterPlugin {
  private static var queue: [UUID] = []
  private static var swizzled = false
  private static let lock = NSLock()

  public static func register(with registrar: FlutterPluginRegistrar) {
    swizzleWebViewInitOnce()
    let channel = FlutterMethodChannel(
      name: "org.weirdbrains.morph_webview/profile",
      binaryMessenger: registrar.messenger)
    let instance = MorphWebViewInAppPlugin()
    registrar.addMethodCallDelegate(instance, channel: channel)
  }

  public func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
    switch call.method {
    case "prepare":
      guard let args = call.arguments as? [String: Any],
            let profileId = args["profileId"] as? String,
            !profileId.isEmpty else {
        result(FlutterError(code: "bad-args", message: "profileId required", details: nil))
        return
      }
      if #available(macOS 14.0, *) {
        MorphWebViewInAppPlugin.lock.lock()
        MorphWebViewInAppPlugin.queue.append(Self.uuid(forProfile: profileId))
        MorphWebViewInAppPlugin.lock.unlock()
        result(true)
      } else {
        result(FlutterError(
          code: "unsupported",
          message: "WKWebsiteDataStore(forIdentifier:) requires macOS 14",
          details: nil))
      }
    default:
      result(FlutterMethodNotImplemented)
    }
  }

  static func takeNextStoreIdentifier() -> UUID? {
    lock.lock()
    defer { lock.unlock() }
    guard !queue.isEmpty else { return nil }
    return queue.removeFirst()
  }

  static func uuid(forProfile id: String) -> UUID {
    let digest = SHA256.hash(data: Data("morph-profile:\(id)".utf8))
    var bytes = Array(digest.prefix(16))
    bytes[6] = (bytes[6] & 0x0F) | 0x50
    bytes[8] = (bytes[8] & 0x3F) | 0x80
    return UUID(uuid: (
      bytes[0], bytes[1], bytes[2], bytes[3],
      bytes[4], bytes[5], bytes[6], bytes[7],
      bytes[8], bytes[9], bytes[10], bytes[11],
      bytes[12], bytes[13], bytes[14], bytes[15]
    ))
  }

  private static func swizzleWebViewInitOnce() {
    lock.lock()
    defer { lock.unlock() }
    if swizzled {
      return
    }
    swizzled = true
    let original = #selector(WKWebView.init(frame:configuration:))
    let swizzledSel = #selector(WKWebView.morph_init(frame:configuration:))
    guard let originalMethod = class_getInstanceMethod(WKWebView.self, original),
          let swizzledMethod = class_getInstanceMethod(WKWebView.self, swizzledSel) else {
      return
    }
    method_exchangeImplementations(originalMethod, swizzledMethod)
  }
}

extension WKWebView {
  @objc func morph_init(frame: CGRect, configuration: WKWebViewConfiguration) -> WKWebView {
    if #available(macOS 14.0, *),
       let identifier = MorphWebViewInAppPlugin.takeNextStoreIdentifier() {
      configuration.websiteDataStore = WKWebsiteDataStore(forIdentifier: identifier)
    }
    return self.morph_init(frame: frame, configuration: configuration)
  }
}
