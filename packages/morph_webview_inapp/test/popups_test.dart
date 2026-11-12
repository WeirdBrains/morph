import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph_webview_inapp/src/popups.dart';

MorphPopup _popup(
  int id, [
  String url = 'https://accounts.google.com/gsi/select',
]) => MorphPopup(
  windowId: id,
  url: url,
  view: ColoredBox(key: ValueKey('view-$id'), color: Colors.white),
);

void main() {
  test('stack adds, removes and refuses past the cap', () {
    final stack = MorphPopupStack();
    var changes = 0;
    stack.addListener(() => changes++);
    for (var i = 1; i <= MorphPopupStack.maxOpen; i++) {
      expect(stack.push(_popup(i)), isTrue);
    }
    expect(stack.isFull, isTrue);
    expect(stack.push(_popup(99)), isFalse);
    expect(stack.remove(2)?.windowId, 2);
    expect(stack.remove(2), isNull);
    expect(stack.open.map((p) => p.windowId), [1, 3, 4]);
    expect(stack.clear().map((p) => p.windowId), [4, 3, 1]);
    expect(stack.open, isEmpty);
    expect(changes, MorphPopupStack.maxOpen + 2);
  });

  test('labels', () {
    expect(morphPopupLabel('', ''), 'New window');
    expect(
      morphPopupLabel('', 'https://accounts.google.com/x'),
      'accounts.google.com',
    );
    expect(
      morphPopupLabel('Sign in', 'https://accounts.google.com/x'),
      'Sign in · accounts.google.com',
    );
  });

  testWidgets('popups cover the page, which stays mounted, and close', (
    tester,
  ) async {
    final stack = MorphPopupStack();
    final closed = <int>[];
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: MorphPopupOverlay(
            stack: stack,
            onClose: (id) {
              closed.add(id);
              stack.remove(id);
            },
            child: const SizedBox.expand(key: ValueKey('opener')),
          ),
        ),
      ),
    );
    expect(find.byKey(const ValueKey('popup-close-1')), findsNothing);

    final popup = _popup(1);
    stack.push(popup);
    await tester.pump();
    expect(find.byKey(const ValueKey('view-1')), findsOneWidget);
    expect(find.byKey(const ValueKey('opener')), findsOneWidget);
    expect(find.text('accounts.google.com'), findsOneWidget);
    popup.title.value = 'Sign in with Google';
    await tester.pump();
    expect(
      find.text('Sign in with Google · accounts.google.com'),
      findsOneWidget,
    );

    stack.push(_popup(2, 'https://example.com/'));
    await tester.pump();
    // The newest window is on top.
    final top = tester.getRect(find.byKey(const ValueKey('view-2')));
    expect(top, tester.getRect(find.byKey(const ValueKey('view-1'))));

    await tester.tap(find.byKey(const ValueKey('popup-close-2')));
    await tester.pump();
    expect(closed, [2]);
    expect(find.byKey(const ValueKey('view-2')), findsNothing);
    expect(find.byKey(const ValueKey('view-1')), findsOneWidget);

    // The page closing itself (window.close) takes the same path.
    stack.remove(1);
    await tester.pump();
    expect(find.byKey(const ValueKey('view-1')), findsNothing);
    expect(find.byKey(const ValueKey('opener')), findsOneWidget);
  });
}
