import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:morph_webview/morph_webview.dart';

import 'api.dart';
import 'shell_shortcuts.dart';
import 'host_friendly_url.dart';
export 'host_friendly_url.dart';
import 'theme/morph_theme.dart';
import 'tabs/tab_overflow.dart';
import 'tabs/tab_position.dart';
import 'tabs/tab_set.dart';
import 'widgets/address_sheet.dart';
import 'widgets/brand.dart';
import 'widgets/desktop_chrome.dart';
import 'widgets/find_bar.dart';
import 'widgets/library_page.dart';
import 'widgets/mobile_gestures.dart';
import 'widgets/profile_menu.dart';
import 'widgets/settings_page.dart';
import 'widgets/start_page.dart';
import 'widgets/tab_switcher.dart';
import 'widgets/window_chrome.dart';
import 'window/morph_desktop_window.dart';
import 'window/window_buttons.dart';
import 'surface_watch.dart';

/// Morph shell: address-bar chrome, Linux web-view texture, and Rust wiring.
class MorphShell extends StatefulWidget {
  const MorphShell({
    super.key,
    required this.api,
    this.home,
    this.subscribeToTicks = true,
    this.loadPageBridgeAsset = true,
    this.showDebugPanel = false,
  });

  final MorphApi api;

  /// Widget tests turn this off so a live tick stream cannot stall the tester.
  final bool subscribeToTicks;

  /// Widget tests turn this off: loading `assets/morph-bridge.js` can stall
  /// inside the fake async zone, and the in-memory fake web view does not
  /// need the page-bridge script to accept a typed URL.
  final bool loadPageBridgeAsset;

  /// When true, the bottom identity/debug panel starts expanded.
  /// Production leaves this off; long-press the mark to toggle it.
  final bool showDebugPanel;

  /// Override for tests and `MORPH_HOME`. Null falls back to the environment,
  /// then to Rust's default (`~/.morph`) without creating it.
  final String? home;

  @override
  State<MorphShell> createState() => MorphShellState();
}

class MorphShellState extends State<MorphShell> with WidgetsBindingObserver {
  final TextEditingController address = TextEditingController();
  final addressFocus = FocusNode();
  final MorphTabSet tabs = MorphTabSet();
  final _controllers = <int, MorphWebViewController>{};
  final _navSubs = <int, StreamSubscription<MorphNavigationEvent>>{};
  final _pageSubs = <int, StreamSubscription<String>>{};
  final _navEvents = <int, MorphNavigationEvent>{};
  final _visited = <int, bool>{};
  MorphTabPosition _tabPosition = MorphTabPosition.top;
  bool _sidebarCollapsed = false;
  double _sidebarWidth = MorphTabOverflow.defaultSidebarWidth;
  ProfileCatalog _catalog = const ProfileCatalog(
    current: 'default',
    profiles: [ProfileInfo(id: 'default', name: 'Default')],
  );
  String _profileId = 'default';
  final _recorded = <int, String>{};
  bool _findOpen = false;
  Set<String> _bookmarkUrls = const {};
  Future<void> _libraryWrites = Future.value();
  bool _showSwitcher = false;
  final _previews = <int, Uint8List?>{};
  late bool _debugOpen;
  String _canonicalAddress = '';
  String? coreVersionLabel;
  String? identityHome;
  bool? homeExists;
  String? userId;
  String? fingerprint;
  String? pageReply;
  int? tick;
  StreamSubscription<int>? _ticks;
  MorphNavigationEvent navigation = const MorphNavigationEvent(url: '');
  String userScript = '';
  bool ready = false;
  Object? error;

  MorphWebViewController get webView => _controllerFor(tabs.activeId);

  MorphWebViewController _controllerFor(int id) {
    return _controllers.putIfAbsent(id, () {
      final tab = tabs.tab(id);
      final initial = (tab != null && tab.url.isNotEmpty)
          ? tab.url
          : 'about:blank';
      final controller = MorphWebViewController(initialUrl: initial);
      _navSubs[id] = controller.navigation.listen(
        (event) => _onTabNav(id, event),
      );
      _pageSubs[id] = controller.pageMessages.listen(_onPageMessage);
      return controller;
    });
  }

  /// Runs per-profile library writes one at a time so read-modify-write
  /// saves in the core never race each other.
  Future<T> _libraryWrite<T>(Future<T> Function() write) {
    final next = _libraryWrites.then((_) => write());
    _libraryWrites = next.then((_) {}, onError: (_) {});
    return next;
  }

  void _recordVisit(int id, MorphNavigationEvent event) {
    final key = '${event.url}\n${event.title}';
    if (_recorded[id] == key) {
      return;
    }
    _recorded[id] = key;
    final profileId = _profileId;
    final home = _home;
    unawaited(
      _libraryWrite(
        () => widget.api.recordVisit(
          profileId,
          event.url,
          event.title,
          home: home,
        ),
      ).catchError((Object _) {}),
    );
  }

  void _onTabNav(int id, MorphNavigationEvent event) {
    if (!mounted) {
      return;
    }
    final hasUrl = event.url.isNotEmpty && event.url != 'about:blank';
    if (hasUrl) {
      _navEvents[id] = event;
      tabs.update(id, url: event.url, title: event.title);
      _visited[id] = true;
      if (!event.isLoading) {
        _recordVisit(id, event);
      }
    } else if (event.title.isNotEmpty) {
      tabs.update(id, title: event.title);
    }
    if (id != tabs.activeId) {
      setState(() {});
      return;
    }
    if (hasUrl && !event.isLoading && event.progress >= 1) {
      // A finished load (including back, forward and reload) is the moment a
      // stale surface shows up as a white page.
      _scheduleSurfaceCheck(const Duration(milliseconds: 800));
    }
    setState(() {
      if (hasUrl) {
        navigation = event;
        _canonicalAddress = event.url;
        _applyAddressDisplay();
      } else if (!_tabHasPage(id)) {
        navigation = event;
      }
      // Ignore a blank or about:blank event on a tab that already has a
      // page so a resume snapshot cannot flash the start page.
    });
  }

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    morphSurfaceWatch.addListener(_onOverlayClosed);
    _debugOpen = widget.showDebugPanel;
    _controllerFor(tabs.activeId);
    addressFocus.addListener(() {
      if (!mounted) {
        return;
      }
      if (addressFocus.hasFocus) {
        final full = _canonicalAddress == 'about:blank'
            ? ''
            : _canonicalAddress;
        final text = full.isEmpty ? address.text : full;
        address.value = TextEditingValue(
          text: text,
          selection: TextSelection(baseOffset: 0, extentOffset: text.length),
        );
      } else {
        _applyAddressDisplay();
      }
    });
    FocusManager.instance.addEarlyKeyEventHandler(_onShellKey);
    unawaited(_load());
  }

  String? get _home {
    final explicit = widget.home;
    if (explicit != null && explicit.isNotEmpty) {
      return explicit;
    }
    final env = Platform.environment['MORPH_HOME'];
    if (env != null && env.isNotEmpty) {
      return env;
    }
    return null;
  }

  Future<void> _load() async {
    try {
      final version = widget.api.coreVersion();
      final info = await widget.api.identityDirInfo(home: _home);
      if (!mounted) {
        return;
      }
      // Identity status should not wait on the page-bridge asset; that load
      // can stall inside widget tests (asset I/O is outside the fake async zone).
      setState(() {
        coreVersionLabel = version;
        identityHome = info.home;
        homeExists = info.homeExists;
        error = null;
      });
      try {
        final position = MorphTabPosition.fromKey(
          await widget.api.loadTabPosition(home: _home),
        );
        final collapsed = await widget.api.loadSidebarCollapsed(home: _home);
        final width = await widget.api.loadSidebarWidth(home: _home);
        if (mounted) {
          setState(() {
            _tabPosition = position;
            _sidebarCollapsed = collapsed;
            _sidebarWidth = width.toDouble();
          });
        }
      } catch (_) {
        // A missing or unreadable preference stays at the defaults.
      }
      try {
        final catalog = await widget.api.listProfiles(home: _home);
        final snapshot = await widget.api.loadProfileTabs(
          home: _home,
          profileId: catalog.current,
        );
        if (mounted) {
          tabs.applySnapshot(snapshot);
          _catalog = catalog;
          _profileId = catalog.current;
          _dropStaleControllers();
          for (final tab in tabs.tabs) {
            if (tab.url.isNotEmpty && tab.url != 'about:blank') {
              _visited[tab.id] = true;
            }
          }
          _controllerFor(tabs.activeId);
          _syncAddressFromActive();
          setState(() {});
        }
        unawaited(_pruneProfileData(catalog));
        unawaited(_loadBookmarks());
      } catch (_) {
        // A missing catalog leaves the default profile and a start tab.
      }
      if (!mounted) {
        return;
      }
      var script = '';
      if (widget.loadPageBridgeAsset) {
        try {
          script = await rootBundle.loadString('assets/morph-bridge.js');
        } catch (_) {
          // A missing asset still gets the native morphRust shim.
        }
      }
      if (!mounted) {
        return;
      }
      setState(() {
        userScript = script;
        ready = true;
      });
      await _ticks?.cancel();
      _ticks = null;
      if (widget.subscribeToTicks) {
        _ticks = widget.api.eventTicks().listen((value) {
          if (mounted) {
            setState(() => tick = value);
          }
        });
      }
    } catch (e) {
      if (mounted) {
        setState(() {
          error = e;
          ready = true;
        });
      }
    }
  }

  Future<void> _onPageMessage(String json) async {
    if (webView.handlesPageBridge) {
      if (mounted) {
        setState(() => pageReply = json);
      }
      return;
    }
    try {
      final reply = await widget.api.handlePageMessage(json: json);
      if (!mounted) {
        return;
      }
      setState(() => pageReply = reply);
      await webView.evaluateJavaScript(
        'window.MorphBridge && MorphBridge.deliver($reply);',
      );
    } catch (e) {
      if (mounted) {
        setState(() => error = e);
      }
    }
  }

  Future<void> _newUser() async {
    try {
      final user = await widget.api.newEphemeralUser();
      if (!mounted) {
        return;
      }
      setState(() {
        userId = user.userId;
        fingerprint = user.fingerprint;
      });
    } catch (e) {
      if (mounted) {
        setState(() => error = e);
      }
    }
  }

  Future<void> _pingPage() async {
    if (webView.isAttached) {
      try {
        await webView.evaluateJavaScript(
          'window.MorphBridge && MorphBridge.ping();',
        );
      } catch (e) {
        if (mounted) {
          setState(() => error = e);
        }
      }
      return;
    }
    try {
      final reply = await widget.api.handlePageMessage(
        json: '{"id":"1","type":"ping"}',
      );
      if (!mounted) {
        return;
      }
      setState(() => pageReply = reply);
    } catch (e) {
      if (mounted) {
        setState(() => error = e);
      }
    }
  }

  bool _isRealUrl(String url) => url.isNotEmpty && url != 'about:blank';

  String _urlForTab(MorphTab tab) {
    final fromEvent = _navEvents[tab.id]?.url ?? '';
    if (_isRealUrl(fromEvent)) {
      return fromEvent;
    }
    return tab.url;
  }

  bool _tabHasPage(int id) {
    if (_visited[id] ?? false) {
      return true;
    }
    final tab = tabs.tab(id);
    if (tab == null) {
      return false;
    }
    return _isRealUrl(_urlForTab(tab));
  }

  bool get _onStartPage => !_tabHasPage(tabs.activeId);

  Future<void> _submitUrl(String raw) async {
    var url = raw.trim();
    if (url.isEmpty) {
      return;
    }
    if (!url.contains('://')) {
      url = 'https://$url';
    }
    _canonicalAddress = url;
    _applyAddressDisplay();
    tabs.update(tabs.activeId, url: url);
    setState(() => _visited[tabs.activeId] = true);
    await webView.loadUrl(url);
    unawaited(_saveCurrentTabs());
  }

  // --- Surface self-heal -------------------------------------------------
  //
  // Android web views can stop painting (a white page that ignores swipes)
  // after anything that hides, covers or resizes them. After each of those
  // the shell asks the visible tab's controller to nudge a repaint and, if
  // the page has content but the surface is still flat, rebuild the native
  // view on the same URL.

  Timer? _surfaceTimer;

  /// How many surface checks the shell has requested. Tests read this.
  @visibleForTesting
  int surfaceCheckRequests = 0;

  void _scheduleSurfaceCheck([
    Duration delay = const Duration(milliseconds: 400),
  ]) {
    if (!_isMobile || !mounted) {
      return;
    }
    _surfaceTimer?.cancel();
    _surfaceTimer = Timer(delay, () {
      if (!mounted || _showSwitcher || _onStartPage) {
        return;
      }
      final controller = _controllers[tabs.activeId];
      if (controller == null || !controller.isAttached) {
        return;
      }
      surfaceCheckRequests++;
      unawaited(controller.ensureSurfaceHealthy());
    });
  }

  void _onOverlayClosed() => _scheduleSurfaceCheck();

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) {
      _scheduleSurfaceCheck(const Duration(milliseconds: 600));
    }
  }

  @override
  void didChangeMetrics() {
    // Keyboard shown or hidden, rotation, split-screen resize.
    _scheduleSurfaceCheck(const Duration(milliseconds: 500));
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    morphSurfaceWatch.removeListener(_onOverlayClosed);
    _surfaceTimer?.cancel();
    FocusManager.instance.removeEarlyKeyEventHandler(_onShellKey);
    unawaited(_saveCurrentTabs());
    unawaited(_ticks?.cancel());
    for (final sub in _navSubs.values) {
      unawaited(sub.cancel());
    }
    for (final sub in _pageSubs.values) {
      unawaited(sub.cancel());
    }
    for (final controller in _controllers.values) {
      unawaited(controller.dispose());
    }
    addressFocus.dispose();
    address.dispose();
    super.dispose();
  }

  bool get _isMobile {
    switch (defaultTargetPlatform) {
      case TargetPlatform.android:
      case TargetPlatform.iOS:
        return true;
      default:
        return false;
    }
  }

  bool _isCompact(BuildContext context) {
    return _isMobile || MediaQuery.sizeOf(context).width < 700;
  }

  void _toggleDebugPanel() {
    setState(() => _debugOpen = !_debugOpen);
  }

  Future<void> _capturePreview(int id) async {
    try {
      final bytes = await _controllerFor(id).takeScreenshot();
      if (!mounted) {
        return;
      }
      setState(() => _previews[id] = bytes);
    } catch (_) {
      // Backends without a screenshot still show a placeholder card.
    }
  }

  Future<void> _openSwitcher() async {
    await _capturePreview(tabs.activeId);
    if (mounted) {
      setState(() => _showSwitcher = true);
    }
  }

  void _closeSwitcher() {
    setState(() => _showSwitcher = false);
    _scheduleSurfaceCheck();
  }

  void _closeAllTabs() {
    final ids = [for (final tab in tabs.tabs) tab.id];
    for (final id in ids) {
      final controller = _controllers.remove(id);
      unawaited(_navSubs.remove(id)?.cancel());
      unawaited(_pageSubs.remove(id)?.cancel());
      unawaited(controller?.dispose());
      _visited.remove(id);
      _navEvents.remove(id);
      _recorded.remove(id);
      _previews.remove(id);
    }
    tabs.closeAll();
    _controllerFor(tabs.activeId);
    _syncAddressFromActive();
    setState(() => _showSwitcher = false);
  }

  void _openTab() {
    final id = tabs.open();
    _controllerFor(id);
    _visited[id] = false;
    navigation = const MorphNavigationEvent(url: '');
    _canonicalAddress = '';
    address.clear();
    _showSwitcher = false;
    setState(() {});
    unawaited(_saveCurrentTabs());
  }

  void _closeTab(int id) {
    final controller = _controllers.remove(id);
    unawaited(_navSubs.remove(id)?.cancel());
    unawaited(_pageSubs.remove(id)?.cancel());
    unawaited(controller?.dispose());
    _visited.remove(id);
    _navEvents.remove(id);
    _recorded.remove(id);
    _previews.remove(id);
    tabs.close(id);
    _controllerFor(tabs.activeId);
    _syncAddressFromActive();
    setState(() {});
    _scheduleSurfaceCheck();
    unawaited(_saveCurrentTabs());
  }

  void _switchTab(int id) {
    if (id != tabs.activeId) {
      unawaited(_capturePreview(tabs.activeId));
      tabs.switchTo(id);
      _controllerFor(id);
      _syncAddressFromActive();
    }
    _showSwitcher = false;
    setState(() {});
    _scheduleSurfaceCheck();
  }

  void _syncAddressFromActive() {
    final tab = tabs.active;
    final url = _urlForTab(tab);
    final stored = _navEvents[tab.id];
    if (stored != null && _isRealUrl(stored.url)) {
      navigation = stored;
    } else {
      navigation = MorphNavigationEvent(
        url: url,
        title: tab.title,
        canGoBack: stored?.canGoBack ?? false,
        canGoForward: stored?.canGoForward ?? false,
        progress: stored?.progress ?? 0,
      );
    }
    _canonicalAddress = url;
    final text = _isRealUrl(url) ? hostFriendlyUrl(url) : '';
    if (address.text != text) {
      address.value = TextEditingValue(
        text: text,
        selection: TextSelection.collapsed(offset: text.length),
      );
    }
  }

  Widget _menuButton(ButtonStyle iconStyle) {
    return PopupMenuButton<String>(
      key: const Key('nav-menu'),
      tooltip: 'Menu',
      icon: const Icon(Icons.more_vert),
      style: iconStyle,
      onSelected: (value) {
        switch (value) {
          case 'find':
            _openFind();
          case 'bookmarks':
            unawaited(_openLibrary(MorphLibraryTab.bookmarks));
          case 'history':
            unawaited(_openLibrary(MorphLibraryTab.history));
          case 'settings':
            unawaited(_openSettings());
        }
      },
      itemBuilder: (context) => const [
        PopupMenuItem(
          key: Key('menu-find'),
          value: 'find',
          child: ListTile(
            leading: Icon(Icons.search),
            title: Text('Find in page'),
            trailing: Text('Ctrl+F'),
          ),
        ),
        PopupMenuItem(
          key: Key('menu-bookmarks'),
          value: 'bookmarks',
          child: ListTile(
            leading: Icon(Icons.bookmarks_outlined),
            title: Text('Bookmarks'),
          ),
        ),
        PopupMenuItem(
          key: Key('menu-history'),
          value: 'history',
          child: ListTile(
            leading: Icon(Icons.history),
            title: Text('History'),
            trailing: Text('Ctrl+H'),
          ),
        ),
        PopupMenuDivider(),
        PopupMenuItem(
          key: Key('menu-settings'),
          value: 'settings',
          child: ListTile(
            leading: Icon(Icons.settings_outlined),
            title: Text('Settings'),
          ),
        ),
      ],
    );
  }

  Widget _profileButton() {
    return MorphProfileButton(
      catalog: _catalog,
      onSwitch: (id) => unawaited(_switchProfile(id)),
      onAdd: _addProfile,
      onRename: _renameProfile,
      onDelete: _deleteProfile,
    );
  }

  MorphProfileSheetSection _profileSheetSection() {
    return MorphProfileSheetSection(
      catalog: _catalog,
      onSwitch: (id) {
        if (Navigator.of(context).canPop()) {
          Navigator.of(context).pop();
        }
        unawaited(_switchProfile(id));
      },
      onAdd: _addProfile,
      onRename: _renameProfile,
      onDelete: _deleteProfile,
    );
  }

  Future<void> _saveCurrentTabs() async {
    try {
      await widget.api.saveProfileTabs(
        tabs.snapshot(),
        home: _home,
        profileId: _profileId,
      );
    } catch (_) {}
  }

  /// initState makes a controller for the placeholder start tab before the
  /// saved tabs load. When a restored tab reuses that id with a real URL, the
  /// placeholder would open about:blank instead, so replace any controller
  /// that has not attached yet and was made for a different URL.
  void _dropStaleControllers() {
    for (final id in _controllers.keys.toList()) {
      final controller = _controllers[id]!;
      final tab = tabs.tab(id);
      final want = (tab != null && tab.url.isNotEmpty)
          ? tab.url
          : 'about:blank';
      if (controller.isAttached || controller.initialUrl == want) {
        continue;
      }
      _controllers.remove(id);
      unawaited(_navSubs.remove(id)?.cancel());
      unawaited(_pageSubs.remove(id)?.cancel());
      unawaited(controller.dispose());
    }
  }

  Future<void> _disposeAllViews() async {
    for (final sub in _navSubs.values) {
      unawaited(sub.cancel());
    }
    _navSubs.clear();
    for (final sub in _pageSubs.values) {
      unawaited(sub.cancel());
    }
    _pageSubs.clear();
    for (final controller in _controllers.values) {
      unawaited(controller.dispose());
    }
    _controllers.clear();
    _navEvents.clear();
    _recorded.clear();
    _visited.clear();
    _previews.clear();
  }

  Future<void> _applyCatalog(ProfileCatalog catalog) async {
    await _disposeAllViews();
    if (!mounted) {
      return;
    }
    _catalog = catalog;
    _profileId = catalog.current;
    _bookmarkUrls = const {};
    unawaited(_loadBookmarks());
    try {
      tabs.applySnapshot(
        await widget.api.loadProfileTabs(home: _home, profileId: _profileId),
      );
    } catch (_) {
      tabs.applySnapshot(ProfileTabSnapshot.empty);
    }
    try {
      _tabPosition = MorphTabPosition.fromKey(
        await widget.api.loadTabPosition(home: _home),
      );
      _sidebarCollapsed = await widget.api.loadSidebarCollapsed(home: _home);
      _sidebarWidth = (await widget.api.loadSidebarWidth(home: _home))
          .toDouble();
    } catch (_) {}
    for (final tab in tabs.tabs) {
      if (tab.url.isNotEmpty && tab.url != 'about:blank') {
        _visited[tab.id] = true;
      }
    }
    _controllerFor(tabs.activeId);
    _syncAddressFromActive();
    if (mounted) {
      setState(() {});
      _scheduleSurfaceCheck(const Duration(milliseconds: 800));
    }
  }

  Future<void> _switchProfile(String id) async {
    if (id == _profileId) {
      return;
    }
    await _saveCurrentTabs();
    try {
      final catalog = await widget.api.setCurrentProfile(id, home: _home);
      await _applyCatalog(catalog);
    } catch (e) {
      if (mounted) {
        setState(() => error = e);
      }
    }
  }

  Future<void> _addProfile(String name, String color) async {
    await _saveCurrentTabs();
    try {
      final catalog = await widget.api.createProfile(
        name,
        home: _home,
        color: color,
      );
      await _applyCatalog(catalog);
    } catch (e) {
      if (mounted) {
        setState(() => error = e);
      }
    }
  }

  Future<void> _renameProfile(String id, String name) async {
    try {
      final catalog = await widget.api.renameProfile(id, name, home: _home);
      if (mounted) {
        setState(() => _catalog = catalog);
      }
    } catch (e) {
      if (mounted) {
        setState(() => error = e);
      }
    }
  }

  Future<void> _deleteProfile(String id) async {
    if (_catalog.profiles.length <= 1) {
      return;
    }
    if (id == _profileId) {
      await _saveCurrentTabs();
    }
    try {
      final catalog = await widget.api.deleteProfile(id, home: _home);
      if (id == _profileId) {
        await _applyCatalog(catalog);
      } else if (mounted) {
        setState(() => _catalog = catalog);
      }
      // Engine storage outside the Morph home (Android WebView profiles).
      // A profile still in use is swept on the next start.
      unawaited(_pruneProfileData(catalog));
    } catch (e) {
      if (mounted) {
        setState(() => error = e);
      }
    }
  }

  Future<void> _pruneProfileData(ProfileCatalog catalog) async {
    try {
      await MorphWebViewPlatform.instance.pruneProfileData(
        catalog.profiles.map((p) => p.id),
      );
    } catch (_) {
      // Leftover engine data is retried on the next start.
    }
  }

  Future<void> _setTabPosition(MorphTabPosition position) async {
    setState(() => _tabPosition = position);
    try {
      await widget.api.saveTabPosition(position.key, home: _home);
    } catch (_) {
      // The in-memory choice still applies if persistence fails.
    }
  }

  Future<void> _toggleSidebar() async {
    final next = !_sidebarCollapsed;
    setState(() => _sidebarCollapsed = next);
    try {
      await widget.api.saveSidebarCollapsed(next, home: _home);
    } catch (_) {
      // The in-memory choice still applies if persistence fails.
    }
  }

  static String _pageKey(String url) =>
      url.trim().replaceFirst(RegExp(r'/+$'), '');

  bool get _hasPage =>
      _canonicalAddress.isNotEmpty && _canonicalAddress != 'about:blank';

  bool get _currentBookmarked =>
      _hasPage && _bookmarkUrls.contains(_pageKey(_canonicalAddress));

  String get _profileName {
    for (final p in _catalog.profiles) {
      if (p.id == _profileId) {
        return p.name;
      }
    }
    return _profileId;
  }

  void _setBookmarks(String profileId, List<MorphBookmark> list) {
    if (mounted && profileId == _profileId) {
      setState(() => _bookmarkUrls = {for (final b in list) _pageKey(b.url)});
    }
  }

  Future<void> _loadBookmarks() async {
    final profileId = _profileId;
    try {
      _setBookmarks(
        profileId,
        await widget.api.listBookmarks(profileId, home: _home),
      );
    } catch (_) {
      _setBookmarks(profileId, const []);
    }
  }

  Future<void> _toggleBookmark() async {
    if (!_hasPage) {
      return;
    }
    final profileId = _profileId;
    final url = _canonicalAddress;
    final title = navigation.title;
    final remove = _currentBookmarked;
    try {
      final list = await _libraryWrite(
        () => remove
            ? widget.api.removeBookmark(profileId, url, home: _home)
            : widget.api.addBookmark(profileId, url, title, home: _home),
      );
      _setBookmarks(profileId, list);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text(
              remove ? 'Bookmark removed' : 'Bookmarked in $_profileName',
            ),
          ),
        );
      }
    } catch (e) {
      if (mounted) {
        setState(() => error = e);
      }
    }
  }

  /// Shows a full-height panel without covering the page with an opaque
  /// route, so platform web views stay composited underneath.
  Future<void> _showPanel(WidgetBuilder builder) async {
    if (!mounted) {
      return;
    }
    if (_isMobile) {
      await showModalBottomSheet<void>(
        context: context,
        isScrollControlled: true,
        useSafeArea: true,
        builder: (context) => FractionallySizedBox(
          heightFactor: 0.94,
          child: ClipRRect(
            borderRadius: const BorderRadius.vertical(top: Radius.circular(16)),
            child: builder(context),
          ),
        ),
      );
    } else {
      await showDialog<void>(
        context: context,
        builder: (context) => Dialog(
          clipBehavior: Clip.antiAlias,
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 720, maxHeight: 680),
            child: builder(context),
          ),
        ),
      );
    }
    _onOverlayClosed();
  }

  Future<void> _openLibrary(MorphLibraryTab tab) async {
    await _showPanel(
      (context) => MorphLibraryPage(
        api: widget.api,
        profileId: _profileId,
        profileName: _profileName,
        home: _home,
        initialTab: tab,
        write: _libraryWrite,
        onOpen: (url) => unawaited(_submitUrl(url)),
      ),
    );
    await _loadBookmarks();
  }

  Future<void> _openSettings() async {
    final desktop = !_isMobile;
    await _showPanel(
      (panel) => MorphSettingsPage(
        profileName: _profileName,
        coreVersion: coreVersionLabel ?? '…',
        renderPath: '${webView.renderPath}',
        tabPosition: desktop ? _tabPosition : null,
        sidebarCollapsed: desktop ? _sidebarCollapsed : null,
        onTabPositionChanged: (p) => unawaited(_setTabPosition(p)),
        onSidebarCollapsedChanged: (value) {
          if (value != _sidebarCollapsed) {
            unawaited(_toggleSidebar());
          }
        },
        onClearHistory: () {
          final profileId = _profileId;
          return _libraryWrite(
            () => widget.api.clearHistory(profileId, home: _home),
          );
        },
        onOpenLibrary: (tab) {
          Navigator.of(panel).pop();
          unawaited(_openLibrary(tab));
        },
      ),
    );
  }

  void _openFind() {
    if (!_findOpen) {
      setState(() => _findOpen = true);
      _scheduleSurfaceCheck();
    }
  }

  void _closeFind() {
    if (!_findOpen) {
      return;
    }
    setState(() => _findOpen = false);
    unawaited(
      webView
          .evaluateJavaScript(morphFindClearScript)
          .then((_) {}, onError: (Object _) {}),
    );
    _scheduleSurfaceCheck();
  }

  Future<MorphFindResult> _findInPage(
    String query, {
    required bool backward,
    required bool reset,
  }) async {
    if (!webView.isAttached || _onStartPage) {
      return MorphFindResult.none;
    }
    final reply = await webView.evaluateJavaScript(
      morphFindScript(query, backward: backward, reset: reset),
    );
    return parseMorphFindResult(reply);
  }

  void _setSidebarWidth(double width) {
    setState(() => _sidebarWidth = width);
  }

  Future<void> _commitSidebarWidth() async {
    try {
      await widget.api.saveSidebarWidth(_sidebarWidth.round(), home: _home);
    } catch (_) {
      // The in-memory choice still applies if persistence fails.
    }
  }

  Future<void> _resetSidebarWidth() async {
    setState(() => _sidebarWidth = MorphTabOverflow.defaultSidebarWidth);
    await _commitSidebarWidth();
  }

  KeyEventResult _onShellKey(KeyEvent event) {
    final shortcut = MorphShellShortcuts.match(event);
    if (shortcut == null) {
      return KeyEventResult.ignored;
    }
    final invoke =
        event is KeyDownEvent ||
        (event is KeyRepeatEvent &&
            (shortcut == MorphShellShortcut.nextTab ||
                shortcut == MorphShellShortcut.prevTab));
    if (invoke && mounted) {
      _invokeShellShortcut(shortcut);
    }
    return KeyEventResult.handled;
  }

  void _invokeShellShortcut(MorphShellShortcut shortcut) {
    switch (shortcut) {
      case MorphShellShortcut.newTab:
        _openTab();
      case MorphShellShortcut.closeTab:
        _closeTab(tabs.activeId);
      case MorphShellShortcut.nextTab:
        _controllerFor(tabs.cycleNext());
        _syncAddressFromActive();
        setState(() {});
      case MorphShellShortcut.prevTab:
        _controllerFor(tabs.cyclePrev());
        _syncAddressFromActive();
        setState(() {});
      case MorphShellShortcut.focusAddress:
        addressFocus.requestFocus();
        _selectAllAddress();
      case MorphShellShortcut.reload:
        unawaited(webView.reload());
      case MorphShellShortcut.find:
        _openFind();
      case MorphShellShortcut.bookmark:
        unawaited(_toggleBookmark());
      case MorphShellShortcut.history:
        unawaited(_openLibrary(MorphLibraryTab.history));
      case MorphShellShortcut.selectTab1:
      case MorphShellShortcut.selectTab2:
      case MorphShellShortcut.selectTab3:
      case MorphShellShortcut.selectTab4:
      case MorphShellShortcut.selectTab5:
      case MorphShellShortcut.selectTab6:
      case MorphShellShortcut.selectTab7:
      case MorphShellShortcut.selectTab8:
      case MorphShellShortcut.selectLastTab:
        final index = MorphShellShortcuts.tabIndex(shortcut);
        if (index == null || tabs.tabs.isEmpty) {
          return;
        }
        final tab = index < 0
            ? tabs.tabs.last
            : (index < tabs.tabs.length ? tabs.tabs[index] : null);
        if (tab != null) {
          _switchTab(tab.id);
        }
    }
  }

  Future<void> _openAddressSheet() async {
    if (!mounted) {
      return;
    }
    final initial =
        (_canonicalAddress.isEmpty || _canonicalAddress == 'about:blank')
        ? address.text
        : _canonicalAddress;
    await showModalBottomSheet<void>(
      context: context,
      isScrollControlled: true,
      backgroundColor: Colors.transparent,
      builder: (context) {
        return MorphAddressSheet(
          initialUrl: initial,
          canGoBack: navigation.canGoBack,
          canGoForward: navigation.canGoForward,
          desktopSite: webView.desktopSite,
          identityLabel: userId,
          onSubmit: _submitUrl,
          onBack: () => unawaited(webView.goBack()),
          onForward: () => unawaited(webView.goForward()),
          onReload: () => unawaited(webView.reload()),
          onDesktopSiteChanged: (value) =>
              unawaited(webView.setDesktopSite(value)),
          onShare: _shareUrl,
          onFind: _openFind,
          bookmarked: _currentBookmarked,
          onToggleBookmark: _hasPage
              ? () => unawaited(_toggleBookmark())
              : null,
          onHistory: () => unawaited(_openLibrary(MorphLibraryTab.history)),
          onBookmarks: () => unawaited(_openLibrary(MorphLibraryTab.bookmarks)),
          onSettings: () => unawaited(_openSettings()),
          profiles: _profileSheetSection(),
        );
      },
    );
  }

  Future<void> _shareUrl(String url) async {
    await Clipboard.setData(ClipboardData(text: url));
  }

  void _selectAllAddress() {
    address.selection = TextSelection(
      baseOffset: 0,
      extentOffset: address.text.length,
    );
  }

  void _applyAddressDisplay() {
    if (addressFocus.hasFocus) {
      return;
    }
    final full = _canonicalAddress == 'about:blank' ? '' : _canonicalAddress;
    if (full.isEmpty) {
      // Keep a typed draft until the user submits it.
      return;
    }
    final text = hostFriendlyUrl(full);
    if (address.text != text) {
      address.value = address.value.copyWith(
        text: text,
        selection: TextSelection.collapsed(offset: text.length),
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final colors = theme.colorScheme;
    final compact = _isCompact(context);
    final iconStyle = IconButton.styleFrom(
      visualDensity: compact ? VisualDensity.compact : VisualDensity.standard,
      minimumSize: compact ? const Size(40, 40) : const Size(48, 48),
      tapTargetSize: compact
          ? MaterialTapTargetSize.shrinkWrap
          : MaterialTapTargetSize.padded,
      padding: compact ? const EdgeInsets.all(8) : null,
    );

    final content = Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (error != null)
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
            child: Text(
              'Error: $error',
              style: MorphTheme.monoStyle(color: colors.error, size: 12),
            ),
          ),
        if (_findOpen)
          MorphFindBar(
            key: ValueKey('find-bar-$_profileId-${tabs.activeId}'),
            onFind: _findInPage,
            onClose: _closeFind,
          ),
        Expanded(child: _webStack(colors)),
      ],
    );

    final body = MorphDesktopWindow.isDesktop
        ? Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Expanded(
                child: MorphDesktopChrome(
                  tabs: tabs.tabs,
                  activeId: tabs.activeId,
                  position: _tabPosition,
                  sidebarCollapsed: _sidebarCollapsed,
                  sidebarWidth: _sidebarWidth,
                  buttonMode: MorphWindowChrome.mode(),
                  useNativeMacButtons:
                      MorphDesktopWindow.usesNativeMacTrafficLights,
                  leadingInset: MorphDesktopWindow.leadingInset(),
                  isMaximized: MorphDesktopWindow.isMaximized,
                  navigation: _navRow(
                    context,
                    iconStyle: iconStyle,
                    compact: compact,
                    includeChrome: false,
                  ),
                  content: content,
                  onSelectTab: _switchTab,
                  onCloseTab: _closeTab,
                  onNewTab: _openTab,
                  onPositionChanged: _setTabPosition,
                  onToggleSidebar: _toggleSidebar,
                  onSidebarWidthChanged: _setSidebarWidth,
                  onSidebarWidthCommit: () {
                    unawaited(_commitSidebarWidth());
                  },
                  onSidebarWidthReset: () {
                    unawaited(_resetSidebarWidth());
                  },
                  onDebugLongPress: _toggleDebugPanel,
                ),
              ),
              if (_debugOpen) _debugPanel(colors),
            ],
          )
        : Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              _navRow(
                context,
                iconStyle: iconStyle,
                compact: compact,
                includeChrome: true,
              ),
              const MorphGlitchRule(),
              Expanded(child: content),
              if (_debugOpen) _debugPanel(colors),
            ],
          );

    final browser = PopScope(
      canPop: !_isMobile || (!_showSwitcher && !navigation.canGoBack),
      onPopInvokedWithResult: (didPop, _) {
        if (didPop) {
          return;
        }
        if (_showSwitcher) {
          _closeSwitcher();
          return;
        }
        if (navigation.canGoBack) {
          unawaited(webView.goBack());
        }
      },
      child: Focus(
        autofocus: true,
        child: Scaffold(
          resizeToAvoidBottomInset: true,
          body: SafeArea(top: false, bottom: false, child: body),
        ),
      ),
    );

    if (!_isMobile) {
      return browser;
    }
    // Keep the browser (and every tab's platform web view) mounted under the
    // grid. Swapping the tree out would dispose each AndroidView/UiKitView;
    // remounting then creates a fresh native WebView on its initial URL, which
    // is why returning from the grid used to show a blank white page.
    return Stack(
      fit: StackFit.expand,
      children: [
        Offstage(
          key: const Key('browser-view'),
          offstage: _showSwitcher,
          child: TickerMode(enabled: !_showSwitcher, child: browser),
        ),
        if (_showSwitcher)
          Scaffold(
            body: MorphTabSwitcher(
              tabs: tabs.tabs,
              activeId: tabs.activeId,
              previews: _previews,
              onSelect: _switchTab,
              onClose: _closeTab,
              onCloseAll: _closeAllTabs,
              onNewTab: _openTab,
            ),
          ),
      ],
    );
  }

  Widget _navRow(
    BuildContext context, {
    required ButtonStyle iconStyle,
    required bool compact,
    required bool includeChrome,
  }) {
    final theme = Theme.of(context);
    final colors = theme.colorScheme;
    return Material(
      color: theme.scaffoldBackgroundColor,
      child: SafeArea(
        bottom: false,
        top: includeChrome,
        child: SizedBox(
          height: compact ? 48 : 52,
          key: const Key('nav-row'),
          child: Padding(
            padding: EdgeInsets.fromLTRB(
              compact ? 4 : 8,
              compact ? 4 : 8,
              compact ? 4 : 4,
              compact ? 4 : 8,
            ),
            child: Row(
              children: [
                if (includeChrome)
                  MorphWindowDragArea(
                    child: GestureDetector(
                      key: const Key('debug-toggle'),
                      onLongPress: _toggleDebugPanel,
                      child: const Padding(
                        padding: EdgeInsets.symmetric(horizontal: 4),
                        child: MorphMark(size: 28),
                      ),
                    ),
                  ),
                if (!_isMobile) ...[
                  IconButton(
                    key: const Key('nav-back'),
                    style: iconStyle,
                    onPressed: navigation.canGoBack ? webView.goBack : null,
                    tooltip: 'Back',
                    icon: const Icon(Icons.arrow_back),
                  ),
                  IconButton(
                    key: const Key('nav-forward'),
                    style: iconStyle,
                    onPressed: navigation.canGoForward
                        ? webView.goForward
                        : null,
                    tooltip: 'Forward',
                    icon: const Icon(Icons.arrow_forward),
                  ),
                  IconButton(
                    key: const Key('nav-reload'),
                    style: iconStyle,
                    onPressed: webView.isAttached && !_onStartPage
                        ? webView.reload
                        : null,
                    tooltip: 'Reload',
                    icon: const Icon(Icons.refresh),
                  ),
                ],
                Expanded(
                  child: TextField(
                    key: const Key('address-bar'),
                    controller: address,
                    focusNode: addressFocus,
                    style: MorphTheme.monoStyle(
                      color: colors.onSurface,
                      size: compact ? 13 : 14,
                    ),
                    decoration: InputDecoration(
                      hintText: 'Enter an address',
                      isDense: compact,
                      contentPadding: EdgeInsets.symmetric(
                        horizontal: compact ? 10 : 12,
                        vertical: compact ? 8 : 10,
                      ),
                    ),
                    keyboardType: TextInputType.url,
                    textInputAction: TextInputAction.go,
                    readOnly: _isMobile,
                    onTap: () {
                      if (_isMobile) {
                        addressFocus.unfocus();
                        unawaited(_openAddressSheet());
                      } else {
                        _selectAllAddress();
                      }
                    },
                    onSubmitted: _submitUrl,
                  ),
                ),
                if (!_isMobile)
                  IconButton(
                    key: const Key('address-go'),
                    style: iconStyle,
                    onPressed: () => unawaited(_submitUrl(address.text)),
                    tooltip: 'Go',
                    icon: const Icon(Icons.arrow_outward),
                  ),
                if (!_isMobile) ...[
                  IconButton(
                    key: const Key('nav-bookmark'),
                    style: iconStyle,
                    onPressed: _hasPage
                        ? () => unawaited(_toggleBookmark())
                        : null,
                    tooltip: _currentBookmarked
                        ? 'Remove bookmark'
                        : 'Bookmark this page',
                    icon: Icon(
                      _currentBookmarked ? Icons.star : Icons.star_border,
                      color: _currentBookmarked ? MorphColors.cyan : null,
                    ),
                  ),
                  _menuButton(iconStyle),
                ],
                if (_isMobile) ...[
                  IconButton(
                    key: const Key('tab-count'),
                    style: iconStyle,
                    onPressed: () => unawaited(_openSwitcher()),
                    tooltip: 'Tabs',
                    icon: _TabCountIcon(count: tabs.length),
                  ),
                  IconButton(
                    key: const Key('tab-new'),
                    style: iconStyle,
                    onPressed: _openTab,
                    tooltip: 'New tab',
                    icon: const Icon(Icons.add),
                  ),
                ],
                if (!_isMobile) _profileButton(),
                if (includeChrome && MorphDesktopWindow.isDesktop)
                  const MorphWindowCloseButton(),
              ],
            ),
          ),
        ),
      ),
    );
  }

  Widget _webStack(ColorScheme colors) {
    final stack = Stack(
      fit: StackFit.expand,
      children: [
        if (ready)
          for (final tab in tabs.tabs)
            Offstage(
              offstage: tab.id != tabs.activeId,
              child: TickerMode(
                enabled: tab.id == tabs.activeId,
                child: MorphWebView(
                  key: Key('web-view-$_profileId-${tab.id}'),
                  controller: _controllerFor(tab.id),
                  profileId: _profileId,
                  userScript: userScript,
                  placeholder: Center(
                    child: CircularProgressIndicator(color: colors.primary),
                  ),
                ),
              ),
            )
        else if (!_onStartPage)
          Center(child: CircularProgressIndicator(color: colors.primary)),
        if (_onStartPage) const MorphStartPage(),
      ],
    );
    if (!_isMobile) {
      return stack;
    }
    return MorphMobileGestures(
      canGoBack: navigation.canGoBack,
      canGoForward: navigation.canGoForward,
      canRefresh: webView.isAttached && !_onStartPage,
      onBack: () => unawaited(webView.goBack()),
      onForward: () => unawaited(webView.goForward()),
      onRefresh: () => unawaited(webView.reload()),
      child: stack,
    );
  }

  Widget _debugPanel(ColorScheme colors) {
    final mono = MorphTheme.monoStyle(color: colors.onSurfaceVariant, size: 12);
    return SafeArea(
      top: false,
      child: Material(
        key: const Key('debug-panel'),
        color: colors.surface,
        child: Padding(
          padding: const EdgeInsets.fromLTRB(16, 8, 16, 10),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Wrap(
                spacing: 16,
                runSpacing: 4,
                children: [
                  Text('morph-core ${coreVersionLabel ?? "…"}', style: mono),
                  Text('Tick ${tick ?? "…"}', style: mono),
                  Text('Home ${identityHome ?? "…"}', style: mono),
                  if (homeExists != null)
                    Text('Home exists $homeExists', style: mono),
                  Text('Render ${webView.renderPath}', style: mono),
                  if (userId != null) Text('User $userId', style: mono),
                  if (fingerprint != null)
                    Text('Fingerprint $fingerprint', style: mono),
                  if (pageReply != null) Text('Page $pageReply', style: mono),
                ],
              ),
              const SizedBox(height: 8),
              Wrap(
                spacing: 8,
                children: [
                  FilledButton(
                    onPressed: _newUser,
                    child: const Text('New ephemeral user'),
                  ),
                  OutlinedButton(
                    onPressed: _pingPage,
                    child: const Text('Ping page bridge'),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _TabCountIcon extends StatelessWidget {
  const _TabCountIcon({required this.count});

  final int count;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final label = count > 99 ? '∞' : '$count';
    return Container(
      width: 24,
      height: 24,
      alignment: Alignment.center,
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(6),
        border: Border.all(color: colors.onSurface, width: 1.5),
      ),
      child: Text(
        label,
        style: MorphTheme.monoStyle(color: colors.onSurface, size: 11),
      ),
    );
  }
}
