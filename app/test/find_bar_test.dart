import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/theme/morph_theme.dart';
import 'package:morph/src/widgets/find_bar.dart';

void main() {
  test('find replies parse with or without quotes', () {
    var r = parseMorphFindResult('3/1');
    expect([r.total, r.current], [3, 1]);
    r = parseMorphFindResult('"12/12"');
    expect([r.total, r.current], [12, 12]);
    r = parseMorphFindResult('2/5');
    expect([r.total, r.current], [2, 2]);
    r = parseMorphFindResult(null);
    expect([r.total, r.current], [0, 0]);
    r = parseMorphFindResult('undefined');
    expect([r.total, r.current], [0, 0]);
  });

  test('find script encodes the query as a JS string', () {
    final script = morphFindScript('a"b</script>\\', backward: true);
    expect(script, contains(r'"a\"b</script>\\"'));
    expect(script, endsWith(',true,false)'));
    expect(morphFindScript('x', reset: true), endsWith(',false,true)'));
  });

  testWidgets('typing resets, arrows step, and close fires', (tester) async {
    final calls = <String>[];
    var closed = false;
    await tester.pumpWidget(
      MaterialApp(
        theme: MorphTheme.dark,
        home: Scaffold(
          body: MorphFindBar(
            onFind: (q, {required backward, required reset}) async {
              calls.add('$q ${backward ? 'back' : 'next'} $reset');
              return q.isEmpty
                  ? MorphFindResult.none
                  : const MorphFindResult(total: 4, current: 2);
            },
            onClose: () => closed = true,
          ),
        ),
      ),
    );
    await tester.pump();

    expect(find.byKey(const Key('find-count')), findsOneWidget);
    await tester.enterText(find.byKey(const Key('find-field')), 'mo');
    await tester.pump();
    expect(find.text('2/4'), findsOneWidget);

    await tester.tap(find.byKey(const Key('find-next')));
    await tester.tap(find.byKey(const Key('find-prev')));
    await tester.pump();
    expect(calls, ['mo next true', 'mo next false', 'mo back false']);

    await tester.tap(find.byKey(const Key('find-close')));
    expect(closed, isTrue);
  });

  testWidgets('shows No matches when nothing is found', (tester) async {
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: MorphFindBar(
            onFind: (q, {required backward, required reset}) async =>
                MorphFindResult.none,
            onClose: () {},
          ),
        ),
      ),
    );
    await tester.enterText(find.byKey(const Key('find-field')), 'zz');
    await tester.pump();
    expect(find.text('No matches'), findsOneWidget);
  });
}
