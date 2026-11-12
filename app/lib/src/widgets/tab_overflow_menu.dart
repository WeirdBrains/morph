import 'package:flutter/material.dart';

import '../tabs/tab_overflow.dart';
import '../tabs/tab_set.dart';
import '../theme/morph_theme.dart';

/// Overflow chevron that lists tabs that do not fit in the strip or list.
///
/// Each entry shows a favicon and title. Tapping the row activates that
/// tab; the close control closes it without activating it.
class MorphTabOverflowButton extends StatelessWidget {
  const MorphTabOverflowButton({
    super.key,
    required this.tabs,
    this.onSelect,
    this.onClose,
  });

  final List<MorphTab> tabs;
  final ValueChanged<int>? onSelect;
  final ValueChanged<int>? onClose;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return SizedBox(
      width: MorphTabOverflow.overflowButtonExtent,
      height: MorphTabOverflow.overflowButtonExtent,
      child: PopupMenuButton<int>(
        key: const Key('tab-overflow'),
        tooltip: 'More tabs',
        padding: EdgeInsets.zero,
        splashRadius: 18,
        offset: const Offset(0, 36),
        onSelected: onSelect,
        itemBuilder: (context) {
          return [
            for (final tab in tabs)
              PopupMenuItem<int>(
                key: Key('tab-overflow-${tab.id}'),
                value: tab.id,
                padding: const EdgeInsets.symmetric(horizontal: 8),
                child: Row(
                  children: [
                    Icon(
                      Icons.public,
                      size: 16,
                      color: colors.onSurfaceVariant,
                    ),
                    const SizedBox(width: 8),
                    Expanded(
                      child: Text(
                        tab.label,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: MorphTheme.monoStyle(
                          color: colors.onSurface,
                          size: 12,
                        ),
                      ),
                    ),
                    IconButton(
                      key: Key('tab-overflow-close-${tab.id}'),
                      tooltip: 'Close tab',
                      icon: const Icon(Icons.close, size: 14),
                      visualDensity: VisualDensity.compact,
                      constraints: const BoxConstraints(
                        minWidth: 28,
                        minHeight: 28,
                      ),
                      padding: EdgeInsets.zero,
                      onPressed: () {
                        Navigator.of(context).pop();
                        onClose?.call(tab.id);
                      },
                    ),
                  ],
                ),
              ),
          ];
        },
        child: Icon(
          Icons.keyboard_arrow_down,
          size: 18,
          color: colors.onSurfaceVariant,
        ),
      ),
    );
  }
}
