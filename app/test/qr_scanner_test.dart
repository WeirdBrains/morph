import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:morph/src/qr_scanner.dart';
import 'package:morph/src/theme/morph_theme.dart';

void main() {
  Future<Future<String?>> open(
    WidgetTester tester,
    MorphQrCamera camera,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        theme: MorphTheme.dark,
        home: const Scaffold(body: Text('below')),
      ),
    );
    final navigator = tester.state<NavigatorState>(find.byType(Navigator));
    final result = navigator.push<String>(
      MaterialPageRoute(builder: (_) => MorphQrScannerPage(camera: camera)),
    );
    await tester.pumpAndSettle();
    return result;
  }

  testWidgets('the first code closes the scanner with its text', (
    tester,
  ) async {
    late ValueChanged<String> onCode;
    final result = await open(tester, (cb) {
      onCode = cb;
      return const SizedBox.expand(key: Key('fake-camera'));
    });
    expect(find.byKey(const Key('fake-camera')), findsOneWidget);
    expect(find.text('Place the code in front of the camera'), findsOneWidget);
    onCode('  weirdnet-morph://invite/desk ');
    onCode('weirdnet-morph://invite/second');
    await tester.pumpAndSettle();
    expect(await result, 'weirdnet-morph://invite/desk');
    expect(find.byKey(const Key('qr-scanner')), findsNothing);
  });

  testWidgets('cancel and back close it with nothing', (tester) async {
    var result = await open(tester, (_) => const SizedBox.expand());
    await tester.tap(find.byKey(const Key('qr-scanner-cancel')));
    await tester.pumpAndSettle();
    expect(await result, isNull);

    result = await open(tester, (_) => const SizedBox.expand());
    final navigator = tester.state<NavigatorState>(find.byType(Navigator));
    await navigator.maybePop();
    await tester.pumpAndSettle();
    expect(await result, isNull);
  });

  test('no scanner on the test host', () async {
    expect(MorphQrScanner.available, isFalse);
    expect(await MorphQrScanner.scan(), isNull);
  });
}
