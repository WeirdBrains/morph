import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/api.dart';
import 'package:morph/src/theme/morph_theme.dart';
import 'package:morph/src/widgets/bookmark_manager.dart';

import 'support/fake_morph_api.dart';

Future<List<String>> _pump(
  WidgetTester tester,
  FakeMorphApi api, {
  required TargetPlatform platform,
  required Size size,
  String profileId = 'work',
}) async {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  final opened = <String>[];
  await tester.pumpWidget(
    MaterialApp(
      theme: MorphTheme.dark.copyWith(platform: platform),
      home: Scaffold(
        body: MorphBookmarkManager(
          api: api,
          profileId: profileId,
          onOpen: opened.add,
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
  return opened;
}

const _desktop = (TargetPlatform.linux, Size(1280, 800));
const _phone = (TargetPlatform.android, Size(412, 915));

/// Titles directly in [parent], in order, folders in brackets.
Future<List<String>> _names(
  FakeMorphApi api,
  String? parent, [
  String profile = 'work',
]) async {
  final tree = await api.bookmarkTree(profile);
  return [
    for (final n in tree.children(parent))
      n.isFolder ? '[${n.folder!.name}]' : n.bookmark!.title,
  ];
}

Future<void> _newFolder(WidgetTester tester, String name) async {
  await tester.tap(find.byKey(const Key('bookmark-new-folder')));
  await tester.pumpAndSettle();
  await tester.enterText(find.byKey(const Key('folder-name-field')), name);
  await tester.tap(find.byKey(const Key('folder-name-save')));
  await tester.pumpAndSettle();
}

/// Drags from [from] to [to]; on touch a long press starts the drag.
Future<void> _drag(
  WidgetTester tester,
  Offset from,
  Offset to, {
  bool longPress = false,
}) async {
  final gesture = await tester.startGesture(
    from,
    kind: longPress ? PointerDeviceKind.touch : PointerDeviceKind.mouse,
  );
  if (longPress) {
    await tester.pump(kLongPressTimeout + const Duration(milliseconds: 100));
  }
  await gesture.moveBy(const Offset(0, 12));
  await tester.pump();
  await gesture.moveTo(to);
  await tester.pump();
  await gesture.moveTo(to + const Offset(0, 1));
  await tester.pump();
  await gesture.up();
  await tester.pumpAndSettle();
}

Offset _handle(WidgetTester tester, String id) =>
    tester.getCenter(find.byKey(Key('bookmark-drag-$id')));

/// A point [fraction] of the way down the row with [key].
Offset _rowAt(WidgetTester tester, String key, double fraction) {
  final rect = tester.getRect(find.byKey(Key(key)));
  return Offset(rect.center.dx, rect.top + rect.height * fraction);
}

void main() {
  test('drop zones', () {
    BookmarkDropZone z(double dy, bool folder) =>
        bookmarkDropZone(dy: dy, height: 100, folder: folder);
    expect(z(10, true), BookmarkDropZone.before);
    expect(z(50, true), BookmarkDropZone.into);
    expect(z(90, true), BookmarkDropZone.after);
    expect(z(40, false), BookmarkDropZone.before);
    expect(z(60, false), BookmarkDropZone.after);
  });

  test('tag parsing', () {
    expect(parseBookmarkTags(' Work, #Recipes  work,,x '), [
      'work',
      'recipes',
      'x',
    ]);
    expect(parseBookmarkTags('  '), isEmpty);
  });

  testWidgets('desktop: folders, drag into, reorder and back out', (
    tester,
  ) async {
    final api = FakeMorphApi();
    await api.addBookmark('work', 'https://c.test/', 'C'); // b1
    await api.addBookmark('work', 'https://b.test/', 'B'); // b2
    await api.addBookmark('work', 'https://a.test/', 'A'); // b3
    final opened = await _pump(
      tester,
      api,
      platform: _desktop.$1,
      size: _desktop.$2,
    );

    await _newFolder(tester, 'Reading');
    expect(await _names(api, null), ['A', 'B', 'C', '[Reading]']);
    final folderId = (await api.bookmarkTree('work')).folders.single.id;
    expect(find.text('0 items'), findsOneWidget);

    // Drop B on the middle of the folder: it moves inside.
    await _drag(
      tester,
      _handle(tester, 'b2'),
      _rowAt(tester, 'folder-$folderId', 0.5),
    );
    expect(await _names(api, null), ['A', 'C', '[Reading]']);
    expect(await _names(api, folderId), ['B']);

    // Drop C on the top of A: it goes before A.
    await _drag(
      tester,
      _handle(tester, 'b1'),
      _rowAt(tester, 'bookmark-https://a.test/', 0.1),
    );
    expect(await _names(api, null), ['C', 'A', '[Reading]']);

    // Drop the folder above everything.
    await _drag(
      tester,
      _handle(tester, folderId),
      _rowAt(tester, 'bookmark-https://c.test/', 0.1),
    );
    expect(await _names(api, null), ['[Reading]', 'C', 'A']);

    // Enter the folder, then drag B onto the "Bookmarks" crumb.
    await tester.tap(find.byKey(Key('folder-$folderId')));
    await tester.pumpAndSettle();
    expect(find.byKey(Key('bookmark-crumb-$folderId')), findsOneWidget);
    expect(find.byKey(const Key('bookmark-https://b.test/')), findsOneWidget);
    expect(find.byKey(const Key('bookmark-https://a.test/')), findsNothing);
    await _drag(
      tester,
      _handle(tester, 'b2'),
      tester.getCenter(find.byKey(const Key('bookmark-crumb-root'))),
    );
    expect(await _names(api, folderId), isEmpty);
    expect(find.text('This folder is empty.'), findsOneWidget);

    await tester.tap(find.byKey(const Key('bookmark-crumb-root')));
    await tester.pumpAndSettle();
    expect(await _names(api, null), ['[Reading]', 'C', 'A', 'B']);
    await tester.tap(find.text('B'));
    expect(opened, ['https://b.test/']);
  });

  testWidgets('desktop: nested folders, rename, cycle guard and delete', (
    tester,
  ) async {
    final api = FakeMorphApi();
    await api.addBookmark('work', 'https://x.test/', 'X');
    final outer = (await api.createBookmarkFolder(
      'work',
      'Outer',
    )).folders.single.id;
    final tree = await api.createBookmarkFolder(
      'work',
      'Inner',
      parentId: outer,
    );
    final inner = tree.folders.firstWhere((f) => f.name == 'Inner').id;
    await api.moveBookmarkItem(
      'work',
      (await api.listBookmarks('work')).single.id,
      parentId: inner,
      index: 0,
    );
    await _pump(tester, api, platform: _desktop.$1, size: _desktop.$2);

    // Rename.
    await tester.tap(find.byKey(Key('folder-menu-$outer')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('folder-rename')));
    await tester.pumpAndSettle();
    await tester.enterText(find.byKey(const Key('folder-name-field')), 'Work');
    await tester.tap(find.byKey(const Key('folder-name-save')));
    await tester.pumpAndSettle();
    expect(find.text('Work'), findsOneWidget);

    // Outer cannot move into itself or Inner.
    await tester.tap(find.byKey(Key('folder-menu-$outer')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('folder-move')));
    await tester.pumpAndSettle();
    expect(
      tester.widget<ListTile>(find.byKey(Key('move-target-$inner'))).enabled,
      isFalse,
    );
    expect(
      tester.widget<ListTile>(find.byKey(Key('move-target-$outer'))).enabled,
      isFalse,
    );
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();

    // Breadcrumbs follow the path down.
    await tester.tap(find.byKey(Key('folder-$outer')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(Key('folder-$inner')));
    await tester.pumpAndSettle();
    expect(find.byKey(Key('bookmark-crumb-$outer')), findsOneWidget);
    expect(find.byKey(Key('bookmark-crumb-$inner')), findsOneWidget);
    expect(find.text('X'), findsOneWidget);
    await tester.tap(find.byKey(const Key('bookmark-crumb-root')));
    await tester.pumpAndSettle();

    // Deleting asks first and counts what goes with it.
    await tester.tap(find.byKey(Key('folder-menu-$outer')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('folder-delete')));
    await tester.pumpAndSettle();
    expect(
      find.text('This also deletes 1 bookmark and 1 folder inside it.'),
      findsOneWidget,
    );
    await tester.tap(find.byKey(const Key('folder-delete-confirm')));
    await tester.pumpAndSettle();
    final after = await api.bookmarkTree('work');
    expect(after.folders, isEmpty);
    expect(after.bookmarks, isEmpty);
    expect(find.text('No bookmarks in this profile yet.'), findsOneWidget);
  });

  testWidgets('tags: edit, filter by tag and search title, URL and tags', (
    tester,
  ) async {
    final api = FakeMorphApi();
    await api.addBookmark('work', 'https://pasta.test/', 'Pasta'); // b1
    await api.addBookmark('work', 'https://rust-lang.org/', 'Rust'); // b2
    await _pump(tester, api, platform: _desktop.$1, size: _desktop.$2);
    expect(find.byKey(const Key('bookmark-tags')), findsNothing);

    await tester.tap(find.byKey(const Key('bookmark-menu-b1')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('bookmark-edit')));
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byKey(const Key('bookmark-edit-title')),
      'Carbonara',
    );
    await tester.enterText(
      find.byKey(const Key('bookmark-edit-tags')),
      'Recipes, #dinner',
    );
    await tester.tap(find.byKey(const Key('bookmark-edit-save')));
    await tester.pumpAndSettle();
    final saved = (await api.bookmarkTree('work')).bookmarks
        .firstWhere((b) => b.id == 'b1');
    expect(saved.title, 'Carbonara');
    expect(saved.tags, ['recipes', 'dinner']);
    expect(
      find.textContaining('#recipes  #dinner', findRichText: true),
      findsOneWidget,
    );

    // Tag chips filter.
    await tester.tap(find.byKey(const Key('bookmark-tag-recipes')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('bookmark-results')), findsOneWidget);
    expect(find.text('Carbonara'), findsOneWidget);
    expect(find.text('Rust'), findsNothing);
    await tester.tap(find.byKey(const Key('bookmark-tag-recipes')));
    await tester.pumpAndSettle();
    expect(find.text('Rust'), findsOneWidget);

    // Search matches URL and tag words.
    for (final (query, hit) in [
      ('rust-lang', 'Rust'),
      ('dinner', 'Carbonara'),
      ('carb', 'Carbonara'),
    ]) {
      await tester.enterText(find.byKey(const Key('bookmark-search')), query);
      await tester.pumpAndSettle();
      expect(find.text(hit), findsOneWidget, reason: query);
      expect(
        find.text(hit == 'Rust' ? 'Carbonara' : 'Rust'),
        findsNothing,
        reason: query,
      );
    }
    // Results say which folder a match lives in.
    final folder = (await api.createBookmarkFolder(
      'work',
      'Food',
    )).folders.single.id;
    await api.moveBookmarkItem('work', 'b1', parentId: folder, index: 0);
    await tester.enterText(find.byKey(const Key('bookmark-search')), 'pasta');
    await tester.pumpAndSettle();
    expect(find.textContaining('▸ Food', findRichText: true), findsOneWidget);

    await tester.enterText(find.byKey(const Key('bookmark-search')), 'zzz');
    await tester.pumpAndSettle();
    expect(find.text('No bookmarks match.'), findsOneWidget);
  });

  testWidgets('phone: long-press drag and Move to…', (tester) async {
    final api = FakeMorphApi();
    await api.addBookmark('work', 'https://b.test/', 'B'); // b1
    await api.addBookmark('work', 'https://a.test/', 'A'); // b2
    await api.createBookmarkFolder('work', 'Later'); // f3
    await _pump(tester, api, platform: _phone.$1, size: _phone.$2);
    // Phones drag the whole row; there is no handle.
    expect(find.byIcon(Icons.drag_indicator), findsNothing);
    expect(find.byType(LongPressDraggable<MorphBookmarkNode>), findsWidgets);

    await _drag(
      tester,
      tester.getCenter(find.text('A')),
      _rowAt(tester, 'bookmark-https://b.test/', 0.9),
      longPress: true,
    );
    expect(await _names(api, null), ['B', 'A', '[Later]']);

    await _drag(
      tester,
      tester.getCenter(find.text('B')),
      _rowAt(tester, 'folder-f3', 0.5),
      longPress: true,
    );
    expect(await _names(api, 'f3'), ['B']);

    await tester.tap(find.byKey(const Key('bookmark-menu-b2')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('bookmark-move')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('move-target-f3')));
    await tester.pumpAndSettle();
    expect(await _names(api, null), ['[Later]']);
    expect(await _names(api, 'f3'), ['B', 'A']);
    expect(find.text('2 items'), findsOneWidget);
  });

  testWidgets('each profile has its own folders and tags', (tester) async {
    final api = FakeMorphApi();
    await api.createBookmarkFolder('default', 'Home stuff');
    await api.addBookmark('default', 'https://home.test/', 'Home');
    final id = (await api.listBookmarks('default')).single.id;
    await api.updateBookmark('default', id, tags: ['family']);
    await api.addBookmark('work', 'https://work.test/', 'Work');
    await _pump(tester, api, platform: _desktop.$1, size: _desktop.$2);

    expect(find.text('Work'), findsOneWidget);
    expect(find.text('Home stuff'), findsNothing);
    expect(find.byKey(const Key('bookmark-tag-family')), findsNothing);
    await _newFolder(tester, 'Mine');
    expect((await api.bookmarkTree('default')).folders.map((f) => f.name), [
      'Home stuff',
    ]);
  });
}
