import '../api.dart';
import '../host_friendly_url.dart';

/// Identifier and presentation state for one shell tab.
///
/// Navigation history lives in the tab's [MorphWebViewController]. This
/// Dart type mirrors `morph_core::tabs` so the Flutter shell and widget
/// tests can run without loading FFI.
class MorphTab {
  MorphTab({required this.id, this.url = '', this.title = ''});

  final int id;
  String url;
  String title;

  /// Title if set, otherwise the host, otherwise 'New tab'.
  String get label {
    if (title.isNotEmpty) {
      return title;
    }
    if (url.isNotEmpty && url != 'about:blank') {
      final host = hostFriendlyUrl(url);
      if (host.isNotEmpty) {
        return host;
      }
    }
    return 'New tab';
  }
}

/// Ordered list of tabs with a single active tab.
///
/// The set always contains at least one tab. Closing the last tab replaces
/// it with a fresh empty tab. Behaviour matches `morph_core::tabs::TabSet`.
class MorphTabSet {
  MorphTabSet() : _tabs = [MorphTab(id: 1)], _active = 1, _nextId = 2;

  final List<MorphTab> _tabs;
  int _active;
  int _nextId;

  int get length => _tabs.length;
  int get activeId => _active;
  MorphTab get active => tab(_active)!;
  List<MorphTab> get tabs => List.unmodifiable(_tabs);

  MorphTab? tab(int id) {
    for (final tab in _tabs) {
      if (tab.id == id) {
        return tab;
      }
    }
    return null;
  }

  int? indexOf(int id) {
    for (var i = 0; i < _tabs.length; i++) {
      if (_tabs[i].id == id) {
        return i;
      }
    }
    return null;
  }

  /// Open a new tab after the active tab and select it.
  int open([String url = '']) {
    final id = _nextId++;
    final tab = MorphTab(id: id, url: url);
    final insertAt = (indexOf(_active) ?? (_tabs.length - 1)) + 1;
    _tabs.insert(insertAt, tab);
    _active = id;
    return id;
  }

  /// Select [id]. Throws if the tab is not in the set.
  void switchTo(int id) {
    if (tab(id) == null) {
      throw ArgumentError.value(id, 'id', 'no tab');
    }
    _active = id;
  }

  /// Close [id]. Returns the active tab id afterwards.
  int close(int id) {
    final index = indexOf(id);
    if (index == null) {
      throw ArgumentError.value(id, 'id', 'no tab');
    }
    final wasActive = _active == id;
    _tabs.removeAt(index);
    if (_tabs.isEmpty) {
      final fresh = _nextId++;
      _tabs.add(MorphTab(id: fresh));
      _active = fresh;
      return _active;
    }
    if (wasActive) {
      final next = index < _tabs.length ? index : _tabs.length - 1;
      _active = _tabs[next].id;
    }
    return _active;
  }

  /// Close every tab and leave a single fresh empty tab selected.
  int closeAll() {
    final fresh = _nextId++;
    _tabs
      ..clear()
      ..add(MorphTab(id: fresh));
    _active = fresh;
    return _active;
  }

  int cycleNext() {
    final index = indexOf(_active) ?? 0;
    _active = _tabs[(index + 1) % _tabs.length].id;
    return _active;
  }

  int cyclePrev() {
    final index = indexOf(_active) ?? 0;
    final prev = index == 0 ? _tabs.length - 1 : index - 1;
    _active = _tabs[prev].id;
    return _active;
  }

  void update(int id, {String? url, String? title}) {
    final target = tab(id);
    if (target == null) {
      throw ArgumentError.value(id, 'id', 'no tab');
    }
    if (url != null) {
      target.url = url;
    }
    if (title != null) {
      target.title = title;
    }
  }

  /// Persistable snapshot of ids, URLs, and titles (not web-view history).
  ProfileTabSnapshot snapshot() {
    return ProfileTabSnapshot(
      activeId: _active,
      nextId: _nextId,
      tabs: [
        for (final tab in _tabs)
          ProfileTab(id: tab.id, url: tab.url, title: tab.title),
      ],
    );
  }

  /// Replace this set with a saved snapshot. An empty snapshot becomes one
  /// fresh start-page tab.
  void applySnapshot(ProfileTabSnapshot snapshot) {
    if (snapshot.tabs.isEmpty) {
      _tabs
        ..clear()
        ..add(MorphTab(id: 1));
      _active = 1;
      _nextId = 2;
      return;
    }
    _tabs
      ..clear()
      ..addAll([
        for (final tab in snapshot.tabs)
          MorphTab(id: tab.id, url: tab.url, title: tab.title),
      ]);
    _active = tab(snapshot.activeId) != null
        ? snapshot.activeId
        : _tabs.first.id;
    var next = snapshot.nextId;
    for (final tab in _tabs) {
      if (tab.id >= next) {
        next = tab.id + 1;
      }
    }
    _nextId = next;
  }
}
