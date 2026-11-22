import 'dart:async';

import 'package:material_ui/material_ui.dart';

import '../api.dart';
import '../default_browser.dart';
import '../theme/morph_theme.dart';
import 'library_page.dart';
import 'linked_devices.dart';
import 'network_settings.dart';
import 'passwords_page.dart';
import 'relay_status.dart';
import 'sync_types.dart';

/// Morph settings: this profile's data, the default browser, and About.
class MorphSettingsPage extends StatefulWidget {
  const MorphSettingsPage({
    super.key,
    required this.profileName,
    required this.coreVersion,
    required this.renderPath,
    required this.onClearHistory,
    required this.onOpenLibrary,
    this.defaultBrowser,
    this.relays,
    this.network,
    this.devices,
    this.profileId,
    this.home,
    this.passwords,
  });

  /// Linked devices of [profileId]; null (or no [profileId]) hides them.
  final MorphDeviceSource? devices;
  final String? profileId;

  /// Null hides the Relays section.
  final MorphRelaySource? relays;

  /// Discovery and manual peers in the Relays section; null hides them.
  final MorphNetworkSource? network;

  /// The Morph home [network] reads (null: the default).
  final String? home;

  /// The API the Passwords page uses; null (or no [profileId]) hides it.
  final MorphApi? passwords;

  /// Null or unsupported hides the Default browser section.
  final MorphDefaultBrowser? defaultBrowser;

  final String profileName;
  final String coreVersion;
  final String renderPath;
  final Future<void> Function() onClearHistory;
  final ValueChanged<MorphLibraryTab> onOpenLibrary;

  @override
  State<MorphSettingsPage> createState() => _MorphSettingsPageState();
}

class _MorphSettingsPageState extends State<MorphSettingsPage> {
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
          if (widget.passwords != null && widget.profileId != null)
            ListTile(
              key: const Key('settings-passwords'),
              leading: const Icon(Icons.password),
              title: const Text('Passwords'),
              subtitle: const Text('Saved logins in this profile'),
              onTap: () {
                Navigator.of(context).push(
                  MaterialPageRoute<void>(
                    builder: (_) => MorphPasswordsPage(
                      api: widget.passwords!,
                      profileId: widget.profileId!,
                      home: widget.home,
                    ),
                  ),
                );
              },
            ),
          if (widget.devices != null && widget.profileId != null) ...[
            _Header('Sync · ${widget.profileName}'),
            MorphSyncTypesSection(
              key: ValueKey('sync-types-${widget.profileId}'),
              source: widget.devices!,
              profileId: widget.profileId!,
              home: widget.home,
            ),
            _Header('Linked devices · ${widget.profileName}'),
            MorphLinkedDevicesSection(
              key: ValueKey('linked-devices-${widget.profileId}'),
              source: widget.devices!,
              profileId: widget.profileId!,
              home: widget.home,
            ),
          ],
          if (widget.defaultBrowser?.supported ?? false) ...[
            const _Header('Browser · all profiles'),
            _defaultBrowserTile(),
          ],
          if (widget.relays != null || widget.network != null) ...[
            const _Header('Relays · all profiles'),
            if (widget.relays != null)
              MorphRelaySection(source: widget.relays!),
            if (widget.network != null)
              MorphNetworkSection(source: widget.network!, home: widget.home),
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
