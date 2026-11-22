import 'dart:async';

import 'package:material_ui/material_ui.dart';

import '../api.dart';

/// Settings → Sync: a switch for each type this device sends and receives
/// for the profile. The choices stay on this device.
class MorphSyncTypesSection extends StatefulWidget {
  const MorphSyncTypesSection({
    super.key,
    required this.source,
    required this.profileId,
    this.home,
  });

  final MorphDeviceSource source;
  final String profileId;
  final String? home;

  @override
  State<MorphSyncTypesSection> createState() => _MorphSyncTypesSectionState();
}

class _MorphSyncTypesSectionState extends State<MorphSyncTypesSection> {
  List<MorphSyncType>? _types;
  String? _error;
  bool _busy = false;

  @override
  void initState() {
    super.initState();
    unawaited(_load());
  }

  Future<void> _load() async {
    try {
      final list = await widget.source.syncTypes(
        widget.profileId,
        home: widget.home,
      );
      if (mounted) {
        setState(() {
          _types = list;
          _error = null;
        });
      }
    } catch (e) {
      if (mounted) {
        setState(() {
          _types = const [];
          _error = '$e';
        });
      }
    }
  }

  Future<void> _set(MorphSyncType typ, bool on) async {
    if (_busy) {
      return;
    }
    setState(() => _busy = true);
    try {
      final list = await widget.source.setSyncType(
        widget.profileId,
        typ.id,
        on,
        home: widget.home,
      );
      if (mounted) {
        setState(() {
          _types = list;
          _error = null;
        });
      }
    } catch (e) {
      if (mounted) {
        setState(() => _error = '$e');
      }
    } finally {
      if (mounted) {
        setState(() => _busy = false);
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final types = _types;
    if (types == null) {
      return const ListTile(
        key: Key('sync-types-loading'),
        title: Text('Loading…'),
      );
    }
    final colors = Theme.of(context).colorScheme;
    if (types.isEmpty) {
      return ListTile(
        key: const Key('sync-types-empty'),
        leading: const Icon(Icons.sync_disabled),
        title: const Text('Synced data'),
        subtitle: Text(
          _error ??
              'Unlock the library and link a device from Identity to choose '
                  'what this device syncs.',
        ),
      );
    }
    return Column(
      key: const Key('sync-types'),
      children: [
        const ListTile(
          key: Key('sync-types-help'),
          leading: Icon(Icons.sync),
          title: Text('Synced data'),
          subtitle: Text(
            'What this device sends and receives for this profile. '
            'Turning a type off does not delete data already here. '
            'These choices stay on this device.',
          ),
        ),
        if (_error != null)
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 16),
            child: Text(
              _error!,
              key: const Key('sync-types-error'),
              style: TextStyle(color: colors.error),
            ),
          ),
        for (final typ in types)
          SwitchListTile(
            key: Key('sync-type-${typ.id}'),
            title: Text(typ.label),
            subtitle: typ.help.isEmpty ? null : Text(typ.help),
            value: typ.on,
            onChanged: _busy ? null : (on) => unawaited(_set(typ, on)),
          ),
      ],
    );
  }
}
