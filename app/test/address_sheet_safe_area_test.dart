import 'package:material_ui/material_ui.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/theme/morph_theme.dart';
import 'package:morph/src/widgets/address_sheet.dart';

void main() {
  testWidgets('address field stays below the status bar with keyboard open', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(400, 500);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      MaterialApp(
        theme: MorphTheme.dark,
        home: MediaQuery(
          data: const MediaQueryData(
            size: Size(400, 500),
            padding: EdgeInsets.only(top: 48),
            viewInsets: EdgeInsets.only(bottom: 250),
          ),
          child: const Scaffold(
            body: MorphAddressSheet(
              initialUrl: '',
              canGoBack: false,
              canGoForward: false,
              desktopSite: false,
            ),
          ),
        ),
      ),
    );
    await tester.pump();
    final top = tester
        .getTopLeft(find.byKey(const Key('address-sheet-url')))
        .dy;
    expect(top, greaterThanOrEqualTo(48));
  });
}
