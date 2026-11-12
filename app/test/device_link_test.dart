import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/api.dart';
import 'package:morph/src/theme/morph_theme.dart';
import 'package:morph/src/widgets/device_link.dart';
import 'package:morph/src/widgets/identity_onboarding.dart';
import 'package:qr/qr.dart';

import 'support/fake_morph_api.dart';

const _poll = Duration(milliseconds: 10);

Future<void> _host(WidgetTester tester, Widget child) async {
  tester.view.physicalSize = const Size(480, 1600);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    MaterialApp(
      theme: MorphTheme.dark,
      home: Scaffold(body: SingleChildScrollView(child: child)),
    ),
  );
  await tester.pump();
}

Future<List<MorphIdentity>> _newDevice(
  WidgetTester tester,
  FakeMorphApi api,
) async {
  final done = <MorphIdentity>[];
  await _host(
    tester,
    MorphLinkFromDevice(
      api: api,
      profileId: 'work',
      deviceName: 'Morph on desk',
      pollEvery: _poll,
      onDone: done.add,
    ),
  );
  return done;
}

Future<void> _poll2(WidgetTester tester) async {
  await tester.pump(_poll * 3);
  await tester.pump();
}

void main() {
  test('the QR encodes the link and has the modules scanners need', () {
    const link = 'weirdnet-morph://link/abc';
    final image = QrImage(
      QrCode(
        payload: QrPayload.fromString(link),
        errorCorrectLevel: QrErrorCorrectLevel.low,
      ),
    );
    expect(image.moduleCount, greaterThanOrEqualTo(21));
    // The three finder patterns are dark in their corners.
    final n = image.moduleCount;
    expect(image.isDark(0, 0), isTrue);
    expect(image.isDark(0, n - 1), isTrue);
    expect(image.isDark(n - 1, 0), isTrue);
  });

  testWidgets('new device: code, compare, password, done', (tester) async {
    final api = FakeMorphApi();
    final done = await _newDevice(tester, api);
    expect(api.linkCalls, ['start Morph on desk']);
    expect(find.byKey(const Key('link-qr')), findsOneWidget);
    expect(find.text('weirdnet-morph://link/fake-offer'), findsOneWidget);

    api.linkStatus = const MorphLinkStatus(
      MorphLinkState.paired,
      sas: '042917',
    );
    await _poll2(tester);
    expect(find.byKey(const Key('link-qr')), findsNothing);
    expect(find.text('042 917'), findsOneWidget);
    await tester.tap(find.byKey(const Key('link-match')));
    await tester.pump();
    expect(api.linkCalls, contains('confirm 7'));
    expect(find.byKey(const Key('link-wait-sender')), findsOneWidget);

    api.linkStatus = const MorphLinkStatus(
      MorphLinkState.received,
      sas: '042917',
    );
    await _poll2(tester);
    await tester.enterText(find.byKey(const Key('link-password')), 'short');
    await tester.enterText(
      find.byKey(const Key('link-password-confirm')),
      'short',
    );
    await tester.tap(find.byKey(const Key('link-finish')));
    await tester.pump();
    expect(find.byKey(const Key('link-error')), findsOneWidget);
    expect(done, isEmpty);

    await tester.enterText(
      find.byKey(const Key('link-password')),
      'long enough',
    );
    await tester.enterText(
      find.byKey(const Key('link-password-confirm')),
      'long enough',
    );
    await tester.tap(find.byKey(const Key('link-finish')));
    await tester.pump();
    expect(api.linkCalls, contains('finish work 7'));
    expect(done.single.linked, isTrue);
    // A finished session is not cancelled on the way out.
    await tester.pumpWidget(const SizedBox());
    expect(api.linkCalls, isNot(contains('cancel 7')));
  });

  testWidgets('new device: different codes cancel; a new code restarts', (
    tester,
  ) async {
    final api = FakeMorphApi();
    await _newDevice(tester, api);
    api.linkStatus = const MorphLinkStatus(
      MorphLinkState.paired,
      sas: '111222',
    );
    await _poll2(tester);
    await tester.tap(find.byKey(const Key('link-differ')));
    await tester.pump();
    expect(api.linkCalls, contains('cancel 7'));
    expect(find.byKey(const Key('link-ended')), findsOneWidget);

    api.linkStatus = const MorphLinkStatus(MorphLinkState.waiting);
    await tester.tap(find.byKey(const Key('link-restart')));
    await tester.pump();
    await tester.pump();
    expect(api.linkCalls.where((c) => c.startsWith('start')), hasLength(2));
    expect(find.byKey(const Key('link-qr')), findsOneWidget);
  });

  testWidgets('new device: an expired code says so; leaving cancels', (
    tester,
  ) async {
    final api = FakeMorphApi();
    await _newDevice(tester, api);
    api.linkStatus = const MorphLinkStatus(MorphLinkState.expired);
    await _poll2(tester);
    expect(find.text('The code expired.'), findsOneWidget);
    await tester.pumpWidget(const SizedBox());
    expect(api.linkCalls.last, 'cancel 7');
  });

  testWidgets('existing device: unlock, paste, compare, send', (tester) async {
    final api = FakeMorphApi();
    await api.createProfileIdentity('work', 'right password');
    final closed = <bool>[];
    await _host(
      tester,
      MorphAddDevice(
        api: api,
        profileId: 'work',
        profileName: 'Work',
        onClose: () => closed.add(true),
      ),
    );
    expect(find.byKey(const Key('add-device-scan')), findsNothing);

    await tester.enterText(
      find.byKey(const Key('add-device-link')),
      'weirdnet-morph://link/fake-offer',
    );
    await tester.tap(find.byKey(const Key('add-device-pair')));
    await tester.pump();
    expect(find.byKey(const Key('add-device-error')), findsOneWidget);

    await tester.enterText(
      find.byKey(const Key('add-device-password')),
      'wrong password',
    );
    await tester.tap(find.byKey(const Key('add-device-pair')));
    await tester.pump();
    expect(find.text('That is not this profile\'s password.'), findsOneWidget);

    await tester.enterText(
      find.byKey(const Key('add-device-password')),
      'right password',
    );
    await tester.tap(find.byKey(const Key('add-device-pair')));
    await tester.pump();
    expect(find.textContaining('Link Morph on laptop?'), findsOneWidget);
    expect(find.text('042 917'), findsOneWidget);
    await tester.tap(find.byKey(const Key('add-device-send')));
    await tester.pump();
    expect(api.linkCalls, contains('send 9'));
    expect(find.byKey(const Key('add-device-sent')), findsOneWidget);
    await tester.tap(find.byKey(const Key('add-device-close')));
    expect(closed, [true]);
    await tester.pumpWidget(const SizedBox());
    expect(api.linkCalls, isNot(contains('pair-cancel 9')));
  });

  testWidgets('existing device: a scan fills the link and pairs', (
    tester,
  ) async {
    final api = FakeMorphApi();
    await _host(
      tester,
      MorphAddDevice(
        api: api,
        profileId: 'work',
        profileName: 'Work',
        scan: () async => ' weirdnet-morph://link/scanned ',
      ),
    );
    await tester.enterText(
      find.byKey(const Key('add-device-password')),
      'any password',
    );
    await tester.tap(find.byKey(const Key('add-device-scan')));
    await tester.pump();
    await tester.pump();
    expect(api.linkCalls, ['pair work weirdnet-morph://link/scanned']);
    await tester.tap(find.byKey(const Key('add-device-cancel')));
    await tester.pump();
    expect(api.linkCalls.last, 'pair-cancel 9');
  });

  testWidgets('onboarding offers linking only when it is wired', (
    tester,
  ) async {
    Widget onboarding({WidgetBuilder? link}) => MaterialApp(
      theme: MorphTheme.dark,
      home: MorphIdentityOnboarding(
        profileName: 'Work',
        onCreate: (_) async => throw UnimplementedError(),
        onImport: (_, _) async => throw UnimplementedError(),
        onDone: (_) {},
        onSkip: () {},
        linkFromDevice: link,
      ),
    );
    tester.view.physicalSize = const Size(480, 1400);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(onboarding());
    expect(find.byKey(const Key('onboarding-link')), findsNothing);
    await tester.pumpWidget(
      onboarding(link: (_) => const Text('link step', key: Key('step'))),
    );
    await tester.tap(find.byKey(const Key('onboarding-link')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('step')), findsOneWidget);
    await tester.tap(find.byTooltip('Back'));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('onboarding-link')), findsOneWidget);
  });
}
