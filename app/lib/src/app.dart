import 'package:flutter/material.dart';

import 'api.dart';
import 'shell.dart';
import 'theme/morph_theme.dart';

class MorphApp extends StatelessWidget {
  const MorphApp({
    super.key,
    required this.api,
    this.home,
    this.subscribeToTicks = true,
  });

  final MorphApi api;
  final String? home;
  final bool subscribeToTicks;

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Morph',
      debugShowCheckedModeBanner: false,
      theme: MorphTheme.light,
      darkTheme: MorphTheme.dark,
      themeMode: ThemeMode.dark,
      home: MorphShell(
        api: api,
        home: home,
        subscribeToTicks: subscribeToTicks,
      ),
    );
  }
}
