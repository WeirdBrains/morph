import 'package:flutter/material.dart';

import 'api.dart';
import 'shell.dart';
import 'theme/morph_theme.dart';
import 'widgets/window_chrome.dart';
import 'window/morph_desktop_window.dart';

class MorphApp extends StatefulWidget {
  const MorphApp({
    super.key,
    required this.api,
    this.home,
    this.subscribeToTicks = true,
    this.loadPageBridgeAsset = true,
  });

  final MorphApi api;
  final String? home;
  final bool subscribeToTicks;
  final bool loadPageBridgeAsset;

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
      builder: (context, child) {
        return MorphDesktopFrame(child: child ?? const SizedBox.shrink());
      },
      home: MorphShell(
        api: widget.api,
        home: widget.home,
        subscribeToTicks: widget.subscribeToTicks,
        loadPageBridgeAsset: widget.loadPageBridgeAsset,
      ),
    );
  }
}
