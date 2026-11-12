import 'dart:async';

import 'package:flutter/material.dart';
import 'package:morph_webview/morph_webview.dart';

import '../theme/morph_theme.dart';
import 'brand.dart';

/// Title and explanation shown for a tab whose renderer [fault]ed.
(String, String) morphPageFaultText(MorphRendererFault fault) {
  return switch (fault) {
    MorphRendererFault.crashed => (
      'This page crashed',
      'Only this tab was affected. Reload to try again.',
    ),
    MorphRendererFault.killed => (
      'This page was stopped',
      'The system ended this page, usually to free memory. Reload to bring it back.',
    ),
    MorphRendererFault.outOfMemory => (
      'This page ran out of memory',
      'It used more memory than a tab may. Reload to try again.',
    ),
    MorphRendererFault.unresponsive => (
      "This page isn't responding",
      'You can wait for it, or reload it.',
    ),
  };
}

/// Covers one tab with a "page crashed, reload" state while its renderer is
/// crashed, killed or hung. Other tabs and the shell keep running.
///
/// A tab whose renderer died while it was in the background (on Android every
/// tab shares one renderer) reloads by itself when it becomes [active].
class MorphRendererFaultGate extends StatefulWidget {
  const MorphRendererFaultGate({
    super.key,
    required this.controller,
    required this.active,
    required this.child,
  });

  final MorphWebViewController controller;
  final bool active;
  final Widget child;

  @override
  State<MorphRendererFaultGate> createState() => _MorphRendererFaultGateState();
}

class _MorphRendererFaultGateState extends State<MorphRendererFaultGate> {
  MorphRendererFault? _fault;
  bool _waiting = false;
  bool _faultedWhileHidden = false;

  @override
  void initState() {
    super.initState();
    widget.controller.rendererFault.addListener(_onFault);
    _fault = widget.controller.rendererFault.value;
  }

  @override
  void didUpdateWidget(MorphRendererFaultGate oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(oldWidget.controller, widget.controller)) {
      oldWidget.controller.rendererFault.removeListener(_onFault);
      widget.controller.rendererFault.addListener(_onFault);
      _fault = widget.controller.rendererFault.value;
    }
    if (widget.active && !oldWidget.active) {
      _maybeRecoverOnShow();
    }
  }

  @override
  void dispose() {
    widget.controller.rendererFault.removeListener(_onFault);
    super.dispose();
  }

  void _onFault() {
    final fault = widget.controller.rendererFault.value;
    setState(() {
      if (fault != _fault) {
        _waiting = false;
      }
      _fault = fault;
      _faultedWhileHidden =
          fault != null &&
          fault != MorphRendererFault.unresponsive &&
          !widget.active;
    });
  }

  void _maybeRecoverOnShow() {
    if (_faultedWhileHidden && _fault != null) {
      _faultedWhileHidden = false;
      unawaited(widget.controller.recoverFromFault());
    }
  }

  void _reload() {
    _faultedWhileHidden = false;
    unawaited(widget.controller.recoverFromFault());
  }

  @override
  Widget build(BuildContext context) {
    final fault = _fault;
    final show = fault != null && !_waiting && !_faultedWhileHidden;
    return Stack(
      fit: StackFit.expand,
      children: [
        widget.child,
        if (show)
          // Opaque so clicks and keys never reach the dead page beneath.
          Listener(
            behavior: HitTestBehavior.opaque,
            child: MorphPageCrashed(
              fault: fault,
              onReload: _reload,
              onWait: fault == MorphRendererFault.unresponsive
                  ? () => setState(() => _waiting = true)
                  : null,
            ),
          ),
      ],
    );
  }
}

/// The per-tab crash panel.
class MorphPageCrashed extends StatelessWidget {
  const MorphPageCrashed({
    super.key,
    required this.fault,
    required this.onReload,
    this.onWait,
  });

  final MorphRendererFault fault;
  final VoidCallback onReload;
  final VoidCallback? onWait;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final colors = theme.colorScheme;
    final (title, body) = morphPageFaultText(fault);
    return Material(
      key: const Key('page-crashed'),
      color: theme.scaffoldBackgroundColor,
      child: SafeArea(
        child: Center(
          child: SingleChildScrollView(
            padding: const EdgeInsets.symmetric(horizontal: 32, vertical: 24),
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 420),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  const MorphMark(size: 56),
                  const SizedBox(height: 20),
                  const MorphGlitchRule(),
                  const SizedBox(height: 20),
                  Text(
                    title,
                    textAlign: TextAlign.center,
                    style: theme.textTheme.titleLarge,
                  ),
                  const SizedBox(height: 8),
                  Text(
                    body,
                    textAlign: TextAlign.center,
                    style: MorphTheme.monoStyle(
                      color: colors.onSurfaceVariant,
                      size: 13,
                    ),
                  ),
                  const SizedBox(height: 24),
                  Wrap(
                    spacing: 12,
                    runSpacing: 8,
                    alignment: WrapAlignment.center,
                    children: [
                      if (onWait != null)
                        OutlinedButton(
                          key: const Key('page-crashed-wait'),
                          onPressed: onWait,
                          child: const Text('Wait'),
                        ),
                      FilledButton.icon(
                        key: const Key('page-crashed-reload'),
                        onPressed: onReload,
                        icon: const Icon(Icons.refresh),
                        label: const Text('Reload'),
                      ),
                    ],
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}
