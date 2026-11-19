import Flutter
import UIKit

@main
@objc class AppDelegate: FlutterAppDelegate, FlutterImplicitEngineDelegate {
  override func application(
    _ application: UIApplication,
    didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
  ) -> Bool {
    return super.application(application, didFinishLaunchingWithOptions: launchOptions)
  }

  func didInitializeImplicitFlutterEngine(_ engineBridge: FlutterImplicitEngineBridge) {
    GeneratedPluginRegistrant.register(with: engineBridge.pluginRegistry)
    if let registrar = engineBridge.pluginRegistry.registrar(forPlugin: "MorphIncomingLinks") {
      MorphIncomingLinks.register(with: registrar)
    }
  }
}

/// `weirdnet-morph://` links opened from other apps (a scanned device-link
/// invite, for one) and Ymir's `weirdnet-ymir://link/…` codes, on the channel Android uses too
/// (`MorphIncomingLinks` in Dart): the launch link waits until Dart asks for
/// it with `initialLink`; later ones are pushed with `open`.
final class MorphIncomingLinks: NSObject, FlutterPlugin, FlutterSceneLifeCycleDelegate {
  static let channelName = "org.weirdbrains.morph/links"

  private let channel: FlutterMethodChannel
  private var initialLink: String?
  private var dartAsked = false

  init(channel: FlutterMethodChannel) {
    self.channel = channel
  }

  static func register(with registrar: FlutterPluginRegistrar) {
    let channel = FlutterMethodChannel(name: channelName, binaryMessenger: registrar.messenger())
    let instance = MorphIncomingLinks(channel: channel)
    registrar.addMethodCallDelegate(instance, channel: channel)
    registrar.addSceneDelegate(instance)
  }

  func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
    switch call.method {
    case "initialLink":
      dartAsked = true
      result(initialLink)
      initialLink = nil
    case "initialShare":
      result(nil)
    default:
      result(FlutterMethodNotImplemented)
    }
  }

  func scene(
    _ scene: UIScene,
    willConnectTo session: UISceneSession,
    options connectionOptions: UIScene.ConnectionOptions?
  ) -> Bool {
    guard let url = connectionOptions?.urlContexts.first(where: { Self.accepts($0.url) })?.url else {
      return false
    }
    deliver(url.absoluteString)
    return true
  }

  func scene(_ scene: UIScene, openURLContexts URLContexts: Set<UIOpenURLContext>) -> Bool {
    let urls = URLContexts.map(\.url).filter(Self.accepts)
    for url in urls {
      deliver(url.absoluteString)
    }
    return !urls.isEmpty
  }

  private func deliver(_ link: String) {
    if dartAsked {
      channel.invokeMethod("open", arguments: link)
    } else {
      initialLink = link
    }
  }

  /// Morph's own scheme, Ymir's device-link codes and web links; nothing else.
  static func accepts(_ url: URL) -> Bool {
    switch url.scheme?.lowercased() {
    case "weirdnet-morph", "weirdnet-ymir", "http", "https": return true
    default: return false
    }
  }
}
