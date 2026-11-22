import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:morph/src/api.dart';
import 'package:morph/src/theme/morph_theme.dart';
import 'package:morph/src/widgets/linked_devices.dart';
import 'package:morph/src/widgets/settings_page.dart';

import 'support/fake_morph_api.dart';

Widget _section(FakeMorphApi api) => MaterialApp(
  theme: MorphTheme.dark,
  home: Scaffold(
    body: MorphLinkedDevicesSection(source: api, profileId: 'p1'),
  ),
);

void main() {
  testWidgets('rows show the state and the key notes', (tester) async {
    final api = FakeMorphApi()
      ..devices = const [
        MorphLinkedDevice(
          peerId: 'me',
          label: 'Pixel',
          state: 'This device',
          thisDevice: true,
          note: 'Needs re-admission from another device',
        ),
        MorphLinkedDevice(
          peerId: 'desk',
          label: 'Desktop',
          state: 'Linked',
          note: 'Waiting for its first sync',
        ),
      ];
    await tester.pumpWidget(_section(api));
    await tester.pumpAndSettle();
    expect(find.text('This device'), findsOneWidget);
    expect(find.text('Needs re-admission from another device'), findsOneWidget);
    expect(find.text('Waiting for its first sync'), findsOneWidget);
    expect(find.byKey(const Key('linked-device-revoke-me')), findsNothing);
    expect(find.byKey(const Key('linked-device-revoke-desk')), findsOneWidget);
  });

  testWidgets('revoke warns about left-out devices, asks, then revokes', (
    tester,
  ) async {
    final api = FakeMorphApi()
      ..devices = const [
        MorphLinkedDevice(
          peerId: 'me',
          label: 'Pixel',
          state: 'This device',
          thisDevice: true,
        ),
        MorphLinkedDevice(peerId: 'desk', label: 'Desktop', state: 'Linked'),
        MorphLinkedDevice(
          peerId: 'old',
          label: 'Tablet',
          state: 'Linked',
          hasKey: false,
        ),
      ]
      ..revokeWarning =
          'Tablet has not synced since updating Morph. Revoking now leaves it '
          'out of the new key; link it again afterwards.';
    await tester.pumpWidget(_section(api));
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(const Key('linked-device-revoke-desk')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('revoke-device-dialog')), findsOneWidget);
    expect(find.byKey(const Key('revoke-device-warning')), findsOneWidget);
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
    expect(api.revoked, isEmpty);

    await tester.tap(find.byKey(const Key('linked-device-revoke-desk')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('revoke-device-confirm')));
    await tester.pumpAndSettle();
    expect(api.revoked, ['desk']);
    expect(find.text('Revoked'), findsOneWidget);
    expect(find.byKey(const Key('linked-device-revoke-desk')), findsNothing);
  });

  testWidgets('no warning when every device has synced', (tester) async {
    final api = FakeMorphApi()
      ..devices = const [
        MorphLinkedDevice(peerId: 'desk', label: 'Desktop', state: 'Linked'),
      ];
    await tester.pumpWidget(_section(api));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('linked-device-revoke-desk')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('revoke-device-dialog')), findsOneWidget);
    expect(find.byKey(const Key('revoke-device-warning')), findsNothing);
  });

  testWidgets('settings shows Linked devices for the profile', (tester) async {
    final api = FakeMorphApi();
    await tester.pumpWidget(
      MaterialApp(
        theme: MorphTheme.dark,
        home: MorphSettingsPage(
          profileName: 'Work',
          coreVersion: '0.1.0',
          renderPath: 'fake',
          onClearHistory: () async {},
          onOpenLibrary: (_) {},
          devices: api,
          profileId: 'p1',
        ),
      ),
    );
    await tester.binding.setSurfaceSize(const Size(800, 2000));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    await tester.pumpAndSettle();
    expect(find.text('SYNC · WORK'), findsOneWidget);
    await tester.scrollUntilVisible(find.text('LINKED DEVICES · WORK'), 200);
    expect(find.text('LINKED DEVICES · WORK'), findsOneWidget);
    expect(find.byKey(const Key('linked-devices-empty')), findsOneWidget);
  });
}
