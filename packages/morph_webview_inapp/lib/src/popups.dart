import 'package:flutter/material.dart';

/// A window a page opened with `window.open` or a `target=_blank` link.
///
/// It is a real second web view handed to the engine for that window, so
/// the page keeps `window.opener`, can `postMessage` back, and can close
/// itself. Sign-in flows (Google, Apple, Microsoft) depend on all three:
/// loading the popup's address into the tab instead strands the sign-in page
/// without its opener, which shows as a blank white page.
class MorphPopup {
  MorphPopup({required this.windowId, required String url, required this.view})
    : address = ValueNotifier(url),
      title = ValueNotifier('');

  final int windowId;
  final Widget view;
  final ValueNotifier<String> address;
  final ValueNotifier<String> title;

  void dispose() {
    address.dispose();
    title.dispose();
  }
}

/// The popups open over one tab, oldest first.
class MorphPopupStack extends ChangeNotifier {
  /// More than this and the page is refused further windows (popup spam).
  static const maxOpen = 4;

  final _open = <MorphPopup>[];

  List<MorphPopup> get open => List.unmodifiable(_open);
  bool get isFull => _open.length >= maxOpen;

  /// Adds [popup] unless [isFull]; returns whether it was added.
  bool push(MorphPopup popup) {
    if (isFull) {
      return false;
    }
    _open.add(popup);
    notifyListeners();
    return true;
  }

  /// Removes the popup for [windowId], if open, and returns it.
  MorphPopup? remove(int windowId) {
    final i = _open.indexWhere((p) => p.windowId == windowId);
    if (i < 0) {
      return null;
    }
    final popup = _open.removeAt(i);
    notifyListeners();
    return popup;
  }

  /// Removes every popup, newest first, and returns them.
  List<MorphPopup> clear() {
    final all = _open.reversed.toList();
    _open.clear();
    if (all.isNotEmpty) {
      notifyListeners();
    }
    return all;
  }
}

/// A tab's page with its open popups stacked over it, each with a bar that
/// names the site and a close button.
class MorphPopupOverlay extends StatelessWidget {
  const MorphPopupOverlay({
    super.key,
    required this.stack,
    required this.onClose,
    required this.child,
  });

  final MorphPopupStack stack;
  final ValueChanged<int> onClose;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: stack,
      builder: (context, _) {
        final open = stack.open;
        return Stack(
          fit: StackFit.expand,
          children: [
            // The opener stays mounted underneath so its page keeps running
            // and can receive the popup's messages.
            child,
            for (final popup in open)
              Positioned.fill(
                key: ValueKey('popup-${popup.windowId}'),
                child: _PopupFrame(
                  popup: popup,
                  onClose: () => onClose(popup.windowId),
                ),
              ),
          ],
        );
      },
    );
  }
}

class _PopupFrame extends StatelessWidget {
  const _PopupFrame({required this.popup, required this.onClose});

  final MorphPopup popup;
  final VoidCallback onClose;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Material(
      color: theme.colorScheme.surface,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          SizedBox(
            height: 44,
            child: Row(
              children: [
                const SizedBox(width: 12),
                const Icon(Icons.open_in_new, size: 18),
                const SizedBox(width: 8),
                Expanded(
                  child: ListenableBuilder(
                    listenable: Listenable.merge([popup.title, popup.address]),
                    builder: (context, _) => Text(
                      morphPopupLabel(popup.title.value, popup.address.value),
                      key: ValueKey('popup-label-${popup.windowId}'),
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                ),
                IconButton(
                  key: ValueKey('popup-close-${popup.windowId}'),
                  tooltip: 'Close window',
                  icon: const Icon(Icons.close),
                  onPressed: onClose,
                ),
              ],
            ),
          ),
          const Divider(height: 1),
          Expanded(child: popup.view),
        ],
      ),
    );
  }
}

/// "Title · host", or whichever of the two is known.
String morphPopupLabel(String title, String address) {
  final host = Uri.tryParse(address)?.host ?? '';
  final t = title.trim();
  if (t.isEmpty) {
    return host.isEmpty ? 'New window' : host;
  }
  return host.isEmpty || t == host ? t : '$t · $host';
}
