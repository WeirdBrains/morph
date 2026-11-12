import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/api.dart';
import 'package:morph/src/app.dart';
import 'package:morph/src/widgets/tab_switcher.dart';
import 'package:morph/src/shell.dart';
import 'package:morph/src/tabs/tab_position.dart';
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
  bool sidebarCollapsed = false;
  int sidebarWidth = 220;

  @override
  Future<String> loadTabPosition({String? home}) async => tabPosition;

  @override
  Future<void> saveTabPosition(String position, {String? home}) async {
    tabPosition = position;
  }

  @override
  Future<bool> loadSidebarCollapsed({String? home}) async => sidebarCollapsed;

  @override
  Future<void> saveSidebarCollapsed(bool collapsed, {String? home}) async {
    sidebarCollapsed = collapsed;
  }

  @override
  Future<int> loadSidebarWidth({String? home}) async => sidebarWidth;

  @override
  Future<void> saveSidebarWidth(int width, {String? home}) async {
    sidebarWidth = width;
  }

  String currentProfileId = 'default';
  final List<ProfileInfo> profiles = [
    const ProfileInfo(id: 'default', name: 'Default'),
  ];
  final Map<String, ProfileTabSnapshot> tabsByProfile = {};
  int _nextProfile = 1;

  ProfileCatalog _catalog() =>
      ProfileCatalog(current: currentProfileId, profiles: List.of(profiles));

  ProfileInfo _require(String id) {
    return profiles.firstWhere(
      (profile) => profile.id == id,
      orElse: () => throw StateError('unknown profile $id'),
    );
  }

  String _home(String? home) => home ?? '/tmp/morph-fake';

  @override
  Future<ProfileCatalog> listProfiles({String? home}) async => _catalog();

  @override
  @override
  Future<ProfileCatalog> createProfile(
    String name, {
    String? home,
    String? color,
  }) async {
    final profile = ProfileInfo(
      id: 'p${_nextProfile++}',
      name: name,
      color: color ?? '',
    );
    profiles.add(profile);
    currentProfileId = profile.id;
    return _catalog();
  }

  @override
  Future<ProfileCatalog> renameProfile(
    String id,
    String name, {
    String? home,
  }) async {
    final index = profiles.indexWhere((profile) => profile.id == id);
    if (index < 0) {
      throw StateError('unknown profile $id');
    }
    final existing = profiles[index];
    profiles[index] = ProfileInfo(id: id, name: name, color: existing.color);
    return _catalog();
  }

  @override
  Future<ProfileCatalog> deleteProfile(String id, {String? home}) async {
    if (profiles.length == 1) {
      throw StateError('cannot delete the last profile');
    }
    _require(id);
    profiles.removeWhere((profile) => profile.id == id);
    tabsByProfile.remove(id);
    if (currentProfileId == id) {
      currentProfileId = profiles.any((profile) => profile.id == 'default')
          ? 'default'
          : profiles.first.id;
    }
    return _catalog();
  }

  @override
  Future<ProfileCatalog> setCurrentProfile(String id, {String? home}) async {
    _require(id);
    currentProfileId = id;
    return _catalog();
  }

  @override
  Future<ProfilePaths> profilePaths(String id, {String? home}) async {
    _require(id);
    final root = '${_home(home)}/profiles/$id';
    return ProfilePaths(
      id: id,
      profileDir: root,
      settingsPath: '$root/settings.json',
      tabsPath: '$root/tabs.json',
      webkitDir: '$root/webkit',
      webkitDataDir: '$root/webkit/data',
      webkitCacheDir: '$root/webkit/cache',
      cookiesPath: '$root/webkit/cookies.sqlite',
    );
  }

  String _tabKey(String? profileId) => profileId ?? currentProfileId;

  @override
  Future<ProfileTabSnapshot> loadProfileTabs({
    String? home,
    String? profileId,
  }) async {
    return tabsByProfile[_tabKey(profileId)] ?? ProfileTabSnapshot.empty;
  }

  @override
  Future<void> saveProfileTabs(
    ProfileTabSnapshot snapshot, {
    String? home,
    String? profileId,
  }) async {
    tabsByProfile[_tabKey(profileId)] = snapshot;
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

      await tester.tap(find.byKey(const Key('sheet-history')));
      await tester.pump();
      expect(find.text('History is coming soon'), findsOneWidget);
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
