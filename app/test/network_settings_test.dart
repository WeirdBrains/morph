import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:morph/src/theme/morph_theme.dart';
import 'package:morph/src/widgets/settings_page.dart';

import 'support/fake_morph_api.dart';

const _peer = '12D3KooWHp56KezbVCubjuznJGqyn9xTe8PWePfuwv4YQUGxaRBB';

Widget _settings(FakeMorphApi api) => MaterialApp(
  theme: MorphTheme.dark,
  home: MorphSettingsPage(
    profileName: 'Work',
    coreVersion: '0.1.0',
    renderPath: 'fake',
    onClearHistory: () async {},
    onOpenLibrary: (_) {},
    relays: api,
    network: api,
    home: '/tmp/morph-test-home',
  ),
);

Future<void> _show(WidgetTester tester, Finder f) async {
  await tester.scrollUntilVisible(
    f,
    200,
    scrollable: find.byType(Scrollable).first,
  );
  await tester.pumpAndSettle();
}

bool _switchOn(WidgetTester tester) => tester
    .widget<SwitchListTile>(find.byKey(const Key('settings-discovery')))
    .value;

void main() {
  testWidgets('discovery is on by default and turns off and on again', (
    tester,
  ) async {
    final api = FakeMorphApi();
    await tester.pumpWidget(_settings(api));
    await tester.pumpAndSettle();
    expect(find.text('RELAYS · ALL PROFILES'), findsOneWidget);
    await _show(tester, find.byKey(const Key('settings-discovery')));
    expect(find.text('Weirdnet discovery'), findsOneWidget);
    expect(_switchOn(tester), isTrue);
    expect(find.textContaining('corporate networks'), findsOneWidget);

    await tester.tap(find.byKey(const Key('settings-discovery')));
    await tester.pumpAndSettle();
    expect(api.networkCalls, ['discovery false']);
    expect(api.discovery, isFalse);
    expect(_switchOn(tester), isFalse);

    await tester.tap(find.byKey(const Key('settings-discovery')));
    await tester.pumpAndSettle();
    expect(api.networkCalls, ['discovery false', 'discovery true']);
    expect(_switchOn(tester), isTrue);
  });

  testWidgets('an admin lock keeps the switch off, disabled, and says why', (
    tester,
  ) async {
    final api = FakeMorphApi()..networkLock = 'env';
    await tester.pumpWidget(_settings(api));
    await tester.pumpAndSettle();
    await _show(tester, find.byKey(const Key('settings-discovery')));
    final tile = tester.widget<SwitchListTile>(
      find.byKey(const Key('settings-discovery')),
    );
    expect(tile.value, isFalse);
    expect(tile.onChanged, isNull);
    expect(
      find.textContaining('Turned off by MORPH_DISABLE_WEIRDNET_DISCOVERY'),
      findsOneWidget,
    );
    expect(find.byIcon(Icons.lock_outline), findsOneWidget);
    await tester.tap(find.byKey(const Key('settings-discovery')));
    await tester.pumpAndSettle();
    expect(api.networkCalls, isEmpty);

    // Manual peers still work while discovery is locked off.
    await _show(tester, find.byKey(const Key('settings-peer-input')));
    await tester.enterText(
      find.byKey(const Key('settings-peer-input')),
      '/ip4/10.0.0.2/tcp/4747/p2p/$_peer',
    );
    await tester.tap(find.byKey(const Key('settings-peer-add')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('settings-peer-$_peer')), findsOneWidget);
  });

  testWidgets('peers are added, rejected with the reason, and removed', (
    tester,
  ) async {
    final api = FakeMorphApi();
    await tester.pumpWidget(_settings(api));
    await tester.pumpAndSettle();
    final input = find.byKey(const Key('settings-peer-input'));
    await _show(tester, input);

    // Empty: the field says what to type, nothing is sent.
    await tester.tap(find.byKey(const Key('settings-peer-add')));
    await tester.pumpAndSettle();
    expect(find.textContaining('Enter a multiaddr'), findsOneWidget);
    expect(api.networkCalls, isEmpty);

    // Invalid: Rust's reason shows under the field, the text stays.
    await tester.enterText(input, '/ip4/127.0.0.1/tcp/1');
    await tester.tap(find.byKey(const Key('settings-peer-add')));
    await tester.pumpAndSettle();
    expect(
      find.text('/ip4/127.0.0.1/tcp/1 has no /p2p/ peer id'),
      findsOneWidget,
    );
    expect(api.networkPeers, isEmpty);
    expect(
      tester.widget<TextField>(input).controller!.text,
      '/ip4/127.0.0.1/tcp/1',
    );

    // Valid: listed with its address, the field clears and the error goes.
    final addr = '/ip4/192.168.1.5/tcp/4747/p2p/$_peer';
    await tester.enterText(input, addr);
    await tester.tap(find.byKey(const Key('settings-peer-add')));
    await tester.pumpAndSettle();
    expect(api.networkCalls.last, 'add $addr');
    expect(find.byKey(const Key('settings-peer-$_peer')), findsOneWidget);
    expect(find.text(addr), findsOneWidget);
    expect(find.textContaining('has no /p2p/'), findsNothing);
    expect(tester.widget<TextField>(input).controller!.text, isEmpty);
    expect(
      _switchOn(tester),
      isTrue,
      reason: 'adding a peer leaves discovery alone',
    );

    await tester.tap(find.byKey(const Key('settings-peer-remove-$_peer')));
    await tester.pumpAndSettle();
    expect(api.networkCalls.last, 'remove $_peer');
    expect(find.byKey(const Key('settings-peer-$_peer')), findsNothing);
  });

  testWidgets('without a network source the controls are hidden', (
    tester,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        home: MorphSettingsPage(
          profileName: 'Default',
          coreVersion: '0.1.0',
          renderPath: 'fake',
          onClearHistory: () async {},
          onOpenLibrary: (_) {},
        ),
      ),
    );
    expect(find.byKey(const Key('settings-discovery')), findsNothing);
    expect(find.byKey(const Key('settings-peer-input')), findsNothing);
  });
}
