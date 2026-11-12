import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/morph_home.dart';

void main() {
  test('uses the app support directory on Android and iOS', () async {
    final dir = Directory('/tmp/morph-app-support');
    for (final platform in [TargetPlatform.android, TargetPlatform.iOS]) {
      final home = await defaultMorphHome(
        platform: platform,
        applicationSupportDirectory: () async => dir,
      );
      expect(home, dir.path, reason: '$platform');
    }
  });

  test('leaves desktop home to MORPH_HOME or ~/.morph', () async {
    const desktops = [
      TargetPlatform.linux,
      TargetPlatform.macOS,
      TargetPlatform.windows,
    ];
    for (final platform in desktops) {
      final home = await defaultMorphHome(
        platform: platform,
        applicationSupportDirectory: () async =>
            Directory('/tmp/should-not-be-used'),
      );
      expect(home, isNull, reason: '$platform');
    }
  });

  test('control socket goes in the app home only on Android', () {
    const home = '/data/user/0/org.weirdbrains.morph/files';
    expect(
      controlSocketDir(home, platform: TargetPlatform.android),
      '$home/run',
    );
    for (final platform in [
      TargetPlatform.iOS,
      TargetPlatform.linux,
      TargetPlatform.macOS,
      TargetPlatform.windows,
    ]) {
      expect(controlSocketDir(home, platform: platform), isNull);
    }
    expect(controlSocketDir(null, platform: TargetPlatform.android), isNull);
  });
}
