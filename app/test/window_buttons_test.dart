import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/window/window_buttons.dart';
import 'package:morph/src/widgets/window_chrome.dart';
import 'package:morph/src/theme/morph_theme.dart';

void main() {
  group('MorphWindowChrome.mode', () {
    const cases =
        <(String, TargetPlatform, Map<String, String>, MorphWindowButtonMode)>[
          (
            'Hyprland desktop and signature',
            TargetPlatform.linux,
            {
              'XDG_CURRENT_DESKTOP': 'Hyprland',
              'HYPRLAND_INSTANCE_SIGNATURE': 'abc',
            },
            MorphWindowButtonMode.closeOnly,
          ),
          (
            'Sway socket',
            TargetPlatform.linux,
            {
              'XDG_CURRENT_DESKTOP': 'sway',
              'SWAYSOCK': '/run/user/1000/sway-ipc.sock',
            },
            MorphWindowButtonMode.closeOnly,
          ),
          (
            'i3 socket and session',
            TargetPlatform.linux,
            {'DESKTOP_SESSION': 'i3', 'I3SOCK': '/tmp/i3'},
            MorphWindowButtonMode.closeOnly,
          ),
          (
            'niri',
            TargetPlatform.linux,
            {'XDG_CURRENT_DESKTOP': 'niri', 'NIRI_SOCKET': '/tmp/niri'},
            MorphWindowButtonMode.closeOnly,
          ),
          (
            'river',
            TargetPlatform.linux,
            {'XDG_CURRENT_DESKTOP': 'river'},
            MorphWindowButtonMode.closeOnly,
          ),
          (
            'awesome',
            TargetPlatform.linux,
            {'XDG_CURRENT_DESKTOP': 'awesome'},
            MorphWindowButtonMode.closeOnly,
          ),
          (
            'bspwm marker',
            TargetPlatform.linux,
            {'BSPWM_SOCKET': '/tmp/bspwm', 'XDG_CURRENT_DESKTOP': 'bspwm'},
            MorphWindowButtonMode.closeOnly,
          ),
          (
            'dwm',
            TargetPlatform.linux,
            {'XDG_CURRENT_DESKTOP': 'dwm'},
            MorphWindowButtonMode.closeOnly,
          ),
          (
            'qtile',
            TargetPlatform.linux,
            {'XDG_CURRENT_DESKTOP': 'qtile'},
            MorphWindowButtonMode.closeOnly,
          ),
          (
            'xmonad',
            TargetPlatform.linux,
            {'XDG_CURRENT_DESKTOP': 'xmonad'},
            MorphWindowButtonMode.closeOnly,
          ),
          (
            'ubuntu GNOME',
            TargetPlatform.linux,
            {'XDG_CURRENT_DESKTOP': 'ubuntu:GNOME'},
            MorphWindowButtonMode.all,
          ),
          (
            'KDE plasma',
            TargetPlatform.linux,
            {'XDG_CURRENT_DESKTOP': 'KDE', 'XDG_SESSION_DESKTOP': 'plasma'},
            MorphWindowButtonMode.all,
          ),
          (
            'XFCE',
            TargetPlatform.linux,
            {'XDG_CURRENT_DESKTOP': 'XFCE'},
            MorphWindowButtonMode.all,
          ),
          (
            'Cinnamon',
            TargetPlatform.linux,
            {'XDG_CURRENT_DESKTOP': 'X-Cinnamon'},
            MorphWindowButtonMode.all,
          ),
          (
            'empty linux session is all',
            TargetPlatform.linux,
            {},
            MorphWindowButtonMode.all,
          ),
          (
            'Windows defaults to all',
            TargetPlatform.windows,
            {},
            MorphWindowButtonMode.all,
          ),
          (
            'macOS defaults to all',
            TargetPlatform.macOS,
            {},
            MorphWindowButtonMode.all,
          ),
          (
            'override close on GNOME',
            TargetPlatform.linux,
            {'XDG_CURRENT_DESKTOP': 'GNOME', 'MORPH_WINDOW_BUTTONS': 'close'},
            MorphWindowButtonMode.closeOnly,
          ),
          (
            'override all on Hyprland',
            TargetPlatform.linux,
            {
              'XDG_CURRENT_DESKTOP': 'Hyprland',
              'HYPRLAND_INSTANCE_SIGNATURE': 'abc',
              'MORPH_WINDOW_BUTTONS': 'all',
            },
            MorphWindowButtonMode.all,
          ),
          (
            'invalid override is ignored',
            TargetPlatform.linux,
            {'XDG_CURRENT_DESKTOP': 'GNOME', 'MORPH_WINDOW_BUTTONS': 'foo'},
            MorphWindowButtonMode.all,
          ),
          (
            'Hyprland marker without XDG',
            TargetPlatform.linux,
            {'HYPRLAND_INSTANCE_SIGNATURE': 'abc'},
            MorphWindowButtonMode.closeOnly,
          ),
          (
            'override close on Windows',
            TargetPlatform.windows,
            {'MORPH_WINDOW_BUTTONS': 'close'},
            MorphWindowButtonMode.closeOnly,
          ),
        ];

    for (final testCase in cases) {
      final (name, platform, env, expected) = testCase;
      test(name, () {
        expect(
          MorphWindowChrome.mode(platform: platform, environment: env),
          expected,
        );
      });
    }
  });

  test('leading inset is 78 on macOS and 0 elsewhere', () {
    expect(
      MorphWindowChrome.leadingInset(platform: TargetPlatform.macOS),
      MorphWindowChrome.macTrafficLightFallbackInset,
    );
    expect(
      MorphWindowChrome.leadingInset(
        platform: TargetPlatform.macOS,
        measuredTrafficLightRight: 62,
      ),
      62 + MorphWindowChrome.macTrafficLightGap,
    );
    expect(
      MorphWindowChrome.leadingInset(
        platform: TargetPlatform.macOS,
        useNativeMacButtons: false,
      ),
      0,
    );
    expect(MorphWindowChrome.leadingInset(platform: TargetPlatform.linux), 0);
    expect(MorphWindowChrome.leadingInset(platform: TargetPlatform.windows), 0);
  });

  testWidgets('all-buttons layout shows minimize, maximize, and close', (
    tester,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        theme: MorphTheme.dark,
        home: const Scaffold(
          body: MorphWindowButtonsBar(mode: MorphWindowButtonMode.all),
        ),
      ),
    );
    expect(find.byKey(const Key('window-minimize')), findsOneWidget);
    expect(find.byKey(const Key('window-maximize')), findsOneWidget);
    expect(find.byKey(const Key('window-close')), findsOneWidget);
    expect(
      tester.getTopLeft(find.byKey(const Key('window-minimize'))).dx,
      lessThan(tester.getTopLeft(find.byKey(const Key('window-maximize'))).dx),
    );
    expect(
      tester.getTopLeft(find.byKey(const Key('window-maximize'))).dx,
      lessThan(tester.getTopLeft(find.byKey(const Key('window-close'))).dx),
    );
  });

  testWidgets('close-only layout hides minimize and maximize', (tester) async {
    await tester.pumpWidget(
      MaterialApp(
        theme: MorphTheme.dark,
        home: const Scaffold(
          body: MorphWindowButtonsBar(mode: MorphWindowButtonMode.closeOnly),
        ),
      ),
    );
    expect(find.byKey(const Key('window-close')), findsOneWidget);
    expect(find.byKey(const Key('window-minimize')), findsNothing);
    expect(find.byKey(const Key('window-maximize')), findsNothing);
  });
}
