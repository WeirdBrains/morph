import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/api.dart';
import 'package:morph/src/app.dart';
import 'package:morph/src/incoming_links.dart';
import 'package:morph/src/widgets/tab_switcher.dart';
import 'package:morph/src/shell.dart';
import 'package:morph/src/tabs/tab_position.dart';
import 'package:morph/src/theme/morph_theme.dart';
import 'package:morph/src/widgets/start_page.dart';
import 'package:morph_webview/morph_webview.dart';

import 'support/fake_morph_api.dart';

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

/// Stands in for a native platform view (Android WebView). Each mount is a new
/// native view, so a dispose here means the page would be lost.
class _NativeViewProbe extends StatefulWidget {
  const _NativeViewProbe();

  static int mounts = 0;
  static int disposals = 0;

  static void reset() {
    mounts = 0;
    disposals = 0;
  }

  @override
  State<_NativeViewProbe> createState() => _NativeViewProbeState();
}

class _NativeViewProbeState extends State<_NativeViewProbe> {
  @override
  void initState() {
    super.initState();
    _NativeViewProbe.mounts++;
  }

  @override
  void dispose() {
    _NativeViewProbe.disposals++;
    super.dispose();
  }

  @override
  Widget build(BuildContext context) =>
      const ColoredBox(key: Key('native-view-probe'), color: Color(0xFF123456));
}

/// Lets the shell's debounced surface check fire and finish.
Future<void> _settleSurface(WidgetTester tester) async {
  await tester.pumpAndSettle();
  await tester.pump(const Duration(seconds: 1));
  await tester.pump(const Duration(seconds: 1));
}

/// Runs [action] and expects it to make the shell check the visible web view
/// surface at least once.
Future<void> _expectSurfaceCheck(
  WidgetTester tester,
  String view,
  Future<void> Function() action,
) async {
  final state = tester.state<MorphShellState>(find.byType(MorphShell));
  await _settleSurface(tester);
  final before = state.surfaceCheckRequests;
  await action();
  await _settleSurface(tester);
  expect(
    state.surfaceCheckRequests,
    greaterThan(before),
    reason: 'no surface check after $view',
  );
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

  testWidgets('a restored tab opens its saved URL on a cold start', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    try {
      final api = FakeMorphApi();
      api.tabsByProfile['default'] = const ProfileTabSnapshot(
        activeId: 1,
        nextId: 2,
        tabs: [ProfileTab(id: 1, url: 'https://restored.test/', title: 'R')],
      );
      await tester.pumpWidget(
        MorphApp(
          api: api,
          home: '/tmp/morph-cold-start',
          subscribeToTicks: false,
          loadPageBridgeAsset: false,
        ),
      );
      await _pumpUntilIdentity(tester);
      await _pumpUntilWebViewAttached(tester, fakeWebView);
      expect(fakeWebView.loadedUrls, ['https://restored.test/']);
      expect(fakeWebView.lastSession!.history.last, 'https://restored.test/');
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('finished page loads are recorded in the profile history', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    try {
      final api = FakeMorphApi();
      await tester.pumpWidget(
        MorphApp(
          api: api,
          home: '/tmp/morph-history-test',
          subscribeToTicks: false,
          loadPageBridgeAsset: false,
        ),
      );
      await _pumpUntilIdentity(tester);
      await _enterAndSubmitUrl(tester, 'example.com');
      await _pumpUntilWebViewAttached(tester, fakeWebView);
      await tester.pump();
      fakeWebView.lastSession!.loadUrl('https://example.org/');
      await tester.pump();
      await tester.pump();

      final urls = [
        for (final e in api.historyByProfile['default'] ?? const []) e.url,
      ];
      expect(urls, ['https://example.org/', 'https://example.com']);
      expect(api.historyByProfile.keys, ['default']);
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
        expect(find.byKey(const Key('tab-count')), findsOneWidget);
        expect(find.byKey(const Key('tab-new')), findsOneWidget);
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

      await tester.tap(find.byKey(const Key('sheet-downloads')));
      await tester.pump();
      expect(find.text('Downloads is coming soon'), findsOneWidget);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('phone tab-count opens a two-column tab grid', (tester) async {
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

      expect(find.byKey(const Key('tab-count')), findsOneWidget);
      await tester.tap(find.byKey(const Key('tab-new')));
      await tester.pump();
      await tester.tap(find.byKey(const Key('tab-count')));
      await tester.pumpAndSettle();

      expect(find.byKey(const Key('tab-switcher')), findsOneWidget);
      expect(find.byKey(const Key('tab-switcher-grid')), findsOneWidget);
      expect(find.byType(MorphTabCard), findsNWidgets(2));
      expect(find.byKey(const Key('tab-switcher-search')), findsOneWidget);
      expect(find.byKey(const Key('tab-switcher-new')), findsOneWidget);

      final first = tester.getRect(find.byType(MorphTabCard).at(0));
      final second = tester.getRect(find.byType(MorphTabCard).at(1));
      expect(first.top, closeTo(second.top, 1));
      expect(first.right, lessThanOrEqualTo(second.left + 0.5));

      // Active card uses the Glitch cyan/magenta outline.
      final active = tester.widget<MorphTabCard>(
        find.byType(MorphTabCard).last,
      );
      expect(active.active, isTrue);

      await tester.tap(find.byType(MorphTabCard).first);
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('tab-switcher')), findsNothing);

      await tester.tap(find.byKey(const Key('tab-count')));
      await tester.pumpAndSettle();
      await tester.fling(
        find.byType(MorphTabCard).first,
        const Offset(-300, 0),
        800,
      );
      await tester.pumpAndSettle();
      expect(find.byType(MorphTabCard), findsOneWidget);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('phone tab grid keeps each tab platform web view alive', (
    tester,
  ) async {
    final view = tester.view;
    view.physicalSize = const Size(412, 915);
    view.devicePixelRatio = 1.0;
    addTearDown(view.reset);
    _NativeViewProbe.reset();
    fakeWebView = FakeMorphWebViewPlatform(
      renderPath: const MorphRenderPathInfo(
        path: 'android-webview',
        reason: 'fake',
      ),
      platformView: const _NativeViewProbe(),
    );
    MorphWebViewPlatform.instance = fakeWebView;

    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    try {
      await tester.pumpWidget(
        MorphApp(
          api: FakeMorphApi(),
          home: '/tmp/morph-does-not-exist',
          subscribeToTicks: false,
          loadPageBridgeAsset: false,
        ),
      );
      await _pumpUntilIdentity(tester);

      await _enterAndSubmitUrl(tester, 'one.example');
      await _pumpUntilWebViewAttached(tester, fakeWebView);
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('native-view-probe')), findsOneWidget);
      expect(_NativeViewProbe.mounts, 1);
      final created = fakeWebView.loadedUrls.length;
      final firstSession = fakeWebView.lastSession;
      final state = tester.state<MorphShellState>(find.byType(MorphShell));

      // Open the grid and tap the same card: the page must not be torn down.
      await tester.tap(find.byKey(const Key('tab-count')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('tab-switcher')), findsOneWidget);
      await tester.tap(find.byType(MorphTabCard).first);
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('tab-switcher')), findsNothing);
      expect(find.byKey(const Key('native-view-probe')), findsOneWidget);
      expect(_NativeViewProbe.disposals, 0);
      expect(_NativeViewProbe.mounts, 1);
      expect(fakeWebView.loadedUrls.length, created);
      expect(fakeWebView.lastSession, same(firstSession));
      expect(state.navigation.url, contains('one.example'));

      // New tab from the grid's "+", then back to the first tab via the grid.
      await tester.tap(find.byKey(const Key('tab-count')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('tab-switcher-new')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('tab-switcher')), findsNothing);
      await _enterAndSubmitUrl(tester, 'two.example');
      for (var i = 0; i < 10 && _NativeViewProbe.mounts < 2; i++) {
        await tester.runAsync(
          () => Future<void>.delayed(const Duration(milliseconds: 20)),
        );
        await tester.pump();
      }
      expect(_NativeViewProbe.mounts, 2);
      expect(state.navigation.url, contains('two.example'));

      await tester.tap(find.byKey(const Key('tab-count')));
      await tester.pumpAndSettle();
      expect(find.byType(MorphTabCard), findsNWidgets(2));
      await tester.tap(find.byType(MorphTabCard).first);
      await tester.pumpAndSettle();
      expect(state.navigation.url, contains('one.example'));

      await tester.tap(find.byKey(const Key('tab-count')));
      await tester.pumpAndSettle();
      await tester.tap(find.byType(MorphTabCard).last);
      await tester.pumpAndSettle();
      expect(state.navigation.url, contains('two.example'));

      // Android back also closes the grid without dropping the page.
      await tester.tap(find.byKey(const Key('tab-count')));
      await tester.pumpAndSettle();
      final handled = await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      expect(handled, isTrue);
      expect(find.byKey(const Key('tab-switcher')), findsNothing);

      expect(_NativeViewProbe.disposals, 0);
      expect(_NativeViewProbe.mounts, 2);
      expect(fakeWebView.loadedUrls.length, created + 1);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('switching desktop tabs restores each tab URL and page', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    final view = tester.view;
    view.physicalSize = const Size(1280, 800);
    view.devicePixelRatio = 1.0;
    addTearDown(view.reset);
    try {
      await tester.pumpWidget(
        MorphApp(
          api: FakeMorphApi(),
          home: '/tmp/morph-does-not-exist',
          subscribeToTicks: false,
          loadPageBridgeAsset: false,
        ),
      );
      await _pumpUntilIdentity(tester);

      await tester.enterText(
        find.byKey(const Key('address-bar')),
        'one.example',
      );
      await tester.tap(find.byKey(const Key('address-go')));
      await tester.pump();
      await _pumpUntilWebViewAttached(tester, fakeWebView);
      expect(find.byKey(const Key('start-page')), findsNothing);

      await tester.tap(find.byKey(const Key('tab-new')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('start-page')), findsOneWidget);
      expect(
        tester
            .widget<TextField>(find.byKey(const Key('address-bar')))
            .controller!
            .text,
        isEmpty,
      );

      await tester.enterText(
        find.byKey(const Key('address-bar')),
        'two.example',
      );
      await tester.tap(find.byKey(const Key('address-go')));
      await tester.pump();
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('start-page')), findsNothing);

      await tester.tap(find.byKey(const Key('tab-1')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('start-page')), findsNothing);
      expect(
        tester
            .widget<TextField>(find.byKey(const Key('address-bar')))
            .controller!
            .text,
        'one.example',
      );

      await tester.tap(find.byKey(const Key('tab-2')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('start-page')), findsNothing);
      expect(
        tester
            .widget<TextField>(find.byKey(const Key('address-bar')))
            .controller!
            .text,
        'two.example',
      );
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('a hidden tab drops its buffering and gets it back on switch', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    final view = tester.view;
    view.physicalSize = const Size(1280, 800);
    view.devicePixelRatio = 1.0;
    addTearDown(view.reset);
    try {
      await tester.pumpWidget(
        MorphApp(
          api: FakeMorphApi(),
          home: '/tmp/morph-does-not-exist',
          subscribeToTicks: false,
          loadPageBridgeAsset: false,
        ),
      );
      await _pumpUntilIdentity(tester);
      await tester.enterText(
        find.byKey(const Key('address-bar')),
        'one.example',
      );
      await tester.tap(find.byKey(const Key('address-go')));
      await tester.pump();
      await _pumpUntilWebViewAttached(tester, fakeWebView);
      final first = fakeWebView.lastSession!;

      await tester.tap(find.byKey(const Key('tab-new')));
      await tester.pumpAndSettle();
      await tester.enterText(
        find.byKey(const Key('address-bar')),
        'two.example',
      );
      await tester.tap(find.byKey(const Key('address-go')));
      await tester.pump();
      await tester.pumpAndSettle();
      final second = fakeWebView.lastSession!;
      expect(identical(first, second), isFalse);

      // Tab 1 is hidden, but not for long enough yet.
      await tester.pump(const Duration(seconds: 10));
      expect(first.bufferingChanges, isEmpty);
      await tester.pump(MorphWebViewController.defaultIdleAfter);
      expect(first.bufferingChanges, [MorphBuffering.idle]);
      expect(second.bufferingChanges, isEmpty, reason: 'the visible tab');

      await tester.tap(find.byKey(const Key('tab-1')));
      await tester.pump();
      expect(first.bufferingChanges, [
        MorphBuffering.idle,
        MorphBuffering.full,
      ]);
      await tester.pumpAndSettle();
      await tester.pump(MorphWebViewController.defaultIdleAfter);
      expect(second.bufferingChanges, [MorphBuffering.idle]);
      expect(first.bufferingChanges, hasLength(2));
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('a crashed tab shows its own crash state and reloads', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    final view = tester.view;
    view.physicalSize = const Size(1280, 800);
    view.devicePixelRatio = 1.0;
    addTearDown(view.reset);
    try {
      await tester.pumpWidget(
        MorphApp(
          api: FakeMorphApi(),
          home: '/tmp/morph-does-not-exist',
          subscribeToTicks: false,
          loadPageBridgeAsset: false,
        ),
      );
      await _pumpUntilIdentity(tester);
      await tester.enterText(
        find.byKey(const Key('address-bar')),
        'one.example',
      );
      await tester.tap(find.byKey(const Key('address-go')));
      await tester.pump();
      await _pumpUntilWebViewAttached(tester, fakeWebView);
      final first = fakeWebView.lastSession!;

      await tester.tap(find.byKey(const Key('tab-new')));
      await tester.pumpAndSettle();
      await tester.enterText(
        find.byKey(const Key('address-bar')),
        'two.example',
      );
      await tester.tap(find.byKey(const Key('address-go')));
      await tester.pump();
      await tester.pumpAndSettle();
      expect(fakeWebView.sessions, hasLength(2));
      final second = fakeWebView.sessions.last;

      // The visible tab's renderer crashes: only it shows the crash state.
      second.emitRenderer(MorphRendererFault.crashed);
      await tester.pump();
      await tester.pump();
      expect(find.byKey(const Key('page-crashed')), findsOneWidget);
      expect(find.text('This page crashed'), findsOneWidget);
      // The shell keeps working: switch to the other tab, which is fine.
      await tester.tap(find.byKey(const Key('tab-1')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('page-crashed')), findsNothing);
      await tester.tap(find.byKey(const Key('tab-2')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('page-crashed')), findsOneWidget);

      await tester.tap(find.byKey(const Key('page-crashed-reload')));
      await tester.pump();
      await tester.pump();
      expect(second.rendererRecoveries, 1);
      expect(first.rendererRecoveries, 0);
      expect(find.byKey(const Key('page-crashed')), findsNothing);

      // A hang offers Wait; waiting hides it, answering again clears it.
      // Only after the grace: heavy pages block the renderer for seconds.
      second.emitRenderer(MorphRendererFault.unresponsive);
      await tester.pump();
      await tester.pump(const Duration(seconds: 2));
      expect(find.text("This page isn't responding"), findsNothing);
      await tester.pump(MorphWebViewController.defaultUnresponsiveGrace);
      await tester.pump();
      expect(find.text("This page isn't responding"), findsOneWidget);
      await tester.tap(find.byKey(const Key('page-crashed-wait')));
      await tester.pump();
      expect(find.byKey(const Key('page-crashed')), findsNothing);
      second.emitRenderer(null);
      await tester.pump();

      // Submitting an address in a crashed tab recovers it and goes there.
      second.emitRenderer(MorphRendererFault.crashed);
      await tester.pump();
      await tester.pump();
      expect(find.byKey(const Key('page-crashed')), findsOneWidget);
      await tester.enterText(
        find.byKey(const Key('address-bar')),
        'three.example',
      );
      await tester.testTextInput.receiveAction(TextInputAction.go);
      await tester.pumpAndSettle();
      expect(second.rendererRecoveries, 2);
      expect(second.history.last, 'https://three.example');
      expect(find.byKey(const Key('page-crashed')), findsNothing);

      // A background tab whose renderer died reloads when it is shown.
      first.emitRenderer(MorphRendererFault.killed);
      await tester.pump();
      await tester.pump();
      expect(find.byKey(const Key('page-crashed')), findsNothing);
      await tester.tap(find.byKey(const Key('tab-1')));
      await tester.pumpAndSettle();
      expect(first.rendererRecoveries, 1);
      expect(find.byKey(const Key('page-crashed')), findsNothing);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets(
    'desktop profile button sits at the right of the nav row under the window buttons',
    (tester) async {
      const missingHome = '/tmp/morph-does-not-exist';
      for (final platform in [
        TargetPlatform.linux,
        TargetPlatform.macOS,
        TargetPlatform.windows,
      ]) {
        debugDefaultTargetPlatformOverride = platform;
        final view = tester.view;
        view.physicalSize = const Size(1280, 800);
        view.devicePixelRatio = 1.0;
        try {
          await tester.pumpWidget(
            MorphApp(
              key: ValueKey('profile-placement-$platform'),
              api: FakeMorphApi(),
              home: missingHome,
              subscribeToTicks: false,
              loadPageBridgeAsset: false,
            ),
          );
          await _pumpUntilIdentity(tester);
          await tester.pumpAndSettle();

          for (final position in MorphTabPosition.values) {
            if (position != MorphTabPosition.top) {
              await tester.tap(find.byKey(const Key('tab-position')));
              await tester.pumpAndSettle();
              await tester.tap(find.byKey(Key('tab-position-${position.key}')));
              await tester.pumpAndSettle();
            }
            _assertProfileAtNavTrailing(tester, platform, position);
          }
        } finally {
          debugDefaultTargetPlatformOverride = null;
          view.reset();
        }
      }
    },
  );

  testWidgets('each new profile is onboarded to its own identity on desktop', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    final view = tester.view;
    view.physicalSize = const Size(1280, 1000);
    view.devicePixelRatio = 1.0;
    final api = FakeMorphApi()..onboarded = false;
    Future<void> createIdentity() async {
      expect(find.byKey(const Key('identity-onboarding')), findsOneWidget);
      await tester.tap(find.byKey(const Key('onboarding-create')));
      await tester.pumpAndSettle();
      await tester.enterText(
        find.byKey(const Key('onboarding-password')),
        'long enough',
      );
      await tester.enterText(
        find.byKey(const Key('onboarding-confirm')),
        'long enough',
      );
      await tester.tap(find.byKey(const Key('onboarding-submit')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('onboarding-saved')));
      await tester.pump();
      await tester.tap(find.byKey(const Key('onboarding-done')));
      await tester.pumpAndSettle();
    }

    try {
      await tester.pumpWidget(
        MorphApp(
          api: api,
          home: '/tmp/morph-does-not-exist',
          subscribeToTicks: false,
          loadPageBridgeAsset: false,
        ),
      );
      await _pumpUntilIdentity(tester);
      await tester.pumpAndSettle();

      // First use of the default profile asks, and the barrier cannot dismiss.
      await tester.tapAt(const Offset(4, 996));
      await tester.pumpAndSettle();
      await createIdentity();
      expect(find.byKey(const Key('identity-onboarding')), findsNothing);
      final first = api.identitiesByProfile['default']!;

      await _addNamedProfile(tester, 'Work');
      await tester.pumpAndSettle();
      await createIdentity();
      final work = api.identitiesByProfile[api.currentProfileId]!;
      expect(work.userId, isNot(first.userId));

      // Not now on a third profile, then switching back to it in the same
      // run does not nag again; switching to an onboarded one never asks.
      await _addNamedProfile(tester, 'Play');
      await tester.pumpAndSettle();
      final play = api.currentProfileId;
      await tester.tap(find.byKey(const Key('onboarding-skip')));
      await tester.pumpAndSettle();
      expect(api.identitiesByProfile.containsKey(play), isFalse);

      await tester.tap(find.byKey(const Key('profile-button')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('profile-item-default')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('identity-onboarding')), findsNothing);

      await tester.tap(find.byKey(const Key('nav-menu')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('menu-identity')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('identity-dialog')), findsOneWidget);
      expect(find.text(first.userId), findsOneWidget);
      await tester.tap(find.text('Close'));
      await tester.pumpAndSettle();

      await tester.tap(find.byKey(const Key('profile-button')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(Key('profile-item-$play')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('identity-onboarding')), findsNothing);

      // The menu still offers setup for the skipped profile.
      await tester.tap(find.byKey(const Key('nav-menu')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('menu-identity')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('identity-onboarding')), findsOneWidget);
    } finally {
      debugDefaultTargetPlatformOverride = null;
      view.reset();
    }
  });

  testWidgets('the identity dialog offers the recovery phrase and a backup', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    final view = tester.view;
    view.physicalSize = const Size(1280, 1000);
    view.devicePixelRatio = 1.0;
    final api = FakeMorphApi()..onboarded = false;
    try {
      await tester.pumpWidget(
        MorphApp(
          api: api,
          home: '/tmp/morph-does-not-exist',
          subscribeToTicks: false,
          loadPageBridgeAsset: false,
        ),
      );
      await _pumpUntilIdentity(tester);
      await tester.pumpAndSettle();
      // Onboarding offers restore from a backup file next to linking.
      expect(find.byKey(const Key('onboarding-backup')), findsOneWidget);
      expect(find.byKey(const Key('onboarding-link')), findsOneWidget);
      await tester.tap(find.byKey(const Key('onboarding-create')));
      await tester.pumpAndSettle();
      for (final key in ['onboarding-password', 'onboarding-confirm']) {
        await tester.enterText(find.byKey(Key(key)), 'long enough');
      }
      await tester.tap(find.byKey(const Key('onboarding-submit')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('onboarding-saved')));
      await tester.pump();
      await tester.tap(find.byKey(const Key('onboarding-done')));
      await tester.pumpAndSettle();

      Future<void> identityAction(String key) async {
        await tester.tap(find.byKey(const Key('nav-menu')));
        await tester.pumpAndSettle();
        await tester.tap(find.byKey(const Key('menu-identity')));
        await tester.pumpAndSettle();
        await tester.tap(find.byKey(Key(key)));
        await tester.pumpAndSettle();
      }

      await identityAction('identity-recovery-phrase');
      await tester.enterText(
        find.byKey(const Key('recovery-phrase-password')),
        'long enough',
      );
      await tester.tap(find.byKey(const Key('recovery-phrase-show')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('recovery-word-grid')), findsOneWidget);
      await tester.tap(find.byKey(const Key('recovery-phrase-close')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('recovery-phrase-dialog')), findsNothing);

      await identityAction('identity-backup');
      expect(find.byKey(const Key('identity-backup-dialog')), findsOneWidget);
      await tester.enterText(
        find.byKey(const Key('backup-profile-password')),
        'long enough',
      );
      await tester.enterText(
        find.byKey(const Key('backup-password')),
        'backup pass',
      );
      await tester.enterText(
        find.byKey(const Key('backup-confirm')),
        'backup pass',
      );
      await tester.tap(find.byKey(const Key('backup-create')));
      await tester.pumpAndSettle();
      expect(api.exportBackupCalls, 1);
      expect(find.byKey(const Key('backup-save')), findsOneWidget);
      await tester.tap(find.byKey(const Key('backup-close')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('identity-backup-dialog')), findsNothing);
    } finally {
      debugDefaultTargetPlatformOverride = null;
      view.reset();
    }
  });

  group('encrypted library unlock', () {
    Future<void> launch(WidgetTester tester, FakeMorphApi api) async {
      debugDefaultTargetPlatformOverride = TargetPlatform.linux;
      tester.view.physicalSize = const Size(1280, 1000);
      tester.view.devicePixelRatio = 1.0;
      await tester.pumpWidget(
        MorphApp(
          api: api,
          home: '/tmp/morph-does-not-exist',
          subscribeToTicks: false,
          loadPageBridgeAsset: false,
        ),
      );
      await _pumpUntilIdentity(tester);
      await tester.pumpAndSettle();
    }

    void reset(WidgetTester tester) {
      debugDefaultTargetPlatformOverride = null;
      tester.view.reset();
    }

    testWidgets('the keystore opens the library with no prompt', (
      tester,
    ) async {
      final api = FakeMorphApi();
      try {
        await launch(tester, api);
        expect(api.openLibraryCalls, ['default']);
        expect(find.byKey(const Key('library-unlock')), findsNothing);
        expect(api.unlockLibraryCalls, isEmpty);
      } finally {
        reset(tester);
      }
    });

    testWidgets('without a saved key the password unlocks once', (
      tester,
    ) async {
      final api = FakeMorphApi()
        ..libraryNeedsPassword['default'] = 'no Secret Service'
        ..libraryPasswords['default'] = 'long enough';
      try {
        await launch(tester, api);
        expect(find.byKey(const Key('library-unlock')), findsOneWidget);
        final loadsBefore = api.listBookmarksCalls;
        expect(
          find.textContaining('no Secret Service'),
          findsOneWidget,
          reason: 'the prompt says why the keyring did not help',
        );
        await tester.enterText(
          find.descendant(
            of: find.byKey(const Key('library-unlock-password')),
            matching: find.byType(TextField),
          ),
          'long enough',
        );
        await tester.tap(find.byKey(const Key('library-unlock-submit')));
        await tester.pumpAndSettle();
        expect(find.byKey(const Key('library-unlock')), findsNothing);
        expect(api.unlockLibraryCalls, ['default']);
        expect(find.byKey(const Key('library-not-remembered')), findsNothing);
        expect(
          api.listBookmarksCalls,
          greaterThan(loadsBefore),
          reason: 'bookmarks reload once the library is open',
        );
      } finally {
        reset(tester);
      }
    });

    testWidgets('a wrong password says so; Not now leaves it for this run', (
      tester,
    ) async {
      final api = FakeMorphApi()
        ..libraryNeedsPassword['default'] = null
        ..libraryPasswords['default'] = 'long enough';
      try {
        await launch(tester, api);
        expect(find.byKey(const Key('library-unlock-reason')), findsNothing);
        await tester.enterText(
          find.descendant(
            of: find.byKey(const Key('library-unlock-password')),
            matching: find.byType(TextField),
          ),
          'wrong one',
        );
        await tester.tap(find.byKey(const Key('library-unlock-submit')));
        await tester.pumpAndSettle();
        expect(find.byKey(const Key('library-unlock-error')), findsOneWidget);
        expect(find.byKey(const Key('library-unlock')), findsOneWidget);
        await tester.tap(find.byKey(const Key('library-unlock-skip')));
        await tester.pumpAndSettle();
        expect(find.byKey(const Key('library-unlock')), findsNothing);
      } finally {
        reset(tester);
      }
    });

    testWidgets('an unlock the keyring cannot keep warns it will ask again', (
      tester,
    ) async {
      final api = FakeMorphApi()
        ..libraryNeedsPassword['default'] = null
        ..libraryPasswords['default'] = 'long enough'
        ..keystoreRemembers = false;
      try {
        await launch(tester, api);
        await tester.enterText(
          find.descendant(
            of: find.byKey(const Key('library-unlock-password')),
            matching: find.byType(TextField),
          ),
          'long enough',
        );
        await tester.tap(find.byKey(const Key('library-unlock-submit')));
        await tester.pumpAndSettle();
        expect(find.byKey(const Key('library-not-remembered')), findsOneWidget);
      } finally {
        reset(tester);
      }
    });
  });

  testWidgets('desktop links a profile from another device, then adds one', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    final view = tester.view;
    view.physicalSize = const Size(1280, 1100);
    view.devicePixelRatio = 1.0;
    final api = FakeMorphApi()..onboarded = false;
    try {
      await tester.pumpWidget(
        MorphApp(
          api: api,
          home: '/tmp/morph-does-not-exist',
          subscribeToTicks: false,
          loadPageBridgeAsset: false,
        ),
      );
      await _pumpUntilIdentity(tester);
      await tester.pumpAndSettle();

      // New device: onboarding shows the code, then the received identity
      // is sealed under this device's password.
      await tester.tap(find.byKey(const Key('onboarding-link')));
      await tester.pump();
      await tester.pump();
      expect(find.byKey(const Key('link-qr')), findsOneWidget);
      expect(api.linkCalls.single, startsWith('start Morph'));
      api.linkStatus = const MorphLinkStatus(
        MorphLinkState.paired,
        sas: '042917',
      );
      await tester.pump(const Duration(milliseconds: 600));
      await tester.pump();
      await tester.tap(find.byKey(const Key('link-match')));
      await tester.pump();
      api.linkStatus = const MorphLinkStatus(
        MorphLinkState.received,
        sas: '042917',
      );
      await tester.pump(const Duration(milliseconds: 600));
      await tester.pump();
      await tester.enterText(
        find.byKey(const Key('link-password')),
        'long enough',
      );
      await tester.enterText(
        find.byKey(const Key('link-password-confirm')),
        'long enough',
      );
      await tester.tap(find.byKey(const Key('link-finish')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('identity-onboarding')), findsNothing);
      expect(api.identitiesByProfile['default']!.linked, isTrue);

      // Existing device: Identity, then Add device, opens the pairing panel.
      await tester.tap(find.byKey(const Key('nav-menu')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('menu-identity')));
      await tester.pumpAndSettle();
      expect(find.text('Linked from another device.'), findsOneWidget);
      await tester.tap(find.byKey(const Key('identity-add-device')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('add-device-panel')), findsOneWidget);
      // No camera on a desktop: paste only.
      expect(find.byKey(const Key('add-device-scan')), findsNothing);
      expect(find.byKey(const Key('add-device-link')), findsOneWidget);
    } finally {
      debugDefaultTargetPlatformOverride = null;
      view.reset();
    }
  });

  testWidgets('phone onboards the profile and shows it in the sheet', (
    tester,
  ) async {
    final view = tester.view;
    view.physicalSize = const Size(412, 915);
    view.devicePixelRatio = 1.0;
    addTearDown(view.reset);
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    final api = FakeMorphApi()..onboarded = false;
    try {
      await tester.pumpWidget(
        MorphApp(
          api: api,
          home: '/tmp/morph-does-not-exist',
          subscribeToTicks: false,
          loadPageBridgeAsset: false,
        ),
      );
      await _pumpUntilIdentity(tester);
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('identity-onboarding')), findsOneWidget);
      await tester.tap(find.byKey(const Key('onboarding-import')));
      await tester.pumpAndSettle();
      await tester.enterText(
        find.byKey(const Key('onboarding-words')),
        FakeMorphApi.fakeWords('phoneuser0000000'),
      );
      await tester.enterText(
        find.byKey(const Key('onboarding-password')),
        'long enough',
      );
      await tester.enterText(
        find.byKey(const Key('onboarding-confirm')),
        'long enough',
      );
      await tester.tap(find.byKey(const Key('onboarding-submit')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('identity-onboarding')), findsNothing);
      expect(api.identitiesByProfile['default']!.imported, isTrue);

      await tester.tap(find.byKey(const Key('address-bar')));
      await tester.pumpAndSettle();
      await tester.scrollUntilVisible(
        find.byKey(const Key('sheet-identity')),
        200,
        scrollable: find
            .descendant(
              of: find.byKey(const Key('address-sheet')),
              matching: find.byType(Scrollable),
            )
            .first,
      );
      expect(find.text('Identity phoneuser000…'), findsOneWidget);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('profiles isolate tabs and refuse to delete the last profile', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    final view = tester.view;
    view.physicalSize = const Size(1280, 800);
    view.devicePixelRatio = 1.0;
    final api = FakeMorphApi();
    try {
      await tester.pumpWidget(
        MorphApp(
          api: api,
          home: '/tmp/morph-does-not-exist',
          subscribeToTicks: false,
          loadPageBridgeAsset: false,
        ),
      );
      await _pumpUntilIdentity(tester);
      await tester.pumpAndSettle();

      await tester.tap(find.byKey(const Key('profile-button')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('profile-delete-default')), findsNothing);
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();

      await tester.enterText(
        find.byKey(const Key('address-bar')),
        'one.example',
      );
      await tester.tap(find.byKey(const Key('address-go')));
      await tester.pump();
      await _pumpUntilWebViewAttached(tester, fakeWebView);
      expect(find.byKey(const Key('start-page')), findsNothing);

      await _addNamedProfile(tester, 'Work');
      expect(api.profiles, hasLength(2));
      expect(find.byKey(const Key('start-page')), findsOneWidget);
      expect(
        tester
            .widget<TextField>(find.byKey(const Key('address-bar')))
            .controller!
            .text,
        isNot(contains('one.example')),
      );

      await _addNamedProfile(tester, 'Home');
      expect(api.profiles, hasLength(3));

      await tester.tap(find.byKey(const Key('profile-button')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('profile-item-default')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('start-page')), findsNothing);
      expect(
        tester
            .widget<TextField>(find.byKey(const Key('address-bar')))
            .controller!
            .text,
        'one.example',
      );

      await tester.tap(find.byKey(const Key('profile-button')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('profile-item-p1')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('start-page')), findsOneWidget);
      expect(
        tester
            .widget<TextField>(find.byKey(const Key('address-bar')))
            .controller!
            .text,
        isNot(contains('one.example')),
      );

      await tester.tap(find.byKey(const Key('profile-button')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('profile-delete-default')), findsOneWidget);
      await tester.tap(find.byKey(const Key('profile-delete-p2')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('profile-delete-confirm')));
      await tester.pumpAndSettle();
      expect(api.profiles, hasLength(2));
      expect(api.profiles.any((profile) => profile.id == 'p2'), isFalse);

      await tester.tap(find.byKey(const Key('profile-button')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('profile-delete-p1')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('profile-delete-confirm')));
      await tester.pumpAndSettle();
      expect(api.profiles, hasLength(1));
      expect(api.currentProfileId, 'default');

      await tester.tap(find.byKey(const Key('profile-button')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('profile-delete-default')), findsNothing);
    } finally {
      debugDefaultTargetPlatformOverride = null;
      view.reset();
    }
  });

  testWidgets(
    'phone surface checks follow real loads, not progress or same-page updates',
    (tester) async {
      final view = tester.view;
      view.physicalSize = const Size(412, 915);
      view.devicePixelRatio = 1.0;
      addTearDown(view.reset);
      _NativeViewProbe.reset();
      fakeWebView = FakeMorphWebViewPlatform(
        renderPath: const MorphRenderPathInfo(
          path: 'android-webview',
          reason: 'fake',
        ),
        platformView: const _NativeViewProbe(),
      );
      MorphWebViewPlatform.instance = fakeWebView;

      debugDefaultTargetPlatformOverride = TargetPlatform.android;
      try {
        await tester.pumpWidget(
          MorphApp(
            api: FakeMorphApi(),
            home: '/tmp/morph-does-not-exist',
            subscribeToTicks: false,
            loadPageBridgeAsset: false,
          ),
        );
        await _pumpUntilIdentity(tester);
        final state = tester.state<MorphShellState>(find.byType(MorphShell));
        await _enterAndSubmitUrl(tester, 'news.example');
        await _pumpUntilWebViewAttached(tester, fakeWebView);
        final session = fakeWebView.lastSession!;
        await _settleSurface(tester);
        final before = state.surfaceCheckRequests;
        final url = state.navigation.url;

        // What Android sends while someone reads and scrolls an article: late
        // "progress 100" reports, title changes, a same-page URL update.
        session.emitNavigation(
          MorphNavigationEvent(url: url, title: 'News', progress: 1),
        );
        session.emitNavigation(
          MorphNavigationEvent(url: '$url#part-2', title: 'News', progress: 1),
        );
        session.emitNavigation(
          MorphNavigationEvent(url: '$url#part-2', title: 'News 2'),
        );
        await _settleSurface(tester);
        expect(state.surfaceCheckRequests, before);

        // A real load, started and finished by the page (a link tap).
        session.emitNavigation(
          MorphNavigationEvent(url: url, isLoading: true, progress: 0.2),
        );
        session.emitNavigation(
          MorphNavigationEvent(url: 'https://news.example/next', progress: 1),
        );
        await _settleSurface(tester);
        expect(state.surfaceCheckRequests, before + 1);
      } finally {
        debugDefaultTargetPlatformOverride = null;
      }
    },
  );

  testWidgets('phone web view surface is checked after every view change', (
    tester,
  ) async {
    final view = tester.view;
    view.physicalSize = const Size(412, 915);
    view.devicePixelRatio = 1.0;
    addTearDown(view.reset);
    _NativeViewProbe.reset();
    fakeWebView = FakeMorphWebViewPlatform(
      renderPath: const MorphRenderPathInfo(
        path: 'android-webview',
        reason: 'fake',
      ),
      platformView: const _NativeViewProbe(),
    );
    MorphWebViewPlatform.instance = fakeWebView;

    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    try {
      await tester.pumpWidget(
        MorphApp(
          api: FakeMorphApi(),
          home: '/tmp/morph-does-not-exist',
          subscribeToTicks: false,
          loadPageBridgeAsset: false,
        ),
      );
      await _pumpUntilIdentity(tester);
      final state = tester.state<MorphShellState>(find.byType(MorphShell));

      // Michael's report: start page -> address sheet with keyboard -> enter.
      await _expectSurfaceCheck(tester, 'address sheet submit', () async {
        await _enterAndSubmitUrl(tester, 'linkedin.com');
        await _pumpUntilWebViewAttached(tester, fakeWebView);
      });
      final session = fakeWebView.lastSession!;
      expect(state.navigation.url, contains('linkedin.com'));

      await _expectSurfaceCheck(tester, 'address sheet dismissed', () async {
        await tester.tap(find.byKey(const Key('address-bar')));
        await tester.pumpAndSettle();
        expect(find.byKey(const Key('address-sheet')), findsOneWidget);
        await tester.binding.handlePopRoute();
        await tester.pumpAndSettle();
        expect(find.byKey(const Key('address-sheet')), findsNothing);
      });

      await _expectSurfaceCheck(tester, 'keyboard shown and hidden', () async {
        view.viewInsets = const FakeViewPadding(bottom: 320);
        await tester.pump();
        view.resetViewInsets();
        await tester.pump();
      });

      await _expectSurfaceCheck(tester, 'rotation', () async {
        view.physicalSize = const Size(915, 412);
        await tester.pump();
        view.physicalSize = const Size(412, 915);
        await tester.pump();
      });

      await _expectSurfaceCheck(tester, 'app resumed', () async {
        tester.binding.handleAppLifecycleStateChanged(
          AppLifecycleState.inactive,
        );
        tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
        tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
        await tester.pump();
        tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
        tester.binding.handleAppLifecycleStateChanged(
          AppLifecycleState.inactive,
        );
        tester.binding.handleAppLifecycleStateChanged(
          AppLifecycleState.resumed,
        );
        await tester.pump();
      });

      await _expectSurfaceCheck(tester, 'reload', () async {
        await state.webView.reload();
        await tester.pump();
      });

      await _expectSurfaceCheck(tester, 'second navigation', () async {
        await _enterAndSubmitUrl(tester, 'two.example');
      });

      await _expectSurfaceCheck(tester, 'back', () async {
        await state.webView.goBack();
        await tester.pump();
      });

      await _expectSurfaceCheck(tester, 'forward', () async {
        await state.webView.goForward();
        await tester.pump();
      });

      await _expectSurfaceCheck(tester, 'tab grid closed', () async {
        await tester.tap(find.byKey(const Key('tab-count')));
        await tester.pumpAndSettle();
        await tester.tap(find.byType(MorphTabCard).first);
        await tester.pumpAndSettle();
      });

      await _expectSurfaceCheck(
        tester,
        'Android back out of the grid',
        () async {
          await tester.tap(find.byKey(const Key('tab-count')));
          await tester.pumpAndSettle();
          await tester.binding.handlePopRoute();
          await tester.pumpAndSettle();
        },
      );

      // New tab from the grid, load it, then switch back to the first tab.
      await tester.tap(find.byKey(const Key('tab-count')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('tab-switcher-new')));
      await tester.pumpAndSettle();
      await _expectSurfaceCheck(tester, 'new tab first load', () async {
        await _enterAndSubmitUrl(tester, 'three.example');
        for (var i = 0; i < 10 && fakeWebView.sessions.length < 2; i++) {
          await tester.runAsync(
            () => Future<void>.delayed(const Duration(milliseconds: 20)),
          );
          await tester.pump();
        }
      });
      await _expectSurfaceCheck(tester, 'tab switch', () async {
        await tester.tap(find.byKey(const Key('tab-count')));
        await tester.pumpAndSettle();
        await tester.tap(find.byType(MorphTabCard).first);
        await tester.pumpAndSettle();
      });
      // The first tab went linkedin -> two.example, back, then forward.
      expect(state.navigation.url, contains('two.example'));

      // A blank surface with content behind it is rebuilt on the same page.
      session.surfaceVerdicts.addAll([
        MorphSurfaceVerdict.blank,
        MorphSurfaceVerdict.blank,
        MorphSurfaceVerdict.healthy,
      ]);
      await _expectSurfaceCheck(tester, 'blank surface', () async {
        tester.binding.handleAppLifecycleStateChanged(
          AppLifecycleState.resumed,
        );
        await tester.pump();
      });
      expect(session.surfaceRebuilds, 1);
      expect(state.navigation.url, contains('two.example'));

      // A dead renderer is rebuilt without waiting for a view change (after
      // the real-clock two-second guard against crash loops).
      await tester.runAsync(
        () => Future<void>.delayed(const Duration(milliseconds: 2100)),
      );
      session.emitSurfaceLost();
      await tester.pump();
      await tester.pump();
      expect(session.surfaceRebuilds, 2);

      // The page web views were never torn down by any of this.
      expect(_NativeViewProbe.disposals, 0);
      expect(_NativeViewProbe.mounts, 2);
      expect(fakeWebView.sessions, hasLength(2));
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('phone profile switch checks the restored page surface', (
    tester,
  ) async {
    final view = tester.view;
    view.physicalSize = const Size(412, 915);
    view.devicePixelRatio = 1.0;
    addTearDown(view.reset);
    fakeWebView = FakeMorphWebViewPlatform(
      renderPath: const MorphRenderPathInfo(
        path: 'android-webview',
        reason: 'fake',
      ),
      platformView: const _NativeViewProbe(),
    );
    MorphWebViewPlatform.instance = fakeWebView;
    final api = FakeMorphApi();

    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    try {
      await tester.pumpWidget(
        MorphApp(
          api: api,
          home: '/tmp/morph-does-not-exist',
          subscribeToTicks: false,
          loadPageBridgeAsset: false,
        ),
      );
      await _pumpUntilIdentity(tester);
      await _enterAndSubmitUrl(tester, 'one.example');
      await _pumpUntilWebViewAttached(tester, fakeWebView);
      await _settleSurface(tester);

      // Add a second profile from the phone address sheet (start page there).
      await tester.tap(find.byKey(const Key('address-bar')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('profile-add')));
      await tester.pumpAndSettle();
      await tester.enterText(find.byKey(const Key('profile-add-name')), 'Work');
      await tester.tap(find.byKey(const Key('profile-add-confirm')));
      await tester.pumpAndSettle();
      if (find.byKey(const Key('address-sheet')).evaluate().isNotEmpty) {
        await tester.binding.handlePopRoute();
        await tester.pumpAndSettle();
      }
      expect(api.profiles, hasLength(2));
      expect(find.byKey(const Key('start-page')), findsOneWidget);

      await _expectSurfaceCheck(tester, 'profile switch', () async {
        await tester.tap(find.byKey(const Key('address-bar')));
        await tester.pumpAndSettle();
        await tester.tap(find.byKey(const Key('profile-item-default')));
        await tester.pumpAndSettle();
        if (find.byKey(const Key('address-sheet')).evaluate().isNotEmpty) {
          await tester.binding.handlePopRoute();
          await tester.pumpAndSettle();
        }
        final before = fakeWebView.sessions.length;
        for (var i = 0; i < 20 && fakeWebView.sessions.length == before; i++) {
          await tester.runAsync(
            () => Future<void>.delayed(const Duration(milliseconds: 20)),
          );
          await tester.pump();
        }
      });
      expect(find.byKey(const Key('start-page')), findsNothing);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('phone address sheet lists profiles', (tester) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    final view = tester.view;
    view.physicalSize = const Size(412, 915);
    view.devicePixelRatio = 1.0;
    try {
      await tester.pumpWidget(
        MorphApp(
          api: FakeMorphApi(),
          home: '/tmp/morph-does-not-exist',
          subscribeToTicks: false,
          loadPageBridgeAsset: false,
        ),
      );
      await _pumpUntilIdentity(tester);
      await tester.tap(find.byKey(const Key('address-bar')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('profile-sheet')), findsOneWidget);
      expect(find.byKey(const Key('profile-add')), findsOneWidget);
      expect(find.byKey(const Key('profile-item-default')), findsOneWidget);
      expect(find.byKey(const Key('profile-delete-default')), findsNothing);
    } finally {
      debugDefaultTargetPlatformOverride = null;
      view.reset();
    }
  });

  testWidgets('Ctrl+F finds in the active page and Escape closes it', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    final view = tester.view;
    view.physicalSize = const Size(1280, 800);
    view.devicePixelRatio = 1.0;
    try {
      await tester.pumpWidget(
        MorphApp(
          api: FakeMorphApi(),
          home: '/tmp/morph-does-not-exist',
          subscribeToTicks: false,
          loadPageBridgeAsset: false,
        ),
      );
      await _pumpUntilIdentity(tester);
      await tester.enterText(find.byKey(const Key('address-bar')), 'f.example');
      await tester.tap(find.byKey(const Key('address-go')));
      await tester.pump();
      await _pumpUntilWebViewAttached(tester, fakeWebView);
      await tester.pump();
      final session = fakeWebView.lastSession!;
      await _focusActiveWebView(tester);

      expect(find.byKey(const Key('find-bar')), findsNothing);
      await _sendChord(
        tester,
        LogicalKeyboardKey.controlLeft,
        LogicalKeyboardKey.keyF,
      );
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('find-bar')), findsOneWidget);
      expect(session.keys.map((e) => e.key), isNot(contains('F')));

      session.evaluated.clear();
      await tester.enterText(find.byKey(const Key('find-field')), 'morph');
      await tester.pump();
      expect(session.evaluated.single, contains('"morph",false,true'));
      expect(find.text('No matches'), findsOneWidget);

      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('find-bar')), findsNothing);
      expect(session.evaluated.last, contains('delete window.__morphFind'));
    } finally {
      debugDefaultTargetPlatformOverride = null;
      view.reset();
    }
  });

  testWidgets('desktop star bookmarks the page in the active profile only', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    final view = tester.view;
    view.physicalSize = const Size(1280, 800);
    view.devicePixelRatio = 1.0;
    try {
      final api = FakeMorphApi();
      await tester.pumpWidget(
        MorphApp(
          api: api,
          home: '/tmp/morph-does-not-exist',
          subscribeToTicks: false,
          loadPageBridgeAsset: false,
        ),
      );
      await _pumpUntilIdentity(tester);
      await tester.enterText(find.byKey(const Key('address-bar')), 'b.example');
      await tester.tap(find.byKey(const Key('address-go')));
      await tester.pump();
      await _pumpUntilWebViewAttached(tester, fakeWebView);
      await tester.pumpAndSettle();

      expect(find.byTooltip('Bookmark this page'), findsOneWidget);
      await tester.tap(find.byKey(const Key('nav-bookmark')));
      await tester.pumpAndSettle();
      expect((await api.listBookmarks('default')).map((b) => b.url), [
        'https://b.example',
      ]);
      expect(api.bookmarksByProfile.keys, ['default']);
      expect(find.byTooltip('Remove bookmark'), findsOneWidget);

      // Ctrl+H opens this profile's history in a panel over the page.
      await _focusActiveWebView(tester);
      await _sendChord(
        tester,
        LogicalKeyboardKey.controlLeft,
        LogicalKeyboardKey.keyH,
      );
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('library-page')), findsOneWidget);
      expect(
        find.byKey(const Key('history-https://b.example')),
        findsOneWidget,
      );
      await tester.tap(find.byKey(const Key('library-tab-bookmarks')));
      await tester.pumpAndSettle();
      expect(
        find.byKey(const Key('bookmark-https://b.example')),
        findsOneWidget,
      );
      await tester.tap(find.byTooltip('Remove bookmark').last);
      await tester.pumpAndSettle();
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('library-page')), findsNothing);
      expect(find.byTooltip('Bookmark this page'), findsOneWidget);
    } finally {
      debugDefaultTargetPlatformOverride = null;
      view.reset();
    }
  });

  testWidgets('phone sheet bookmarks a page and opens Bookmarks', (
    tester,
  ) async {
    final view = tester.view;
    view.physicalSize = const Size(412, 915);
    view.devicePixelRatio = 1.0;
    addTearDown(view.reset);
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    try {
      final api = FakeMorphApi();
      await tester.pumpWidget(
        MorphApp(
          api: api,
          home: '/tmp/morph-does-not-exist',
          subscribeToTicks: false,
          loadPageBridgeAsset: false,
        ),
      );
      await _pumpUntilIdentity(tester);
      await _enterAndSubmitUrl(tester, 'm.example');
      await _pumpUntilWebViewAttached(tester, fakeWebView);
      await tester.pumpAndSettle();

      await tester.tap(find.byKey(const Key('address-bar')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('sheet-bookmark')));
      await tester.pumpAndSettle();
      expect((await api.listBookmarks('default')).map((b) => b.url), [
        'https://m.example',
      ]);

      await tester.tap(find.byKey(const Key('address-bar')));
      await tester.pumpAndSettle();
      expect(find.text('Remove bookmark'), findsOneWidget);
      await tester.tap(find.byKey(const Key('sheet-bookmarks')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('library-page')), findsOneWidget);
      expect(find.byKey(const Key('address-sheet')), findsNothing);
      expect(
        find.byKey(const Key('bookmark-https://m.example')),
        findsOneWidget,
      );
      final nav = tester.state<NavigatorState>(find.byType(Navigator).first);
      nav.pop();
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('library-page')), findsNothing);
      expect(find.byKey(const Key('address-sheet')), findsNothing);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  for (final source in ['bookmarks', 'history']) {
    testWidgets('phone opens a page from $source after the sheet closes', (
      tester,
    ) async {
      final view = tester.view;
      view.physicalSize = const Size(412, 915);
      view.devicePixelRatio = 1.0;
      addTearDown(view.reset);
      fakeWebView = FakeMorphWebViewPlatform(
        renderPath: const MorphRenderPathInfo(
          path: 'android-webview',
          reason: 'fake',
        ),
        platformView: const _NativeViewProbe(),
      );
      MorphWebViewPlatform.instance = fakeWebView;
      debugDefaultTargetPlatformOverride = TargetPlatform.android;
      try {
        final api = FakeMorphApi();
        await api.addBookmark('default', 'https://saved.example', 'Saved');
        await api.recordVisit('default', 'https://saved.example', 'Saved');
        await tester.pumpWidget(
          MorphApp(
            api: api,
            home: '/tmp/morph-does-not-exist',
            subscribeToTicks: false,
            loadPageBridgeAsset: false,
          ),
        );
        await _pumpUntilIdentity(tester);
        await _enterAndSubmitUrl(tester, 'first.example');
        await _pumpUntilWebViewAttached(tester, fakeWebView);
        await tester.pumpAndSettle();
        final session = fakeWebView.lastSession!;
        final state = tester.state<MorphShellState>(find.byType(MorphShell));

        await tester.tap(find.byKey(const Key('address-bar')));
        await tester.pumpAndSettle();
        await tester.tap(find.byKey(Key('sheet-$source')));
        await tester.pumpAndSettle();
        expect(find.byKey(const Key('library-page')), findsOneWidget);
        final entry = find.byKey(
          Key(
            source == 'bookmarks'
                ? 'bookmark-https://saved.example'
                : 'history-https://saved.example',
          ),
        );
        expect(entry, findsOneWidget);
        await _expectSurfaceCheck(tester, 'open from $source', () async {
          final loadsBefore = session.history.length;
          await tester.tap(entry);
          // Nothing loads while the sheet still covers the page.
          expect(session.history.length, loadsBefore);
          await tester.pumpAndSettle();
        });
        expect(find.byKey(const Key('library-page')), findsNothing);
        expect(session.history.last, 'https://saved.example');
        expect(state.navigation.url, 'https://saved.example');
        expect(find.byKey(const Key('native-view-probe')), findsOneWidget);
      } finally {
        debugDefaultTargetPlatformOverride = null;
      }
    });
  }

  testWidgets('desktop menu opens a bookmark and a history entry', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    final view = tester.view;
    view.physicalSize = const Size(1280, 800);
    view.devicePixelRatio = 1.0;
    addTearDown(view.reset);
    try {
      final api = FakeMorphApi();
      await api.addBookmark('default', 'https://b.example', 'B');
      await api.recordVisit('default', 'https://h.example', 'H');
      await tester.pumpWidget(
        MorphApp(
          api: api,
          home: '/tmp/morph-does-not-exist',
          subscribeToTicks: false,
          loadPageBridgeAsset: false,
        ),
      );
      await _pumpUntilIdentity(tester);
      await _enterAndSubmitUrl(tester, 'first.example');
      await _pumpUntilWebViewAttached(tester, fakeWebView);
      await tester.pumpAndSettle();
      final session = fakeWebView.lastSession!;
      for (final (item, key, url) in [
        ('bookmarks', 'bookmark-https://b.example', 'https://b.example'),
        ('history', 'history-https://h.example', 'https://h.example'),
      ]) {
        await tester.tap(find.byKey(const Key('nav-menu')));
        await tester.pumpAndSettle();
        await tester.tap(find.byKey(Key('menu-$item')));
        await tester.pumpAndSettle();
        await tester.tap(find.byKey(Key(key)));
        await tester.pumpAndSettle();
        expect(find.byKey(const Key('library-page')), findsNothing);
        expect(session.history.last, url, reason: 'from $item');
      }
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('links from other apps open in the active profile', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    final view = tester.view;
    view.physicalSize = const Size(1280, 800);
    view.devicePixelRatio = 1.0;
    addTearDown(view.reset);
    try {
      await tester.pumpWidget(
        MorphApp(
          api: FakeMorphApi(),
          home: '/tmp/morph-does-not-exist',
          subscribeToTicks: false,
          loadPageBridgeAsset: false,
          links: MorphIncomingLinks(
            arguments: ['--verbose', 'https://launch.example/'],
            channel: const MethodChannel('test/shell-links'),
          ),
        ),
      );
      await _pumpUntilIdentity(tester);
      await tester.runAsync(
        () => Future<void>.delayed(const Duration(milliseconds: 50)),
      );
      await _pumpUntilWebViewAttached(tester, fakeWebView);
      await tester.pumpAndSettle();
      final state = tester.state<MorphShellState>(find.byType(MorphShell));
      // The launch link uses the empty start tab instead of adding one.
      expect(state.tabs.tabs, hasLength(1));
      expect(state.tabs.active.url, 'https://launch.example/');
      expect(fakeWebView.lastSession!.history.last, 'https://launch.example/');

      await state.openIncomingLink('https://later.example/');
      await tester.pumpAndSettle();
      expect(state.tabs.tabs, hasLength(2));
      expect(state.tabs.active.url, 'https://later.example/');
      expect(state.tabs.tabs.first.url, 'https://launch.example/');
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('desktop menu settings clear only the active profile history', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    final view = tester.view;
    view.physicalSize = const Size(1280, 800);
    view.devicePixelRatio = 1.0;
    try {
      final api = FakeMorphApi();
      await api.recordVisit('other', 'https://keep.test/', 'Keep');
      await tester.pumpWidget(
        MorphApp(
          api: api,
          home: '/tmp/morph-does-not-exist',
          subscribeToTicks: false,
          loadPageBridgeAsset: false,
        ),
      );
      await _pumpUntilIdentity(tester);
      await tester.enterText(find.byKey(const Key('address-bar')), 's.example');
      await tester.tap(find.byKey(const Key('address-go')));
      await tester.pump();
      await _pumpUntilWebViewAttached(tester, fakeWebView);
      await tester.pumpAndSettle();
      expect(await api.listHistory('default'), isNotEmpty);

      await tester.tap(find.byKey(const Key('nav-menu')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('menu-settings')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('settings-page')), findsOneWidget);
      expect(find.byKey(const Key('settings-tab-position')), findsOneWidget);

      await tester.tap(find.byKey(const Key('settings-clear-history')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('clear-history-confirm')));
      await tester.pumpAndSettle();
      expect(await api.listHistory('default'), isEmpty);
      expect(await api.listHistory('other'), isNotEmpty);
    } finally {
      debugDefaultTargetPlatformOverride = null;
      view.reset();
    }
  });

  testWidgets('phone sheet opens settings without desktop layout', (
    tester,
  ) async {
    final view = tester.view;
    view.physicalSize = const Size(412, 915);
    view.devicePixelRatio = 1.0;
    addTearDown(view.reset);
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    try {
      await tester.pumpWidget(
        MorphApp(
          api: FakeMorphApi(),
          home: '/tmp/morph-does-not-exist',
          subscribeToTicks: false,
          loadPageBridgeAsset: false,
        ),
      );
      await _pumpUntilIdentity(tester);
      await tester.tap(find.byKey(const Key('address-bar')));
      await tester.pumpAndSettle();
      await tester.scrollUntilVisible(
        find.byKey(const Key('sheet-settings')),
        200,
        scrollable: find
            .descendant(
              of: find.byKey(const Key('address-sheet')),
              matching: find.byType(Scrollable),
            )
            .first,
      );
      await tester.tap(find.byKey(const Key('sheet-settings')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('settings-page')), findsOneWidget);
      expect(find.byKey(const Key('settings-tab-position')), findsNothing);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('phone address sheet opens find in page', (tester) async {
    final view = tester.view;
    view.physicalSize = const Size(412, 915);
    view.devicePixelRatio = 1.0;
    addTearDown(view.reset);
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    try {
      await tester.pumpWidget(
        MorphApp(
          api: FakeMorphApi(),
          home: '/tmp/morph-does-not-exist',
          subscribeToTicks: false,
          loadPageBridgeAsset: false,
        ),
      );
      await _pumpUntilIdentity(tester);
      await tester.tap(find.byKey(const Key('address-bar')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('sheet-find')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('address-sheet')), findsNothing);
      expect(find.byKey(const Key('find-bar')), findsOneWidget);
      await tester.tap(find.byKey(const Key('find-close')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('find-bar')), findsNothing);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets(
    'shell shortcuts work while the web view has focus and are not forwarded',
    (tester) async {
      debugDefaultTargetPlatformOverride = TargetPlatform.linux;
      final view = tester.view;
      view.physicalSize = const Size(1280, 800);
      view.devicePixelRatio = 1.0;
      try {
        await tester.pumpWidget(
          MorphApp(
            api: FakeMorphApi(),
            home: '/tmp/morph-does-not-exist',
            subscribeToTicks: false,
            loadPageBridgeAsset: false,
          ),
        );
        await _pumpUntilIdentity(tester);
        await tester.enterText(
          find.byKey(const Key('address-bar')),
          'one.example',
        );
        await tester.tap(find.byKey(const Key('address-go')));
        await tester.pump();
        await _pumpUntilWebViewAttached(tester, fakeWebView);
        await tester.pump();

        final session = fakeWebView.lastSession!;
        await _focusActiveWebView(tester);
        session.keys.clear();

        await tester.sendKeyEvent(LogicalKeyboardKey.keyA);
        await tester.pump();
        expect(
          session.keys.map((event) => event.key),
          contains('A'),
          reason: 'plain keys should still reach the web view',
        );
        session.keys.clear();

        await _sendChord(
          tester,
          LogicalKeyboardKey.controlLeft,
          LogicalKeyboardKey.keyT,
        );
        await tester.pumpAndSettle();
        expect(find.byKey(const Key('tab-2')), findsOneWidget);
        expect(
          session.keys.map((event) => event.key),
          isNot(contains('T')),
          reason: 'Ctrl+T must not be forwarded to the web view',
        );

        await _sendChord(
          tester,
          LogicalKeyboardKey.controlLeft,
          LogicalKeyboardKey.keyW,
        );
        await tester.pumpAndSettle();
        expect(find.byKey(const Key('tab-2')), findsNothing);
        expect(find.byKey(const Key('tab-1')), findsOneWidget);

        await _focusActiveWebView(tester);
        await _sendChord(
          tester,
          LogicalKeyboardKey.metaLeft,
          LogicalKeyboardKey.keyT,
        );
        await tester.pumpAndSettle();
        expect(find.byKey(const Key('tab-3')), findsOneWidget);

        await tester.tap(find.byKey(const Key('tab-new')));
        await tester.pumpAndSettle();
        expect(find.byKey(const Key('tab-4')), findsOneWidget);

        await _sendChord(
          tester,
          LogicalKeyboardKey.controlLeft,
          LogicalKeyboardKey.digit1,
        );
        await tester.pumpAndSettle();
        expect(
          tester
              .widget<TextField>(find.byKey(const Key('address-bar')))
              .controller!
              .text,
          'one.example',
        );

        await _sendChord(
          tester,
          LogicalKeyboardKey.controlLeft,
          LogicalKeyboardKey.digit9,
        );
        await tester.pumpAndSettle();
        expect(find.byKey(const Key('tab-4')), findsOneWidget);

        await _sendChord(
          tester,
          LogicalKeyboardKey.controlLeft,
          LogicalKeyboardKey.tab,
        );
        await tester.pumpAndSettle();
        expect(
          tester
              .widget<TextField>(find.byKey(const Key('address-bar')))
              .controller!
              .text,
          'one.example',
        );

        await _sendChord(
          tester,
          LogicalKeyboardKey.controlLeft,
          LogicalKeyboardKey.tab,
          shift: true,
        );
        await tester.pumpAndSettle();
        expect(find.byKey(const Key('tab-4')), findsOneWidget);

        await tester.tap(find.byKey(const Key('tab-1')));
        await tester.pumpAndSettle();
        await _focusActiveWebView(tester);
        final reloads = session.reloads;
        session.keys.clear();
        await _sendChord(
          tester,
          LogicalKeyboardKey.controlLeft,
          LogicalKeyboardKey.keyR,
        );
        await tester.pump();
        expect(session.reloads, reloads + 1);
        expect(session.keys.map((event) => event.key), isNot(contains('R')));

        session.keys.clear();
        await _sendChord(
          tester,
          LogicalKeyboardKey.controlLeft,
          LogicalKeyboardKey.keyL,
        );
        await tester.pumpAndSettle();
        expect(tester.binding.focusManager.primaryFocus, isNotNull);
        expect(
          tester
              .widget<TextField>(find.byKey(const Key('address-bar')))
              .focusNode!
              .hasFocus,
          isTrue,
        );
        expect(session.keys.map((event) => event.key), isNot(contains('L')));
      } finally {
        debugDefaultTargetPlatformOverride = null;
        view.reset();
      }
    },
  );
}

Future<void> _sendChord(
  WidgetTester tester,
  LogicalKeyboardKey modifier,
  LogicalKeyboardKey key, {
  bool shift = false,
}) async {
  await tester.sendKeyDownEvent(modifier);
  if (shift) {
    await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
  }
  await tester.sendKeyEvent(key);
  if (shift) {
    await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
  }
  await tester.sendKeyUpEvent(modifier);
}

Future<void> _focusActiveWebView(WidgetTester tester) async {
  var focused = false;
  void visit(Element element) {
    final widget = element.widget;
    if (widget is Offstage && widget.offstage) {
      return;
    }
    if (widget is Focus && widget.focusNode?.debugLabel == 'morph-webview') {
      widget.focusNode!.requestFocus();
      focused = true;
      return;
    }
    element.visitChildren(visit);
  }

  tester.element(find.byType(MorphShell)).visitChildren(visit);
  await tester.pump();
  expect(focused, isTrue, reason: 'active web view was not focusable');
}

Future<void> _addNamedProfile(WidgetTester tester, String name) async {
  await tester.tap(find.byKey(const Key('profile-button')));
  await tester.pumpAndSettle();
  await tester.tap(find.byKey(const Key('profile-add')));
  await tester.pumpAndSettle();
  expect(find.byKey(const Key('profile-add-dialog')), findsOneWidget);
  await tester.enterText(find.byKey(const Key('profile-add-name')), name);
  await tester.tap(find.byKey(const Key('profile-add-confirm')));
  await tester.pumpAndSettle();
}

void _assertProfileAtNavTrailing(
  WidgetTester tester,
  TargetPlatform platform,
  MorphTabPosition position,
) {
  final reason = '$platform $position';
  expect(
    find.byKey(const Key('profile-button')),
    findsOneWidget,
    reason: reason,
  );
  final nav = tester.getRect(find.byKey(const Key('nav-row')));
  final profile = tester.getRect(find.byKey(const Key('profile-button')));
  expect(profile.top, greaterThanOrEqualTo(nav.top - 0.5), reason: reason);
  expect(profile.bottom, lessThanOrEqualTo(nav.bottom + 0.5), reason: reason);
  expect(nav.right - profile.right, lessThan(24), reason: reason);

  if (position == MorphTabPosition.top) {
    final strip = tester.getRect(find.byKey(const Key('tab-strip')));
    expect(
      profile.top,
      greaterThanOrEqualTo(strip.bottom - 0.5),
      reason: reason,
    );
  } else {
    final caption = tester.getRect(find.byKey(const Key('caption-bar')));
    expect(
      profile.top,
      greaterThanOrEqualTo(caption.bottom - 0.5),
      reason: reason,
    );
    if (position == MorphTabPosition.bottom) {
      final strip = tester.getRect(find.byKey(const Key('tab-strip')));
      expect(
        profile.bottom,
        lessThanOrEqualTo(strip.top + 0.5),
        reason: reason,
      );
    }
  }

  if (find.byKey(const Key('window-close')).evaluate().isNotEmpty) {
    final close = tester.getRect(find.byKey(const Key('window-close')));
    expect(
      profile.top,
      greaterThanOrEqualTo(close.bottom - 0.5),
      reason: reason,
    );
    expect((profile.right - close.right).abs(), lessThan(40), reason: reason);
  }
}
