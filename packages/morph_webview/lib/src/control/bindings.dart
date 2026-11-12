import 'dart:async';

/// Host bindings that start the in-process control server and emit view events.
///
/// The Flutter app registers [MorphWebViewControl.start] (typically wrapping
/// morph-bridge FRB APIs) so platform packages do not depend on the app crate.
class MorphControlBindings {
  MorphControlBindings({
    required this.endpoint,
    required this.backendEvents,
    required this.emitNavigation,
    required this.emitPageMessage,
    required this.completeEvaluateJs,
  });

  final String endpoint;
  final Stream<MorphControlCommand> backendEvents;
  final Future<void> Function({
    required int viewId,
    required String url,
    required String title,
    required bool canGoBack,
    required bool canGoForward,
    required bool isLoading,
    required double progress,
  })
  emitNavigation;
  final Future<void> Function({required int viewId, required String json})
  emitPageMessage;
  final Future<void> Function({
    required int viewId,
    required BigInt requestId,
    required String result,
    required String error,
  })
  completeEvaluateJs;
}

/// A command the platform web view should apply.
class MorphControlCommand {
  const MorphControlCommand({
    required this.kind,
    required this.viewId,
    this.url = '',
    this.requestId = 0,
    this.script = '',
  });

  final String kind;
  final int viewId;
  final String url;
  final int requestId;
  final String script;
}

/// Process-wide registration point for the control server starter.
class MorphWebViewControl {
  static Future<MorphControlBindings> Function()? start;
  static MorphControlBindings? instance;

  static Future<MorphControlBindings> ensureStarted() async {
    final existing = instance;
    if (existing != null) {
      return existing;
    }
    final startFn = start;
    if (startFn == null) {
      throw StateError(
        'MorphWebViewControl.start was not registered. '
        'The app should set it during startup on macOS and Windows.',
      );
    }
    return instance = await startFn();
  }
}
