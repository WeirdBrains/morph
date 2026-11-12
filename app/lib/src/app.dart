import 'package:flutter/material.dart';

import 'api.dart';
import 'shell.dart';

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
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(seedColor: const Color(0xFF1A1B26)),
        useMaterial3: true,
      ),
      home: MorphShell(
        api: api,
        home: home,
        subscribeToTicks: subscribeToTicks,
      ),
    );
  }
}
