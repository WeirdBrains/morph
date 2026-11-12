import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:morph_webview/morph_webview.dart';

import 'api.dart';
import 'theme/morph_theme.dart';
import 'widgets/brand.dart';
import 'widgets/start_page.dart';

/// Morph shell: address-bar chrome, Linux web-view texture, and Rust wiring.
class MorphShell extends StatefulWidget {
  const MorphShell({
    super.key,
    required this.api,
    this.home,
    this.subscribeToTicks = true,
  });

  final MorphApi api;

  /// Widget tests turn this off so a live tick stream cannot stall the tester.
  final bool subscribeToTicks;

  /// Override for tests and `MORPH_HOME`. Null falls back to the environment,
  /// then to Rust's default (`~/.morph`) without creating it.
  final String? home;

  @override
  State<MorphShell> createState() => MorphShellState();
}

class MorphShellState extends State<MorphShell> {
  final TextEditingController address = TextEditingController();
  late final MorphWebViewController webView;
  String? coreVersionLabel;
  String? identityHome;
  bool? homeExists;
  String? userId;
  String? fingerprint;
  String? pageReply;
  int? tick;
  StreamSubscription<int>? _ticks;
  StreamSubscription<MorphNavigationEvent>? _navSub;
  StreamSubscription<String>? _pageSub;
  MorphNavigationEvent navigation = const MorphNavigationEvent(url: '');
  String userScript = '';
  bool ready = false;
  Object? error;
  bool _visitedPage = false;

  @override
  void initState() {
    super.initState();
    webView = MorphWebViewController(initialUrl: 'about:blank');
    _navSub = webView.navigation.listen((event) {
      if (!mounted) {
        return;
      }
      final previousUrl = navigation.url;
      setState(() {
        navigation = event;
        if (event.url.isNotEmpty &&
            event.url != previousUrl &&
            event.url != 'about:blank') {
          address.value = address.value.copyWith(
            text: event.url,
            selection: TextSelection.collapsed(offset: event.url.length),
          );
        }
      });
    });
    _pageSub = webView.pageMessages.listen(_onPageMessage);
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
      var script = '';
      try {
        script = await rootBundle.loadString('assets/morph-bridge.js');
      } catch (_) {
        // Tests or a missing asset still get the native morphRust shim.
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
    if (!_visitedPage) {
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
      address.text = url;
    }
    setState(() => _visitedPage = true);
    await webView.loadUrl(url);
  }

  @override
  void dispose() {
    unawaited(_ticks?.cancel());
    unawaited(_navSub?.cancel());
    unawaited(_pageSub?.cancel());
    unawaited(webView.dispose());
    address.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final colors = theme.colorScheme;
    final mono = MorphTheme.monoStyle(color: colors.onSurfaceVariant, size: 12);

    return Scaffold(
      body: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Material(
            color: theme.scaffoldBackgroundColor,
            child: SafeArea(
              bottom: false,
              child: Padding(
                padding: const EdgeInsets.fromLTRB(8, 8, 12, 8),
                child: Row(
                  children: [
                    const Padding(
                      padding: EdgeInsets.symmetric(horizontal: 4),
                      child: MorphMark(size: 28),
                    ),
                    IconButton(
                      onPressed: navigation.canGoBack ? webView.goBack : null,
                      tooltip: 'Back',
                      icon: const Icon(Icons.arrow_back),
                    ),
                    IconButton(
                      onPressed: navigation.canGoForward
                          ? webView.goForward
                          : null,
                      tooltip: 'Forward',
                      icon: const Icon(Icons.arrow_forward),
                    ),
                    IconButton(
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
                        style: MorphTheme.monoStyle(
                          color: colors.onSurface,
                          size: 14,
                        ),
                        decoration: const InputDecoration(
                          hintText: 'Enter an address',
                        ),
                        keyboardType: TextInputType.url,
                        textInputAction: TextInputAction.go,
                        onSubmitted: _submitUrl,
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ),
          const MorphGlitchRule(),
          if (error != null)
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
              child: Text(
                'Error: $error',
                style: MorphTheme.monoStyle(color: colors.error, size: 12),
              ),
            ),
          Expanded(
            child: Stack(
              fit: StackFit.expand,
              children: [
                if (ready)
                  MorphWebView(
                    key: const Key('web-view'),
                    controller: webView,
                    userScript: userScript,
                    placeholder: Center(
                      child: CircularProgressIndicator(color: colors.primary),
                    ),
                  )
                else if (!_onStartPage)
                  Center(
                    child: CircularProgressIndicator(color: colors.primary),
                  ),
                if (_onStartPage) const MorphStartPage(),
              ],
            ),
          ),
          Material(
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
                      Text(
                        'morph-core ${coreVersionLabel ?? "…"}',
                        style: mono,
                      ),
                      Text('Tick ${tick ?? "…"}', style: mono),
                      Text('Home ${identityHome ?? "…"}', style: mono),
                      if (homeExists != null)
                        Text('Home exists $homeExists', style: mono),
                      Text('Render ${webView.renderPath}', style: mono),
                      if (userId != null) Text('User $userId', style: mono),
                      if (fingerprint != null)
                        Text('Fingerprint $fingerprint', style: mono),
                      if (pageReply != null)
                        Text('Page $pageReply', style: mono),
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
        ],
      ),
    );
  }
}
