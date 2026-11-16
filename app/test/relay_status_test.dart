import 'package:material_ui/material_ui.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/api.dart';
import 'package:morph/src/theme/morph_theme.dart';
import 'package:morph/src/widgets/device_link.dart';
import 'package:morph/src/widgets/relay_status.dart';
import 'package:morph/src/widgets/settings_page.dart';

import 'support/fake_morph_api.dart';

const _peer = '12D3KooWHp56KezbVCubjuznJGqyn9xTe8PWePfuwv4YQUGxaRBB';

MorphRelayReport _report(MorphRelayState state, {bool checking = false}) =>
    MorphRelayReport(
      nodes: state == MorphRelayState.noNode ? 0 : 1,
      checking: checking,
      summary: switch (state) {
        MorphRelayState.reserved =>
          'Relay WeirdBrains public relay: connected, slot reserved',
        MorphRelayState.connected =>
          'Relay WeirdBrains public relay: connected, reserving a slot…',
        MorphRelayState.notConnected =>
          'Relay WeirdBrains public relay: connecting…',
        MorphRelayState.noNode => 'Not connected to a relay.',
      },
      relays: [
        MorphRelay(
          peer: _peer,
          addrs: const ['/dns4/relay.net.weirdbrains.com/tcp/4747/p2p/$_peer'],
          title: 'WeirdBrains public relay',
          host: 'relay.net.weirdbrains.com',
          state: state,
          label: switch (state) {
            MorphRelayState.reserved => 'Connected, slot reserved',
            MorphRelayState.connected => 'Connected, reserving a slot…',
            MorphRelayState.notConnected => 'Connecting…',
            MorphRelayState.noNode => 'Not connected',
          },
        ),
      ],
    );

Widget _settings(FakeMorphApi api) => MaterialApp(
  theme: MorphTheme.dark,
  home: MorphSettingsPage(
    profileName: 'Work',
    coreVersion: '0.1.0',
    renderPath: 'fake',
    onClearHistory: () async {},
    onOpenLibrary: (_) {},
    relays: api,
  ),
);

void main() {
  test('the bridge words map to states', () {
    expect(MorphRelayState.parse('reserved'), MorphRelayState.reserved);
    expect(MorphRelayState.parse('connected'), MorphRelayState.connected);
    expect(
      MorphRelayState.parse('not-connected'),
      MorphRelayState.notConnected,
    );
    expect(MorphRelayState.parse('no-node'), MorphRelayState.noNode);
    expect(MorphRelayState.parse('anything else'), MorphRelayState.noNode);
  });

  testWidgets('settings list the relays and follow the live state', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(600, 2400);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    final api = FakeMorphApi();
    await tester.pumpWidget(_settings(api));
    await tester.pumpAndSettle();
    expect(find.text('RELAYS · ALL PROFILES'), findsOneWidget);
    expect(find.byKey(const Key('settings-relays-summary')), findsOneWidget);
    expect(find.textContaining('Not connected to a relay'), findsOneWidget);
    expect(find.text('Not connected'), findsOneWidget);
    expect(find.text(_peer), findsOneWidget);

    await tester.tap(find.byKey(const Key('settings-relays-check')));
    await tester.pump();
    expect(api.relayChecks, 1);

    api.relays = _report(MorphRelayState.connected, checking: true);
    await tester.pump(morphRelayPoll);
    expect(find.text('Connected, reserving a slot…'), findsOneWidget);
    expect(find.text('Checking…'), findsOneWidget);
    final button = tester.widget<FilledButton>(
      find.byKey(const Key('settings-relays-check')),
    );
    expect(button.onPressed, isNull, reason: 'one check at a time');

    api.relays = _report(MorphRelayState.reserved);
    await tester.pump(morphRelayPoll);
    expect(find.text('Connected, slot reserved'), findsOneWidget);
    expect(find.text('WeirdBrains public relay'), findsOneWidget);
    expect(find.text('Check now'), findsOneWidget);
    await tester.pumpAndSettle();
  });

  testWidgets('settings without a relay source hide the section', (
    tester,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        home: MorphSettingsPage(
          profileName: 'Work',
          coreVersion: '0.1.0',
          renderPath: 'fake',
          onClearHistory: () async {},
          onOpenLibrary: (_) {},
        ),
      ),
    );
    expect(find.byKey(const Key('settings-relays')), findsNothing);
  });

  testWidgets('the link screens show the relay status line', (tester) async {
    tester.view.physicalSize = const Size(480, 1800);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    final api = FakeMorphApi();
    await tester.pumpWidget(
      MaterialApp(
        theme: MorphTheme.dark,
        home: Scaffold(
          body: SingleChildScrollView(
            child: MorphLinkFromDevice(
              api: api,
              profileId: 'work',
              deviceName: 'Morph on desk',
              pollEvery: const Duration(milliseconds: 10),
              onDone: (_) {},
            ),
          ),
        ),
      ),
    );
    await tester.pump();
    await tester.pump();
    expect(find.byKey(const Key('link-qr')), findsOneWidget);
    expect(find.byKey(const Key('link-relay-status')), findsOneWidget);
    expect(find.textContaining('Not connected to a relay'), findsOneWidget);
    api.relays = _report(MorphRelayState.reserved);
    await tester.pump(morphRelayPoll);
    expect(
      find.text('Relay WeirdBrains public relay: connected, slot reserved'),
      findsOneWidget,
    );

    await tester.pumpWidget(
      MaterialApp(
        theme: MorphTheme.dark,
        home: Scaffold(
          body: SingleChildScrollView(
            child: MorphAddDevice(
              api: api,
              profileId: 'work',
              profileName: 'Work',
            ),
          ),
        ),
      ),
    );
    await tester.pump();
    expect(find.byKey(const Key('link-relay-status')), findsOneWidget);
  });
}
