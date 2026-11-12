import 'dart:typed_data';

import 'package:flutter/material.dart';

import '../tabs/tab_set.dart';
import '../theme/morph_theme.dart';

/// Full-screen Firefox-Android-style tab grid for mobile.
class MorphTabSwitcher extends StatefulWidget {
  const MorphTabSwitcher({
    super.key,
    required this.tabs,
    required this.activeId,
    this.previews = const {},
    this.onSelect,
    this.onClose,
    this.onCloseAll,
    this.onNewTab,
  });

  final List<MorphTab> tabs;
  final int activeId;
  final Map<int, Uint8List?> previews;
  final ValueChanged<int>? onSelect;
  final ValueChanged<int>? onClose;
  final VoidCallback? onCloseAll;
  final VoidCallback? onNewTab;

  @override
  State<MorphTabSwitcher> createState() => _MorphTabSwitcherState();
}

class _MorphTabSwitcherState extends State<MorphTabSwitcher> {
  final _search = TextEditingController();

  @override
  void dispose() {
    _search.dispose();
    super.dispose();
  }

  List<MorphTab> get _visible {
    final query = _search.text.trim().toLowerCase();
    if (query.isEmpty) {
      return widget.tabs;
    }
    return widget.tabs
        .where(
          (tab) =>
              tab.label.toLowerCase().contains(query) ||
              tab.url.toLowerCase().contains(query),
        )
        .toList();
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final visible = _visible;
    return Material(
      key: const Key('tab-switcher'),
      color: colors.surface,
      child: SafeArea(
        child: Stack(
          children: [
            Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Padding(
                  padding: const EdgeInsets.fromLTRB(12, 8, 12, 8),
                  child: Row(
                    children: [
                      _Segment(
                        key: const Key('tab-switcher-segment-tabs'),
                        label: '${widget.tabs.length}',
                        caption: 'Tabs',
                        selected: true,
                      ),
                      const SizedBox(width: 8),
                      const _Segment(
                        key: Key('tab-switcher-segment-private'),
                        label: '',
                        caption: 'Private',
                        selected: false,
                        placeholder: true,
                      ),
                      const SizedBox(width: 8),
                      const _Segment(
                        key: Key('tab-switcher-segment-synced'),
                        label: '',
                        caption: 'Synced',
                        selected: false,
                        placeholder: true,
                      ),
                    ],
                  ),
                ),
                ExpansionTile(
                  key: const Key('tab-switcher-inactive'),
                  title: Text(
                    'Inactive tabs',
                    style: MorphTheme.monoStyle(
                      color: colors.onSurfaceVariant,
                      size: 12,
                    ),
                  ),
                  children: const [
                    Padding(
                      padding: EdgeInsets.fromLTRB(16, 0, 16, 12),
                      child: Text('Coming soon'),
                    ),
                  ],
                ),
                Expanded(
                  child: GridView.builder(
                    key: const Key('tab-switcher-grid'),
                    padding: const EdgeInsets.fromLTRB(12, 4, 12, 88),
                    gridDelegate:
                        const SliverGridDelegateWithFixedCrossAxisCount(
                          crossAxisCount: 2,
                          mainAxisSpacing: 10,
                          crossAxisSpacing: 10,
                          childAspectRatio: 0.75,
                        ),
                    itemCount: visible.length,
                    itemBuilder: (context, index) {
                      final tab = visible[index];
                      return MorphTabCard(
                        tab: tab,
                        active: tab.id == widget.activeId,
                        preview: widget.previews[tab.id],
                        onSelect: () => widget.onSelect?.call(tab.id),
                        onClose: () => widget.onClose?.call(tab.id),
                      );
                    },
                  ),
                ),
              ],
            ),
            Positioned(
              left: 16,
              right: 88,
              bottom: 16,
              child: Material(
                color: colors.surfaceContainerHighest,
                borderRadius: BorderRadius.circular(28),
                child: Row(
                  children: [
                    const SizedBox(width: 12),
                    const Icon(Icons.search, size: 20),
                    Expanded(
                      child: TextField(
                        key: const Key('tab-switcher-search'),
                        controller: _search,
                        decoration: const InputDecoration(
                          hintText: 'Search tabs',
                          border: InputBorder.none,
                          isDense: true,
                        ),
                        onChanged: (_) => setState(() {}),
                      ),
                    ),
                    PopupMenuButton<String>(
                      key: const Key('tab-switcher-menu'),
                      tooltip: 'Tab options',
                      onSelected: (value) {
                        if (value == 'close-all') {
                          widget.onCloseAll?.call();
                        }
                      },
                      itemBuilder: (context) => const [
                        PopupMenuItem(
                          key: Key('tab-switcher-close-all'),
                          value: 'close-all',
                          child: Text('Close all tabs'),
                        ),
                      ],
                      icon: const Icon(Icons.more_vert),
                    ),
                  ],
                ),
              ),
            ),
            Positioned(
              right: 16,
              bottom: 16,
              child: FloatingActionButton(
                key: const Key('tab-switcher-new'),
                tooltip: 'New tab',
                onPressed: widget.onNewTab,
                backgroundColor: MorphColors.cyan,
                foregroundColor: MorphColors.charcoal,
                child: const Icon(Icons.add),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// One grid card: preview, favicon, truncated title, close, swipe to close.
class MorphTabCard extends StatelessWidget {
  const MorphTabCard({
    super.key,
    required this.tab,
    required this.active,
    this.preview,
    this.onSelect,
    this.onClose,
  });

  final MorphTab tab;
  final bool active;
  final Uint8List? preview;
  final VoidCallback? onSelect;
  final VoidCallback? onClose;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final radius = BorderRadius.circular(12);
    return Dismissible(
      key: Key('tab-card-${tab.id}'),
      direction: DismissDirection.horizontal,
      onDismissed: (_) => onClose?.call(),
      child: DecoratedBox(
        decoration: BoxDecoration(
          borderRadius: radius,
          gradient: active
              ? const LinearGradient(
                  colors: [MorphColors.cyan, MorphColors.magenta],
                  begin: Alignment.topLeft,
                  end: Alignment.bottomRight,
                )
              : null,
          border: active
              ? null
              : Border.all(color: colors.outline.withValues(alpha: 0.4)),
        ),
        child: Padding(
          padding: EdgeInsets.all(active ? 2 : 0),
          child: Material(
            color: colors.surfaceContainerHighest,
            borderRadius: BorderRadius.circular(active ? 10 : 12),
            clipBehavior: Clip.antiAlias,
            child: InkWell(
              onTap: onSelect,
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Padding(
                    padding: const EdgeInsets.fromLTRB(8, 6, 2, 6),
                    child: Row(
                      children: [
                        Icon(
                          Icons.public,
                          size: 14,
                          color: colors.onSurfaceVariant,
                        ),
                        const SizedBox(width: 6),
                        Expanded(
                          child: Text(
                            tab.label,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: MorphTheme.monoStyle(
                              color: colors.onSurface,
                              size: 11,
                            ),
                          ),
                        ),
                        IconButton(
                          key: Key('tab-card-close-${tab.id}'),
                          tooltip: 'Close tab',
                          onPressed: onClose,
                          icon: const Icon(Icons.close, size: 16),
                          visualDensity: VisualDensity.compact,
                          padding: EdgeInsets.zero,
                          constraints: const BoxConstraints(
                            minWidth: 28,
                            minHeight: 28,
                          ),
                        ),
                      ],
                    ),
                  ),
                  Expanded(
                    child: ColoredBox(
                      color: MorphColors.charcoal,
                      child: preview != null
                          ? Image.memory(
                              preview!,
                              fit: BoxFit.cover,
                              width: double.infinity,
                              height: double.infinity,
                              errorBuilder: (context, error, stack) =>
                                  const Center(
                                    child: Icon(
                                      Icons.public,
                                      color: MorphColors.cyan,
                                      size: 32,
                                    ),
                                  ),
                            )
                          : const Center(
                              child: Icon(
                                Icons.public,
                                color: MorphColors.cyan,
                                size: 32,
                              ),
                            ),
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _Segment extends StatelessWidget {
  const _Segment({
    super.key,
    required this.label,
    required this.caption,
    required this.selected,
    this.placeholder = false,
  });

  final String label;
  final String caption;
  final bool selected;
  final bool placeholder;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Expanded(
      child: AnimatedContainer(
        duration: const Duration(milliseconds: 160),
        padding: const EdgeInsets.symmetric(vertical: 8),
        decoration: BoxDecoration(
          color: selected ? colors.surfaceContainerHighest : Colors.transparent,
          borderRadius: BorderRadius.circular(20),
          border: Border.all(
            color: selected
                ? MorphColors.cyan
                : colors.outline.withValues(alpha: 0.4),
          ),
        ),
        child: Column(
          children: [
            if (label.isNotEmpty)
              Text(
                label,
                style: MorphTheme.monoStyle(
                  color: selected ? MorphColors.cyan : colors.onSurfaceVariant,
                  size: 16,
                ),
              ),
            Text(
              caption,
              style: TextStyle(
                fontSize: 11,
                color: placeholder
                    ? colors.onSurface.withValues(alpha: 0.5)
                    : colors.onSurface,
              ),
            ),
          ],
        ),
      ),
    );
  }
}
