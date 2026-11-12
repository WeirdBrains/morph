import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:qr/qr.dart';

import '../api.dart';
import '../theme/morph_theme.dart';
import 'identity_onboarding.dart' show morphMinPasswordLength;
import 'password_field.dart';

/// What this device calls itself on the other device's screen.
String morphDeviceName() {
  if (kIsWeb) {
    return 'Morph';
  }
  if (Platform.isAndroid) {
    return 'Morph on Android';
  }
  final host = Platform.localHostname.trim();
  return host.isEmpty ? 'Morph' : 'Morph on $host';
}

/// A QR code for [data], dark modules on a white card with the quiet zone
/// scanners need.
class MorphQrCode extends StatelessWidget {
  const MorphQrCode({super.key, required this.data, this.size = 240});

  final String data;
  final double size;

  @override
  Widget build(BuildContext context) {
    final image = QrImage(
      QrCode(
        payload: QrPayload.fromString(data),
        errorCorrectLevel: QrErrorCorrectLevel.low,
      ),
    );
    return Semantics(
      label: 'QR code',
      child: Container(
        color: Colors.white,
        padding: EdgeInsets.all(size / 14),
        child: CustomPaint(size: Size.square(size), painter: _QrPainter(image)),
      ),
    );
  }
}

class _QrPainter extends CustomPainter {
  _QrPainter(this.image);

  final QrImage image;

  @override
  void paint(Canvas canvas, Size size) {
    final n = image.moduleCount;
    final cell = size.width / n;
    final paint = Paint()..color = Colors.black;
    for (var row = 0; row < n; row++) {
      for (var col = 0; col < n; col++) {
        if (image.isDark(row, col)) {
          // A hair of overlap so neighbouring modules leave no seams.
          canvas.drawRect(
            Rect.fromLTWH(col * cell, row * cell, cell + 0.5, cell + 0.5),
            paint,
          );
        }
      }
    }
  }

  @override
  bool shouldRepaint(_QrPainter old) => old.image != image;
}

class _Sas extends StatelessWidget {
  const _Sas(this.sas);

  final String sas;

  @override
  Widget build(BuildContext context) {
    final spaced = sas.length == 6
        ? '${sas.substring(0, 3)} ${sas.substring(3)}'
        : sas;
    return Text(
      spaced,
      key: const Key('link-sas'),
      textAlign: TextAlign.center,
      style: MorphTheme.monoStyle(
        color: MorphColors.cyan,
        size: 40,
      ).copyWith(fontWeight: FontWeight.w700, letterSpacing: 4),
    );
  }
}

String _friendlyLinkError(Object e) {
  final text = '$e'.replaceFirst(RegExp(r'^(Bad state|Exception): '), '');
  if (text.contains('password_open') ||
      text.contains('decrypt') ||
      text.contains('wrong password')) {
    return 'That is not this profile\'s password.';
  }
  return text;
}

/// The new device's side of a QR login: show the code, compare the six
/// digits, then choose this device's password for the profile.
class MorphLinkFromDevice extends StatefulWidget {
  const MorphLinkFromDevice({
    super.key,
    required this.api,
    required this.profileId,
    required this.onDone,
    this.home,
    this.deviceName,
    this.pollEvery = const Duration(milliseconds: 500),
  });

  final MorphApi api;
  final String profileId;
  final String? home;
  final String? deviceName;
  final ValueChanged<MorphIdentity> onDone;
  final Duration pollEvery;

  @override
  State<MorphLinkFromDevice> createState() => _MorphLinkFromDeviceState();
}

class _MorphLinkFromDeviceState extends State<MorphLinkFromDevice> {
  MorphLinkOffer? _offer;
  MorphLinkStatus _status = const MorphLinkStatus(MorphLinkState.waiting);
  bool _confirmed = false;
  bool _busy = false;
  bool _finished = false;
  String? _error;
  Timer? _poll;
  final _password = TextEditingController();
  final _confirm = TextEditingController();

  @override
  void initState() {
    super.initState();
    unawaited(_start());
  }

  Future<void> _start() async {
    _poll?.cancel();
    setState(() {
      _offer = null;
      _status = const MorphLinkStatus(MorphLinkState.waiting);
      _confirmed = false;
      _error = null;
    });
    try {
      final offer = await widget.api.linkNewDeviceStart(
        widget.deviceName ?? morphDeviceName(),
      );
      if (!mounted) {
        unawaited(widget.api.linkNewDeviceCancel(offer.session));
        return;
      }
      setState(() => _offer = offer);
      _poll = Timer.periodic(widget.pollEvery, (_) => unawaited(_refresh()));
    } catch (e) {
      if (mounted) {
        setState(() => _error = _friendlyLinkError(e));
      }
    }
  }

  Future<void> _refresh() async {
    final offer = _offer;
    if (offer == null || _busy) {
      return;
    }
    try {
      final status = await widget.api.linkNewDeviceStatus(offer.session);
      if (!mounted) {
        return;
      }
      if (status.ended) {
        _poll?.cancel();
      }
      setState(() => _status = status);
    } catch (e) {
      _poll?.cancel();
      if (mounted) {
        setState(() => _error = _friendlyLinkError(e));
      }
    }
  }

  Future<void> _codesMatch() async {
    final offer = _offer;
    if (offer == null) {
      return;
    }
    await widget.api.linkNewDeviceConfirm(offer.session);
    if (mounted) {
      setState(() => _confirmed = true);
    }
  }

  Future<void> _codesDiffer() async {
    final offer = _offer;
    _poll?.cancel();
    if (offer != null) {
      await widget.api.linkNewDeviceCancel(offer.session);
    }
    if (mounted) {
      setState(() => _status = const MorphLinkStatus(MorphLinkState.cancelled));
    }
  }

  Future<void> _finish() async {
    final offer = _offer;
    if (offer == null) {
      return;
    }
    if (_password.text.length < morphMinPasswordLength) {
      setState(
        () => _error = 'Use at least $morphMinPasswordLength characters.',
      );
      return;
    }
    if (_password.text != _confirm.text) {
      setState(() => _error = 'The passwords do not match.');
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final identity = await widget.api.linkNewDeviceFinish(
        widget.profileId,
        offer.session,
        _password.text,
        home: widget.home,
      );
      _finished = true;
      _poll?.cancel();
      _password.clear();
      _confirm.clear();
      widget.onDone(identity);
    } catch (e) {
      if (mounted) {
        setState(() => _error = _friendlyLinkError(e));
      }
    } finally {
      if (mounted) {
        setState(() => _busy = false);
      }
    }
  }

  @override
  void dispose() {
    _poll?.cancel();
    final offer = _offer;
    if (offer != null && !_finished) {
      unawaited(widget.api.linkNewDeviceCancel(offer.session));
    }
    _password.dispose();
    _confirm.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final muted = TextStyle(color: colors.onSurface.withValues(alpha: 0.75));
    final offer = _offer;
    final children = <Widget>[];
    if (offer == null && _error == null) {
      children.add(
        const Padding(
          padding: EdgeInsets.all(32),
          child: Center(child: CircularProgressIndicator()),
        ),
      );
    } else if (_status.ended || (offer == null && _error != null)) {
      children.addAll([
        Text(
          switch (_status.state) {
            MorphLinkState.expired => 'The code expired.',
            MorphLinkState.cancelled => 'Linking was cancelled.',
            MorphLinkState.failed => 'Linking failed: ${_status.reason ?? ''}',
            _ => _error ?? 'Linking stopped.',
          },
          key: const Key('link-ended'),
          textAlign: TextAlign.center,
        ),
        const SizedBox(height: 16),
        FilledButton.icon(
          key: const Key('link-restart'),
          onPressed: _start,
          icon: const Icon(Icons.refresh),
          label: const Text('Show a new code'),
        ),
      ]);
    } else if (_status.state == MorphLinkState.waiting) {
      children.addAll([
        Text(
          'On the device that already has this identity, open the menu, '
          'then Identity, then Add device, and scan this code. Or paste the '
          'link below there.',
          textAlign: TextAlign.center,
          style: muted,
        ),
        const SizedBox(height: 20),
        Center(
          child: MorphQrCode(key: const Key('link-qr'), data: offer!.link),
        ),
        const SizedBox(height: 16),
        SelectableText(
          offer.link,
          key: const Key('link-text'),
          style: MorphTheme.monoStyle(color: colors.onSurface, size: 11),
          maxLines: 4,
        ),
        const SizedBox(height: 8),
        Row(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            TextButton.icon(
              key: const Key('link-copy'),
              onPressed: () =>
                  Clipboard.setData(ClipboardData(text: offer.link)),
              icon: const Icon(Icons.copy, size: 18),
              label: const Text('Copy link'),
            ),
          ],
        ),
        Text(
          'The code works once and for two minutes. It holds no secret.',
          textAlign: TextAlign.center,
          style: muted,
        ),
      ]);
    } else if (!_confirmed) {
      children.addAll([
        Text(
          'Check that the other device shows the same code.',
          textAlign: TextAlign.center,
          style: muted,
        ),
        const SizedBox(height: 16),
        _Sas(_status.sas ?? ''),
        const SizedBox(height: 24),
        FilledButton(
          key: const Key('link-match'),
          onPressed: _codesMatch,
          child: const Text('Codes match'),
        ),
        const SizedBox(height: 8),
        TextButton(
          key: const Key('link-differ'),
          onPressed: _codesDiffer,
          child: const Text('They are different'),
        ),
      ]);
    } else if (_status.state != MorphLinkState.received) {
      children.addAll([
        _Sas(_status.sas ?? ''),
        const SizedBox(height: 16),
        Text(
          'Now confirm the code on the other device.',
          key: const Key('link-wait-sender'),
          textAlign: TextAlign.center,
          style: muted,
        ),
        const SizedBox(height: 16),
        const Center(child: CircularProgressIndicator()),
      ]);
    } else {
      children.addAll([
        Text(
          'The identity arrived. Choose a password for it on this device; '
          'it can differ from the other device\'s.',
          textAlign: TextAlign.center,
          style: muted,
        ),
        const SizedBox(height: 16),
        MorphPasswordField(
          key: const Key('link-password'),
          controller: _password,
          label: 'New password',
          autofillHints: const [AutofillHints.newPassword],
        ),
        const SizedBox(height: 12),
        MorphPasswordField(
          key: const Key('link-password-confirm'),
          controller: _confirm,
          label: 'Repeat password',
          autofillHints: const [AutofillHints.newPassword],
        ),
        const SizedBox(height: 16),
        FilledButton(
          key: const Key('link-finish'),
          onPressed: _busy ? null : _finish,
          child: const Text('Finish'),
        ),
      ]);
    }
    if (_error != null && offer != null) {
      children.addAll([
        const SizedBox(height: 12),
        Text(
          _error!,
          key: const Key('link-error'),
          textAlign: TextAlign.center,
          style: TextStyle(color: colors.error),
        ),
      ]);
    }
    return Column(
      key: const Key('link-from-device'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: children,
    );
  }
}

/// The existing device's side: unlock with the profile password, scan or
/// paste the new device's code, compare the six digits, send.
class MorphAddDevice extends StatefulWidget {
  const MorphAddDevice({
    super.key,
    required this.api,
    required this.profileId,
    required this.profileName,
    this.home,
    this.scan,
    this.onClose,
  });

  final MorphApi api;
  final String profileId;
  final String profileName;
  final String? home;

  /// Opens the camera and returns the scanned text; null hides the button.
  final Future<String?> Function()? scan;
  final VoidCallback? onClose;

  @override
  State<MorphAddDevice> createState() => _MorphAddDeviceState();
}

class _MorphAddDeviceState extends State<MorphAddDevice> {
  final _password = TextEditingController();
  final _link = TextEditingController();
  MorphLinkPairing? _pairing;
  bool _busy = false;
  bool _sent = false;
  String? _error;

  Future<void> _scan() async {
    final text = await widget.scan?.call();
    if (text != null && mounted) {
      setState(() => _link.text = text.trim());
      if (_password.text.isNotEmpty) {
        await _pair();
      }
    }
  }

  Future<void> _pair() async {
    final link = _link.text.trim();
    if (_password.text.isEmpty) {
      setState(() => _error = 'Enter this profile\'s password.');
      return;
    }
    if (!link.startsWith('weirdnet-morph://link/')) {
      setState(
        () => _error = 'Scan the code on the new device, or paste its link.',
      );
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final pairing = await widget.api.linkAddDeviceStart(
        widget.profileId,
        widget.profileName,
        _password.text,
        link,
        home: widget.home,
      );
      _password.clear();
      if (!mounted) {
        unawaited(widget.api.linkAddDeviceCancel(pairing.session));
        return;
      }
      setState(() => _pairing = pairing);
    } catch (e) {
      if (mounted) {
        setState(() => _error = _friendlyLinkError(e));
      }
    } finally {
      if (mounted) {
        setState(() => _busy = false);
      }
    }
  }

  Future<void> _send() async {
    final pairing = _pairing;
    if (pairing == null) {
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await widget.api.linkAddDeviceSend(pairing.session);
      if (mounted) {
        setState(() => _sent = true);
      }
    } catch (e) {
      if (mounted) {
        setState(() => _error = _friendlyLinkError(e));
      }
    } finally {
      if (mounted) {
        setState(() => _busy = false);
      }
    }
  }

  Future<void> _cancel() async {
    final pairing = _pairing;
    if (pairing != null && !_sent) {
      await widget.api.linkAddDeviceCancel(pairing.session);
    }
    if (mounted) {
      setState(() => _pairing = null);
    }
    widget.onClose?.call();
  }

  @override
  void dispose() {
    final pairing = _pairing;
    if (pairing != null && !_sent) {
      unawaited(widget.api.linkAddDeviceCancel(pairing.session));
    }
    _password.dispose();
    _link.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final muted = TextStyle(color: colors.onSurface.withValues(alpha: 0.75));
    final pairing = _pairing;
    final children = <Widget>[];
    if (_sent) {
      children.addAll([
        const Icon(Icons.check_circle, size: 48, color: MorphColors.cyan),
        const SizedBox(height: 12),
        Text(
          'Sent to ${pairing?.device ?? 'the new device'}. Finish there by '
          'choosing its password.',
          key: const Key('add-device-sent'),
          textAlign: TextAlign.center,
        ),
        const SizedBox(height: 16),
        FilledButton(
          key: const Key('add-device-close'),
          onPressed: widget.onClose,
          child: const Text('Done'),
        ),
      ]);
    } else if (pairing != null) {
      children.addAll([
        Text(
          'Link ${pairing.device}? Check that it shows the same code.',
          key: const Key('add-device-ask'),
          textAlign: TextAlign.center,
          style: muted,
        ),
        const SizedBox(height: 16),
        _Sas(pairing.sas),
        const SizedBox(height: 24),
        FilledButton(
          key: const Key('add-device-send'),
          onPressed: _busy ? null : _send,
          child: const Text('Codes match, send identity'),
        ),
        const SizedBox(height: 8),
        TextButton(
          key: const Key('add-device-cancel'),
          onPressed: _busy ? null : _cancel,
          child: const Text('They are different'),
        ),
      ]);
    } else {
      children.addAll([
        Text(
          'On the new device, pick "Link from another device" in the '
          'identity setup of a profile. Then unlock ${widget.profileName} '
          'here and scan the code it shows${widget.scan == null ? ', or paste its link' : ''}.',
          textAlign: TextAlign.center,
          style: muted,
        ),
        const SizedBox(height: 16),
        MorphPasswordField(
          key: const Key('add-device-password'),
          controller: _password,
          label: '${widget.profileName} password',
        ),
        const SizedBox(height: 12),
        if (widget.scan != null) ...[
          FilledButton.icon(
            key: const Key('add-device-scan'),
            onPressed: _busy ? null : _scan,
            icon: const Icon(Icons.qr_code_scanner),
            label: const Text('Scan QR code'),
          ),
          const SizedBox(height: 12),
        ],
        TextField(
          key: const Key('add-device-link'),
          controller: _link,
          minLines: 1,
          maxLines: 3,
          autocorrect: false,
          enableSuggestions: false,
          decoration: const InputDecoration(
            labelText: 'Or paste the link',
            hintText: 'weirdnet-morph://link/…',
          ),
        ),
        const SizedBox(height: 16),
        OutlinedButton(
          key: const Key('add-device-pair'),
          onPressed: _busy ? null : _pair,
          child: _busy
              ? const SizedBox.square(
                  dimension: 18,
                  child: CircularProgressIndicator(strokeWidth: 2),
                )
              : const Text('Connect'),
        ),
      ]);
    }
    if (_error != null) {
      children.addAll([
        const SizedBox(height: 12),
        Text(
          _error!,
          key: const Key('add-device-error'),
          textAlign: TextAlign.center,
          style: TextStyle(color: colors.error),
        ),
      ]);
    }
    return Column(
      key: const Key('add-device'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: children,
    );
  }
}
