import 'package:flutter/gestures.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';

import '../tabs/tab_overflow.dart';
import '../tabs/tab_position.dart';
import '../tabs/tab_set.dart';
import '../theme/morph_theme.dart';
import '../window/window_buttons.dart';
import 'brand.dart';
import 'tab_active_border.dart';
import 'tab_overflow_menu.dart';
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
    this.sidebarWidth = MorphTabOverflow.defaultSidebarWidth,
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
    this.onSidebarWidthChanged,
    this.onSidebarWidthCommit,
    this.onSidebarWidthReset,
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
  final double sidebarWidth;
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
  final ValueChanged<double>? onSidebarWidthChanged;
  final VoidCallback? onSidebarWidthCommit;
  final VoidCallback? onSidebarWidthReset;
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
        MorphGlitchRule(
          key: const Key('glitch-rule'),
          accents:
              position == MorphTabPosition.top ||
              position == MorphTabPosition.bottom,
        ),
        Expanded(
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (position == MorphTabPosition.left)
                MorphTabList(
                  tabs: tabs,
                  activeId: activeId,
                  side: MorphTabPosition.left,
                  collapsed: sidebarCollapsed,
                  width: sidebarWidth,
                  onSelectTab: onSelectTab,
                  onCloseTab: onCloseTab,
                  onNewTab: onNewTab,
                  onPositionChanged: onPositionChanged,
                  onToggleSidebar: onToggleSidebar,
                  onWidthChanged: onSidebarWidthChanged,
                  onWidthCommit: onSidebarWidthCommit,
                  onWidthReset: onSidebarWidthReset,
                ),
              Expanded(child: content),
              if (position == MorphTabPosition.right)
                MorphTabList(
                  tabs: tabs,
                  activeId: activeId,
                  side: MorphTabPosition.right,
                  collapsed: sidebarCollapsed,
                  width: sidebarWidth,
                  onSelectTab: onSelectTab,
                  onCloseTab: onCloseTab,
                  onNewTab: onNewTab,
                  onPositionChanged: onPositionChanged,
                  onToggleSidebar: onToggleSidebar,
                  onWidthChanged: onSidebarWidthChanged,
                  onWidthCommit: onSidebarWidthCommit,
                  onWidthReset: onSidebarWidthReset,
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
            child: Stack(
              alignment: Alignment.centerLeft,
              children: [
                if (includeWindowChrome)
                  const MorphWindowDragArea(child: SizedBox.expand()),
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 4),
                  child: LayoutBuilder(
                    builder: (context, constraints) {
                      final visibleCount = MorphTabOverflow.visibleCount(
                        tabCount: tabs.length,
                        available: constraints.maxWidth,
                        minTabExtent: MorphTabOverflow.horizontalMinExtent,
                        trailingExtent: MorphTabOverflow.trailingGroupExtent(
                          horizontal: true,
                        ),
                      );
                      final activeIndex = tabs.indexWhere(
                        (tab) => tab.id == activeId,
                      );
                      final (start, end) = MorphTabOverflow.visibleRange(
                        tabCount: tabs.length,
                        visibleCount: visibleCount,
                        activeIndex: activeIndex < 0 ? 0 : activeIndex,
                      );
                      final visible = tabs.sublist(start, end);
                      final hidden = [
                        ...tabs.sublist(0, start),
                        ...tabs.sublist(end),
                      ];
                      final tabWidth = MorphTabOverflow.tabWidth(
                        visibleTabCount: visible.length,
                        available: constraints.maxWidth,
                        trailingExtent: MorphTabOverflow.trailingGroupExtent(
                          horizontal: true,
                        ),
                        overflow: hidden.isNotEmpty,
                      );
                      return MorphTrailingAfterChildren(
                        axis: Axis.horizontal,
                        spacing: MorphTabOverflow.spacing,
                        trailing: Row(
                          key: const Key('tab-strip-actions'),
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            _IconAction(
                              key: const Key('tab-new'),
                              tooltip: 'New tab',
                              icon: Icons.add,
                              onPressed: onNewTab,
                            ),
                            MorphTabPositionButton(
                              onSelected: onPositionChanged,
                            ),
                          ],
                        ),
                        children: [
                          for (final tab in visible)
                            MorphTabChip(
                              tab: tab,
                              active: tab.id == activeId,
                              flipped: flipped,
                              width: tabWidth,
                              onSelect: () => onSelectTab?.call(tab.id),
                              onClose: () => onCloseTab?.call(tab.id),
                            ),
                          if (hidden.isNotEmpty)
                            MorphTabOverflowButton(
                              tabs: hidden,
                              onSelect: onSelectTab,
                              onClose: onCloseTab,
                            ),
                        ],
                      );
                    },
                  ),
                ),
              ],
            ),
          ),
          if (includeWindowChrome && showCustomButtons) windowButtons,
        ],
      ),
    );
  }
}

/// Lays out [children] followed by [trailing].
///
/// If they fit, [trailing] sits immediately after the last child. If they
/// overflow, [children] scroll and [trailing] is pinned at the end of the
/// viewport so it stays reachable and never covers a sibling outside this
/// widget (for example window buttons on the tab strip).
class MorphTrailingAfterChildren extends StatefulWidget {
  const MorphTrailingAfterChildren({
    super.key,
    required this.axis,
    required this.trailing,
    this.spacing = 0,
    required this.children,
  });

  final Axis axis;
  final List<Widget> children;
  final Widget trailing;
  final double spacing;

  @override
  State<MorphTrailingAfterChildren> createState() =>
      _MorphTrailingAfterChildrenState();
}

class _MorphTrailingAfterChildrenState
    extends State<MorphTrailingAfterChildren> {
  double _childrenExtent = 0;
  double _trailingExtent = 0;

  bool get _horizontal => widget.axis == Axis.horizontal;

  @override
  void didUpdateWidget(covariant MorphTrailingAfterChildren oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.children.length != widget.children.length) {
      // Drop the stale cluster width so a tab-count change cannot paint a
      // RenderFlex stripe while AnimatedContainer widths are still catching up.
      _childrenExtent = 0;
    }
  }

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final max = _horizontal ? constraints.maxWidth : constraints.maxHeight;
        final gap = widget.children.isEmpty ? 0.0 : widget.spacing;
        final needed = _childrenExtent + gap + _trailingExtent;
        // Wait until both clusters are measured. Trailing can report first
        // and a packed layout with an unknown child extent would overflow.
        final measured =
            (_trailingExtent > 0) &&
            (_childrenExtent > 0 || widget.children.isEmpty);
        // Before the first measure, pin trailing so a wide first frame cannot
        // push window buttons off the side of the strip. After measuring,
        // treat a near-full strip as overflow so a 1–4px under-measure cannot
        // paint a RenderFlex stripe.
        final overflow = !measured || needed > max + 0.5;

        final tabs = _MeasureExtent(
          axis: widget.axis,
          onExtent: (value) {
            if ((value - _childrenExtent).abs() > 0.5) {
              setState(() => _childrenExtent = value);
            }
          },
          child: _horizontal
              ? Row(
                  mainAxisSize: MainAxisSize.min,
                  children: _intersperse(widget.children),
                )
              : Column(
                  mainAxisSize: MainAxisSize.min,
                  children: _intersperse(widget.children),
                ),
        );
        final trailing = _MeasureExtent(
          axis: widget.axis,
          onExtent: (value) {
            if ((value - _trailingExtent).abs() > 0.5) {
              setState(() => _trailingExtent = value);
            }
          },
          child: widget.trailing,
        );

        if (overflow) {
          if (_horizontal) {
            return Row(
              children: [
                Expanded(
                  child: SingleChildScrollView(
                    scrollDirection: Axis.horizontal,
                    child: tabs,
                  ),
                ),
                if (gap > 0) SizedBox(width: gap),
                trailing,
              ],
            );
          }
          return Column(
            children: [
              Expanded(child: SingleChildScrollView(child: tabs)),
              if (gap > 0) SizedBox(height: gap),
              trailing,
            ],
          );
        }

        if (_horizontal) {
          return Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              _clipCluster(tabs, max - _trailingExtent - gap),
              if (gap > 0) SizedBox(width: gap),
              trailing,
            ],
          );
        }
        return Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            _clipCluster(tabs, max - _trailingExtent - gap),
            if (gap > 0) SizedBox(height: gap),
            trailing,
          ],
        );
      },
    );
  }

  /// Lets [cluster] lay out at its intrinsic size while clipping any
  /// leftover pixels, so a 1–2 px under-measure cannot overflow a Flex.
  Widget _clipCluster(Widget cluster, double maxExtent) {
    if (maxExtent.isFinite && maxExtent > 0) {
      return ClipRect(
        child: ConstrainedBox(
          constraints: _horizontal
              ? BoxConstraints(maxWidth: maxExtent)
              : BoxConstraints(maxHeight: maxExtent),
          child: UnconstrainedBox(
            alignment: _horizontal ? Alignment.centerLeft : Alignment.topCenter,
            constrainedAxis: _horizontal ? Axis.vertical : Axis.horizontal,
            clipBehavior: Clip.hardEdge,
            child: cluster,
          ),
        ),
      );
    }
    return cluster;
  }

  List<Widget> _intersperse(List<Widget> children) {
    if (widget.spacing <= 0 || children.length < 2) {
      return children;
    }
    final out = <Widget>[];
    for (var i = 0; i < children.length; i++) {
      if (i > 0) {
        out.add(
          _horizontal
              ? SizedBox(width: widget.spacing)
              : SizedBox(height: widget.spacing),
        );
      }
      out.add(children[i]);
    }
    return out;
  }
}

class _MeasureExtent extends StatelessWidget {
  const _MeasureExtent({
    required this.axis,
    required this.onExtent,
    required this.child,
  });

  final Axis axis;
  final ValueChanged<double> onExtent;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!context.mounted) {
        return;
      }
      final box = context.findRenderObject() as RenderBox?;
      if (box == null || !box.hasSize) {
        return;
      }
      onExtent(axis == Axis.horizontal ? box.size.width : box.size.height);
    });
    return child;
  }
}

/// Vertical list of tabs for the left or right side (not a tab-shaped strip).
class MorphTabList extends StatelessWidget {
  const MorphTabList({
    super.key,
    required this.tabs,
    required this.activeId,
    required this.side,
    required this.collapsed,
    this.onSelectTab,
    this.onCloseTab,
    this.onNewTab,
    this.onPositionChanged,
    this.onToggleSidebar,
    this.width = MorphTabOverflow.defaultSidebarWidth,
    this.onWidthChanged,
    this.onWidthCommit,
    this.onWidthReset,
  });

  static const double collapsedWidth = MorphTabOverflow.collapsedSidebarWidth;
  static const double expandedWidth = MorphTabOverflow.defaultSidebarWidth;

  final List<MorphTab> tabs;
  final int activeId;

  /// [MorphTabPosition.left] or [MorphTabPosition.right].
  final MorphTabPosition side;
  final bool collapsed;
  final ValueChanged<int>? onSelectTab;
  final ValueChanged<int>? onCloseTab;
  final VoidCallback? onNewTab;
  final ValueChanged<MorphTabPosition>? onPositionChanged;
  final VoidCallback? onToggleSidebar;
  final double width;
  final ValueChanged<double>? onWidthChanged;
  final VoidCallback? onWidthCommit;
  final VoidCallback? onWidthReset;

  bool get _right => side == MorphTabPosition.right;

  IconData get _toggleIcon {
    // Expand points toward the content; collapse points toward the window edge.
    if (collapsed) {
      return _right
          ? Icons.keyboard_double_arrow_left
          : Icons.keyboard_double_arrow_right;
    }
    return _right
        ? Icons.keyboard_double_arrow_right
        : Icons.keyboard_double_arrow_left;
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final newTab = _IconAction(
      key: const Key('tab-new'),
      tooltip: 'New tab',
      icon: Icons.add,
      onPressed: onNewTab,
    );
    final positionButton = MorphTabPositionButton(
      onSelected: onPositionChanged,
    );
    final toggle = _IconAction(
      key: const Key('tab-list-collapse'),
      tooltip: collapsed ? 'Expand tabs' : 'Collapse tabs',
      icon: _toggleIcon,
      onPressed: onToggleSidebar,
    );
    final actions = Column(
      key: const Key('tab-list-actions'),
      mainAxisSize: MainAxisSize.min,
      children: [newTab, positionButton],
    );
    final listWidth = collapsed ? collapsedWidth : width;
    return Material(
      key: const Key('tab-list'),
      color: colors.surface,
      child: SizedBox(
        width: listWidth,
        child: Stack(
          children: [
            Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Align(alignment: Alignment.center, child: toggle),
                Expanded(
                  child: LayoutBuilder(
                    builder: (context, constraints) {
                      final visibleCount = MorphTabOverflow.visibleCount(
                        tabCount: tabs.length,
                        available: constraints.maxHeight,
                        minTabExtent: MorphTabOverflow.verticalMinExtent,
                        trailingExtent: MorphTabOverflow.trailingGroupExtent(
                          horizontal: false,
                        ),
                      );
                      final activeIndex = tabs.indexWhere(
                        (tab) => tab.id == activeId,
                      );
                      final (start, end) = MorphTabOverflow.visibleRange(
                        tabCount: tabs.length,
                        visibleCount: visibleCount,
                        activeIndex: activeIndex < 0 ? 0 : activeIndex,
                      );
                      final visible = tabs.sublist(start, end);
                      final hidden = [
                        ...tabs.sublist(0, start),
                        ...tabs.sublist(end),
                      ];
                      return MorphTrailingAfterChildren(
                        axis: Axis.vertical,
                        spacing: MorphTabOverflow.spacing,
                        trailing: actions,
                        children: [
                          for (final tab in visible)
                            MorphTabListTile(
                              tab: tab,
                              active: tab.id == activeId,
                              collapsed: collapsed,
                              side: side,
                              onSelect: () => onSelectTab?.call(tab.id),
                              onClose: () => onCloseTab?.call(tab.id),
                            ),
                          if (hidden.isNotEmpty)
                            MorphTabOverflowButton(
                              tabs: hidden,
                              onSelect: onSelectTab,
                              onClose: onCloseTab,
                            ),
                        ],
                      );
                    },
                  ),
                ),
              ],
            ),
            if (!collapsed)
              Positioned(
                top: 0,
                bottom: 0,
                right: side == MorphTabPosition.left ? 0 : null,
                left: side == MorphTabPosition.right ? 0 : null,
                child: MorphSidebarResizeHandle(
                  side: side,
                  width: width,
                  windowWidth: MediaQuery.sizeOf(context).width,
                  onWidthChanged: onWidthChanged,
                  onWidthCommit: onWidthCommit,
                  onReset: onWidthReset,
                ),
              ),
          ],
        ),
      ),
    );
  }
}

/// Draggable inner edge of an expanded left or right tab list.
class MorphSidebarResizeHandle extends StatefulWidget {
  const MorphSidebarResizeHandle({
    super.key,
    required this.side,
    required this.width,
    this.windowWidth,
    this.onWidthChanged,
    this.onWidthCommit,
    this.onReset,
  });

  static const double hitExtent = 6;

  final MorphTabPosition side;
  final double width;
  final double? windowWidth;
  final ValueChanged<double>? onWidthChanged;
  final VoidCallback? onWidthCommit;
  final VoidCallback? onReset;

  @override
  State<MorphSidebarResizeHandle> createState() =>
      _MorphSidebarResizeHandleState();
}

class _MorphSidebarResizeHandleState extends State<MorphSidebarResizeHandle> {
  double _origin = 0;
  double _delta = 0;

  void _apply() {
    final next = MorphTabOverflow.clampSidebarWidth(
      _origin + _delta,
      windowWidth: widget.windowWidth,
    );
    widget.onWidthChanged?.call(next);
  }

  @override
  Widget build(BuildContext context) {
    return MouseRegion(
      cursor: SystemMouseCursors.resizeColumn,
      child: GestureDetector(
        key: const Key('sidebar-resize'),
        behavior: HitTestBehavior.opaque,
        onHorizontalDragStart: (details) {
          _origin = widget.width;
          _delta = 0;
        },
        onHorizontalDragUpdate: (details) {
          final increment = widget.side == MorphTabPosition.left
              ? details.delta.dx
              : -details.delta.dx;
          _delta += increment;
          _apply();
        },
        onHorizontalDragEnd: (_) => widget.onWidthCommit?.call(),
        onHorizontalDragCancel: () => widget.onWidthCommit?.call(),
        onDoubleTap: widget.onReset,
        child: const SizedBox(
          width: MorphSidebarResizeHandle.hitExtent,
          height: double.infinity,
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
    this.width = MorphTabOverflow.horizontalMaxExtent,
    this.onSelect,
    this.onClose,
  });

  final MorphTab tab;
  final bool active;
  final bool flipped;

  /// Share of the strip, already clamped to 50–150 px.
  final double width;
  final VoidCallback? onSelect;
  final VoidCallback? onClose;

  /// Width at which the title is hidden so the favicon can stay centered.
  static const double titleMinWidth = 96;

  /// Width at which the close control leaves the row (it still appears on
  /// the active tab or on hover, overlaid so the favicon stays centered).
  static const double closeRowMinWidth = 64;

  @override
  State<MorphTabChip> createState() => _MorphTabChipState();
}

class _MorphTabChipState extends State<MorphTabChip> {
  bool _hover = false;

  bool get _showTitle => widget.width >= MorphTabChip.titleMinWidth;

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
    final showClose = widget.active || _hover;
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
            curve: Curves.easeOut,
            width: widget.width,
            height: 40,
            decoration: BoxDecoration(
              color: widget.active
                  ? colors.surface
                  : (_hover
                        ? colors.surfaceContainerHighest
                        : Colors.transparent),
              borderRadius: shape,
            ),
            child: Stack(
              children: [
                Positioned.fill(
                  child: Padding(
                    padding: EdgeInsets.only(
                      left: 8,
                      right: 8,
                      top: widget.flipped
                          ? 0
                          : MorphTabActiveBorder.defaultStrokeWidth,
                      bottom: widget.flipped
                          ? MorphTabActiveBorder.defaultStrokeWidth
                          : 0,
                    ),
                    child: _showTitle
                        ? Row(
                            crossAxisAlignment: CrossAxisAlignment.center,
                            children: [
                              _favicon(colors.onSurfaceVariant),
                              const SizedBox(width: 6),
                              Expanded(child: _title(colors.onSurface)),
                              if (showClose) _closeButton(),
                            ],
                          )
                        : Stack(
                            alignment: Alignment.center,
                            children: [
                              _favicon(colors.onSurfaceVariant),
                              if (showClose)
                                Align(
                                  alignment: Alignment.centerRight,
                                  child: _closeButton(),
                                ),
                            ],
                          ),
                  ),
                ),
                if (widget.active)
                  Positioned(
                    key: Key('tab-indicator-${widget.tab.id}'),
                    left: 0,
                    right: 0,
                    top: widget.flipped ? null : 0,
                    bottom: widget.flipped ? 0 : null,
                    height: MorphTabActiveBorder.bandExtent,
                    child: MorphTabActiveBorder(
                      edge: widget.flipped
                          ? MorphTabActiveEdge.bottom
                          : MorphTabActiveEdge.top,
                    ),
                  ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  Widget _favicon(Color color) {
    return Icon(
      Icons.public,
      key: Key('tab-favicon-${widget.tab.id}'),
      size: 14,
      color: color,
    );
  }

  Widget _title(Color color) {
    return Text(
      widget.tab.label,
      key: Key('tab-title-${widget.tab.id}'),
      maxLines: 1,
      overflow: TextOverflow.ellipsis,
      style: MorphTheme.monoStyle(color: color, size: 12),
    );
  }

  Widget _closeButton() {
    return _IconAction(
      key: Key('tab-close-${widget.tab.id}'),
      tooltip: 'Close tab',
      icon: Icons.close,
      onPressed: widget.onClose,
      size: 28,
      iconSize: 14,
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
    required this.side,
    this.onSelect,
    this.onClose,
  });

  final MorphTab tab;
  final bool active;
  final bool collapsed;

  /// [MorphTabPosition.left] or [MorphTabPosition.right]. The active border
  /// sits on the inner edge facing the page.
  final MorphTabPosition side;
  final VoidCallback? onSelect;
  final VoidCallback? onClose;

  @override
  State<MorphTabListTile> createState() => _MorphTabListTileState();
}

class _MorphTabListTileState extends State<MorphTabListTile> {
  bool _hover = false;

  bool get _right => widget.side == MorphTabPosition.right;

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
          child: SizedBox(
            height: MorphTabOverflow.verticalMinExtent,
            child: Stack(
              children: [
                Positioned.fill(
                  child: Padding(
                    padding: EdgeInsets.symmetric(
                      horizontal: widget.collapsed ? 0 : 8,
                    ),
                    child: Row(
                      crossAxisAlignment: CrossAxisAlignment.center,
                      mainAxisAlignment: widget.collapsed
                          ? MainAxisAlignment.center
                          : MainAxisAlignment.start,
                      children: [
                        Icon(
                          Icons.public,
                          key: Key('tab-favicon-${widget.tab.id}'),
                          size: 16,
                          color: colors.onSurfaceVariant,
                        ),
                        if (!widget.collapsed) ...[
                          const SizedBox(width: 8),
                          Expanded(
                            child: Text(
                              widget.tab.label,
                              key: Key('tab-title-${widget.tab.id}'),
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
                if (widget.active)
                  Positioned(
                    key: Key('tab-indicator-${widget.tab.id}'),
                    top: 0,
                    bottom: 0,
                    left: _right ? 0 : null,
                    right: _right ? null : 0,
                    width: MorphTabActiveBorder.bandExtent,
                    child: MorphTabActiveBorder(
                      edge: _right
                          ? MorphTabActiveEdge.left
                          : MorphTabActiveEdge.right,
                      cornerRadius: 0,
                    ),
                  ),
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
      padding: EdgeInsets.zero,
      splashRadius: 18,
      style: IconButton.styleFrom(
        visualDensity: VisualDensity.compact,
        minimumSize: const Size(36, 36),
        maximumSize: const Size(36, 36),
        fixedSize: const Size(36, 36),
        tapTargetSize: MaterialTapTargetSize.shrinkWrap,
        padding: EdgeInsets.zero,
      ),
      onSelected: onSelected,
      itemBuilder: (context) => [
        for (final position in MorphTabPosition.values)
          PopupMenuItem(
            key: Key('tab-position-${position.key}'),
            value: position,
            child: Text(_label(position)),
          ),
      ],
      icon: const Icon(Icons.view_sidebar_outlined, size: 18),
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
