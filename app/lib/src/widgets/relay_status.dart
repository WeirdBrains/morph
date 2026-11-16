import 'dart:async';

import 'package:material_ui/material_ui.dart';

import '../api.dart';
import '../theme/morph_theme.dart';

/// How often the relay widgets read the state again (a cheap, local call).
const morphRelayPoll = Duration(seconds: 1);

/// Reads [source] now and every [morphRelayPoll] while mounted.
mixin _RelayPolling<T extends StatefulWidget> on State<T> {
  MorphRelaySource get source;
  MorphRelayReport? report;
  Timer? _timer;

  void startPolling() {
    _read();
    _timer = Timer.periodic(morphRelayPoll, (_) => _read());
  }

  void _read() {
    MorphRelayReport? next;
    try {
      next = source.relayReport();
    } catch (_) {
      next = null;
    }
    // Only a change redraws, so an idle page settles.
    if (mounted && _signature(next) != _signature(report)) {
      setState(() => report = next);
    }
  }

  void refresh() => _read();

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }
}

String _signature(MorphRelayReport? r) => r == null
    ? ''
    : [
        r.nodes,
        r.checking,
        r.summary,
        for (final x in r.relays) '${x.peer} ${x.title} ${x.state} ${x.rttMs}',
      ].join('|');

Color _stateColor(MorphRelayState state, ColorScheme colors) {
  final dark = colors.brightness == Brightness.dark;
  return switch (state) {
    MorphRelayState.reserved => dark ? MorphColors.cyan : MorphColors.cyanText,
    MorphRelayState.connected =>
      dark ? MorphColors.warningDark : MorphColors.warningLight,
    _ => colors.onSurface.withValues(alpha: 0.7),
  };
}

/// The relay's state, worst first, across [report]'s relays.
MorphRelayState _overall(MorphRelayReport report) {
  if (report.relays.isEmpty) {
    return MorphRelayState.noNode;
  }
  return report.relays
      .map((r) => r.state)
      .reduce((a, b) => a.index < b.index ? a : b);
}

/// One line on the device-link screens: which relay this device is using
/// and whether it is connected and holds a slot.
class MorphRelayStatusLine extends StatefulWidget {
  const MorphRelayStatusLine({super.key, required this.source});

  final MorphRelaySource source;

  @override
  State<MorphRelayStatusLine> createState() => _MorphRelayStatusLineState();
}

class _MorphRelayStatusLineState extends State<MorphRelayStatusLine>
    with _RelayPolling {
  @override
  MorphRelaySource get source => widget.source;

  @override
  void initState() {
    super.initState();
    startPolling();
  }

  @override
  Widget build(BuildContext context) {
    final r = report;
    if (r == null) {
      return const SizedBox.shrink();
    }
    final colors = Theme.of(context).colorScheme;
    final color = _stateColor(_overall(r), colors);
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 8),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Icon(Icons.hub_outlined, size: 16, color: color),
          const SizedBox(width: 6),
          Flexible(
            child: Text(
              r.summary,
              key: const Key('link-relay-status'),
              textAlign: TextAlign.center,
              style: TextStyle(fontSize: 12, color: color),
            ),
          ),
        ],
      ),
    );
  }
}

/// Settings → Relays: every configured relay, its state, and a check that
/// reaches them for a moment when no link runs.
class MorphRelaySection extends StatefulWidget {
  const MorphRelaySection({super.key, required this.source});

  final MorphRelaySource source;

  @override
  State<MorphRelaySection> createState() => _MorphRelaySectionState();
}

class _MorphRelaySectionState extends State<MorphRelaySection>
    with _RelayPolling {
  bool _starting = false;
  String? _error;

  @override
  MorphRelaySource get source => widget.source;

  @override
  void initState() {
    super.initState();
    startPolling();
  }

  Future<void> _check() async {
    setState(() {
      _starting = true;
      _error = null;
    });
    try {
      await widget.source.relayCheckStart();
    } catch (e) {
      if (mounted) {
        setState(() => _error = 'Could not check the relays: $e');
      }
    } finally {
      if (mounted) {
        setState(() => _starting = false);
        refresh();
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final r = report;
    final colors = Theme.of(context).colorScheme;
    final muted = TextStyle(
      fontSize: 12,
      color: colors.onSurface.withValues(alpha: 0.7),
    );
    final mono = MorphTheme.monoStyle(
      color: colors.onSurface.withValues(alpha: 0.6),
      size: 11,
    );
    if (r == null) {
      return const ListTile(
        key: Key('settings-relays'),
        title: Text('Relay state is not available'),
      );
    }
    final checking = r.checking || _starting;
    return Column(
      key: const Key('settings-relays'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        ListTile(
          leading: const Icon(Icons.hub_outlined),
          title: const Text('Relays'),
          subtitle: Text(r.summary, key: const Key('settings-relays-summary')),
          trailing: r.relays.isEmpty
              ? null
              : FilledButton.tonal(
                  key: const Key('settings-relays-check'),
                  onPressed: checking ? null : () => unawaited(_check()),
                  child: Text(checking ? 'Checking…' : 'Check now'),
                ),
        ),
        for (final relay in r.relays)
          ListTile(
            key: Key('settings-relay-${relay.peer}'),
            dense: true,
            leading: Icon(
              relay.state == MorphRelayState.reserved
                  ? Icons.link
                  : Icons.link_off,
              color: _stateColor(relay.state, colors),
            ),
            title: Text(relay.title),
            subtitle: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  relay.rttMs == null
                      ? relay.label
                      : '${relay.label} · ${relay.rttMs} ms',
                  key: Key('settings-relay-state-${relay.peer}'),
                  style: TextStyle(
                    fontSize: 12,
                    color: _stateColor(relay.state, colors),
                  ),
                ),
                SelectableText(relay.peer, style: mono),
                for (final a in relay.addrs) SelectableText(a, style: mono),
              ],
            ),
          ),
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
          child: Text(
            'Morph reaches a relay only while linking a device or during a '
            'check. A reserved slot lets a device on another network reach '
            'this one.',
            style: muted,
          ),
        ),
        if (_error != null)
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 16),
            child: Text(_error!, style: TextStyle(color: colors.error)),
          ),
      ],
    );
  }
}
