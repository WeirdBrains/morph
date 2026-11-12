import 'package:flutter/widgets.dart';

/// Tells the shell when a route above the browser (address sheet, menu,
/// dialog, profile sheet) goes away, so it can make sure the page surface
/// that was covered is painting again.
class MorphSurfaceWatch extends NavigatorObserver {
  final _listeners = <VoidCallback>{};

  void addListener(VoidCallback listener) => _listeners.add(listener);

  void removeListener(VoidCallback listener) => _listeners.remove(listener);

  void _notify() {
    for (final listener in List<VoidCallback>.of(_listeners)) {
      listener();
    }
  }

  @override
  void didPop(Route<dynamic> route, Route<dynamic>? previousRoute) => _notify();

  @override
  void didRemove(Route<dynamic> route, Route<dynamic>? previousRoute) =>
      _notify();
}

/// The app-wide watch, registered on [MaterialApp.navigatorObservers].
final morphSurfaceWatch = MorphSurfaceWatch();
