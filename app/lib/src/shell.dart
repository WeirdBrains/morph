import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:morph_webview/morph_webview.dart';

import 'api.dart';

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
  final TextEditingController address = TextEditingController(
    text: 'https://example.com',
  );
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

  @override
  void initState() {
    super.initState();
    webView = MorphWebViewController(initialUrl: address.text);
    _navSub = webView.navigation.listen((event) {
      if (!mounted) {
        return;
      }
      final previousUrl = navigation.url;
      setState(() {
        navigation = event;
        if (event.url.isNotEmpty && event.url != previousUrl) {
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
        coreVersionLabel = version;
        identityHome = info.home;
        homeExists = info.homeExists;
        userScript = script;
        ready = true;
        error = null;
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

  Future<void> _submitUrl(String raw) async {
    var url = raw.trim();
    if (url.isEmpty) {
      return;
    }
    if (!url.contains('://')) {
      url = 'https://$url';
      address.text = url;
    }
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
    return Scaffold(
      appBar: AppBar(title: const Text('Morph')),
      body: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              children: [
                IconButton(
                  onPressed: navigation.canGoBack ? webView.goBack : null,
                  tooltip: 'Back',
                  icon: const Icon(Icons.arrow_back),
                ),
                IconButton(
                  onPressed: navigation.canGoForward ? webView.goForward : null,
                  tooltip: 'Forward',
                  icon: const Icon(Icons.arrow_forward),
                ),
                IconButton(
                  onPressed: webView.isAttached ? webView.reload : null,
                  tooltip: 'Reload',
                  icon: const Icon(Icons.refresh),
                ),
                Expanded(
                  child: TextField(
                    key: const Key('address-bar'),
                    controller: address,
                    decoration: const InputDecoration(
                      border: OutlineInputBorder(),
                      labelText: 'Address',
                    ),
                    keyboardType: TextInputType.url,
                    textInputAction: TextInputAction.go,
                    onSubmitted: _submitUrl,
                  ),
                ),
              ],
            ),
            const SizedBox(height: 8),
            if (error != null) Text('Error: $error'),
            Text('morph-core ${coreVersionLabel ?? "…"}'),
            Text('Tick ${tick ?? "…"}'),
            Text('Home ${identityHome ?? "…"}'),
            if (homeExists != null) Text('Home exists $homeExists'),
            Text('Render ${webView.renderPath}'),
            if (userId != null) Text('User $userId'),
            if (fingerprint != null) Text('Fingerprint $fingerprint'),
            if (pageReply != null) Text('Page $pageReply'),
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
            const SizedBox(height: 8),
            Expanded(
              child: ready
                  ? MorphWebView(
                      key: const Key('web-view'),
                      controller: webView,
                      userScript: userScript,
                      placeholder: const Center(child: Text('Loading page…')),
                    )
                  : const Center(child: CircularProgressIndicator()),
            ),
          ],
        ),
      ),
    );
  }
}
