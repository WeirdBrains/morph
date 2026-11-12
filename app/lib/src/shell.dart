import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:morph_webview/morph_webview.dart';

import 'api.dart';
import 'host_friendly_url.dart';
export 'host_friendly_url.dart';
import 'theme/morph_theme.dart';
import 'tabs/tab_position.dart';
import 'tabs/tab_set.dart';
import 'widgets/brand.dart';
import 'widgets/desktop_chrome.dart';
import 'widgets/start_page.dart';
import 'widgets/window_chrome.dart';
import 'window/morph_desktop_window.dart';
import 'window/window_buttons.dart';

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

class MorphShellState extends State<MorphShell> {
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

  void _onTabNav(int id, MorphNavigationEvent event) {
    if (!mounted) {
      return;
    }
    _navEvents[id] = event;
    if (event.url.isNotEmpty && event.url != 'about:blank') {
      tabs.update(id, url: event.url, title: event.title);
      _visited[id] = true;
    } else if (event.title.isNotEmpty) {
      tabs.update(id, title: event.title);
    }
    if (id != tabs.activeId) {
      setState(() {});
      return;
    }
    setState(() {
      navigation = event;
      if (event.url.isNotEmpty && event.url != 'about:blank') {
        _canonicalAddress = event.url;
        _applyAddressDisplay();
      }
    });
  }

  @override
  void initState() {
    super.initState();
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
        if (mounted) {
          setState(() => _tabPosition = position);
        }
      } catch (_) {
        // A missing or unreadable preference stays at the default (top).
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

  bool get _onStartPage {
    if (!(_visited[tabs.activeId] ?? false)) {
      return true;
    }
    final url = navigation.url.isEmpty ? address.text : navigation.url;
    return url.isEmpty || url == 'about:blank';
  }

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
  }

  @override
  void dispose() {
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

  void _openTab() {
    final id = tabs.open();
    _controllerFor(id);
    _visited[id] = false;
    navigation = const MorphNavigationEvent(url: '');
    _canonicalAddress = '';
    address.clear();
    setState(() {});
  }

  void _closeTab(int id) {
    final controller = _controllers.remove(id);
    unawaited(_navSubs.remove(id)?.cancel());
    unawaited(_pageSubs.remove(id)?.cancel());
    unawaited(controller?.dispose());
    _visited.remove(id);
    _navEvents.remove(id);
    tabs.close(id);
    _controllerFor(tabs.activeId);
    _syncAddressFromActive();
    setState(() {});
  }

  void _switchTab(int id) {
    if (id == tabs.activeId) {
      return;
    }
    tabs.switchTo(id);
    _controllerFor(id);
    _syncAddressFromActive();
    setState(() {});
  }

  void _syncAddressFromActive() {
    final tab = tabs.active;
    navigation = _navEvents[tab.id] ?? MorphNavigationEvent(url: tab.url);
    _canonicalAddress = tab.url;
    _applyAddressDisplay();
  }

  Future<void> _setTabPosition(MorphTabPosition position) async {
    setState(() => _tabPosition = position);
    try {
      await widget.api.saveTabPosition(position.key, home: _home);
    } catch (_) {
      // The in-memory choice still applies if persistence fails.
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
                  onToggleSidebar: () {
                    setState(() => _sidebarCollapsed = !_sidebarCollapsed);
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

    return CallbackShortcuts(
      bindings: {
        const SingleActivator(LogicalKeyboardKey.keyT, control: true): _openTab,
        const SingleActivator(LogicalKeyboardKey.keyT, meta: true): _openTab,
        const SingleActivator(LogicalKeyboardKey.keyW, control: true): () {
          _closeTab(tabs.activeId);
        },
        const SingleActivator(LogicalKeyboardKey.keyW, meta: true): () {
          _closeTab(tabs.activeId);
        },
        const SingleActivator(LogicalKeyboardKey.tab, control: true): () {
          _controllerFor(tabs.cycleNext());
          _syncAddressFromActive();
          setState(() {});
        },
        const SingleActivator(
          LogicalKeyboardKey.tab,
          control: true,
          shift: true,
        ): () {
          _controllerFor(tabs.cyclePrev());
          _syncAddressFromActive();
          setState(() {});
        },
      },
      child: Focus(
        autofocus: true,
        child: Scaffold(
          resizeToAvoidBottomInset: true,
          body: SafeArea(top: false, bottom: false, child: body),
        ),
      ),
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
                IconButton(
                  key: const Key('nav-back'),
                  style: iconStyle,
                  onPressed: navigation.canGoBack ? webView.goBack : null,
                  tooltip: 'Back',
                  icon: const Icon(Icons.arrow_back),
                ),
                if (!compact || navigation.canGoForward)
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
                    onTap: _selectAllAddress,
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
    return Stack(
      fit: StackFit.expand,
      children: [
        if (ready)
          for (final tab in tabs.tabs)
            Offstage(
              offstage: tab.id != tabs.activeId,
              child: TickerMode(
                enabled: tab.id == tabs.activeId,
                child: MorphWebView(
                  key: Key('web-view-${tab.id}'),
                  controller: _controllerFor(tab.id),
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
