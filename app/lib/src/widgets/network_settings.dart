import 'dart:async';

import 'package:material_ui/material_ui.dart';

import '../api.dart';
import '../theme/morph_theme.dart';

/// Settings → Relays: the Weirdnet discovery switch and manually added
/// peers, as on the desktop. Turning discovery off keeps sharing, relays and
/// sync with added peers; an admin lock keeps the switch off.
class MorphNetworkSection extends StatefulWidget {
  const MorphNetworkSection({super.key, required this.source, this.home});

  final MorphNetworkSource source;
  final String? home;

  @override
  State<MorphNetworkSection> createState() => _MorphNetworkSectionState();
}

class _MorphNetworkSectionState extends State<MorphNetworkSection> {
  final _peer = TextEditingController();
  MorphNetworkSettings? _net;
  String? _error;
  String? _peerError;
  bool _busy = false;

  @override
  void initState() {
    super.initState();
    unawaited(_run(() => widget.source.loadNetwork(home: widget.home)));
  }

  @override
  void dispose() {
    _peer.dispose();
    super.dispose();
  }

  /// Runs one change; a failure shows under the switch, or under the peer
  /// field when [peer] is set.
  Future<bool> _run(
    Future<MorphNetworkSettings> Function() call, {
    bool peer = false,
  }) async {
    setState(() => _busy = true);
    try {
      final next = await call();
      if (mounted) {
        setState(() {
          _net = next;
          _error = null;
          _peerError = null;
        });
      }
      return true;
    } catch (e) {
      if (mounted) {
        setState(() {
          if (peer) {
            _peerError = '$e';
          } else {
            _error = 'Could not save the network settings: $e';
          }
        });
      }
      return false;
    } finally {
      if (mounted) {
        setState(() => _busy = false);
      }
    }
  }

  Future<void> _setDiscovery(bool on) =>
      _run(() => widget.source.setDiscovery(on, home: widget.home));

  Future<void> _addPeer() async {
    final spec = _peer.text.trim();
    if (spec.isEmpty) {
      setState(
        () => _peerError = 'Enter a multiaddr ending in /p2p/<id>, or a peer id and an address',
      );
      return;
    }
    final ok = await _run(
      () => widget.source.addNetworkPeer(spec, home: widget.home),
      peer: true,
    );
    if (ok && mounted) {
      _peer.clear();
    }
  }

  Future<void> _removePeer(String id) =>
      _run(() => widget.source.removeNetworkPeer(id, home: widget.home));

  @override
  Widget build(BuildContext context) {
    final net = _net;
    final colors = Theme.of(context).colorScheme;
    final muted = TextStyle(
      fontSize: 12,
      color: colors.onSurface.withValues(alpha: 0.7),
    );
    final mono = MorphTheme.monoStyle(
      color: colors.onSurface.withValues(alpha: 0.6),
      size: 11,
    );
    if (net == null) {
      return ListTile(
        key: const Key('settings-network'),
        title: const Text('Weirdnet discovery'),
        subtitle: Text(_error ?? 'Loading…'),
      );
    }
    final locked = net.discoveryLocked;
    return Column(
      key: const Key('settings-network'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        SwitchListTile(
          key: const Key('settings-discovery'),
          secondary: Icon(locked ? Icons.lock_outline : Icons.radar),
          title: const Text('Weirdnet discovery'),
          subtitle: Text(
            net.discoveryHelp,
            key: const Key('settings-discovery-help'),
          ),
          value: net.discoveryEnabled,
          onChanged: locked || _busy
              ? null
              : (on) => unawaited(_setDiscovery(on)),
        ),
        if (_error != null)
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 16),
            child: Text(
              _error!,
              key: const Key('settings-network-error'),
              style: TextStyle(color: colors.error),
            ),
          ),
        ListTile(
          leading: const Icon(Icons.device_hub),
          title: const Text('Manual peers'),
          subtitle: Text(
            'Add a peer by multiaddr (ending in /p2p/<id>) to sync without '
            'discovery. An address is not trust; certificates authorize a '
            'node.',
            style: muted,
          ),
        ),
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Expanded(
                child: TextField(
                  key: const Key('settings-peer-input'),
                  controller: _peer,
                  autocorrect: false,
                  enableSuggestions: false,
                  keyboardType: TextInputType.url,
                  textInputAction: TextInputAction.done,
                  style: MorphTheme.monoStyle(
                    color: colors.onSurface,
                    size: 12,
                  ),
                  decoration: InputDecoration(
                    isDense: true,
                    hintText: '/ip4/192.168.1.5/tcp/4747/p2p/12D3KooW…',
                    errorText: _peerError,
                    errorMaxLines: 3,
                  ),
                  onSubmitted: (_) => unawaited(_addPeer()),
                ),
              ),
              const SizedBox(width: 8),
              FilledButton.tonal(
                key: const Key('settings-peer-add'),
                onPressed: _busy ? null : () => unawaited(_addPeer()),
                child: const Text('Add'),
              ),
            ],
          ),
        ),
        for (final p in net.peers)
          ListTile(
            key: Key('settings-peer-${p.peerId}'),
            dense: true,
            leading: const Icon(Icons.computer_outlined),
            title: SelectableText(p.peerId, style: mono),
            subtitle: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                for (final a in p.addrs) SelectableText(a, style: mono),
              ],
            ),
            trailing: IconButton(
              key: Key('settings-peer-remove-${p.peerId}'),
              tooltip: 'Remove',
              icon: const Icon(Icons.close),
              onPressed: _busy ? null : () => unawaited(_removePeer(p.peerId)),
            ),
          ),
      ],
    );
  }
}
