import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';

import 'api.dart';

/// First-slice Morph shell: address bar chrome plus Rust wiring checks.
/// The live web view (Linux texture backend and Android in-app web view)
/// is the rest of milestone M1.
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
  String? coreVersionLabel;
  String? identityHome;
  bool? homeExists;
  String? userId;
  String? fingerprint;
  String? pageReply;
  int? tick;
  StreamSubscription<int>? _ticks;
  Object? error;

  @override
  void initState() {
    super.initState();
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
      setState(() {
        coreVersionLabel = version;
        identityHome = info.home;
        homeExists = info.homeExists;
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

  @override
  void dispose() {
    unawaited(_ticks?.cancel());
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
                const IconButton(
                  onPressed: null,
                  tooltip: 'Back',
                  icon: Icon(Icons.arrow_back),
                ),
                const IconButton(
                  onPressed: null,
                  tooltip: 'Forward',
                  icon: Icon(Icons.arrow_forward),
                ),
                const IconButton(
                  onPressed: null,
                  tooltip: 'Reload',
                  icon: Icon(Icons.refresh),
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
                  ),
                ),
              ],
            ),
            const SizedBox(height: 16),
            if (error != null) Text('Error: $error'),
            Text('morph-core ${coreVersionLabel ?? "…"}'),
            Text('Tick ${tick ?? "…"}'),
            Text('Home ${identityHome ?? "…"}'),
            if (homeExists != null) Text('Home exists $homeExists'),
            if (userId != null) Text('User $userId'),
            if (fingerprint != null) Text('Fingerprint $fingerprint'),
            if (pageReply != null) Text('Page $pageReply'),
            const SizedBox(height: 12),
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
    );
  }
}
