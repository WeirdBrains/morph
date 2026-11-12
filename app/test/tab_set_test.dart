import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/tabs/tab_set.dart';

void main() {
  test('starts with one empty active tab', () {
    final set = MorphTabSet();
    expect(set.length, 1);
    expect(set.activeId, 1);
    expect(set.active.url, isEmpty);
    expect(set.active.label, 'New tab');
  });

  test('open inserts after the active tab and selects it', () {
    final set = MorphTabSet();
    set.update(1, url: 'https://a.example/', title: 'A');
    final second = set.open('https://b.example/');
    final third = set.open('https://c.example/');
    expect(set.length, 3);
    expect(set.activeId, third);
    expect(set.tabs.map((t) => t.id).toList(), [1, second, third]);
  });

  test('switch and close follow the core tab-set rules', () {
    final set = MorphTabSet();
    final b = set.open('https://b.example/');
    final c = set.open('https://c.example/');
    set.switchTo(b);
    expect(set.close(b), c);
    expect(set.length, 2);
    expect(set.tab(b), isNull);

    expect(set.close(c), 1);
    expect(set.close(1), isNot(1));
    expect(set.length, 1);
    expect(set.active.url, isEmpty);
  });

  test('closeAll and cycle wrap', () {
    final set = MorphTabSet();
    set.open('b');
    final c = set.open('c');
    expect(set.activeId, c);
    expect(set.cycleNext(), 1);
    expect(set.cycleNext(), isNot(1));
    expect(set.cyclePrev(), 1);
    final first = set.tabs.first.id;
    final active = set.closeAll();
    expect(set.length, 1);
    expect(active, isNot(first));
  });

  test('label falls back from title to host to New tab', () {
    final tab = MorphTab(id: 1);
    expect(tab.label, 'New tab');
    tab.url = 'https://www.linkedin.com/feed/';
    expect(tab.label, 'linkedin.com/feed/');
    tab.title = 'Feed | LinkedIn';
    expect(tab.label, 'Feed | LinkedIn');
  });
}
