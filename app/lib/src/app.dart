import 'package:flutter/material.dart';

import 'api.dart';
import 'shell.dart';
import 'theme/morph_theme.dart';
import 'widgets/window_chrome.dart';
import 'window/morph_desktop_window.dart';
import 'surface_watch.dart';

class MorphApp extends StatefulWidget {
  const MorphApp({
    super.key,
    required this.api,
    this.home,
    this.subscribeToTicks = true,
    this.loadPageBridgeAsset = true,
    this.showDebugPanel = false,
  });

  final MorphApi api;
  final String? home;
  final bool subscribeToTicks;
  final bool loadPageBridgeAsset;
  final bool showDebugPanel;

  @override
  State<MorphApp> createState() => _MorphAppState();
}

class _MorphAppState extends State<MorphApp> {
  @override
  void initState() {
    super.initState();
    MorphDesktopWindow.applyChromeless();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      MorphDesktopWindow.applyChromeless();
    });
  }

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Morph',
      debugShowCheckedModeBanner: false,
      theme: MorphTheme.light,
      darkTheme: MorphTheme.dark,
      themeMode: ThemeMode.dark,
      navigatorObservers: [morphSurfaceWatch],
      builder: (context, child) {
        return MorphDesktopFrame(child: child ?? const SizedBox.shrink());
      },
      home: MorphShell(
        api: widget.api,
        home: widget.home,
        subscribeToTicks: widget.subscribeToTicks,
        loadPageBridgeAsset: widget.loadPageBridgeAsset,
        showDebugPanel: widget.showDebugPanel,
      ),
    );
  }
}
