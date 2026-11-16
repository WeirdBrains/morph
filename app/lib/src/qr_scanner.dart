import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:material_ui/material_ui.dart';
import 'package:mobile_scanner/mobile_scanner.dart';

import 'theme/morph_theme.dart';

/// The app's navigator, so [MorphQrScanner.scan] can push its screen
/// without a [BuildContext] (set on Morph's MaterialApp).
final morphNavigatorKey = GlobalKey<NavigatorState>();

/// The camera QR scanner for device linking: [MorphQrScannerPage] on top of
/// Morph's navigator (mobile_scanner: bundled ML Kit on Android, Vision on
/// iOS). Desktops have no scanner; they paste the link text instead.
class MorphQrScanner {
  static bool get available =>
      !kIsWeb && (Platform.isAndroid || Platform.isIOS);

  /// The text of the first QR code the camera sees, or null when the user
  /// backed out, denied the camera, or there is no scanner.
  static Future<String?> scan() async {
    final navigator = morphNavigatorKey.currentState;
    if (!available || navigator == null) {
      return null;
    }
    return navigator.push<String>(
      MaterialPageRoute(
        fullscreenDialog: true,
        builder: (_) => const MorphQrScannerPage(),
      ),
    );
  }
}

/// Builds the camera view; it calls `onCode` with each code's text.
typedef MorphQrCamera = Widget Function(ValueChanged<String> onCode);

/// Full-screen camera that closes with the first QR code's text, or with
/// null on Back or Cancel.
class MorphQrScannerPage extends StatefulWidget {
  const MorphQrScannerPage({super.key, this.camera});

  /// The camera view; null uses mobile_scanner (tests pass a fake).
  final MorphQrCamera? camera;

  @override
  State<MorphQrScannerPage> createState() => _MorphQrScannerPageState();
}

class _MorphQrScannerPageState extends State<MorphQrScannerPage> {
  bool _done = false;

  void _onCode(String text) {
    if (_done || text.trim().isEmpty || !mounted) {
      return;
    }
    _done = true;
    Navigator.of(context).pop(text.trim());
  }

  Widget _camera() {
    final camera = widget.camera;
    if (camera != null) {
      return camera(_onCode);
    }
    return MobileScanner(
      key: const Key('qr-camera'),
      controller: MobileScannerController(
        formats: const [BarcodeFormat.qrCode],
      ),
      onDetect: (capture) {
        for (final barcode in capture.barcodes) {
          final text = barcode.rawValue;
          if (text != null && text.isNotEmpty) {
            _onCode(text);
            return;
          }
        }
      },
      errorBuilder: (context, error) => Center(
        child: Padding(
          padding: const EdgeInsets.all(32),
          child: Text(
            error.errorCode == MobileScannerErrorCode.permissionDenied
                ? 'Morph may not use the camera. Allow it in the system '
                      'settings, or paste the code instead.'
                : 'The camera did not start. Paste the code instead.',
            key: const Key('qr-camera-error'),
            textAlign: TextAlign.center,
            style: const TextStyle(color: Colors.white),
          ),
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Scaffold(
      key: const Key('qr-scanner'),
      backgroundColor: Colors.black,
      appBar: AppBar(
        backgroundColor: Colors.black,
        foregroundColor: Colors.white,
        title: const Text('Scan a code'),
        leading: IconButton(
          key: const Key('qr-scanner-cancel'),
          tooltip: 'Cancel',
          icon: const Icon(Icons.close),
          onPressed: () => Navigator.of(context).pop(),
        ),
      ),
      body: Stack(
        fit: StackFit.expand,
        children: [
          _camera(),
          Positioned(
            left: 0,
            right: 0,
            bottom: 0,
            child: SafeArea(
              top: false,
              child: Container(
                color: MorphColors.cyan.withValues(alpha: 0.9),
                padding: const EdgeInsets.symmetric(
                  horizontal: 16,
                  vertical: 14,
                ),
                child: Text(
                  'Place the code in front of the camera',
                  key: const Key('qr-scanner-hint'),
                  textAlign: TextAlign.center,
                  style: theme.textTheme.bodyMedium?.copyWith(
                    color: Colors.black,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}
