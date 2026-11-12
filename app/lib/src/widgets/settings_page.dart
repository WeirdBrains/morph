import 'dart:async';

import 'package:flutter/material.dart';

import '../default_browser.dart';

import '../tabs/tab_position.dart';
import '../theme/morph_theme.dart';
import 'library_page.dart';

/// Morph settings: this profile's data, then app-wide layout and About.
class MorphSettingsPage extends StatefulWidget {
  const MorphSettingsPage({
    super.key,
    required this.profileName,
    required this.coreVersion,
    required this.renderPath,
    required this.onClearHistory,
    required this.onOpenLibrary,
    this.tabPosition,
    this.sidebarCollapsed,
    this.onTabPositionChanged,
    this.onSidebarCollapsedChanged,
    this.defaultBrowser,
  });

  /// Null or unsupported hides the Default browser section.
  final MorphDefaultBrowser? defaultBrowser;

  final String profileName;
  final String coreVersion;
  final String renderPath;
  final Future<void> Function() onClearHistory;
  final ValueChanged<MorphLibraryTab> onOpenLibrary;

  /// Desktop layout. Null hides the section (phones).
  final MorphTabPosition? tabPosition;
  final bool? sidebarCollapsed;
  final ValueChanged<MorphTabPosition>? onTabPositionChanged;
  final ValueChanged<bool>? onSidebarCollapsedChanged;

  @override
  State<MorphSettingsPage> createState() => _MorphSettingsPageState();
}

class _MorphSettingsPageState extends State<MorphSettingsPage> {
  late MorphTabPosition? _position = widget.tabPosition;
  late bool? _collapsed = widget.sidebarCollapsed;
  MorphDefaultBrowserState? _browserState;
  bool _askingDefault = false;

  @override
  void initState() {
    super.initState();
    unawaited(_refreshDefault());
  }

  Future<void> _refreshDefault() async {
    final browser = widget.defaultBrowser;
    if (browser == null || !browser.supported) {
      return;
    }
    final state = await browser.status();
    if (mounted) {
      setState(() => _browserState = state);
    }
  }

  Future<void> _makeDefault() async {
    final browser = widget.defaultBrowser;
    if (browser == null || _askingDefault) {
      return;
    }
    setState(() => _askingDefault = true);
    final outcome = await browser.makeDefault();
    if (!mounted) {
      return;
    }
    setState(() => _askingDefault = false);
    final message = switch (outcome) {
      MorphDefaultBrowserRequest.madeDefault =>
        'Morph is now your default browser',
      MorphDefaultBrowserRequest.openedSettings =>
        'Pick Morph as the web browser in the system settings',
      MorphDefaultBrowserRequest.declined => 'Morph was not made the default',
      MorphDefaultBrowserRequest.failed =>
        "This system didn't let Morph become the default",
    };
    ScaffoldMessenger.of(context)
        .showSnackBar(SnackBar(content: Text(message)));
    await _refreshDefault();
  }

  Widget _defaultBrowserTile() {
    final state = _browserState;
    final isDefault = state == MorphDefaultBrowserState.isDefault;
    return ListTile(
      key: const Key('settings-default-browser'),
      leading: Icon(isDefault ? Icons.verified_outlined : Icons.public),
      title: const Text('Default browser'),
      subtitle: Text(
        key: const Key('settings-default-browser-status'),
        switch (state) {
          null => 'Checking…',
          MorphDefaultBrowserState.isDefault => 'Morph opens your links',
          MorphDefaultBrowserState.notDefault =>
            'Another browser opens your links',
          MorphDefaultBrowserState.unknown => "Couldn't tell which browser",
        },
      ),
      trailing: isDefault
          ? null
          : FilledButton.tonal(
              key: const Key('settings-make-default'),
              onPressed: _askingDefault
                  ? null
                  : () => unawaited(_makeDefault()),
              child: const Text('Make default'),
            ),
    );
  }

  Future<void> _clearHistory() async {
    if (!await confirmClearHistory(context, widget.profileName)) {
      return;
    }
    await widget.onClearHistory();
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('History cleared in ${widget.profileName}')),
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final mono = MorphTheme.monoStyle(
      color: colors.onSurface.withValues(alpha: 0.7),
      size: 12,
    );
    return Scaffold(
      key: const Key('settings-page'),
      appBar: AppBar(title: const Text('Settings')),
      body: ListView(
        children: [
          _Header('Profile · ${widget.profileName}'),
          ListTile(
            key: const Key('settings-bookmarks'),
            leading: const Icon(Icons.bookmarks_outlined),
            title: const Text('Bookmarks'),
            onTap: () => widget.onOpenLibrary(MorphLibraryTab.bookmarks),
          ),
          ListTile(
            key: const Key('settings-history'),
            leading: const Icon(Icons.history),
            title: const Text('History'),
            onTap: () => widget.onOpenLibrary(MorphLibraryTab.history),
          ),
          ListTile(
            key: const Key('settings-clear-history'),
            leading: const Icon(
              Icons.delete_sweep_outlined,
              color: MorphColors.magenta,
            ),
            title: const Text('Clear history'),
            subtitle: Text('Only ${widget.profileName} is affected'),
            onTap: _clearHistory,
          ),
          if (widget.defaultBrowser?.supported ?? false) ...[
            const _Header('Browser · all profiles'),
            _defaultBrowserTile(),
          ],
          if (_position != null) ...[
            const _Header('Layout · all profiles'),
            ListTile(
              leading: const Icon(Icons.tab_outlined),
              title: const Text('Tab position'),
              trailing: DropdownButton<MorphTabPosition>(
                key: const Key('settings-tab-position'),
                value: _position,
                underline: const SizedBox.shrink(),
                items: [
                  for (final p in MorphTabPosition.values)
                    DropdownMenuItem(
                      value: p,
                      child: Text(
                        '${p.name[0].toUpperCase()}${p.name.substring(1)}',
                      ),
                    ),
                ],
                onChanged: (value) {
                  if (value == null) {
                    return;
                  }
                  setState(() => _position = value);
                  widget.onTabPositionChanged?.call(value);
                },
              ),
            ),
            if (_collapsed != null)
              SwitchListTile(
                key: const Key('settings-sidebar-collapsed'),
                secondary: const Icon(Icons.view_sidebar_outlined),
                title: const Text('Collapse side tab list'),
                value: _collapsed!,
                activeThumbColor: MorphColors.cyan,
                onChanged: (value) {
                  setState(() => _collapsed = value);
                  widget.onSidebarCollapsedChanged?.call(value);
                },
              ),
          ],
          const _Header('About'),
          ListTile(
            leading: const Icon(Icons.info_outline),
            title: const Text('Morph'),
            subtitle: Text('core ${widget.coreVersion}', style: mono),
          ),
          ListTile(
            leading: const Icon(Icons.layers_outlined),
            title: const Text('Web view'),
            subtitle: Text(widget.renderPath, style: mono),
          ),
        ],
      ),
    );
  }
}

class _Header extends StatelessWidget {
  const _Header(this.text);

  final String text;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 20, 16, 6),
      child: Text(
        text.toUpperCase(),
        style: const TextStyle(
          color: MorphColors.cyan,
          fontSize: 12,
          fontWeight: FontWeight.w700,
          letterSpacing: 1.2,
        ),
      ),
    );
  }
}
