import 'package:flutter/gestures.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';

import '../tabs/tab_position.dart';
import '../tabs/tab_set.dart';
import '../theme/morph_theme.dart';
import '../window/window_buttons.dart';
import 'brand.dart';
import 'window_chrome.dart';

/// Desktop chrome: tab strip or list, a top navigation row, window buttons,
/// and a drag region.
///
/// Navigation ([navigation]) is always at the top. Only the tabs move:
/// above the nav row ([MorphTabPosition.top]), below the content
/// ([MorphTabPosition.bottom], decorations flipped downward), or in a
/// vertical list beside the content ([MorphTabPosition.left] /
/// [MorphTabPosition.right]). When tabs are not on top, a slim caption bar
/// holds the mark, drag region, and window buttons.
class MorphDesktopChrome extends StatelessWidget {
  const MorphDesktopChrome({
    super.key,
    required this.tabs,
    required this.activeId,
    required this.position,
    required this.navigation,
    required this.content,
    this.sidebarCollapsed = false,
    this.buttonMode = MorphWindowButtonMode.all,
    this.platform,
    this.useNativeMacButtons = true,
    this.leadingInset,
    this.isMaximized = false,
    this.onSelectTab,
    this.onCloseTab,
    this.onNewTab,
    this.onPositionChanged,
    this.onToggleSidebar,
    this.onMinimize,
    this.onToggleMaximize,
    this.onCloseWindow,
    this.onDebugLongPress,
  });

  final List<MorphTab> tabs;
  final int activeId;
  final MorphTabPosition position;
  final Widget navigation;
  final Widget content;
  final bool sidebarCollapsed;
  final MorphWindowButtonMode buttonMode;
  final TargetPlatform? platform;
  final bool useNativeMacButtons;
  final double? leadingInset;
  final bool isMaximized;
  final ValueChanged<int>? onSelectTab;
  final ValueChanged<int>? onCloseTab;
  final VoidCallback? onNewTab;
  final ValueChanged<MorphTabPosition>? onPositionChanged;
  final VoidCallback? onToggleSidebar;
  final VoidCallback? onMinimize;
  final VoidCallback? onToggleMaximize;
  final VoidCallback? onCloseWindow;
  final VoidCallback? onDebugLongPress;

  TargetPlatform get _platform => platform ?? defaultTargetPlatform;

  double get _inset =>
      leadingInset ??
      MorphWindowChrome.leadingInset(
        platform: _platform,
        useNativeMacButtons: useNativeMacButtons,
      );

  bool get _customButtons => MorphWindowChrome.showsCustomButtons(
    platform: _platform,
    useNativeMacButtons: useNativeMacButtons,
  );

  Widget get _windowButtons => MorphWindowButtonsBar(
    mode: buttonMode,
    platform: _platform,
    isMaximized: isMaximized,
    onMinimize: onMinimize,
    onToggleMaximize: onToggleMaximize,
    onClose: onCloseWindow,
  );

  @override
  Widget build(BuildContext context) {
    final topTabs = position == MorphTabPosition.top;
    return Column(
      key: const Key('desktop-chrome'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (topTabs)
          MorphDesktopTabStrip(
            tabs: tabs,
            activeId: activeId,
            flipped: false,
            includeWindowChrome: true,
            leadingInset: _inset,
            showCustomButtons: _customButtons,
            windowButtons: _windowButtons,
            onSelectTab: onSelectTab,
            onCloseTab: onCloseTab,
            onNewTab: onNewTab,
            onPositionChanged: onPositionChanged,
            onDebugLongPress: onDebugLongPress,
          )
        else
          MorphDesktopCaptionBar(
            leadingInset: _inset,
            showCustomButtons: _customButtons,
            windowButtons: _windowButtons,
            onDebugLongPress: onDebugLongPress,
          ),
        navigation,
        const MorphGlitchRule(),
        Expanded(
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (position == MorphTabPosition.left)
                MorphTabList(
                  tabs: tabs,
                  activeId: activeId,
                  collapsed: sidebarCollapsed,
                  onSelectTab: onSelectTab,
                  onCloseTab: onCloseTab,
                  onNewTab: onNewTab,
                  onPositionChanged: onPositionChanged,
                  onToggleSidebar: onToggleSidebar,
                ),
              Expanded(child: content),
              if (position == MorphTabPosition.right)
                MorphTabList(
                  tabs: tabs,
                  activeId: activeId,
                  collapsed: sidebarCollapsed,
                  onSelectTab: onSelectTab,
                  onCloseTab: onCloseTab,
                  onNewTab: onNewTab,
                  onPositionChanged: onPositionChanged,
                  onToggleSidebar: onToggleSidebar,
                ),
            ],
          ),
        ),
        if (position == MorphTabPosition.bottom)
          MorphDesktopTabStrip(
            tabs: tabs,
            activeId: activeId,
            flipped: true,
            includeWindowChrome: false,
            leadingInset: 0,
            showCustomButtons: false,
            windowButtons: const SizedBox.shrink(),
            onSelectTab: onSelectTab,
            onCloseTab: onCloseTab,
            onNewTab: onNewTab,
            onPositionChanged: onPositionChanged,
          ),
      ],
    );
  }
}

/// Slim top bar used when tabs are not on top: mark, drag region, buttons.
class MorphDesktopCaptionBar extends StatelessWidget {
  const MorphDesktopCaptionBar({
    super.key,
    required this.leadingInset,
    required this.showCustomButtons,
    required this.windowButtons,
    this.onDebugLongPress,
  });

  final double leadingInset;
  final bool showCustomButtons;
  final Widget windowButtons;
  final VoidCallback? onDebugLongPress;

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      key: const Key('caption-bar'),
      height: 40,
      child: Row(
        children: [
          if (leadingInset > 0)
            SizedBox(
              key: const Key('chrome-leading-inset'),
              width: leadingInset,
            ),
          _MarkButton(onLongPress: onDebugLongPress),
          const Expanded(child: MorphWindowDragArea(child: SizedBox.expand())),
          if (showCustomButtons) windowButtons,
        ],
      ),
    );
  }
}

/// Horizontal tab strip for the top or bottom edge.
class MorphDesktopTabStrip extends StatelessWidget {
  const MorphDesktopTabStrip({
    super.key,
    required this.tabs,
    required this.activeId,
    required this.flipped,
    required this.includeWindowChrome,
    required this.leadingInset,
    required this.showCustomButtons,
    required this.windowButtons,
    this.onSelectTab,
    this.onCloseTab,
    this.onNewTab,
    this.onPositionChanged,
    this.onDebugLongPress,
  });

  final List<MorphTab> tabs;
  final int activeId;
  final bool flipped;
  final bool includeWindowChrome;
  final double leadingInset;
  final bool showCustomButtons;
  final Widget windowButtons;
  final ValueChanged<int>? onSelectTab;
  final ValueChanged<int>? onCloseTab;
  final VoidCallback? onNewTab;
  final ValueChanged<MorphTabPosition>? onPositionChanged;
  final VoidCallback? onDebugLongPress;

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      key: const Key('tab-strip'),
      height: 40,
      child: Row(
        children: [
          if (includeWindowChrome && leadingInset > 0)
            SizedBox(
              key: const Key('chrome-leading-inset'),
              width: leadingInset,
            ),
          if (includeWindowChrome) _MarkButton(onLongPress: onDebugLongPress),
          Expanded(
            child: ListView.separated(
              scrollDirection: Axis.horizontal,
              padding: const EdgeInsets.symmetric(horizontal: 4),
              itemCount: tabs.length,
              separatorBuilder: (_, _) => const SizedBox(width: 2),
              itemBuilder: (context, index) {
                final tab = tabs[index];
                return MorphTabChip(
                  tab: tab,
                  active: tab.id == activeId,
                  flipped: flipped,
                  onSelect: () => onSelectTab?.call(tab.id),
                  onClose: () => onCloseTab?.call(tab.id),
                );
              },
            ),
          ),
          _IconAction(
            key: const Key('tab-new'),
            tooltip: 'New tab',
            icon: Icons.add,
            onPressed: onNewTab,
          ),
          MorphTabPositionButton(onSelected: onPositionChanged),
          if (includeWindowChrome) ...[
            const Expanded(
              child: MorphWindowDragArea(child: SizedBox.expand()),
            ),
            if (showCustomButtons) windowButtons,
          ],
        ],
      ),
    );
  }
}

/// Vertical list of tabs for the left or right side (not a tab-shaped strip).
class MorphTabList extends StatelessWidget {
  const MorphTabList({
    super.key,
    required this.tabs,
    required this.activeId,
    required this.collapsed,
    this.onSelectTab,
    this.onCloseTab,
    this.onNewTab,
    this.onPositionChanged,
    this.onToggleSidebar,
  });

  final List<MorphTab> tabs;
  final int activeId;
  final bool collapsed;
  final ValueChanged<int>? onSelectTab;
  final ValueChanged<int>? onCloseTab;
  final VoidCallback? onNewTab;
  final ValueChanged<MorphTabPosition>? onPositionChanged;
  final VoidCallback? onToggleSidebar;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Material(
      key: const Key('tab-list'),
      color: colors.surface,
      child: SizedBox(
        width: collapsed ? 48 : 220,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            SizedBox(
              height: 40,
              child: Row(
                children: [
                  _IconAction(
                    key: const Key('tab-new'),
                    tooltip: 'New tab',
                    icon: Icons.add,
                    onPressed: onNewTab,
                  ),
                  MorphTabPositionButton(onSelected: onPositionChanged),
                  const Spacer(),
                  _IconAction(
                    key: const Key('tab-list-collapse'),
                    tooltip: collapsed ? 'Expand tabs' : 'Collapse tabs',
                    icon: collapsed
                        ? Icons.keyboard_double_arrow_right
                        : Icons.keyboard_double_arrow_left,
                    onPressed: onToggleSidebar,
                  ),
                ],
              ),
            ),
            Expanded(
              child: ListView.builder(
                itemCount: tabs.length,
                itemBuilder: (context, index) {
                  final tab = tabs[index];
                  return MorphTabListTile(
                    tab: tab,
                    active: tab.id == activeId,
                    collapsed: collapsed,
                    onSelect: () => onSelectTab?.call(tab.id),
                    onClose: () => onCloseTab?.call(tab.id),
                  );
                },
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// One horizontal tab. When [flipped] is true (bottom strip), rounded
/// corners and the active indicator mirror vertically so the tab attaches
/// to the content above.
class MorphTabChip extends StatefulWidget {
  const MorphTabChip({
    super.key,
    required this.tab,
    required this.active,
    required this.flipped,
    this.onSelect,
    this.onClose,
  });

  final MorphTab tab;
  final bool active;
  final bool flipped;
  final VoidCallback? onSelect;
  final VoidCallback? onClose;

  @override
  State<MorphTabChip> createState() => _MorphTabChipState();
}

class _MorphTabChipState extends State<MorphTabChip> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final radius = const Radius.circular(8);
    final shape = BorderRadius.only(
      topLeft: widget.flipped ? Radius.zero : radius,
      topRight: widget.flipped ? Radius.zero : radius,
      bottomLeft: widget.flipped ? radius : Radius.zero,
      bottomRight: widget.flipped ? radius : Radius.zero,
    );
    final indicator = BorderSide(color: MorphColors.cyan, width: 2);
    return MouseRegion(
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: Listener(
        onPointerDown: (event) {
          if (event.buttons == kMiddleMouseButton) {
            widget.onClose?.call();
          }
        },
        child: InkWell(
          key: Key('tab-${widget.tab.id}'),
          onTap: widget.onSelect,
          borderRadius: shape,
          child: AnimatedContainer(
            duration: const Duration(milliseconds: 120),
            width: 180,
            padding: const EdgeInsets.symmetric(horizontal: 8),
            decoration: BoxDecoration(
              color: widget.active
                  ? colors.surface
                  : (_hover
                        ? colors.surfaceContainerHighest
                        : Colors.transparent),
              borderRadius: shape,
              border: Border(
                top: widget.flipped && widget.active
                    ? indicator
                    : BorderSide.none,
                bottom: !widget.flipped && widget.active
                    ? indicator
                    : BorderSide.none,
              ),
            ),
            child: Row(
              children: [
                Icon(Icons.public, size: 14, color: colors.onSurfaceVariant),
                const SizedBox(width: 6),
                Expanded(
                  child: Text(
                    widget.tab.label,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: MorphTheme.monoStyle(
                      color: colors.onSurface,
                      size: 12,
                    ),
                  ),
                ),
                if (widget.active || _hover)
                  _IconAction(
                    key: Key('tab-close-${widget.tab.id}'),
                    tooltip: 'Close tab',
                    icon: Icons.close,
                    onPressed: widget.onClose,
                    size: 28,
                    iconSize: 14,
                  ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

/// One row in the left/right tab list: favicon, truncated title, close.
class MorphTabListTile extends StatefulWidget {
  const MorphTabListTile({
    super.key,
    required this.tab,
    required this.active,
    required this.collapsed,
    this.onSelect,
    this.onClose,
  });

  final MorphTab tab;
  final bool active;
  final bool collapsed;
  final VoidCallback? onSelect;
  final VoidCallback? onClose;

  @override
  State<MorphTabListTile> createState() => _MorphTabListTileState();
}

class _MorphTabListTileState extends State<MorphTabListTile> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return MouseRegion(
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: Listener(
        onPointerDown: (event) {
          if (event.buttons == kMiddleMouseButton) {
            widget.onClose?.call();
          }
        },
        child: InkWell(
          key: Key('tab-${widget.tab.id}'),
          onTap: widget.onSelect,
          child: Container(
            height: 36,
            padding: const EdgeInsets.symmetric(horizontal: 8),
            decoration: BoxDecoration(
              color: widget.active
                  ? colors.surfaceContainerHighest
                  : Colors.transparent,
              border: widget.active
                  ? const Border(
                      left: BorderSide(color: MorphColors.cyan, width: 2),
                    )
                  : null,
            ),
            child: Row(
              children: [
                Icon(Icons.public, size: 16, color: colors.onSurfaceVariant),
                if (!widget.collapsed) ...[
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      widget.tab.label,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: MorphTheme.monoStyle(
                        color: colors.onSurface,
                        size: 12,
                      ),
                    ),
                  ),
                  if (widget.active || _hover)
                    _IconAction(
                      key: Key('tab-close-${widget.tab.id}'),
                      tooltip: 'Close tab',
                      icon: Icons.close,
                      onPressed: widget.onClose,
                      size: 28,
                      iconSize: 14,
                    ),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }
}

/// Layout-menu button that picks top, bottom, left, or right.
class MorphTabPositionButton extends StatelessWidget {
  const MorphTabPositionButton({super.key, this.onSelected});

  final ValueChanged<MorphTabPosition>? onSelected;

  @override
  Widget build(BuildContext context) {
    return PopupMenuButton<MorphTabPosition>(
      key: const Key('tab-position'),
      tooltip: 'Tab position',
      onSelected: onSelected,
      itemBuilder: (context) => [
        for (final position in MorphTabPosition.values)
          PopupMenuItem(
            key: Key('tab-position-${position.key}'),
            value: position,
            child: Text(_label(position)),
          ),
      ],
      child: const Padding(
        padding: EdgeInsets.all(8),
        child: Icon(Icons.view_sidebar_outlined, size: 18),
      ),
    );
  }

  static String _label(MorphTabPosition position) {
    switch (position) {
      case MorphTabPosition.top:
        return 'Tabs on top';
      case MorphTabPosition.bottom:
        return 'Tabs on bottom';
      case MorphTabPosition.left:
        return 'Tabs on left';
      case MorphTabPosition.right:
        return 'Tabs on right';
    }
  }
}

class _MarkButton extends StatelessWidget {
  const _MarkButton({this.onLongPress});

  final VoidCallback? onLongPress;

  @override
  Widget build(BuildContext context) {
    return MorphWindowDragArea(
      child: GestureDetector(
        key: const Key('debug-toggle'),
        onLongPress: onLongPress,
        child: const Padding(
          padding: EdgeInsets.symmetric(horizontal: 6),
          child: MorphMark(size: 24),
        ),
      ),
    );
  }
}

class _IconAction extends StatelessWidget {
  const _IconAction({
    super.key,
    required this.tooltip,
    required this.icon,
    this.onPressed,
    this.size = 36,
    this.iconSize = 18,
  });

  final String tooltip;
  final IconData icon;
  final VoidCallback? onPressed;
  final double size;
  final double iconSize;

  @override
  Widget build(BuildContext context) {
    return IconButton(
      tooltip: tooltip,
      onPressed: onPressed,
      icon: Icon(icon, size: iconSize),
      style: IconButton.styleFrom(
        visualDensity: VisualDensity.compact,
        minimumSize: Size(size, size),
        tapTargetSize: MaterialTapTargetSize.shrinkWrap,
        padding: EdgeInsets.zero,
      ),
    );
  }
}
