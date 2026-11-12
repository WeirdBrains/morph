import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'controller.dart';
import 'events.dart';

/// Renders a platform web-view texture and routes Wayland-first input to the backend.
class MorphWebView extends StatefulWidget {
  const MorphWebView({
    super.key,
    required this.controller,
    this.userScript = '',
    this.placeholder,
  });

  final MorphWebViewController controller;
  final String userScript;

  /// Shown in widget tests or before the texture id is available.
  final Widget? placeholder;

  @override
  State<MorphWebView> createState() => _MorphWebViewState();
}

class _MorphWebViewState extends State<MorphWebView> {
  Size? _lastSize;
  double? _lastDpr;
  Object? _error;

  @override
  void dispose() {
    // The controller is owned by the parent (the shell).
    super.dispose();
  }

  Future<void> _ensureAttached(BoxConstraints constraints, double dpr) async {
    final width = constraints.maxWidth.isFinite
        ? constraints.maxWidth.floor().clamp(1, 8192)
        : 800;
    final height = constraints.maxHeight.isFinite
        ? constraints.maxHeight.floor().clamp(1, 8192)
        : 600;
    final size = Size(width.toDouble(), height.toDouble());
    if (_lastSize == size && _lastDpr == dpr && widget.controller.isAttached) {
      return;
    }
    _lastSize = size;
    _lastDpr = dpr;
    try {
      await widget.controller.attach(
        width: width,
        height: height,
        devicePixelRatio: dpr,
        userScript: widget.userScript,
      );
      if (mounted) {
        setState(() => _error = null);
      }
    } catch (e) {
      if (mounted) {
        setState(() => _error = e);
      }
    }
  }

  void _pointer(PointerEvent event, String phase) {
    widget.controller.sendPointer(
      MorphPointerEvent(
        phase: phase,
        x: event.localPosition.dx,
        y: event.localPosition.dy,
        button: event.buttons == kSecondaryMouseButton
            ? 3
            : event.buttons == kMiddleMouseButton
            ? 2
            : phase == 'scroll'
            ? 0
            : 1,
        scrollDeltaX: event is PointerScrollEvent ? event.scrollDelta.dx : 0,
        scrollDeltaY: event is PointerScrollEvent ? event.scrollDelta.dy : 0,
      ),
    );
  }

  KeyEventResult _onKey(FocusNode node, KeyEvent event) {
    final phase = event is KeyDownEvent || event is KeyRepeatEvent
        ? 'down'
        : 'up';
    widget.controller.sendKey(
      MorphKeyEvent(
        phase: phase,
        key: event.logicalKey.keyLabel.isNotEmpty
            ? event.logicalKey.keyLabel
            : event.logicalKey.debugName ?? event.logicalKey.keyId.toString(),
        hardwareKeycode: event.physicalKey.usbHidUsage,
        ctrl:
            HardwareKeyboard.instance.logicalKeysPressed.contains(
              LogicalKeyboardKey.controlLeft,
            ) ||
            HardwareKeyboard.instance.logicalKeysPressed.contains(
              LogicalKeyboardKey.controlRight,
            ),
        alt:
            HardwareKeyboard.instance.logicalKeysPressed.contains(
              LogicalKeyboardKey.altLeft,
            ) ||
            HardwareKeyboard.instance.logicalKeysPressed.contains(
              LogicalKeyboardKey.altRight,
            ),
        shift:
            HardwareKeyboard.instance.logicalKeysPressed.contains(
              LogicalKeyboardKey.shiftLeft,
            ) ||
            HardwareKeyboard.instance.logicalKeysPressed.contains(
              LogicalKeyboardKey.shiftRight,
            ),
        meta:
            HardwareKeyboard.instance.logicalKeysPressed.contains(
              LogicalKeyboardKey.metaLeft,
            ) ||
            HardwareKeyboard.instance.logicalKeysPressed.contains(
              LogicalKeyboardKey.metaRight,
            ),
      ),
    );
    return KeyEventResult.handled;
  }

  @override
  Widget build(BuildContext context) {
    final dpr = MediaQuery.devicePixelRatioOf(context);
    return LayoutBuilder(
      builder: (context, constraints) {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) {
            unawaitedAttach(constraints, dpr);
          }
        });
        final textureId = widget.controller.textureId;
        Widget child;
        if (_error != null) {
          child = Center(child: Text('Web view error: $_error'));
        } else if (textureId != null) {
          child = Focus(
            onKeyEvent: _onKey,
            child: Listener(
              onPointerDown: (e) => _pointer(e, 'down'),
              onPointerMove: (e) => _pointer(e, 'move'),
              onPointerUp: (e) => _pointer(e, 'up'),
              onPointerCancel: (e) => _pointer(e, 'cancel'),
              onPointerSignal: (e) {
                if (e is PointerScrollEvent) {
                  _pointer(e, 'scroll');
                }
              },
              child: Texture(textureId: textureId),
            ),
          );
        } else {
          child =
              widget.placeholder ??
              const Center(child: CircularProgressIndicator());
        }
        return child;
      },
    );
  }

  void unawaitedAttach(BoxConstraints constraints, double dpr) {
    // ignore: discarded_futures
    _ensureAttached(constraints, dpr);
  }
}
