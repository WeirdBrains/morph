import AVFoundation
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
    if let registrar = engineBridge.pluginRegistry.registrar(forPlugin: "MorphQrScanner") {
      MorphQrScanner.register(with: registrar)
    }
  }
}

/// `weirdnet-morph://` links opened from other apps (a scanned device-link
/// invite, for one), on the channel Android uses too
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

  /// Morph's own scheme and web links; nothing else.
  static func accepts(_ url: URL) -> Bool {
    switch url.scheme?.lowercased() {
    case "weirdnet-morph", "http", "https": return true
    default: return false
    }
  }
}

/// The camera QR scanner for device linking (Android has its own,
/// `QrScanActivity`): AVFoundation's built-in QR detection, no extra
/// libraries. `scan` answers the first code's text, or nil when the user
/// cancels or the camera is not allowed.
final class MorphQrScanner: NSObject, FlutterPlugin {
  private weak var registrar: FlutterPluginRegistrar?
  private var scanning = false

  init(registrar: FlutterPluginRegistrar) {
    self.registrar = registrar
  }

  static func register(with registrar: FlutterPluginRegistrar) {
    let channel = FlutterMethodChannel(name: "org.weirdbrains.morph/scan", binaryMessenger: registrar.messenger())
    registrar.addMethodCallDelegate(MorphQrScanner(registrar: registrar), channel: channel)
  }

  func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
    guard call.method == "scan" else {
      result(FlutterMethodNotImplemented)
      return
    }
    if scanning {
      result(nil)
      return
    }
    scanning = true
    let finish: (String?) -> Void = { [weak self] text in
      self?.scanning = false
      result(text)
    }
    switch AVCaptureDevice.authorizationStatus(for: .video) {
    case .authorized:
      present(finish)
    case .notDetermined:
      AVCaptureDevice.requestAccess(for: .video) { granted in
        DispatchQueue.main.async {
          if granted {
            self.present(finish)
          } else {
            finish(nil)
          }
        }
      }
    default:
      finish(nil)
    }
  }

  private func present(_ finish: @escaping (String?) -> Void) {
    guard var top = registrar?.viewController ?? Self.keyWindowRoot() else {
      finish(nil)
      return
    }
    while let shown = top.presentedViewController {
      top = shown
    }
    let scanner = QrScanViewController(onDone: finish)
    scanner.modalPresentationStyle = .fullScreen
    top.present(scanner, animated: true)
  }

  private static func keyWindowRoot() -> UIViewController? {
    UIApplication.shared.connectedScenes
      .compactMap { $0 as? UIWindowScene }
      .flatMap(\.windows)
      .first(where: \.isKeyWindow)?
      .rootViewController
  }
}

/// Full-screen camera preview that closes on the first QR code or on Cancel.
final class QrScanViewController: UIViewController, AVCaptureMetadataOutputObjectsDelegate {
  private let session = AVCaptureSession()
  private var preview: AVCaptureVideoPreviewLayer?
  private var onDone: ((String?) -> Void)?

  init(onDone: @escaping (String?) -> Void) {
    self.onDone = onDone
    super.init(nibName: nil, bundle: nil)
  }

  required init?(coder: NSCoder) {
    fatalError("init(coder:) is not used")
  }

  override func viewDidLoad() {
    super.viewDidLoad()
    view.backgroundColor = .black
    guard
      let camera = AVCaptureDevice.default(for: .video),
      let input = try? AVCaptureDeviceInput(device: camera),
      session.canAddInput(input)
    else {
      DispatchQueue.main.async { self.close(nil) }
      return
    }
    session.addInput(input)
    let output = AVCaptureMetadataOutput()
    guard session.canAddOutput(output) else {
      DispatchQueue.main.async { self.close(nil) }
      return
    }
    session.addOutput(output)
    output.setMetadataObjectsDelegate(self, queue: .main)
    if output.availableMetadataObjectTypes.contains(.qr) {
      output.metadataObjectTypes = [.qr]
    }

    let layer = AVCaptureVideoPreviewLayer(session: session)
    layer.videoGravity = .resizeAspectFill
    layer.frame = view.bounds
    view.layer.addSublayer(layer)
    preview = layer

    let hint = UILabel()
    hint.text = "Point the camera at the code on the other device"
    hint.textColor = .white
    hint.textAlignment = .center
    hint.numberOfLines = 0
    hint.translatesAutoresizingMaskIntoConstraints = false
    view.addSubview(hint)

    let cancel = UIButton(type: .system)
    cancel.setTitle("Cancel", for: .normal)
    cancel.setTitleColor(.white, for: .normal)
    cancel.titleLabel?.font = .preferredFont(forTextStyle: .headline)
    cancel.accessibilityIdentifier = "qr-scan-cancel"
    cancel.addTarget(self, action: #selector(cancelTapped), for: .touchUpInside)
    cancel.translatesAutoresizingMaskIntoConstraints = false
    view.addSubview(cancel)

    let guide = view.safeAreaLayoutGuide
    NSLayoutConstraint.activate([
      hint.leadingAnchor.constraint(equalTo: guide.leadingAnchor, constant: 24),
      hint.trailingAnchor.constraint(equalTo: guide.trailingAnchor, constant: -24),
      hint.topAnchor.constraint(equalTo: guide.topAnchor, constant: 24),
      cancel.centerXAnchor.constraint(equalTo: guide.centerXAnchor),
      cancel.bottomAnchor.constraint(equalTo: guide.bottomAnchor, constant: -24),
    ])
  }

  override func viewDidLayoutSubviews() {
    super.viewDidLayoutSubviews()
    preview?.frame = view.bounds
  }

  override func viewWillAppear(_ animated: Bool) {
    super.viewWillAppear(animated)
    let session = self.session
    DispatchQueue.global(qos: .userInitiated).async { session.startRunning() }
  }

  override func viewWillDisappear(_ animated: Bool) {
    super.viewWillDisappear(animated)
    let session = self.session
    DispatchQueue.global(qos: .userInitiated).async { session.stopRunning() }
  }

  func metadataOutput(
    _ output: AVCaptureMetadataOutput,
    didOutput metadataObjects: [AVMetadataObject],
    from connection: AVCaptureConnection
  ) {
    guard
      let code = metadataObjects.compactMap({ $0 as? AVMetadataMachineReadableCodeObject }).first(where: { $0.type == .qr }),
      let text = code.stringValue
    else {
      return
    }
    close(text)
  }

  @objc private func cancelTapped() {
    close(nil)
  }

  private func close(_ text: String?) {
    guard let done = onDone else { return }
    onDone = nil
    dismiss(animated: true) { done(text) }
  }
}
