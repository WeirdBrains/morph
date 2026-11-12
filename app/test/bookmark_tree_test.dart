import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/api.dart';

void main() {
  const tree = MorphBookmarkTree(
    folders: [
      MorphBookmarkFolder(id: 'work', name: 'Work', order: 'b'),
      MorphBookmarkFolder(
        id: 'docs',
        name: 'Docs',
        parentId: 'work',
        order: 'a',
      ),
      MorphBookmarkFolder(
        id: 'deep',
        name: 'Deep',
        parentId: 'docs',
        order: 'a',
      ),
    ],
    bookmarks: [
      MorphBookmark(id: 'x', url: 'https://x', order: 'a'),
      MorphBookmark(id: 'y', url: 'https://y', order: 'c'),
      MorphBookmark(id: 'z', url: 'https://z', folderId: 'deep', order: 'a'),
      MorphBookmark(id: 'w', url: 'https://w', folderId: 'work', order: 'b'),
    ],
  );

  test('children interleave folders and bookmarks by order', () {
    expect(tree.children(null).map((n) => n.id), ['x', 'work', 'y']);
    expect(tree.children('work').map((n) => n.id), ['docs', 'w']);
  });

  test('paths, subtrees and counts', () {
    expect(tree.path('deep').map((f) => f.name), ['Work', 'Docs', 'Deep']);
    expect(tree.path(null), isEmpty);
    expect(tree.subtree('work'), {'work', 'docs', 'deep'});
    expect(tree.bookmarksIn('work'), 2);
    expect(tree.bookmarksIn('deep'), 1);
  });
}
