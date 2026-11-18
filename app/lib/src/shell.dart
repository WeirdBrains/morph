import 'default_browser.dart';
import 'incoming_links.dart';

import 'dart:async';
import 'dart:io';

import 'package:material_ui/material_ui.dart';
import 'package:flutter/services.dart';
import 'package:morph_webview/morph_webview.dart';

import 'api.dart';
import 'share.dart';
import 'shell_shortcuts.dart';
import 'host_friendly_url.dart';
export 'host_friendly_url.dart';
import 'theme/morph_theme.dart';
import 'tabs/tab_set.dart';
import 'widgets/address_sheet.dart';
import 'widgets/brand.dart';
import 'widgets/find_bar.dart';
import 'widgets/device_link.dart';
import 'widgets/identity_backup.dart';
import 'widgets/identity_onboarding.dart';
import 'widgets/library_page.dart';
import 'widgets/library_unlock.dart';
import 'widgets/mobile_gestures.dart';
import 'widgets/page_crashed.dart';
import 'widgets/page_error.dart';
import 'widgets/profile_menu.dart';
import 'widgets/settings_page.dart';
import 'widgets/start_page.dart';
import 'widgets/tab_switcher.dart';
import 'qr_scanner.dart';
import 'surface_watch.dart';

/// Morph's mobile shell (Android, iOS): address bar and sheet, tab grid,
/// platform web views, and the Rust wiring. The desktop shell is the GPUI
/// app (`crates/morph-desktop`).
class MorphShell extends StatefulWidget {
  const MorphShell({
    super.key,
    required this.api,
    this.home,
    this.subscribeToTicks = true,
    this.loadPageBridgeAsset = true,
    this.showDebugPanel = false,
    this.links,
    this.defaultBrowser,
    this.scanQr,
  });

  final MorphApi api;

  /// The QR scanner for "Add device"; null uses [MorphQrScanner] where the
  /// platform has one (Android). Tests pass a fake.
  final Future<String?> Function()? scanQr;

  /// Web links from the system, each opened in a new tab.
  final MorphIncomingLinks? links;

  /// Null uses [MorphDefaultBrowser.forPlatform] when Settings opens.
  final MorphDefaultBrowser? defaultBrowser;

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

  /// Whether each tab's last navigation event said it was loading, so a
  /// finished load can be told from the page's later progress, title and
  /// same-page URL updates.
  final _navLoading = <int, bool>{};
  final _visited = <int, bool>{};

  /// Notifies the open address sheet, whose profile list outlives a build.
  final _catalogNotifier = ValueNotifier<ProfileCatalog>(
    const ProfileCatalog(
      current: 'default',
      profiles: [ProfileInfo(id: 'default', name: 'Default')],
    ),
  );
  ProfileCatalog get _catalog => _catalogNotifier.value;
  set _catalog(ProfileCatalog value) => _catalogNotifier.value = value;
  String _profileId = 'default';
  final _recorded = <int, String>{};
  bool _findOpen = false;
  Set<String> _bookmarkUrls = const {};
  MorphIdentity? _identity;
  final _onboardingSkipped = <String>{};
  bool _onboardingOpen = false;

  /// Profiles whose library unlock was put off this run ("Not now").
  final _libraryUnlockSkipped = <String>{};
  bool _libraryUnlockOpen = false;
  Future<void> _libraryWrites = Future.value();
  bool _showSwitcher = false;
  final _previews = <int, Uint8List?>{};
  late bool _debugOpen;
  String _canonicalAddress = '';

  /// The search engine new tabs use (the profile's setting) and the ones
  /// tabs picked with the selector beside the address, as on desktop.
  String _defaultEngine = 'google';
  final _tabEngines = <int, String>{};
  String get _tabEngine => _tabEngines[tabs.activeId] ?? _defaultEngine;
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

  void _onTabNav(int id, MorphNavigationEvent raw) {
    if (!mounted) {
      return;
    }
    final event = _shownEvent(raw);
    final hasUrl = event.url.isNotEmpty && event.url != 'about:blank';
    final loadFinished = (_navLoading[id] ?? false) && !event.isLoading;
    _navLoading[id] = event.isLoading;
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
    if (hasUrl && loadFinished && event.progress >= 1) {
      // A finished load (including back, forward and reload) is the moment a
      // stale surface shows up as a white page. Android repeats "progress
      // 100" for same-page and subframe activity while the user reads and
      // scrolls; those are not loads and must not trigger captures.
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
        unawaited(_loadSearchEngine());
        unawaited(_pruneProfileData(catalog));
        unawaited(_loadBookmarks());
        unawaited(_checkIdentity());
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
      unawaited(_listenForLinks());
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

  StreamSubscription<String>? _linkSub;

  Future<void> _listenForLinks() async {
    final links = widget.links;
    if (links == null || _linkSub != null) {
      return;
    }
    _linkSub = links.later.listen((url) => unawaited(openIncomingLink(url)));
    for (final url in await links.initialLinks()) {
      await openIncomingLink(url);
    }
  }

  /// Opens a link another app handed Morph in a new tab of the active
  /// profile (reusing the tab if it is still an empty start page).
  @visibleForTesting
  Future<void> openIncomingLink(String url) async {
    if (!mounted) {
      return;
    }
    if (morphIsLinkInvite(url)) {
      await _openLinkInvite(url);
      return;
    }
    if (_tabHasPage(tabs.activeId) || _showSwitcher) {
      _openTab();
    }
    await _submitUrl(url);
  }

  Future<void> _loadSearchEngine() async {
    final engine = await widget.api.defaultSearchEngine(home: _home);
    if (mounted) {
      setState(() => _defaultEngine = engine);
    }
  }

  Future<void> _submitUrl(String raw) async {
    // A new address drops an app call under way in this tab.
    _appCalls.remove(tabs.activeId);
    if (morphIsLinkInvite(raw.trim())) {
      await _openLinkInvite(raw.trim());
      return;
    }
    final url = widget.api.omniboxTargetFor(raw, _tabEngine);
    if (url == null) {
      return;
    }
    final own = widget.api.ownAddress(url);
    if (own != null) {
      await _openOwnAddress(url, own);
      return;
    }
    _canonicalAddress = url;
    _applyAddressDisplay();
    tabs.update(tabs.activeId, url: url);
    setState(() => _visited[tabs.activeId] = true);
    if (webView.rendererFault.value != null) {
      // Typing an address into a crashed tab means "go there": bring the
      // renderer back first, or the crash screen stays over the new page.
      await webView.recoverFromFault();
    }
    await webView.loadUrl(url);
    unawaited(_saveCurrentTabs());
  }

  /// `morph://` pages open Morph's own screens; an unknown one, and a
  /// `weirdnet-<app>://` link this build cannot call over Weirdnet, cover the
  /// tab with Morph's error page. None of them is loaded, searched or
  /// handed to the OS, and the tab keeps its page (Back, Reload).
  Future<void> _openOwnAddress(String url, OwnAddress own) async {
    if (own.app) {
      return _openApp(url);
    }
    switch (own.page) {
      case 'bookmarks':
        return _openLibrary(MorphLibraryTab.bookmarks);
      case 'history':
        return _openLibrary(MorphLibraryTab.history);
      case 'identity':
        return _openIdentity();
      case 'profiles' || 'search':
        // The address sheet holds the profiles and the engine selector.
        return _openAddressSheet();
      case 'about' || 'downloads' || 'relays' || 'settings':
        return _openSettings();
      case _:
        break;
    }
    if (own.page != null) {
      return _openSettings();
    }
    _canonicalAddress = url;
    _applyAddressDisplay();
    setState(() => _visited[tabs.activeId] = true);
    webView.loadError.value = MorphLoadError(
      url: url,
      kind: MorphLoadErrorKind.other,
      message: own.body,
    );
    _ownErrors[tabs.activeId] = (url, own.title, own.body);
  }

  /// Morph's error pages shown over tabs, by tab: address, title and body.
  final _ownErrors = <int, (String, String, String)>{};

  /// Tabs with an app call under way, and the call's number (a newer
  /// call or address in the tab makes an older call's answer stale).
  final _appCalls = <int, int>{};
  int _appCallSeq = 0;

  /// App pages by their `data:` token, and the app address each shows.
  final _appPages = <String, String>{};

  /// Call the app a `weirdnet-<app>://` address names, over Weirdnet: the
  /// tab shows the address and a progress line while the bridge finds the
  /// peer and reads its answer off the UI thread, then the app's page (a
  /// `data:` address the tab shows as [url]) or Morph's error page saying
  /// why not. Never a search, never handed to the OS.
  Future<void> _openApp(String url) async {
    final tab = tabs.activeId;
    final call = ++_appCallSeq;
    _canonicalAddress = url;
    _applyAddressDisplay();
    _ownErrors.remove(tab);
    setState(() {
      _visited[tab] = true;
      _appCalls[tab] = call;
    });
    final page = await widget.api.openApp(url);
    if (!mounted || _appCalls[tab] != call) {
      return;
    }
    setState(() => _appCalls.remove(tab));
    final controller = _controllerFor(tab);
    final dataUrl = page.dataUrl;
    if (dataUrl == null) {
      controller.loadError.value = MorphLoadError(
        url: url,
        kind: MorphLoadErrorKind.other,
        message: page.body,
      );
      _ownErrors[tab] = (url, page.title, page.body);
      return;
    }
    final token = morphAppPageToken(dataUrl);
    if (token != null) {
      _appPages[token] = url;
    }
    tabs.update(tab, url: url, title: page.title);
    if (controller.rendererFault.value != null) {
      await controller.recoverFromFault();
    }
    await controller.loadUrl(dataUrl);
  }

  /// [event] with an app page's `data:` address shown as its app address.
  MorphNavigationEvent _shownEvent(MorphNavigationEvent event) {
    final token = morphAppPageToken(event.url);
    final app = token == null ? null : _appPages[token];
    if (app == null) {
      return event;
    }
    return MorphNavigationEvent(
      url: app,
      title: event.title,
      canGoBack: event.canGoBack,
      canGoForward: event.canGoForward,
      isLoading: event.isLoading,
      progress: event.progress,
    );
  }

  /// Reload: an app's page is called again rather than reloaded from its
  /// `data:` copy.
  Future<void> _reloadPage() async {
    final own = _canonicalAddress.isEmpty
        ? null
        : widget.api.ownAddress(_canonicalAddress);
    if (own != null && own.app) {
      await _submitUrl(_canonicalAddress);
      return;
    }
    await webView.reload();
  }

  (String, String)? _ownErrorText(int tab, MorphLoadError error) {
    final own = _ownErrors[tab];
    return own != null && own.$1 == error.url ? (own.$2, own.$3) : null;
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
    if (!mounted) {
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
    _catalogNotifier.dispose();
    _linkInvite.dispose();
    unawaited(_linkSub?.cancel());
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
      _navLoading.remove(id);
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
    _navLoading.remove(id);
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
      // Warm the target tab's renderer back up before the switch is drawn.
      _controllerFor(id).setVisible(true);
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

  MorphProfileSheetSection _profileSheetSection() {
    return MorphProfileSheetSection(
      catalog: _catalog,
      onSwitch: (id) {
        if (Navigator.of(context).canPop()) {
          Navigator.of(context).pop();
        }
        unawaited(_switchProfile(id));
      },
      onAdd: (name, color) async {
        // The new profile becomes current: close the sheet, which still
        // shows the old catalog, so its onboarding is not under it.
        if (Navigator.of(context).canPop()) {
          Navigator.of(context).pop();
        }
        await _addProfile(name, color);
      },
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
    _identity = null;
    unawaited(_loadBookmarks());
    unawaited(_checkIdentity());
    try {
      tabs.applySnapshot(
        await widget.api.loadProfileTabs(home: _home, profileId: _profileId),
      );
    } catch (_) {
      tabs.applySnapshot(ProfileTabSnapshot.empty);
    }
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
  Future<void> _showPanel(
    WidgetBuilder builder, {
    bool dismissible = true,
  }) async {
    if (!mounted) {
      return;
    }
    await showModalBottomSheet<void>(
      context: context,
      isScrollControlled: true,
      useSafeArea: true,
      isDismissible: dismissible,
      enableDrag: dismissible,
      builder: (context) => FractionallySizedBox(
        heightFactor: 0.94,
        child: ClipRRect(
          borderRadius: const BorderRadius.vertical(top: Radius.circular(16)),
          child: builder(context),
        ),
      ),
    );
    _onOverlayClosed();
  }

  String get _identityLabel {
    final id = _identity;
    if (id == null) {
      return 'Set up identity';
    }
    final short = id.userId.length > 12
        ? '${id.userId.substring(0, 12)}…'
        : id.userId;
    return 'Identity $short';
  }

  /// Looks up the active profile's identity and starts onboarding the first
  /// time a profile without one is used (once per profile per run if skipped).
  Future<void> _checkIdentity() async {
    final profileId = _profileId;
    MorphIdentity? identity;
    try {
      identity = await widget.api.profileIdentity(profileId, home: _home);
    } catch (e) {
      // No readable profile store yet: nothing to onboard into.
      debugPrint('Morph: identity check for $profileId failed: $e');
      return;
    }
    if (!mounted || profileId != _profileId) {
      return;
    }
    setState(() => _identity = identity);
    if (identity == null) {
      if (!_onboardingSkipped.contains(profileId)) {
        await _openOnboarding();
      }
      return;
    }
    await _unlockLibrary();
  }

  /// Opens the active profile's encrypted library: from the system keystore
  /// with no prompt, else by asking for the profile password once per run.
  Future<void> _unlockLibrary() async {
    final profileId = _profileId;
    final home = _home;
    MorphLibraryOpen open;
    try {
      open = await widget.api.openLibrary(profileId, home: home);
    } catch (e) {
      debugPrint('Morph: opening the library of $profileId failed: $e');
      return;
    }
    if (!mounted || profileId != _profileId) {
      return;
    }
    if (open.ready) {
      // The first bookmark load may have raced the unlock.
      unawaited(_loadBookmarks());
      return;
    }
    if (!open.needsPassword ||
        _libraryUnlockOpen ||
        _libraryUnlockSkipped.contains(profileId)) {
      return;
    }
    _libraryUnlockOpen = true;
    MorphLibraryUnlock? unlocked;
    try {
      await showDialog<void>(
        context: context,
        barrierDismissible: false,
        builder: (dialog) => MorphLibraryUnlockDialog(
          profileName: _profileName,
          reason: open.reason,
          onUnlock: (password) =>
              widget.api.unlockLibrary(profileId, password, home: home),
          onDone: (result) {
            unlocked = result;
            Navigator.of(dialog).pop();
          },
          onSkip: () => Navigator.of(dialog).pop(),
        ),
      );
    } finally {
      _libraryUnlockOpen = false;
    }
    final result = unlocked;
    if (result == null) {
      _libraryUnlockSkipped.add(profileId);
      return;
    }
    if (!mounted || profileId != _profileId) {
      return;
    }
    unawaited(_loadBookmarks());
    if (!result.remembered) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(
          key: Key('library-not-remembered'),
          content: Text(
            "Unlocked. The system keyring isn't available, so Morph will "
            'ask for this password next time.',
          ),
        ),
      );
    }
  }

  /// The camera QR scanner, where there is one.
  Future<String?> Function()? get _scanQr =>
      widget.scanQr ?? (MorphQrScanner.available ? MorphQrScanner.scan : null);

  /// Another device's device-link invite waiting for the onboarding's "Link
  /// from another device" screen to answer it.
  final _linkInvite = ValueNotifier<String?>(null);

  /// Another device's device-link invite (scanned, opened as a link, or
  /// scanned under Add a device): this device is the new one, so the
  /// identity comes here. Only a profile without an identity can take it.
  Future<void> _openLinkInvite(String invite) async {
    if (!mounted) {
      return;
    }
    final profileId = _profileId;
    MorphIdentity? identity = _identity;
    try {
      identity = await widget.api.profileIdentity(profileId, home: _home);
    } catch (_) {
      // Keep what the shell knows.
    }
    if (!mounted || profileId != _profileId) {
      return;
    }
    if (identity == null) {
      _linkInvite.value = invite.trim();
      await _openOnboarding();
      return;
    }
    await showDialog<void>(
      context: context,
      builder: (context) => AlertDialog(
        key: const Key('link-invite-has-identity'),
        title: const Text('This profile already has an identity'),
        content: Text(
          'That code is from a device adding a new device. $_profileName '
          'on this device already has an identity, so it cannot take '
          'another one. Switch to a profile without an identity and open '
          'the code again, or, to add the other device to this identity, '
          'choose Link from another device there and scan its code here '
          'under Add a device.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text('OK'),
          ),
        ],
      ),
    );
    _onOverlayClosed();
  }

  Future<void> _openOnboarding() async {
    if (_onboardingOpen || !mounted) {
      return;
    }
    _onboardingOpen = true;
    final profileId = _profileId;
    final home = _home;
    var finished = false;
    try {
      await _showPanel(
        dismissible: false,
        (panel) => MorphIdentityOnboarding(
          profileName: _profileName,
          onCreate: (password) =>
              widget.api.createProfileIdentity(profileId, password, home: home),
          onImport: (words, password) => widget.api.importProfileIdentity(
            profileId,
            words,
            password,
            home: home,
          ),
          backupKind: widget.api.identityBackupKind,
          onRestoreBackup: (contents, backupPassword, password) =>
              widget.api.restoreProfileBackup(
                profileId,
                contents,
                backupPassword: backupPassword,
                password: password,
                home: home,
              ),
          onDone: (identity) {
            finished = true;
            if (mounted && profileId == _profileId) {
              setState(() => _identity = identity);
            }
            Navigator.of(panel).pop();
          },
          onSkip: () => Navigator.of(panel).pop(),
          linkInvite: _linkInvite,
          linkFromDevice: (_) => ValueListenableBuilder<String?>(
            valueListenable: _linkInvite,
            builder: (_, invite, _) => MorphLinkFromDevice(
              api: widget.api,
              profileId: profileId,
              home: home,
              scan: _scanQr,
              invite: invite,
              onDone: (identity) {
                finished = true;
                if (mounted && profileId == _profileId) {
                  setState(() => _identity = identity);
                }
                Navigator.of(panel).pop();
              },
            ),
          ),
        ),
      );
    } finally {
      _onboardingOpen = false;
      _linkInvite.value = null;
    }
    if (!finished) {
      _onboardingSkipped.add(profileId);
    } else if (mounted && profileId == _profileId) {
      // A new identity encrypts the library (Rust moves it in): reload.
      unawaited(_loadBookmarks());
    }
  }

  Future<void> _openIdentity() async {
    final identity = _identity;
    if (identity == null) {
      await _openOnboarding();
      return;
    }
    if (!mounted) {
      return;
    }
    final colors = Theme.of(context).colorScheme;
    final mono = MorphTheme.monoStyle(color: colors.onSurface, size: 12);
    await showDialog<void>(
      context: context,
      builder: (context) => AlertDialog(
        key: const Key('identity-dialog'),
        // Short (tiled) windows: the details and the four actions scroll.
        scrollable: true,
        title: Text('$_profileName identity'),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const Text('User id'),
            SelectableText(identity.userId, style: mono),
            const SizedBox(height: 8),
            const Text('Fingerprint'),
            SelectableText(identity.fingerprint, style: mono),
            const SizedBox(height: 8),
            Text(
              identity.linked
                  ? 'Linked from another device.'
                  : identity.imported
                  ? 'Restored from recovery words or a backup.'
                  : 'Created in this profile.',
            ),
          ],
        ),
        actions: [
          TextButton.icon(
            key: const Key('identity-add-device'),
            onPressed: () {
              Navigator.of(context).pop();
              unawaited(_openAddDevice());
            },
            icon: const Icon(Icons.qr_code_scanner),
            label: const Text('Add device'),
          ),
          TextButton.icon(
            key: const Key('identity-recovery-phrase'),
            onPressed: () {
              Navigator.of(context).pop();
              unawaited(_openRecoveryPhrase(identity));
            },
            icon: const Icon(Icons.key),
            label: const Text('Recovery phrase'),
          ),
          TextButton.icon(
            key: const Key('identity-backup'),
            onPressed: () {
              Navigator.of(context).pop();
              unawaited(_openIdentityBackup());
            },
            icon: const Icon(Icons.save_alt),
            label: const Text('Back up'),
          ),
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text('Close'),
          ),
        ],
      ),
    );
    _onOverlayClosed();
  }

  /// The active profile's recovery phrase, after its password.
  Future<void> _openRecoveryPhrase(MorphIdentity identity) async {
    if (!mounted) {
      return;
    }
    final profileId = _profileId;
    final home = _home;
    await showDialog<void>(
      context: context,
      builder: (context) => MorphRecoveryPhraseDialog(
        profileName: _profileName,
        userId: identity.userId,
        reveal: (password) =>
            widget.api.profileRecoveryWords(profileId, password, home: home),
      ),
    );
    _onOverlayClosed();
  }

  /// A password-sealed backup of the active profile's identity.
  Future<void> _openIdentityBackup() async {
    if (!mounted) {
      return;
    }
    final profileId = _profileId;
    final home = _home;
    await showDialog<void>(
      context: context,
      builder: (context) => MorphIdentityBackupDialog(
        profileName: _profileName,
        export: (password, backupPassword) => widget.api.exportProfileBackup(
          profileId,
          password,
          backupPassword,
          home: home,
        ),
      ),
    );
    _onOverlayClosed();
  }

  /// QR login, existing-device side: send this profile's identity to a new
  /// device that shows a link code.
  Future<void> _openAddDevice() async {
    if (!mounted) {
      return;
    }
    await _showPanel(
      (panel) => Scaffold(
        key: const Key('add-device-panel'),
        appBar: AppBar(title: Text('Add a device · $_profileName')),
        body: SafeArea(
          child: SingleChildScrollView(
            padding: const EdgeInsets.fromLTRB(20, 8, 20, 20),
            child: MorphAddDevice(
              api: widget.api,
              profileId: _profileId,
              profileName: _profileName,
              home: _home,
              scan: _scanQr,
              onClose: () => Navigator.of(panel).pop(),
              onInvite: (invite) {
                Navigator.of(panel).pop();
                unawaited(_openLinkInvite(invite));
              },
            ),
          ),
        ),
      ),
    );
  }

  Future<void> _openLibrary(MorphLibraryTab tab) async {
    String? chosen;
    ModalRoute<Object?>? panelRoute;
    await _showPanel((context) {
      panelRoute = ModalRoute.of(context);
      return MorphLibraryPage(
        api: widget.api,
        profileId: _profileId,
        profileName: _profileName,
        home: _home,
        initialTab: tab,
        write: _libraryWrite,
        onOpen: (url) => chosen = url,
      );
    });
    final url = chosen;
    if (url != null) {
      // The panel's pop resolves when it starts closing; wait for the close
      // animation to finish too.
      await panelRoute?.completed;
    }
    if (url != null && mounted) {
      // Navigate once the panel is gone, not while it still covers the web
      // view: on Android a load that lands under a closing sheet can leave
      // the surface white (the bookmark and history white page).
      await _submitUrl(url);
      _scheduleSurfaceCheck();
    }
    await _loadBookmarks();
  }

  late final MorphDefaultBrowser _defaultBrowser =
      widget.defaultBrowser ?? MorphDefaultBrowser.forPlatform();

  Future<void> _openSettings() async {
    await _showPanel(
      (panel) => MorphSettingsPage(
        relays: widget.api,
        network: widget.api,
        home: _home,
        profileName: _profileName,
        coreVersion: coreVersionLabel ?? '…',
        renderPath: '${webView.renderPath}',
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
        defaultBrowser: _defaultBrowser,
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
          identityLabel: _identityLabel,
          onIdentity: () => unawaited(_openIdentity()),
          onSubmit: _submitUrl,
          engines: widget.api.searchEngines(),
          engine: _tabEngine,
          onEngineChanged: (id) =>
              setState(() => _tabEngines[tabs.activeId] = id),
          onBack: () => unawaited(webView.goBack()),
          onForward: () => unawaited(webView.goForward()),
          onReload: () => unawaited(_reloadPage()),
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
          profiles: ValueListenableBuilder<ProfileCatalog>(
            valueListenable: _catalogNotifier,
            builder: (context, catalog, child) => _profileSheetSection(),
          ),
        );
      },
    );
  }

  Future<void> _shareUrl(String url) async {
    final title = navigation.title;
    final sheet = await MorphShare.shareLink(url, title: title);
    if (!sheet && mounted) {
      ScaffoldMessenger.of(context)
          .showSnackBar(const SnackBar(content: Text('Link copied')));
    }
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
    final iconStyle = IconButton.styleFrom(
      visualDensity: VisualDensity.compact,
      minimumSize: const Size(40, 40),
      tapTargetSize: MaterialTapTargetSize.shrinkWrap,
      padding: const EdgeInsets.all(8),
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

    final body = Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _navRow(context, iconStyle: iconStyle),
        if (_appCalls.containsKey(tabs.activeId))
          const LinearProgressIndicator(
            key: Key('app-call-loading'),
            minHeight: 2,
          )
        else
          const MorphGlitchRule(),
        Expanded(child: content),
        if (_debugOpen) _debugPanel(colors),
      ],
    );

    final browser = PopScope(
      canPop: !_showSwitcher && !navigation.canGoBack,
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

  Widget _navRow(BuildContext context, {required ButtonStyle iconStyle}) {
    final theme = Theme.of(context);
    final colors = theme.colorScheme;
    return Material(
      color: theme.scaffoldBackgroundColor,
      child: SafeArea(
        bottom: false,
        child: SizedBox(
          height: 48,
          key: const Key('nav-row'),
          child: Padding(
            padding: const EdgeInsets.all(4),
            child: Row(
              children: [
                GestureDetector(
                  key: const Key('debug-toggle'),
                  onLongPress: _toggleDebugPanel,
                  child: const Padding(
                    padding: EdgeInsets.symmetric(horizontal: 4),
                    child: MorphMark(size: 28),
                  ),
                ),
                Expanded(
                  child: TextField(
                    key: const Key('address-bar'),
                    controller: address,
                    focusNode: addressFocus,
                    style: MorphTheme.monoStyle(
                      color: colors.onSurface,
                      size: 13,
                    ),
                    decoration: const InputDecoration(
                      hintText: 'Enter an address',
                      isDense: true,
                      contentPadding: EdgeInsets.symmetric(
                        horizontal: 10,
                        vertical: 8,
                      ),
                    ),
                    keyboardType: TextInputType.url,
                    textInputAction: TextInputAction.go,
                    readOnly: true,
                    onTap: () {
                      addressFocus.unfocus();
                      unawaited(_openAddressSheet());
                    },
                    onSubmitted: _submitUrl,
                  ),
                ),
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
                child: MorphRendererFaultGate(
                  controller: _controllerFor(tab.id),
                  active: tab.id == tabs.activeId,
                  child: MorphLoadErrorGate(
                    controller: _controllerFor(tab.id),
                    ownText: (error) => _ownErrorText(tab.id, error),
                    onOwnRetry: (url) => unawaited(_submitUrl(url)),
                    child: MorphWebView(
                      key: Key('web-view-$_profileId-${tab.id}'),
                      controller: _controllerFor(tab.id),
                      active: tab.id == tabs.activeId,
                      profileId: _profileId,
                      userScript: userScript,
                      placeholder: Center(
                        child: CircularProgressIndicator(color: colors.primary),
                      ),
                    ),
                  ),
                ),
              ),
            )
        else if (!_onStartPage)
          Center(child: CircularProgressIndicator(color: colors.primary)),
        if (_onStartPage) const MorphStartPage(),
      ],
    );
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
