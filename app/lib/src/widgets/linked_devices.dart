import 'dart:async';

import 'package:material_ui/material_ui.dart';

import '../api.dart';
import '../theme/morph_theme.dart';

/// Settings → Linked devices: this profile's devices with their state, the
/// key notes after a rotation ("Needs re-admission from another device",
/// "Waiting for its first sync"), and Revoke, which asks first and warns
/// about devices the new key would leave out.
class MorphLinkedDevicesSection extends StatefulWidget {
  const MorphLinkedDevicesSection({
    super.key,
    required this.source,
    required this.profileId,
    this.home,
  });

  final MorphDeviceSource source;
  final String profileId;
  final String? home;

  @override
  State<MorphLinkedDevicesSection> createState() =>
      _MorphLinkedDevicesSectionState();
}

class _MorphLinkedDevicesSectionState extends State<MorphLinkedDevicesSection> {
  List<MorphLinkedDevice>? _devices;
  String? _error;
  bool _busy = false;

  @override
  void initState() {
    super.initState();
    unawaited(_load());
  }

  Future<void> _load() async {
    try {
      final list = await widget.source.linkedDevices(
        widget.profileId,
        home: widget.home,
      );
      if (mounted) {
        setState(() {
          _devices = list;
          _error = null;
        });
      }
    } catch (e) {
      if (mounted) {
        setState(() {
          _devices = const [];
          _error = '$e';
        });
      }
    }
  }

  Future<void> _revoke(MorphLinkedDevice device) async {
    if (_busy) {
      return;
    }
    setState(() => _busy = true);
    String? warning;
    try {
      warning = await widget.source.linkedDeviceRevokeWarning(
        widget.profileId,
        device.peerId,
        home: widget.home,
      );
    } catch (_) {
      warning = null;
    }
    if (!mounted) {
      return;
    }
    final name = device.label.isEmpty ? 'this device' : device.label;
    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        key: const Key('revoke-device-dialog'),
        scrollable: true,
        title: Text('Revoke $name?'),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const Text(
              "It stops syncing with this profile's devices, and the sync key "
              'changes, so it cannot read anything synced afterwards, even '
              'with your recovery words.',
            ),
            if (warning != null) ...[
              const SizedBox(height: 12),
              Text(
                warning,
                key: const Key('revoke-device-warning'),
                style: const TextStyle(color: MorphColors.magenta),
              ),
            ],
          ],
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            key: const Key('revoke-device-confirm'),
            onPressed: () => Navigator.of(context).pop(true),
            child: const Text('Revoke'),
          ),
        ],
      ),
    );
    if (ok != true) {
      if (mounted) {
        setState(() => _busy = false);
      }
      return;
    }
    try {
      final list = await widget.source.revokeLinkedDevice(
        widget.profileId,
        device.peerId,
        home: widget.home,
      );
      if (mounted) {
        setState(() => _devices = list);
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('$name revoked')));
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(
          context,
        ).showSnackBar(SnackBar(content: Text('Could not revoke $name: $e')));
      }
    } finally {
      if (mounted) {
        setState(() => _busy = false);
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final devices = _devices;
    if (devices == null) {
      return const ListTile(
        key: Key('linked-devices-loading'),
        title: Text('Loading…'),
      );
    }
    if (devices.isEmpty) {
      return ListTile(
        key: const Key('linked-devices-empty'),
        leading: const Icon(Icons.devices_other),
        title: const Text('No linked devices'),
        subtitle: Text(
          _error ??
              'Link a device from Identity → Add device to sync this '
                  "profile's library.",
        ),
      );
    }
    return Column(
      key: const Key('linked-devices'),
      children: [
        for (final d in devices)
          ListTile(
            key: Key('linked-device-${d.peerId}'),
            leading: Icon(
              d.thisDevice ? Icons.smartphone : Icons.devices_other,
              color: d.revoked ? MorphColors.magenta : null,
            ),
            title: Text(d.label.isEmpty ? 'Unnamed device' : d.label),
            subtitle: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(d.state),
                if (d.note != null && !d.revoked)
                  Text(
                    d.note!,
                    key: Key('linked-device-note-${d.peerId}'),
                    style: const TextStyle(color: MorphColors.cyan),
                  ),
              ],
            ),
            trailing: d.thisDevice || d.revoked
                ? null
                : TextButton(
                    key: Key('linked-device-revoke-${d.peerId}'),
                    onPressed: _busy ? null : () => unawaited(_revoke(d)),
                    child: const Text('Revoke'),
                  ),
          ),
      ],
    );
  }
}
