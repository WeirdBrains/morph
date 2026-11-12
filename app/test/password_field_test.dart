import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/widgets/password_field.dart';

void main() {
  testWidgets('the eye button shows and hides what was typed', (tester) async {
    final controller = TextEditingController();
    addTearDown(controller.dispose);
    await tester.pumpWidget(
      MaterialApp(
        home: Material(
          child: MorphPasswordField(controller: controller, label: 'Password'),
        ),
      ),
    );
    await tester.enterText(find.byType(TextField), 'hunter22');
    TextField field() => tester.widget<TextField>(find.byType(TextField));
    expect(field().obscureText, isTrue);
    expect(find.byTooltip('Show password'), findsOneWidget);

    await tester.tap(find.byKey(const Key('password-visibility')));
    await tester.pump();
    expect(field().obscureText, isFalse);
    expect(find.byTooltip('Hide password'), findsOneWidget);
    expect(controller.text, 'hunter22');

    await tester.tap(find.byKey(const Key('password-visibility')));
    await tester.pump();
    expect(field().obscureText, isTrue);
  });

  test('every obscured field in the app is a MorphPasswordField', () {
    final offenders = <String>[];
    for (final entity in Directory('lib').listSync(recursive: true)) {
      if (entity is! File || !entity.path.endsWith('.dart')) {
        continue;
      }
      if (entity.path.endsWith('password_field.dart') ||
          entity.path.contains('/rust/')) {
        continue;
      }
      if (entity.readAsStringSync().contains('obscureText')) {
        offenders.add(entity.path);
      }
    }
    expect(offenders, isEmpty, reason: 'use MorphPasswordField instead');
  });
}
