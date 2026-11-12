import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/api.dart';
import 'package:morph/src/app.dart';
import 'package:morph/src/widgets/address_sheet.dart';
import 'package:morph/src/shell.dart';
import 'package:morph/src/theme/morph_theme.dart';
import 'package:morph/src/widgets/start_page.dart';
import 'package:morph_webview/morph_webview.dart';

class FakeMorphApi implements MorphApi {
  @override
  String coreVersion() => '0.1.0';

  @override
  Future<IdentityStatusView> identityDirInfo({String? home}) async {
    return IdentityStatusView(
      home: home ?? '/tmp/morph-fake',
      homeExists: false,
      profile: 'weirdnet/keys/v1',
    );
  }

  @override
  Stream<int> eventTicks() => const Stream<int>.empty();

  @override
  Future<EphemeralUser> newEphemeralUser() async {
    return const EphemeralUser(
      userId: '0123456789abcdef0123456789abcdef',
      fingerprint: 'abcd 1234',
    );
  }

  @override
  Future<String> handlePageMessage({required String json}) async {
    return '{"id":"1","type":"pong"}';
  }

  String tabPosition = 'top';

  @override
  Future<String> loadTabPosition({String? home}) async => tabPosition;

  @override
  Future<void> saveTabPosition(String position, {String? home}) async {
    tabPosition = position;
  }
}

/// Wait until identity info is on screen. Do not wait for `ready`: that flag
/// also covers the page-bridge asset, which can stall in the fake async zone.
Future<void> _pumpUntilIdentity(WidgetTester tester) async {
  await tester.pump();
  await tester.pump();
  final state = tester.state<MorphShellState>(find.byType(MorphShell));
  expect(
    state.coreVersionLabel,
    isNotNull,
    reason: 'shell failed to load identity: ${state.error}',
  );
}

Future<void> _pumpUntilWebViewAttached(
  WidgetTester tester,
  FakeMorphWebViewPlatform fake,
) async {
  for (var i = 0; i < 25; i++) {
    if (fake.lastSession != null) {
      await tester.pump();
      return;
    }
    await tester.runAsync(() async {
      await Future<void>.delayed(const Duration(milliseconds: 50));
    });
    await tester.pump();
  }
  final state = tester.state<MorphShellState>(find.byType(MorphShell));
  fail('web view did not attach; ready=${state.ready} error=${state.error}');
}

Future<void> _enterAndSubmitUrl(WidgetTester tester, String url) async {
  final mobile =
      debugDefaultTargetPlatformOverride == TargetPlatform.android ||
      debugDefaultTargetPlatformOverride == TargetPlatform.iOS;
  if (mobile) {
    await tester.tap(find.byKey(const Key('address-bar')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('address-sheet')), findsOneWidget);
    await tester.enterText(find.byKey(const Key('address-sheet-url')), url);
    await tester.testTextInput.receiveAction(TextInputAction.go);
    await tester.pumpAndSettle();
  } else {
    await tester.enterText(find.byKey(const Key('address-bar')), url);
    await tester.testTextInput.receiveAction(TextInputAction.go);
    await tester.pump();
  }
}

void main() {
  late FakeMorphWebViewPlatform fakeWebView;

  setUp(() {
    fakeWebView = FakeMorphWebViewPlatform();
    MorphWebViewPlatform.instance = fakeWebView;
  });

  testWidgets('shell shows the address bar and core version', (tester) async {
    const missingHome = '/tmp/morph-does-not-exist';
    final api = FakeMorphApi();
    await tester.pumpWidget(
      MaterialApp(
        theme: MorphTheme.dark,
        home: MorphShell(
          api: api,
          home: missingHome,
          subscribeToTicks: false,
          showDebugPanel: true,
        ),
      ),
    );
    await _pumpUntilIdentity(tester);

    expect(find.byKey(const Key('address-bar')), findsOneWidget);
    expect(find.byKey(const Key('start-page')), findsOneWidget);
    expect(find.byType(MorphStartPage), findsOneWidget);
    expect(find.byKey(const Key('morph-wordmark')), findsOneWidget);
    expect(find.textContaining('morph-core 0.1.0'), findsOneWidget);
    expect(find.textContaining('Home exists false'), findsOneWidget);

    await tester.tap(find.text('Ping page bridge'));
    await tester.pump();
    await tester.pump();
    expect(find.textContaining('pong'), findsOneWidget);

    await tester.tap(find.text('New ephemeral user'));
    await tester.pump();
    expect(
      find.textContaining('0123456789abcdef0123456789abcdef'),
      findsOneWidget,
    );
  });

  testWidgets('MorphApp hides the debug banner and shows branded chrome', (
    tester,
  ) async {
    const missingHome = '/tmp/morph-does-not-exist';
    await tester.pumpWidget(
      MorphApp(api: FakeMorphApi(), home: missingHome, subscribeToTicks: false),
    );
    await _pumpUntilIdentity(tester);
    expect(find.byType(CheckedModeBanner), findsNothing);
    expect(find.byType(Banner), findsNothing);
    expect(find.byKey(const Key('morph-mark')), findsOneWidget);
    expect(find.byKey(const Key('morph-wordmark')), findsOneWidget);
    expect(find.byKey(const Key('start-page')), findsOneWidget);
    expect(find.byKey(const Key('address-bar')), findsOneWidget);
  });

  testWidgets('desktop chrome shows a close button', (tester) async {
    const missingHome = '/tmp/morph-does-not-exist';
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    try {
      await tester.pumpWidget(
        MorphApp(
          api: FakeMorphApi(),
          home: missingHome,
          subscribeToTicks: false,
        ),
      );
      await _pumpUntilIdentity(tester);
      expect(find.byKey(const Key('window-close')), findsOneWidget);
      expect(find.byTooltip('Close'), findsOneWidget);
      expect(find.byKey(const Key('address-bar')), findsOneWidget);
      expect(find.byTooltip('Back'), findsOneWidget);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('address bar loads a typed URL on Android', (tester) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    const missingHome = '/tmp/morph-does-not-exist';
    try {
      await tester.pumpWidget(
        MorphApp(
          api: FakeMorphApi(),
          home: missingHome,
          subscribeToTicks: false,
          loadPageBridgeAsset: false,
        ),
      );
      await _pumpUntilIdentity(tester);

      expect(find.byKey(const Key('window-close')), findsNothing);
      expect(find.byKey(const Key('address-go')), findsNothing);
      expect(find.byKey(const Key('nav-forward')), findsNothing);
      expect(find.byKey(const Key('debug-panel')), findsNothing);
      expect(find.byKey(const Key('address-bar')), findsOneWidget);

      await _enterAndSubmitUrl(tester, 'example.com');
      await _pumpUntilWebViewAttached(tester, fakeWebView);

      expect(fakeWebView.lastSession, isNotNull);
      expect(fakeWebView.lastSession!.history, contains('https://example.com'));
      expect(find.byKey(const Key('start-page')), findsNothing);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('Go button submits the address bar', (tester) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    const missingHome = '/tmp/morph-does-not-exist';
    try {
      await tester.pumpWidget(
        MorphApp(
          api: FakeMorphApi(),
          home: missingHome,
          subscribeToTicks: false,
          loadPageBridgeAsset: false,
        ),
      );
      await _pumpUntilIdentity(tester);

      await tester.enterText(
        find.byKey(const Key('address-bar')),
        'example.org',
      );
      await tester.tap(find.byKey(const Key('address-go')));
      await tester.pump();
      await _pumpUntilWebViewAttached(tester, fakeWebView);

      expect(fakeWebView.lastSession, isNotNull);
      expect(fakeWebView.lastSession!.history, contains('https://example.org'));
      expect(find.byKey(const Key('start-page')), findsNothing);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets(
    'phone chrome hides debug chrome and keeps a wide address field',
    (tester) async {
      final view = tester.view;
      view.physicalSize = const Size(412, 915);
      view.devicePixelRatio = 1.0;
      view.padding = const FakeViewPadding(top: 44, bottom: 34);
      view.viewPadding = const FakeViewPadding(top: 44, bottom: 34);
      view.viewInsets = FakeViewPadding.zero;
      addTearDown(view.reset);

      debugDefaultTargetPlatformOverride = TargetPlatform.android;
      try {
        const missingHome = '/tmp/morph-does-not-exist';
        await tester.pumpWidget(
          MorphApp(
            api: FakeMorphApi(),
            home: missingHome,
            subscribeToTicks: false,
            loadPageBridgeAsset: false,
          ),
        );
        await _pumpUntilIdentity(tester);

        expect(find.byKey(const Key('debug-panel')), findsNothing);
        expect(find.byKey(const Key('address-go')), findsNothing);
        expect(find.byKey(const Key('nav-forward')), findsNothing);
        expect(find.byKey(const Key('nav-back')), findsNothing);
        expect(find.byKey(const Key('nav-reload')), findsNothing);
        expect(find.byKey(const Key('mobile-gestures')), findsOneWidget);
        expect(find.text('Ping page bridge'), findsNothing);
        expect(find.textContaining('Error: no home directory'), findsNothing);

        final address = tester.getRect(find.byKey(const Key('address-bar')));
        expect(address.top, greaterThanOrEqualTo(44));
        expect(address.width, greaterThan(200));

        await tester.longPress(find.byKey(const Key('debug-toggle')));
        await tester.pumpAndSettle();
        expect(find.byKey(const Key('debug-panel')), findsOneWidget);
        expect(find.text('Ping page bridge'), findsOneWidget);
        final panel = tester.getRect(find.byKey(const Key('debug-panel')));
        expect(panel.bottom, lessThanOrEqualTo(915 - 34 + 0.5));
      } finally {
        debugDefaultTargetPlatformOverride = null;
      }
    },
  );

  testWidgets('tapping the URL field selects all text on phone and desktop', (
    tester,
  ) async {
    const missingHome = '/tmp/morph-does-not-exist';
    for (final platform in [
      TargetPlatform.android,
      TargetPlatform.iOS,
      TargetPlatform.linux,
      TargetPlatform.macOS,
      TargetPlatform.windows,
    ]) {
      debugDefaultTargetPlatformOverride = platform;
      try {
        await tester.pumpWidget(
          MorphApp(
            api: FakeMorphApi(),
            home: missingHome,
            subscribeToTicks: false,
            loadPageBridgeAsset: false,
          ),
        );
        await _pumpUntilIdentity(tester);

        const url = 'https://example.com/path';
        final mobile =
            platform == TargetPlatform.android ||
            platform == TargetPlatform.iOS;
        if (mobile) {
          await tester.tap(find.byKey(const Key('address-bar')));
          await tester.pumpAndSettle();
          expect(find.byKey(const Key('address-sheet')), findsOneWidget);
          await tester.enterText(
            find.byKey(const Key('address-sheet-url')),
            url,
          );
          await tester.tap(find.byKey(const Key('address-sheet-url')));
          await tester.pump();
          final field = tester.widget<TextField>(
            find.byKey(const Key('address-sheet-url')),
          );
          expect(
            field.controller!.value.selection,
            const TextSelection(baseOffset: 0, extentOffset: url.length),
            reason: '$platform',
          );
        } else {
          await tester.enterText(find.byKey(const Key('address-bar')), url);
          await tester.tap(find.byKey(const Key('address-bar')));
          await tester.pump();
          final field = tester.widget<TextField>(
            find.byKey(const Key('address-bar')),
          );
          expect(
            field.controller!.value.selection,
            const TextSelection(baseOffset: 0, extentOffset: url.length),
            reason: '$platform',
          );
        }
      } finally {
        debugDefaultTargetPlatformOverride = null;
        await tester.pumpWidget(const SizedBox.shrink());
        await tester.pump();
      }
    }
  });

  testWidgets('phone edge swipe goes back and pull-to-refresh reloads', (
    tester,
  ) async {
    final view = tester.view;
    view.physicalSize = const Size(412, 915);
    view.devicePixelRatio = 1.0;
    addTearDown(view.reset);

    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    const missingHome = '/tmp/morph-does-not-exist';
    try {
      await tester.pumpWidget(
        MorphApp(
          api: FakeMorphApi(),
          home: missingHome,
          subscribeToTicks: false,
          loadPageBridgeAsset: false,
        ),
      );
      await _pumpUntilIdentity(tester);

      await _enterAndSubmitUrl(tester, 'example.com');
      await _pumpUntilWebViewAttached(tester, fakeWebView);
      expect(fakeWebView.lastSession, isNotNull);

      // Walk history so back is available: load a second page.
      await _enterAndSubmitUrl(tester, 'example.org');
      await _pumpUntilWebViewAttached(tester, fakeWebView);

      final session = fakeWebView.lastSession!;
      expect(session.history, contains('https://example.org'));

      // Edge-swipe back from the left.
      final gestures = tester.getRect(find.byKey(const Key('mobile-gestures')));
      await tester.dragFrom(
        Offset(gestures.left + 8, gestures.center.dy),
        const Offset(80, 0),
      );
      await tester.pumpAndSettle();
      expect(session.history[session.historyIndex], 'https://example.com');

      // Pull to refresh from the top strip (inside the content, past the edges).
      final reloads = session.reloads;
      await tester.dragFrom(
        Offset(gestures.center.dx, gestures.top + 20),
        const Offset(0, 80),
      );
      await tester.pumpAndSettle();
      expect(session.reloads, reloads + 1);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('phone address sheet has Firefox-style actions and coming soon', (
    tester,
  ) async {
    final view = tester.view;
    view.physicalSize = const Size(412, 915);
    view.devicePixelRatio = 1.0;
    addTearDown(view.reset);

    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    const missingHome = '/tmp/morph-does-not-exist';
    try {
      await tester.pumpWidget(
        MorphApp(
          api: FakeMorphApi(),
          home: missingHome,
          subscribeToTicks: false,
          loadPageBridgeAsset: false,
        ),
      );
      await _pumpUntilIdentity(tester);

      await tester.tap(find.byKey(const Key('address-bar')));
      await tester.pumpAndSettle();

      expect(find.byKey(const Key('address-sheet')), findsOneWidget);
      expect(find.byKey(const Key('address-sheet-url')), findsOneWidget);
      expect(find.byKey(const Key('sheet-back')), findsOneWidget);
      expect(find.byKey(const Key('sheet-forward')), findsOneWidget);
      expect(find.byKey(const Key('sheet-share')), findsOneWidget);
      expect(find.byKey(const Key('sheet-reload')), findsOneWidget);
      expect(find.byKey(const Key('sheet-desktop')), findsOneWidget);
      expect(find.text('VPN'), findsNothing);

      await tester.tap(find.byKey(const Key('sheet-history')));
      await tester.pump();
      expect(find.text('History is coming soon'), findsOneWidget);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });
}
