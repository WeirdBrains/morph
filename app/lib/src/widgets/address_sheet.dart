import 'package:material_ui/material_ui.dart';
import 'package:flutter/services.dart';

import '../api.dart';
import '../theme/morph_theme.dart';

/// Firefox-mobile-style address and actions sheet (no VPN row).
///
/// The editable URL sits at the top so typing still works. Back, forward,
/// share, refresh, and the desktop-site toggle call through to the shell.
/// Everything else shows a "Coming soon" snackbar.
class MorphAddressSheet extends StatefulWidget {
  const MorphAddressSheet({
    super.key,
    required this.initialUrl,
    required this.canGoBack,
    required this.canGoForward,
    required this.desktopSite,
    this.onSubmit,
    this.onBack,
    this.onForward,
    this.onShare,
    this.onReload,
    this.onDesktopSiteChanged,
    this.onFind,
    this.bookmarked = false,
    this.onToggleBookmark,
    this.onHistory,
    this.onBookmarks,
    this.onSettings,
    this.onIdentity,
    this.identityLabel,
    this.profiles,
    this.engines = const [],
    this.engine,
    this.onEngineChanged,
  });

  final String initialUrl;
  final bool canGoBack;
  final bool canGoForward;
  final bool desktopSite;
  final ValueChanged<String>? onSubmit;
  final VoidCallback? onBack;
  final VoidCallback? onForward;
  final Future<void> Function(String url)? onShare;
  final VoidCallback? onReload;
  final ValueChanged<bool>? onDesktopSiteChanged;
  final VoidCallback? onFind;
  final bool bookmarked;
  final VoidCallback? onToggleBookmark;
  final VoidCallback? onHistory;
  final VoidCallback? onBookmarks;
  final VoidCallback? onSettings;
  final VoidCallback? onIdentity;
  final String? identityLabel;

  /// Optional profiles section, listed under the actions.
  final Widget? profiles;

  /// The search engines for the selector left of the address (as on
  /// desktop), the one this tab searches with, and what picking one does.
  final List<SearchEngineOption> engines;
  final String? engine;
  final ValueChanged<String>? onEngineChanged;

  @override
  State<MorphAddressSheet> createState() => _MorphAddressSheetState();
}

class _MorphAddressSheetState extends State<MorphAddressSheet> {
  late final TextEditingController _url;
  final _focus = FocusNode();
  late bool _desktop;
  String? _engine;

  @override
  void initState() {
    super.initState();
    _desktop = widget.desktopSite;
    _engine = widget.engine;
    _url = TextEditingController(text: widget.initialUrl);
    _url.selection = TextSelection(
      baseOffset: 0,
      extentOffset: widget.initialUrl.length,
    );
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) {
        _focus.requestFocus();
        _url.selection = TextSelection(
          baseOffset: 0,
          extentOffset: _url.text.length,
        );
      }
    });
  }

  @override
  void dispose() {
    _focus.dispose();
    _url.dispose();
    super.dispose();
  }

  /// Closes the sheet, then runs [action], or says [feature] is coming soon.
  VoidCallback _closeThen(VoidCallback? action, String feature) {
    if (action == null) {
      return () => _comingSoon(feature);
    }
    return () {
      // pop, not maybePop: maybePop resolves later and would close whatever
      // the action opens instead of this sheet.
      Navigator.of(context).pop();
      action();
    };
  }

  void _comingSoon(String feature) {
    ScaffoldMessenger.of(context)
        .showSnackBar(SnackBar(content: Text('$feature is coming soon')));
  }

  Widget _urlField(ColorScheme colors) {
    return TextField(
      key: const Key('address-sheet-url'),
      controller: _url,
      focusNode: _focus,
      autofocus: true,
      keyboardType: TextInputType.url,
      textInputAction: TextInputAction.go,
      style: MorphTheme.monoStyle(color: colors.onSurface, size: 14),
      decoration: const InputDecoration(hintText: 'Enter an address'),
      onTap: () {
        _url.selection = TextSelection(
          baseOffset: 0,
          extentOffset: _url.text.length,
        );
      },
      onSubmitted: (value) {
        // Close first: what the address opens (a page, or for morph://
        // addresses a panel of its own) must not be what this pop closes.
        final submit = widget.onSubmit;
        Navigator.of(context).pop();
        submit?.call(value);
      },
    );
  }

  /// The engine phrases typed here go to, left of the address.
  Widget _engineSelector(ColorScheme colors) {
    final current = widget.engines.firstWhere(
      (e) => e.id == _engine,
      orElse: () => widget.engines.first,
    );
    return PopupMenuButton<String>(
      key: const Key('address-sheet-engine'),
      tooltip: 'Search with ${current.name}',
      initialValue: current.id,
      onSelected: (id) {
        setState(() => _engine = id);
        widget.onEngineChanged?.call(id);
        _focus.requestFocus();
      },
      itemBuilder: (context) => [
        for (final e in widget.engines)
          PopupMenuItem<String>(
            key: Key('address-sheet-engine-${e.id}'),
            value: e.id,
            child: Text(e.name),
          ),
      ],
      child: Padding(
        padding: const EdgeInsets.only(right: 8),
        child: CircleAvatar(
          radius: 16,
          backgroundColor: colors.surfaceContainerHighest,
          child: Text(
            current.mark,
            style: MorphTheme.monoStyle(color: colors.onSurface, size: 14),
          ),
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final bottom = MediaQuery.viewInsetsOf(context).bottom;
    return Padding(
      padding: EdgeInsets.only(bottom: bottom),
      child: Material(
        key: const Key('address-sheet'),
        color: colors.surface,
        borderRadius: const BorderRadius.vertical(top: Radius.circular(16)),
        child: SafeArea(
          top: false,
          child: SingleChildScrollView(
            padding: const EdgeInsets.fromLTRB(16, 12, 16, 16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              mainAxisSize: MainAxisSize.min,
              children: [
                Center(
                  child: Container(
                    width: 36,
                    height: 4,
                    margin: const EdgeInsets.only(bottom: 12),
                    decoration: BoxDecoration(
                      color: colors.outline,
                      borderRadius: BorderRadius.circular(2),
                    ),
                  ),
                ),
                Row(
                  children: [
                    if (widget.engines.isNotEmpty) _engineSelector(colors),
                    Expanded(child: _urlField(colors)),
                  ],
                ),
                const SizedBox(height: 12),
                Row(
                  mainAxisAlignment: MainAxisAlignment.spaceEvenly,
                  children: [
                    _Action(
                      key: const Key('sheet-back'),
                      icon: Icons.arrow_back,
                      label: 'Back',
                      enabled: widget.canGoBack,
                      onTap: widget.canGoBack
                          ? () {
                              widget.onBack?.call();
                              Navigator.of(context).maybePop();
                            }
                          : null,
                    ),
                    _Action(
                      key: const Key('sheet-forward'),
                      icon: Icons.arrow_forward,
                      label: 'Forward',
                      enabled: widget.canGoForward,
                      onTap: widget.canGoForward
                          ? () {
                              widget.onForward?.call();
                              Navigator.of(context).maybePop();
                            }
                          : null,
                    ),
                    _Action(
                      key: const Key('sheet-share'),
                      icon: Icons.share_outlined,
                      label: 'Share',
                      onTap: () async {
                        final url = _url.text.trim();
                        if (url.isEmpty) {
                          return;
                        }
                        if (widget.onShare != null) {
                          await widget.onShare!(url);
                        } else {
                          await Clipboard.setData(ClipboardData(text: url));
                          if (context.mounted) {
                            ScaffoldMessenger.of(context).showSnackBar(
                              const SnackBar(content: Text('Link copied')),
                            );
                          }
                        }
                      },
                    ),
                    _Action(
                      key: const Key('sheet-reload'),
                      icon: Icons.refresh,
                      label: 'Refresh',
                      onTap: () {
                        widget.onReload?.call();
                        Navigator.of(context).maybePop();
                      },
                    ),
                  ],
                ),
                const SizedBox(height: 8),
                _RowTile(
                  key: const Key('sheet-bookmark'),
                  icon: widget.bookmarked
                      ? Icons.bookmark
                      : Icons.bookmark_border,
                  label: widget.bookmarked
                      ? 'Remove bookmark'
                      : 'Bookmark page',
                  onTap: _closeThen(widget.onToggleBookmark, 'Bookmarks'),
                ),
                _RowTile(
                  key: const Key('sheet-find'),
                  icon: Icons.search,
                  label: 'Find in page',
                  onTap: _closeThen(widget.onFind, 'Find in page'),
                ),
                SwitchListTile(
                  key: const Key('sheet-desktop'),
                  contentPadding: EdgeInsets.zero,
                  secondary: const Icon(Icons.computer_outlined),
                  title: const Text('Desktop site'),
                  value: _desktop,
                  activeThumbColor: MorphColors.cyan,
                  onChanged: (value) {
                    setState(() => _desktop = value);
                    widget.onDesktopSiteChanged?.call(value);
                  },
                ),
                _RowTile(
                  key: const Key('sheet-extensions'),
                  icon: Icons.extension_outlined,
                  label: 'Extensions',
                  trailing: const Icon(Icons.expand_more),
                  onTap: () => _comingSoon('Extensions'),
                ),
                _RowTile(
                  key: const Key('sheet-more'),
                  icon: Icons.more_horiz,
                  label: 'More',
                  trailing: const Icon(Icons.expand_more),
                  onTap: () => _comingSoon('More'),
                ),
                const SizedBox(height: 8),
                Row(
                  children: [
                    _Tile(
                      key: const Key('sheet-history'),
                      icon: Icons.history,
                      label: 'History',
                      onTap: _closeThen(widget.onHistory, 'History'),
                    ),
                    _Tile(
                      key: const Key('sheet-bookmarks'),
                      icon: Icons.bookmarks_outlined,
                      label: 'Bookmarks',
                      onTap: _closeThen(widget.onBookmarks, 'Bookmarks'),
                    ),
                    _Tile(
                      key: const Key('sheet-downloads'),
                      icon: Icons.download_outlined,
                      label: 'Downloads',
                      onTap: () => _comingSoon('Downloads'),
                    ),
                    _Tile(
                      key: const Key('sheet-passwords'),
                      icon: Icons.password,
                      label: 'Passwords',
                      onTap: () => _comingSoon('Passwords'),
                    ),
                  ],
                ),
                const SizedBox(height: 8),
                if (widget.profiles != null) ...[
                  const SizedBox(height: 8),
                  widget.profiles!,
                  const SizedBox(height: 8),
                ],
                _RowTile(
                  key: const Key('sheet-identity'),
                  icon: Icons.fingerprint,
                  label: widget.identityLabel ?? 'Identity',
                  onTap: widget.onIdentity != null
                      ? _closeThen(widget.onIdentity, 'Identity')
                      : widget.identityLabel == null
                      ? () => _comingSoon('Identity')
                      : null,
                ),
                _RowTile(
                  key: const Key('sheet-settings'),
                  icon: Icons.settings_outlined,
                  label: 'Settings',
                  onTap: _closeThen(widget.onSettings, 'Settings'),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _Action extends StatelessWidget {
  const _Action({
    super.key,
    required this.icon,
    required this.label,
    this.enabled = true,
    this.onTap,
  });

  final IconData icon;
  final String label;
  final bool enabled;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final color = enabled
        ? colors.onSurface
        : colors.onSurface.withValues(alpha: 0.38);
    return InkWell(
      onTap: enabled ? onTap : null,
      borderRadius: BorderRadius.circular(12),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 8),
        child: Column(
          children: [
            Icon(icon, color: color),
            const SizedBox(height: 4),
            Text(label, style: TextStyle(color: color, fontSize: 11)),
          ],
        ),
      ),
    );
  }
}

class _RowTile extends StatelessWidget {
  const _RowTile({
    super.key,
    required this.icon,
    required this.label,
    this.trailing,
    this.onTap,
  });

  final IconData icon;
  final String label;
  final Widget? trailing;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    return ListTile(
      contentPadding: EdgeInsets.zero,
      leading: Icon(icon),
      title: Text(label),
      trailing: trailing,
      onTap: onTap,
    );
  }
}

class _Tile extends StatelessWidget {
  const _Tile({super.key, required this.icon, required this.label, this.onTap});

  final IconData icon;
  final String label;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    return Expanded(
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(12),
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 12),
          child: Column(
            children: [
              Icon(icon),
              const SizedBox(height: 6),
              Text(
                label,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: const TextStyle(fontSize: 11),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
