import 'package:flutter/material.dart';

import '../api.dart';
import '../theme/morph_theme.dart';

/// Which list [MorphLibraryPage] opens on.
enum MorphLibraryTab { bookmarks, history }

/// Runs a library write after any earlier ones, so saves never race.
typedef MorphLibraryWrite = Future<T> Function<T>(Future<T> Function() write);

Future<T> _direct<T>(Future<T> Function() write) => write();

/// Bookmarks and history for one Morph profile.
///
/// Everything shown here comes from that profile's own store in the core, so
/// switching profiles shows a different library.
class MorphLibraryPage extends StatefulWidget {
  const MorphLibraryPage({
    super.key,
    required this.api,
    required this.profileId,
    required this.profileName,
    required this.onOpen,
    this.home,
    this.initialTab = MorphLibraryTab.bookmarks,
    this.write = _direct,
  });

  final MorphApi api;
  final String profileId;
  final String profileName;
  final String? home;
  final MorphLibraryTab initialTab;
  final ValueChanged<String> onOpen;
  final MorphLibraryWrite write;

  @override
  State<MorphLibraryPage> createState() => _MorphLibraryPageState();
}

class _MorphLibraryPageState extends State<MorphLibraryPage>
    with SingleTickerProviderStateMixin {
  late final TabController _tabs = TabController(
    length: 2,
    vsync: this,
    initialIndex: widget.initialTab.index,
  );
  final _query = TextEditingController();
  List<MorphBookmark>? _bookmarks;
  List<MorphHistoryEntry>? _history;
  Object? _error;

  @override
  void initState() {
    super.initState();
    _tabs.addListener(() => setState(() {}));
    _reload();
  }

  @override
  void dispose() {
    _tabs.dispose();
    _query.dispose();
    super.dispose();
  }

  Future<void> _reload() async {
    try {
      final bookmarks = await widget.api.listBookmarks(
        widget.profileId,
        home: widget.home,
      );
      final history = await widget.api.listHistory(
        widget.profileId,
        query: _query.text.trim(),
        home: widget.home,
      );
      if (mounted) {
        setState(() {
          _bookmarks = bookmarks;
          _history = history;
          _error = null;
        });
      }
    } catch (e) {
      if (mounted) {
        setState(() => _error = e);
      }
    }
  }

  void _open(String url) {
    Navigator.of(context).maybePop();
    widget.onOpen(url);
  }

  Future<void> _removeBookmark(MorphBookmark bookmark) async {
    final list = await widget.write(
      () => widget.api.removeBookmark(
        widget.profileId,
        bookmark.url,
        home: widget.home,
      ),
    );
    if (mounted) {
      setState(() => _bookmarks = list);
    }
  }

  Future<void> _deleteVisit(MorphHistoryEntry entry) async {
    await widget.write(
      () => widget.api.deleteHistoryUrl(
        widget.profileId,
        entry.url,
        home: widget.home,
      ),
    );
    await _reload();
  }

  Future<void> _clearHistory() async {
    final ok = await confirmClearHistory(context, widget.profileName);
    if (!ok) {
      return;
    }
    await widget.write(
      () => widget.api.clearHistory(widget.profileId, home: widget.home),
    );
    await _reload();
  }

  @override
  Widget build(BuildContext context) {
    final history = _tabs.index == MorphLibraryTab.history.index;
    return Scaffold(
      key: const Key('library-page'),
      appBar: AppBar(
        title: Text('Library · ${widget.profileName}'),
        actions: [
          if (history)
            IconButton(
              key: const Key('library-clear-history'),
              tooltip: 'Clear history',
              icon: const Icon(Icons.delete_sweep_outlined),
              onPressed: (_history?.isEmpty ?? true) ? null : _clearHistory,
            ),
        ],
        bottom: TabBar(
          controller: _tabs,
          indicatorColor: MorphColors.cyan,
          tabs: const [
            Tab(key: Key('library-tab-bookmarks'), text: 'Bookmarks'),
            Tab(key: Key('library-tab-history'), text: 'History'),
          ],
        ),
      ),
      body: _error != null
          ? _Empty(
              icon: Icons.error_outline,
              text: 'This profile has no library yet.',
            )
          : TabBarView(
              controller: _tabs,
              children: [_bookmarkList(), _historyList()],
            ),
    );
  }

  Widget _bookmarkList() {
    final list = _bookmarks;
    if (list == null) {
      return const Center(child: CircularProgressIndicator());
    }
    if (list.isEmpty) {
      return const _Empty(
        icon: Icons.bookmark_border,
        text: 'No bookmarks in this profile yet.',
      );
    }
    return ListView.builder(
      key: const Key('library-bookmarks'),
      itemCount: list.length,
      itemBuilder: (context, i) {
        final b = list[i];
        return _Row(
          key: Key('bookmark-${b.url}'),
          icon: Icons.bookmark,
          title: b.title,
          url: b.url,
          onTap: () => _open(b.url),
          onDelete: () => _removeBookmark(b),
          deleteTooltip: 'Remove bookmark',
        );
      },
    );
  }

  Widget _historyList() {
    final list = _history;
    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 12, 16, 4),
          child: TextField(
            key: const Key('library-history-search'),
            controller: _query,
            decoration: const InputDecoration(
              prefixIcon: Icon(Icons.search),
              hintText: 'Search history',
            ),
            onChanged: (_) => _reload(),
          ),
        ),
        Expanded(
          child: list == null
              ? const Center(child: CircularProgressIndicator())
              : list.isEmpty
              ? const _Empty(icon: Icons.history, text: 'No history here.')
              : ListView.builder(
                  key: const Key('library-history'),
                  itemCount: list.length,
                  itemBuilder: (context, i) {
                    final e = list[i];
                    final day = historyDayLabel(e.visited, DateTime.now());
                    final showDay =
                        i == 0 ||
                        historyDayLabel(list[i - 1].visited, DateTime.now()) !=
                            day;
                    return Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        if (showDay)
                          Padding(
                            padding: const EdgeInsets.fromLTRB(16, 12, 16, 4),
                            child: Text(
                              day,
                              style: const TextStyle(
                                color: MorphColors.cyan,
                                fontWeight: FontWeight.w600,
                                fontSize: 12,
                              ),
                            ),
                          ),
                        _Row(
                          key: Key('history-${e.url}'),
                          icon: Icons.history,
                          title: e.title,
                          url: e.url,
                          onTap: () => _open(e.url),
                          onDelete: () => _deleteVisit(e),
                          deleteTooltip: 'Delete from history',
                        ),
                      ],
                    );
                  },
                ),
        ),
      ],
    );
  }
}

/// "Today", "Yesterday", or a short date for history section headers.
String historyDayLabel(DateTime visited, DateTime now) {
  final day = DateTime(visited.year, visited.month, visited.day);
  final today = DateTime(now.year, now.month, now.day);
  final diff = today.difference(day).inDays;
  if (diff <= 0) {
    return 'Today';
  }
  if (diff == 1) {
    return 'Yesterday';
  }
  const months = [
    'Jan',
    'Feb',
    'Mar',
    'Apr',
    'May',
    'Jun',
    'Jul',
    'Aug',
    'Sep',
    'Oct',
    'Nov',
    'Dec',
  ];
  final label = '${months[day.month - 1]} ${day.day}';
  return day.year == today.year ? label : '$label, ${day.year}';
}

/// Asks before wiping a profile's history. Returns whether to go ahead.
Future<bool> confirmClearHistory(BuildContext context, String profile) async {
  final ok = await showDialog<bool>(
    context: context,
    builder: (context) => AlertDialog(
      key: const Key('clear-history-dialog'),
      title: const Text('Clear history?'),
      content: Text('This removes every visit saved in $profile.'),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(false),
          child: const Text('Cancel'),
        ),
        TextButton(
          key: const Key('clear-history-confirm'),
          onPressed: () => Navigator.of(context).pop(true),
          style: TextButton.styleFrom(foregroundColor: MorphColors.magenta),
          child: const Text('Clear'),
        ),
      ],
    ),
  );
  return ok ?? false;
}

class _Row extends StatelessWidget {
  const _Row({
    super.key,
    required this.icon,
    required this.title,
    required this.url,
    required this.onTap,
    required this.onDelete,
    required this.deleteTooltip,
  });

  final IconData icon;
  final String title;
  final String url;
  final VoidCallback onTap;
  final VoidCallback onDelete;
  final String deleteTooltip;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return ListTile(
      leading: Icon(icon, color: MorphColors.cyan),
      title: Text(
        title.isEmpty ? url : title,
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
      ),
      subtitle: Text(
        url,
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: MorphTheme.monoStyle(
          color: colors.onSurface.withValues(alpha: 0.6),
          size: 12,
        ),
      ),
      trailing: IconButton(
        tooltip: deleteTooltip,
        icon: const Icon(Icons.close),
        onPressed: onDelete,
      ),
      onTap: onTap,
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
