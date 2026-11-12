import 'package:flutter/material.dart';

import '../api.dart';
import '../theme/morph_theme.dart';

/// Runs a bookmark write after any earlier ones, so saves never race.
typedef MorphBookmarkWrite = Future<T> Function<T>(Future<T> Function() write);

Future<T> _direct<T>(Future<T> Function() write) => write();

/// Where a dragged item lands relative to the row it is dropped on.
enum BookmarkDropZone { before, into, after }

/// Which zone of a row of [height] a pointer at [dy] (from the row's top)
/// is over. Folders take drops in their middle; bookmarks only split in two.
BookmarkDropZone bookmarkDropZone({
  required double dy,
  required double height,
  required bool folder,
}) {
  if (folder) {
    if (dy < height * 0.25) {
      return BookmarkDropZone.before;
    }
    if (dy > height * 0.75) {
      return BookmarkDropZone.after;
    }
    return BookmarkDropZone.into;
  }
  return dy < height / 2 ? BookmarkDropZone.before : BookmarkDropZone.after;
}

/// Splits what the user typed into tags: commas or spaces separate them.
List<String> parseBookmarkTags(String text) => {
  for (final t in text.split(RegExp(r'[,\s]+')))
    if (t.replaceAll('#', '').trim().isNotEmpty)
      t.replaceAll('#', '').trim().toLowerCase(),
}.toList();

/// One profile's bookmarks: nested folders, tags, search and drag to
/// reorder or move.
///
/// Desktop drags from a handle on each row; phones long-press the row.
/// "Move to…" in each row's menu does the same without dragging.
class MorphBookmarkManager extends StatefulWidget {
  const MorphBookmarkManager({
    super.key,
    required this.api,
    required this.profileId,
    required this.onOpen,
    this.home,
    this.write = _direct,
  });

  final MorphApi api;
  final String profileId;
  final String? home;
  final ValueChanged<String> onOpen;
  final MorphBookmarkWrite write;

  @override
  State<MorphBookmarkManager> createState() => MorphBookmarkManagerState();
}

class MorphBookmarkManagerState extends State<MorphBookmarkManager> {
  final _query = TextEditingController();
  MorphBookmarkTree? _tree;
  List<MorphBookmarkTag> _tags = const [];
  List<MorphBookmark>? _results;
  String? _folderId;
  String? _tag;
  Object? _error;
  int _searchSeq = 0;

  /// Row the drag is over and where it would land.
  (String, BookmarkDropZone)? _hover;

  /// Folder the user is looking at; null is the top level.
  String? get folderId => _folderId;

  bool get _searching => _query.text.trim().isNotEmpty || _tag != null;

  bool get _touch {
    final p = Theme.of(context).platform;
    return p == TargetPlatform.android || p == TargetPlatform.iOS;
  }

  @override
  void initState() {
    super.initState();
    _reload();
  }

  @override
  void dispose() {
    _query.dispose();
    super.dispose();
  }

  Future<void> _reload() async {
    try {
      final tree = await widget.api.bookmarkTree(
        widget.profileId,
        home: widget.home,
      );
      final tags = await widget.api.bookmarkTags(
        widget.profileId,
        home: widget.home,
      );
      if (!mounted) {
        return;
      }
      setState(() {
        _tree = tree;
        _tags = tags;
        _error = null;
        if (_tag != null && !tags.any((t) => t.name == _tag)) {
          _tag = null;
        }
        if (tree.folder(_folderId) == null) {
          _folderId = null;
        }
      });
      await _search();
    } catch (e) {
      if (mounted) {
        setState(() => _error = e);
      }
    }
  }

  Future<void> _search() async {
    if (!_searching) {
      if (_results != null) {
        setState(() => _results = null);
      }
      return;
    }
    final seq = ++_searchSeq;
    final found = await widget.api.searchBookmarks(
      widget.profileId,
      query: _query.text.trim(),
      tag: _tag,
      home: widget.home,
    );
    if (mounted && seq == _searchSeq) {
      setState(() => _results = found);
    }
  }

  Future<void> _apply(Future<MorphBookmarkTree> Function() write) async {
    try {
      await widget.write(write);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.maybeOf(context)
            ?.showSnackBar(SnackBar(content: Text('Could not save: $e')));
      }
    }
    await _reload();
  }

  void _enter(String? folderId) => setState(() => _folderId = folderId);

  // ---- writes -------------------------------------------------------------

  Future<void> _newFolder() async {
    final name = await _askName(context, title: 'New folder');
    if (name == null) {
      return;
    }
    await _apply(
      () => widget.api.createBookmarkFolder(
        widget.profileId,
        name,
        parentId: _folderId,
        home: widget.home,
      ),
    );
  }

  Future<void> _renameFolder(MorphBookmarkFolder f) async {
    final name = await _askName(
      context,
      title: 'Rename folder',
      initial: f.name,
    );
    if (name == null || name == f.name) {
      return;
    }
    await _apply(
      () => widget.api.renameBookmarkFolder(
        widget.profileId,
        f.id,
        name,
        home: widget.home,
      ),
    );
  }

  Future<void> _deleteFolder(MorphBookmarkFolder f) async {
    final tree = _tree ?? MorphBookmarkTree.empty;
    final folders = tree.subtree(f.id).length - 1;
    final bookmarks = tree.bookmarksIn(f.id);
    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        key: const Key('folder-delete-dialog'),
        title: Text('Delete “${f.name}”?'),
        content: Text(_deleteSummary(bookmarks, folders)),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: const Text('Cancel'),
          ),
          TextButton(
            key: const Key('folder-delete-confirm'),
            onPressed: () => Navigator.of(context).pop(true),
            style: TextButton.styleFrom(foregroundColor: MorphColors.magenta),
            child: const Text('Delete'),
          ),
        ],
      ),
    );
    if (ok != true) {
      return;
    }
    await _apply(
      () => widget.api.deleteBookmarkFolder(
        widget.profileId,
        f.id,
        home: widget.home,
      ),
    );
  }

  Future<void> _edit(MorphBookmark b) async {
    final edit = await showDialog<(String, String, List<String>)>(
      context: context,
      builder: (context) => _EditBookmarkDialog(bookmark: b),
    );
    if (edit == null) {
      return;
    }
    final (title, url, tags) = edit;
    await _apply(
      () => widget.api.updateBookmark(
        widget.profileId,
        b.id,
        title: title,
        url: url,
        tags: tags,
        home: widget.home,
      ),
    );
  }

  Future<void> _delete(MorphBookmark b) => _apply(
    () => widget.api.deleteBookmark(widget.profileId, b.id, home: widget.home),
  );

  Future<void> _moveTo(MorphBookmarkNode node) async {
    final tree = _tree ?? MorphBookmarkTree.empty;
    final blocked = node.isFolder ? tree.subtree(node.id) : const <String>{};
    final current = node.folder?.parentId ?? node.bookmark?.folderId;
    final target = await showDialog<_Target>(
      context: context,
      builder: (context) =>
          _MovePicker(tree: tree, blocked: blocked, current: current),
    );
    if (target == null || target.id == current) {
      return;
    }
    await _moveInto(node, target.id);
  }

  /// Moves [node] to the end of folder [parentId].
  Future<void> _moveInto(MorphBookmarkNode node, String? parentId) {
    final tree = _tree ?? MorphBookmarkTree.empty;
    final end = tree.children(parentId).where((n) => n.id != node.id).length;
    return _move(node, parentId, end);
  }

  Future<void> _move(MorphBookmarkNode node, String? parentId, int index) =>
      _apply(
        () => widget.api.moveBookmarkItem(
          widget.profileId,
          node.id,
          parentId: parentId,
          index: index,
          home: widget.home,
        ),
      );

  bool _canDrop(MorphBookmarkNode dragged, String? parentId) {
    if (!dragged.isFolder || parentId == null) {
      return true;
    }
    return !(_tree ?? MorphBookmarkTree.empty)
        .subtree(dragged.id)
        .contains(parentId);
  }

  Future<void> _drop(
    MorphBookmarkNode dragged,
    MorphBookmarkNode target,
    BookmarkDropZone zone,
  ) async {
    setState(() => _hover = null);
    if (dragged.id == target.id) {
      return;
    }
    if (zone == BookmarkDropZone.into) {
      await _moveInto(dragged, target.id);
      return;
    }
    final parent = target.folder?.parentId ?? target.bookmark?.folderId;
    final siblings = (_tree ?? MorphBookmarkTree.empty)
        .children(parent)
        .where((n) => n.id != dragged.id)
        .toList();
    final at = siblings.indexWhere((n) => n.id == target.id);
    await _move(dragged, parent, zone == BookmarkDropZone.before ? at : at + 1);
  }

  // ---- layout -------------------------------------------------------------

  @override
  Widget build(BuildContext context) {
    final tree = _tree;
    if (_error != null) {
      return const _Empty(
        icon: Icons.error_outline,
        text: 'This profile has no bookmarks yet.',
      );
    }
    if (tree == null) {
      return const Center(child: CircularProgressIndicator());
    }
    return Column(
      key: const Key('bookmark-manager'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 12, 16, 4),
          child: TextField(
            key: const Key('bookmark-search'),
            controller: _query,
            decoration: const InputDecoration(
              prefixIcon: Icon(Icons.search),
              hintText: 'Search bookmarks, URLs and tags',
            ),
            onChanged: (_) => _search(),
          ),
        ),
        if (_tags.isNotEmpty) _tagBar(),
        if (!_searching) _crumbs(tree),
        Expanded(child: _searching ? _resultList() : _folderList(tree)),
      ],
    );
  }

  Widget _tagBar() => SizedBox(
    height: 44,
    child: ListView(
      key: const Key('bookmark-tags'),
      scrollDirection: Axis.horizontal,
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
      children: [
        for (final t in _tags)
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 4),
            child: FilterChip(
              key: Key('bookmark-tag-${t.name}'),
              label: Text('#${t.name} ${t.count}'),
              selected: _tag == t.name,
              onSelected: (on) {
                setState(() => _tag = on ? t.name : null);
                _search();
              },
            ),
          ),
      ],
    ),
  );

  Widget _crumbs(MorphBookmarkTree tree) {
    final path = tree.path(_folderId);
    final crumbs = <Widget>[
      _Crumb(
        key: const Key('bookmark-crumb-root'),
        label: 'Bookmarks',
        current: _folderId == null,
        onTap: () => _enter(null),
        canDrop: (n) => _canDrop(n, null),
        onDrop: (n) => _moveInto(n, null),
      ),
      for (final f in path) ...[
        const Icon(Icons.chevron_right, size: 16),
        _Crumb(
          key: Key('bookmark-crumb-${f.id}'),
          label: f.name,
          current: f.id == _folderId,
          onTap: () => _enter(f.id),
          canDrop: (n) => _canDrop(n, f.id),
          onDrop: (n) => _moveInto(n, f.id),
        ),
      ],
    ];
    return Padding(
      padding: const EdgeInsets.fromLTRB(8, 4, 8, 0),
      child: Row(
        children: [
          Expanded(
            child: SingleChildScrollView(
              scrollDirection: Axis.horizontal,
              reverse: true,
              child: Row(children: crumbs),
            ),
          ),
          IconButton(
            key: const Key('bookmark-new-folder'),
            tooltip: 'New folder',
            icon: const Icon(Icons.create_new_folder_outlined),
            onPressed: _newFolder,
          ),
        ],
      ),
    );
  }

  Widget _folderList(MorphBookmarkTree tree) {
    final nodes = tree.children(_folderId);
    if (nodes.isEmpty) {
      return _Empty(
        icon: _folderId == null ? Icons.bookmark_border : Icons.folder_open,
        text: _folderId == null
            ? 'No bookmarks in this profile yet.'
            : 'This folder is empty.',
      );
    }
    return ListView.builder(
      key: const Key('library-bookmarks'),
      itemCount: nodes.length,
      itemBuilder: (context, i) => _draggableRow(nodes[i]),
    );
  }

  Widget _resultList() {
    final list = _results;
    if (list == null) {
      return const Center(child: CircularProgressIndicator());
    }
    if (list.isEmpty) {
      return const _Empty(icon: Icons.search_off, text: 'No bookmarks match.');
    }
    final tree = _tree ?? MorphBookmarkTree.empty;
    return ListView.builder(
      key: const Key('bookmark-results'),
      itemCount: list.length,
      itemBuilder: (context, i) {
        final b = list[i];
        final where = tree.path(b.folderId).map((f) => f.name).join(' / ');
        return _bookmarkRow(b, folderLabel: where, handle: null);
      },
    );
  }

  Widget _draggableRow(MorphBookmarkNode node) {
    final hover = _hover?.$1 == node.id ? _hover!.$2 : null;
    Widget row(Widget? handle) => node.isFolder
        ? _folderRow(node.folder!, handle: handle, hover: hover)
        : _bookmarkRow(node.bookmark!, handle: handle, hover: hover);

    final feedback = Material(
      elevation: 6,
      color: Theme.of(context).colorScheme.surfaceContainerHighest,
      borderRadius: BorderRadius.circular(8),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(
              node.isFolder ? Icons.folder : Icons.bookmark,
              color: MorphColors.cyan,
              size: 18,
            ),
            const SizedBox(width: 8),
            Text(
              node.folder?.name ?? _label(node.bookmark!),
              overflow: TextOverflow.ellipsis,
            ),
          ],
        ),
      ),
    );

    final Widget content;
    if (_touch) {
      content = LongPressDraggable<MorphBookmarkNode>(
        key: Key('bookmark-drag-${node.id}'),
        data: node,
        dragAnchorStrategy: pointerDragAnchorStrategy,
        feedback: feedback,
        childWhenDragging: Opacity(opacity: 0.4, child: row(null)),
        onDragEnd: (_) => setState(() => _hover = null),
        child: row(null),
      );
    } else {
      content = row(
        Draggable<MorphBookmarkNode>(
          key: Key('bookmark-drag-${node.id}'),
          data: node,
          dragAnchorStrategy: pointerDragAnchorStrategy,
          feedback: feedback,
          onDragEnd: (_) => setState(() => _hover = null),
          child: const MouseRegion(
            cursor: SystemMouseCursors.grab,
            child: Padding(
              padding: EdgeInsets.all(4),
              child: Icon(Icons.drag_indicator, size: 20),
            ),
          ),
        ),
      );
    }
    return _DropRow(
      node: node,
      canDrop: (dragged, zone) =>
          dragged.id != node.id &&
          _canDrop(
            dragged,
            zone == BookmarkDropZone.into
                ? node.id
                : node.folder?.parentId ?? node.bookmark?.folderId,
          ),
      onHover: (zone) {
        final next = zone == null ? null : (node.id, zone);
        if (next != _hover) {
          setState(() => _hover = next);
        }
      },
      onDrop: (dragged, zone) => _drop(dragged, node, zone),
      child: content,
    );
  }

  Widget _folderRow(
    MorphBookmarkFolder f, {
    Widget? handle,
    BookmarkDropZone? hover,
  }) {
    final tree = _tree ?? MorphBookmarkTree.empty;
    final count = tree.children(f.id).length;
    return _Highlight(
      zone: hover,
      child: ListTile(
        key: Key('folder-${f.id}'),
        leading: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            ?handle,
            Icon(
              hover == BookmarkDropZone.into ? Icons.folder_open : Icons.folder,
              color: MorphColors.cyan,
            ),
          ],
        ),
        title: Text(f.name, maxLines: 1, overflow: TextOverflow.ellipsis),
        subtitle: Text(count == 1 ? '1 item' : '$count items'),
        onTap: () => _enter(f.id),
        trailing: PopupMenuButton<String>(
          key: Key('folder-menu-${f.id}'),
          tooltip: 'Folder actions',
          onSelected: (action) {
            switch (action) {
              case 'rename':
                _renameFolder(f);
              case 'move':
                _moveTo(MorphBookmarkNode.folder(f));
              case 'delete':
                _deleteFolder(f);
            }
          },
          itemBuilder: (context) => const [
            PopupMenuItem(
              key: Key('folder-rename'),
              value: 'rename',
              child: Text('Rename'),
            ),
            PopupMenuItem(
              key: Key('folder-move'),
              value: 'move',
              child: Text('Move to…'),
            ),
            PopupMenuItem(
              key: Key('folder-delete'),
              value: 'delete',
              child: Text('Delete'),
            ),
          ],
        ),
      ),
    );
  }

  Widget _bookmarkRow(
    MorphBookmark b, {
    Widget? handle,
    BookmarkDropZone? hover,
    String folderLabel = '',
  }) {
    final colors = Theme.of(context).colorScheme;
    final muted = colors.onSurface.withValues(alpha: 0.6);
    return _Highlight(
      zone: hover,
      child: ListTile(
        key: Key('bookmark-${b.url}'),
        leading: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            ?handle,
            const Icon(Icons.bookmark, color: MorphColors.cyan),
          ],
        ),
        title: Text(_label(b), maxLines: 1, overflow: TextOverflow.ellipsis),
        subtitle: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              b.url,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: MorphTheme.monoStyle(color: muted, size: 12),
            ),
            if (b.tags.isNotEmpty || folderLabel.isNotEmpty)
              Text(
                [
                  if (folderLabel.isNotEmpty) folderLabel,
                  ...b.tags.map((t) => '#$t'),
                ].join('  '),
                key: Key('bookmark-tags-${b.id}'),
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(color: MorphColors.magenta, fontSize: 12),
              ),
          ],
        ),
        onTap: () => widget.onOpen(b.url),
        trailing: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            PopupMenuButton<String>(
              key: Key('bookmark-menu-${b.id}'),
              tooltip: 'Bookmark actions',
              onSelected: (action) {
                switch (action) {
                  case 'edit':
                    _edit(b);
                  case 'move':
                    _moveTo(MorphBookmarkNode.bookmark(b));
                }
              },
              itemBuilder: (context) => const [
                PopupMenuItem(
                  key: Key('bookmark-edit'),
                  value: 'edit',
                  child: Text('Edit'),
                ),
                PopupMenuItem(
                  key: Key('bookmark-move'),
                  value: 'move',
                  child: Text('Move to…'),
                ),
              ],
            ),
            IconButton(
              tooltip: 'Remove bookmark',
              icon: const Icon(Icons.close),
              onPressed: () => _delete(b),
            ),
          ],
        ),
      ),
    );
  }
}

String _label(MorphBookmark b) => b.title.isEmpty ? b.url : b.title;

String _deleteSummary(int bookmarks, int folders) {
  if (bookmarks == 0 && folders == 0) {
    return 'The folder is empty.';
  }
  String n(int count, String one) => count == 1 ? '1 $one' : '$count ${one}s';
  final parts = [
    if (bookmarks > 0) n(bookmarks, 'bookmark'),
    if (folders > 0) n(folders, 'folder'),
  ];
  return 'This also deletes ${parts.join(' and ')} inside it.';
}

/// A row that takes drops, telling the manager which zone the drag is over.
class _DropRow extends StatelessWidget {
  const _DropRow({
    required this.node,
    required this.canDrop,
    required this.onHover,
    required this.onDrop,
    required this.child,
  });

  final MorphBookmarkNode node;
  final bool Function(MorphBookmarkNode dragged, BookmarkDropZone zone) canDrop;
  final ValueChanged<BookmarkDropZone?> onHover;
  final void Function(MorphBookmarkNode dragged, BookmarkDropZone zone) onDrop;
  final Widget child;

  BookmarkDropZone? _zone(BuildContext context, Offset global) {
    final box = context.findRenderObject() as RenderBox?;
    if (box == null || !box.hasSize) {
      return null;
    }
    final local = box.globalToLocal(global);
    return bookmarkDropZone(
      dy: local.dy,
      height: box.size.height,
      folder: node.isFolder,
    );
  }

  @override
  Widget build(BuildContext context) {
    return DragTarget<MorphBookmarkNode>(
      onWillAcceptWithDetails: (d) => d.data.id != node.id,
      onMove: (d) {
        final zone = _zone(context, d.offset);
        onHover(zone != null && canDrop(d.data, zone) ? zone : null);
      },
      onLeave: (_) => onHover(null),
      onAcceptWithDetails: (d) {
        final zone = _zone(context, d.offset);
        if (zone != null && canDrop(d.data, zone)) {
          onDrop(d.data, zone);
        } else {
          onHover(null);
        }
      },
      builder: (context, _, _) => child,
    );
  }
}

/// Shows where a drop would land: a line above or below, or a filled row.
class _Highlight extends StatelessWidget {
  const _Highlight({required this.zone, required this.child});

  final BookmarkDropZone? zone;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    const line = BorderSide(color: MorphColors.cyan, width: 2);
    return DecoratedBox(
      decoration: BoxDecoration(
        color: zone == BookmarkDropZone.into
            ? MorphColors.cyan.withValues(alpha: 0.15)
            : null,
        border: Border(
          top: zone == BookmarkDropZone.before ? line : BorderSide.none,
          bottom: zone == BookmarkDropZone.after ? line : BorderSide.none,
        ),
      ),
      child: Material(type: MaterialType.transparency, child: child),
    );
  }
}

class _Crumb extends StatelessWidget {
  const _Crumb({
    super.key,
    required this.label,
    required this.current,
    required this.onTap,
    required this.canDrop,
    required this.onDrop,
  });

  final String label;
  final bool current;
  final VoidCallback onTap;
  final bool Function(MorphBookmarkNode) canDrop;
  final ValueChanged<MorphBookmarkNode> onDrop;

  @override
  Widget build(BuildContext context) {
    return DragTarget<MorphBookmarkNode>(
      onWillAcceptWithDetails: (d) => canDrop(d.data),
      onAcceptWithDetails: (d) => onDrop(d.data),
      builder: (context, candidates, _) => TextButton(
        onPressed: current ? null : onTap,
        style: TextButton.styleFrom(
          backgroundColor: candidates.isNotEmpty
              ? MorphColors.cyan.withValues(alpha: 0.2)
              : null,
          disabledForegroundColor: Theme.of(context).colorScheme.onSurface,
        ),
        child: Text(
          label,
          style: TextStyle(
            fontWeight: current ? FontWeight.w600 : FontWeight.normal,
          ),
        ),
      ),
    );
  }
}

class _Target {
  const _Target(this.id);
  final String? id;
}

/// Picks a destination folder; [blocked] folders (a folder and what is
/// inside it) cannot be chosen.
class _MovePicker extends StatelessWidget {
  const _MovePicker({
    required this.tree,
    required this.blocked,
    required this.current,
  });

  final MorphBookmarkTree tree;
  final Set<String> blocked;
  final String? current;

  @override
  Widget build(BuildContext context) {
    final rows = <Widget>[
      _option(context, null, 'Bookmarks', 0, Icons.bookmarks_outlined),
    ];
    void walk(String? parent, int depth) {
      for (final node in tree.children(parent)) {
        final f = node.folder;
        if (f == null) {
          continue;
        }
        rows.add(_option(context, f.id, f.name, depth, Icons.folder));
        walk(f.id, depth + 1);
      }
    }

    walk(null, 1);
    return AlertDialog(
      key: const Key('move-picker'),
      title: const Text('Move to'),
      content: SizedBox(
        width: 360,
        child: ListView(shrinkWrap: true, children: rows),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: const Text('Cancel'),
        ),
      ],
    );
  }

  Widget _option(
    BuildContext context,
    String? id,
    String name,
    int depth,
    IconData icon,
  ) {
    final disabled = id != null && blocked.contains(id);
    return ListTile(
      key: Key('move-target-${id ?? 'root'}'),
      enabled: !disabled,
      contentPadding: EdgeInsets.only(left: 8.0 + depth * 20, right: 8),
      leading: Icon(icon, color: disabled ? null : MorphColors.cyan),
      title: Text(name),
      trailing: id == current ? const Icon(Icons.check, size: 18) : null,
      onTap: () => Navigator.of(context).pop(_Target(id)),
    );
  }
}

class _EditBookmarkDialog extends StatefulWidget {
  const _EditBookmarkDialog({required this.bookmark});

  final MorphBookmark bookmark;

  @override
  State<_EditBookmarkDialog> createState() => _EditBookmarkDialogState();
}

class _EditBookmarkDialogState extends State<_EditBookmarkDialog> {
  late final _title = TextEditingController(text: widget.bookmark.title);
  late final _url = TextEditingController(text: widget.bookmark.url);
  late final _tags = TextEditingController(
    text: widget.bookmark.tags.join(', '),
  );

  @override
  void dispose() {
    _title.dispose();
    _url.dispose();
    _tags.dispose();
    super.dispose();
  }

  void _save() {
    final url = _url.text.trim();
    if (url.isEmpty) {
      return;
    }
    Navigator.of(context)
        .pop((_title.text.trim(), url, parseBookmarkTags(_tags.text)));
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      key: const Key('bookmark-edit-dialog'),
      title: const Text('Edit bookmark'),
      content: SizedBox(
        width: 400,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            TextField(
              key: const Key('bookmark-edit-title'),
              controller: _title,
              decoration: const InputDecoration(labelText: 'Name'),
            ),
            TextField(
              key: const Key('bookmark-edit-url'),
              controller: _url,
              decoration: const InputDecoration(labelText: 'URL'),
              keyboardType: TextInputType.url,
            ),
            TextField(
              key: const Key('bookmark-edit-tags'),
              controller: _tags,
              decoration: const InputDecoration(
                labelText: 'Tags',
                hintText: 'work, recipes',
              ),
              onSubmitted: (_) => _save(),
            ),
          ],
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: const Text('Cancel'),
        ),
        TextButton(
          key: const Key('bookmark-edit-save'),
          onPressed: _save,
          child: const Text('Save'),
        ),
      ],
    );
  }
}

Future<String?> _askName(
  BuildContext context, {
  required String title,
  String initial = '',
}) => showDialog<String>(
  context: context,
  builder: (context) => _FolderNameDialog(title: title, initial: initial),
);

class _FolderNameDialog extends StatefulWidget {
  const _FolderNameDialog({required this.title, required this.initial});

  final String title;
  final String initial;

  @override
  State<_FolderNameDialog> createState() => _FolderNameDialogState();
}

class _FolderNameDialogState extends State<_FolderNameDialog> {
  late final _name = TextEditingController(text: widget.initial);

  @override
  void dispose() {
    _name.dispose();
    super.dispose();
  }

  void _done() {
    final name = _name.text.trim();
    if (name.isNotEmpty) {
      Navigator.of(context).pop(name);
    }
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      key: const Key('folder-name-dialog'),
      title: Text(widget.title),
      content: TextField(
        key: const Key('folder-name-field'),
        controller: _name,
        autofocus: true,
        decoration: const InputDecoration(labelText: 'Folder name'),
        onSubmitted: (_) => _done(),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: const Text('Cancel'),
        ),
        TextButton(
          key: const Key('folder-name-save'),
          onPressed: _done,
          child: const Text('Save'),
        ),
      ],
    );
  }
}

class _Empty extends StatelessWidget {
  const _Empty({required this.icon, required this.text});

  final IconData icon;
  final String text;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(icon, size: 40, color: colors.onSurface.withValues(alpha: 0.4)),
          const SizedBox(height: 12),
          Text(
            text,
            style: TextStyle(color: colors.onSurface.withValues(alpha: 0.7)),
          ),
        ],
      ),
    );
  }
}
