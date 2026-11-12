import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/api.dart';
import 'package:morph/src/app.dart';
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
        home: MorphShell(api: api, home: missingHome, subscribeToTicks: false),
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
}
