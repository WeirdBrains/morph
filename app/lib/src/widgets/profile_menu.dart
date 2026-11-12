import 'dart:async';

import 'package:flutter/material.dart';

import '../api.dart';
import '../profiles/profile_color.dart';
import '../theme/morph_theme.dart';

/// Circle avatar with the profile initial on a Glitch color.
class MorphProfileAvatar extends StatelessWidget {
  const MorphProfileAvatar({super.key, required this.profile, this.size = 28});

  final ProfileInfo profile;
  final double size;

  @override
  Widget build(BuildContext context) {
    final color = MorphProfileColors.of(profile);
    final onColor = color.computeLuminance() > 0.55
        ? MorphColors.charcoal
        : MorphColors.white;
    return Container(
      width: size,
      height: size,
      alignment: Alignment.center,
      decoration: BoxDecoration(
        color: color,
        shape: BoxShape.circle,
        border: Border.all(color: color.withValues(alpha: 0.4)),
      ),
      child: Text(
        MorphProfileColors.initial(profile.name),
        style: MorphTheme.monoStyle(color: onColor, size: size * 0.45),
      ),
    );
  }
}

/// Desktop chrome control that opens the profile menu.
class MorphProfileButton extends StatelessWidget {
  const MorphProfileButton({
    super.key,
    required this.catalog,
    required this.onSwitch,
    required this.onAdd,
    required this.onRename,
    required this.onDelete,
  });

  final ProfileCatalog catalog;
  final ValueChanged<String> onSwitch;
  final Future<void> Function(String name, String color) onAdd;
  final Future<void> Function(String id, String name) onRename;
  final Future<void> Function(String id) onDelete;

  ProfileInfo get _current => catalog.profiles.firstWhere(
    (profile) => profile.id == catalog.current,
    orElse: () => catalog.profiles.first,
  );

  @override
  Widget build(BuildContext context) {
    return IconButton(
      key: const Key('profile-button'),
      tooltip: 'Profiles',
      onPressed: () => unawaited(_openMenu(context)),
      padding: EdgeInsets.zero,
      visualDensity: VisualDensity.compact,
      icon: MorphProfileAvatar(
        key: const Key('profile-avatar'),
        profile: _current,
        size: 26,
      ),
    );
  }

  Future<void> _openMenu(BuildContext context) async {
    final button = context.findRenderObject() as RenderBox?;
    final overlay =
        Overlay.of(context).context.findRenderObject() as RenderBox?;
    if (button == null || overlay == null) {
      return;
    }
    final origin = button.localToGlobal(Offset.zero, ancestor: overlay);
    final selected = await showMenu<String>(
      context: context,
      position: RelativeRect.fromLTRB(
        origin.dx,
        origin.dy + button.size.height,
        overlay.size.width - origin.dx - button.size.width,
        0,
      ),
      color: Theme.of(context).colorScheme.surface,
      items: [
        for (final profile in catalog.profiles)
          PopupMenuItem(
            key: Key('profile-item-${profile.id}'),
            value: 'switch:${profile.id}',
            child: MorphProfileTile(
              profile: profile,
              current: profile.id == catalog.current,
              canDelete: catalog.profiles.length > 1,
              onRename: () => Navigator.pop(context, 'rename:${profile.id}'),
              onDelete: () => Navigator.pop(context, 'delete:${profile.id}'),
              onOpenInNewWindow: () =>
                  Navigator.pop(context, 'window:${profile.id}'),
            ),
          ),
        const PopupMenuDivider(),
        const PopupMenuItem(
          key: Key('profile-add'),
          value: 'add',
          child: ListTile(
            dense: true,
            contentPadding: EdgeInsets.zero,
            leading: Icon(Icons.add, color: MorphColors.cyan),
            title: Text('Add profile'),
          ),
        ),
      ],
    );
    if (!context.mounted || selected == null) {
      return;
    }
    await _handleAction(context, selected);
  }

  Future<void> _handleAction(BuildContext context, String selected) async {
    if (selected == 'add') {
      final created = await showMorphAddProfileDialog(context);
      if (created != null) {
        await onAdd(created.$1, created.$2);
      }
      return;
    }
    final parts = selected.split(':');
    if (parts.length != 2) {
      return;
    }
    final action = parts[0];
    final id = parts[1];
    final profile = catalog.profiles.cast<ProfileInfo?>().firstWhere(
      (item) => item?.id == id,
      orElse: () => null,
    );
    if (profile == null) {
      return;
    }
    switch (action) {
      case 'switch':
        if (id != catalog.current) {
          onSwitch(id);
        }
      case 'rename':
        final name = await showMorphRenameProfileDialog(context, profile);
        if (name != null) {
          await onRename(id, name);
        }
      case 'delete':
        if (catalog.profiles.length <= 1) {
          return;
        }
        final confirmed = await showMorphDeleteProfileDialog(context, profile);
        if (confirmed == true) {
          await onDelete(id);
        }
      case 'window':
        if (context.mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            const SnackBar(content: Text('Open in new window is coming soon')),
          );
        }
    }
  }
}

/// A profile row used in the desktop menu and the mobile address sheet.
class MorphProfileTile extends StatelessWidget {
  const MorphProfileTile({
    super.key,
    required this.profile,
    required this.current,
    required this.canDelete,
    this.onSelect,
    this.onRename,
    this.onDelete,
    this.onOpenInNewWindow,
  });

  final ProfileInfo profile;
  final bool current;
  final bool canDelete;
  final VoidCallback? onSelect;
  final VoidCallback? onRename;
  final VoidCallback? onDelete;
  final VoidCallback? onOpenInNewWindow;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Row(
      children: [
        SizedBox(
          width: 18,
          child: current
              ? const Icon(Icons.check, size: 18, color: MorphColors.cyan)
              : null,
        ),
        const SizedBox(width: 8),
        MorphProfileAvatar(profile: profile, size: 28),
        const SizedBox(width: 10),
        Expanded(
          child: Text(
            profile.name,
            overflow: TextOverflow.ellipsis,
            style: MorphTheme.monoStyle(color: colors.onSurface, size: 13),
          ),
        ),
        IconButton(
          key: Key('profile-rename-${profile.id}'),
          tooltip: 'Rename',
          icon: const Icon(Icons.edit_outlined, size: 18),
          visualDensity: VisualDensity.compact,
          onPressed: onRename,
        ),
        if (canDelete)
          IconButton(
            key: Key('profile-delete-${profile.id}'),
            tooltip: 'Delete',
            icon: const Icon(Icons.delete_outline, size: 18),
            visualDensity: VisualDensity.compact,
            onPressed: onDelete,
          ),
        IconButton(
          key: Key('profile-open-window-${profile.id}'),
          tooltip: 'Open in new window',
          icon: const Icon(Icons.open_in_new, size: 18),
          visualDensity: VisualDensity.compact,
          onPressed: onOpenInNewWindow,
        ),
      ],
    );
  }
}

/// Profiles section for the mobile address sheet.
class MorphProfileSheetSection extends StatelessWidget {
  const MorphProfileSheetSection({
    super.key,
    required this.catalog,
    required this.onSwitch,
    required this.onAdd,
    required this.onRename,
    required this.onDelete,
  });

  final ProfileCatalog catalog;
  final ValueChanged<String> onSwitch;
  final Future<void> Function(String name, String color) onAdd;
  final Future<void> Function(String id, String name) onRename;
  final Future<void> Function(String id) onDelete;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Column(
      key: const Key('profile-sheet'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(
          'Profiles',
          style: MorphTheme.monoStyle(color: colors.onSurfaceVariant, size: 12),
        ),
        const SizedBox(height: 6),
        for (final profile in catalog.profiles)
          InkWell(
            key: Key('profile-item-${profile.id}'),
            onTap: () {
              if (profile.id != catalog.current) {
                onSwitch(profile.id);
              }
            },
            child: Padding(
              padding: const EdgeInsets.symmetric(vertical: 4),
              child: MorphProfileTile(
                profile: profile,
                current: profile.id == catalog.current,
                canDelete: catalog.profiles.length > 1,
                onRename: () async {
                  final name = await showMorphRenameProfileDialog(
                    context,
                    profile,
                  );
                  if (name != null) {
                    await onRename(profile.id, name);
                  }
                },
                onDelete: () async {
                  final confirmed = await showMorphDeleteProfileDialog(
                    context,
                    profile,
                  );
                  if (confirmed == true) {
                    await onDelete(profile.id);
                  }
                },
                onOpenInNewWindow: () {
                  ScaffoldMessenger.of(context).showSnackBar(
                    const SnackBar(
                      content: Text('Open in new window is coming soon'),
                    ),
                  );
                },
              ),
            ),
          ),
        TextButton.icon(
          key: const Key('profile-add'),
          onPressed: () async {
            final created = await showMorphAddProfileDialog(context);
            if (created != null) {
              await onAdd(created.$1, created.$2);
            }
          },
          icon: const Icon(Icons.add, color: MorphColors.cyan),
          label: const Text('Add profile'),
        ),
      ],
    );
  }
}

Future<(String, String)?> showMorphAddProfileDialog(BuildContext context) {
  return showDialog<(String, String)>(
    context: context,
    builder: (context) => const _AddProfileDialog(),
  );
}

Future<String?> showMorphRenameProfileDialog(
  BuildContext context,
  ProfileInfo profile,
) {
  return showDialog<String>(
    context: context,
    builder: (context) => _RenameProfileDialog(profile: profile),
  );
}

Future<bool?> showMorphDeleteProfileDialog(
  BuildContext context,
  ProfileInfo profile,
) {
  return showDialog<bool>(
    context: context,
    builder: (context) => AlertDialog(
      key: const Key('profile-delete-dialog'),
      title: const Text('Delete profile?'),
      content: Text(
        'Delete “${profile.name}” and all of its browsing data? This cannot be undone.',
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context, false),
          child: const Text('Cancel'),
        ),
        TextButton(
          key: const Key('profile-delete-confirm'),
          onPressed: () => Navigator.pop(context, true),
          child: const Text(
            'Delete',
            style: TextStyle(color: MorphColors.errorDark),
          ),
        ),
      ],
    ),
  );
}

class _AddProfileDialog extends StatefulWidget {
  const _AddProfileDialog();

  @override
  State<_AddProfileDialog> createState() => _AddProfileDialogState();
}

class _AddProfileDialogState extends State<_AddProfileDialog> {
  final _name = TextEditingController();
  late String _color = MorphProfileColors.token(
    MorphProfileColors.palette.first,
  );

  @override
  void dispose() {
    _name.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      key: const Key('profile-add-dialog'),
      title: const Text('Add profile'),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          TextField(
            key: const Key('profile-add-name'),
            controller: _name,
            autofocus: true,
            decoration: const InputDecoration(labelText: 'Name'),
            onSubmitted: (_) => _submit(),
          ),
          const SizedBox(height: 16),
          const Text('Color'),
          const SizedBox(height: 8),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              for (final color in MorphProfileColors.palette)
                _ColorDot(
                  key: Key(
                    'profile-add-color-${MorphProfileColors.token(color)}',
                  ),
                  color: color,
                  selected: _color == MorphProfileColors.token(color),
                  onTap: () =>
                      setState(() => _color = MorphProfileColors.token(color)),
                ),
            ],
          ),
        ],
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('Cancel'),
        ),
        TextButton(
          key: const Key('profile-add-confirm'),
          onPressed: _submit,
          child: const Text('Add'),
        ),
      ],
    );
  }

  void _submit() {
    final name = _name.text.trim();
    if (name.isEmpty) {
      return;
    }
    Navigator.pop(context, (name, _color));
  }
}

class _RenameProfileDialog extends StatefulWidget {
  const _RenameProfileDialog({required this.profile});

  final ProfileInfo profile;

  @override
  State<_RenameProfileDialog> createState() => _RenameProfileDialogState();
}

class _RenameProfileDialogState extends State<_RenameProfileDialog> {
  late final TextEditingController _name = TextEditingController(
    text: widget.profile.name,
  );

  @override
  void dispose() {
    _name.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      key: const Key('profile-rename-dialog'),
      title: const Text('Rename profile'),
      content: TextField(
        key: const Key('profile-rename-name'),
        controller: _name,
        autofocus: true,
        decoration: const InputDecoration(labelText: 'Name'),
        onSubmitted: (_) => _submit(),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('Cancel'),
        ),
        TextButton(
          key: const Key('profile-rename-confirm'),
          onPressed: _submit,
          child: const Text('Save'),
        ),
      ],
    );
  }

  void _submit() {
    final name = _name.text.trim();
    if (name.isEmpty) {
      return;
    }
    Navigator.pop(context, name);
  }
}

class _ColorDot extends StatelessWidget {
  const _ColorDot({
    super.key,
    required this.color,
    required this.selected,
    required this.onTap,
  });

  final Color color;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return InkWell(
      onTap: onTap,
      customBorder: const CircleBorder(),
      child: Container(
        width: 28,
        height: 28,
        decoration: BoxDecoration(
          color: color,
          shape: BoxShape.circle,
          border: Border.all(
            color: selected ? MorphColors.white : color,
            width: selected ? 2 : 1,
          ),
        ),
      ),
    );
  }
}
