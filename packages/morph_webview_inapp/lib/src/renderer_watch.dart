import 'dart:async';

import 'package:morph_webview/morph_webview.dart';

/// Tracks one view's renderer for the system web view callbacks and turns
/// them into [MorphRendererEvent]s.
///
/// Android: every WebView in an app shares one renderer process, so a crash
/// or a terminate reaches every tab at once (the Flutter shell survives).
/// macOS/iOS: one web content process per view, so only this tab.
class InAppRendererWatch {
  InAppRendererWatch({this.terminateWait = const Duration(seconds: 7)});

  /// How long [terminate] waits for a hung renderer to be ended. Android asks
  /// about a hung renderer again at most every five seconds.
  final Duration terminateWait;

  final _events = StreamController<MorphRendererEvent>.broadcast();
  bool _gone = false;
  bool _terminateRequested = false;
  Completer<void>? _ended;

  Stream<MorphRendererEvent> get events => _events.stream;

  /// True once the renderer behind this view is gone; the native view is
  /// then unusable until it is rebuilt.
  bool get gone => _gone;

  void _emit(MorphRendererFault? fault) {
    if (!_events.isClosed) {
      _events.add(MorphRendererEvent(fault));
    }
  }

  /// Android onRenderProcessGone.
  void renderProcessGone({required bool didCrash}) {
    _end(didCrash ? MorphRendererFault.crashed : MorphRendererFault.killed);
  }

  /// macOS/iOS webContentProcessDidTerminate.
  void webContentProcessTerminated() => _end(MorphRendererFault.crashed);

  void _end(MorphRendererFault fault) {
    _gone = true;
    _terminateRequested = false;
    final ended = _ended;
    if (ended != null && !ended.isCompleted) {
      // The renderer ended because the user chose to reload; the tab is
      // already recovering, so this is not a new fault to show.
      ended.complete();
      return;
    }
    _emit(fault);
  }

  /// Android onRenderProcessUnresponsive. Returns true when the renderer
  /// should be terminated now (the user chose to reload).
  bool renderProcessUnresponsive() {
    _emit(MorphRendererFault.unresponsive);
    return _terminateRequested;
  }

  /// Android onRenderProcessResponsive.
  void renderProcessResponsive() => _emit(null);

  /// Asks for a hung renderer to be ended at its next unresponsive report and
  /// waits for it to go (or [terminateWait]).
  Future<void> terminate() async {
    if (_gone) {
      return;
    }
    _terminateRequested = true;
    final ended = _ended ??= Completer<void>();
    try {
      await ended.future.timeout(terminateWait);
    } on TimeoutException {
      // It answered again, or was never reported; rebuild anyway.
    } finally {
      _terminateRequested = false;
      _ended = null;
    }
  }

  /// A fresh native view (and renderer) replaced the dead one.
  void rebuilt() {
    _gone = false;
    _terminateRequested = false;
  }

  Future<void> close() => _events.close();
}
