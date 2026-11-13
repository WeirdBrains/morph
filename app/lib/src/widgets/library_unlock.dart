import 'package:flutter/material.dart';

import '../api.dart';
import 'password_field.dart';

/// Asks for the profile password when the system keystore could not open the
/// profile's encrypted bookmarks and history. Shown at launch or on a profile
/// switch; skipping leaves the library locked for this run.
class MorphLibraryUnlockDialog extends StatefulWidget {
  const MorphLibraryUnlockDialog({
    super.key,
    required this.profileName,
    required this.onUnlock,
    required this.onDone,
    required this.onSkip,
    this.reason,
  });

  final String profileName;

  /// Why the keystore could not help, if it said.
  final String? reason;
  final Future<MorphLibraryUnlock> Function(String password) onUnlock;
  final ValueChanged<MorphLibraryUnlock> onDone;
  final VoidCallback onSkip;

  @override
  State<MorphLibraryUnlockDialog> createState() =>
      _MorphLibraryUnlockDialogState();
}

class _MorphLibraryUnlockDialogState extends State<MorphLibraryUnlockDialog> {
  final _password = TextEditingController();
  bool _busy = false;
  String? _error;

  @override
  void dispose() {
    _password.dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    if (_busy || _password.text.isEmpty) {
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final result = await widget.onUnlock(_password.text);
      if (mounted) {
        widget.onDone(result);
      }
    } catch (_) {
      if (mounted) {
        setState(() {
          _busy = false;
          _error = 'That password did not unlock this profile.';
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final reason = widget.reason;
    return AlertDialog(
      key: const Key('library-unlock'),
      title: Text('Unlock ${widget.profileName}'),
      content: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 420),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const Text(
              "This profile's bookmarks and history are encrypted. Enter "
              'the profile password to open them.',
            ),
            if (reason != null) ...[
              const SizedBox(height: 8),
              Text(
                "Morph couldn't use the system keyring ($reason).",
                key: const Key('library-unlock-reason'),
                style: theme.textTheme.bodySmall,
              ),
            ],
            const SizedBox(height: 16),
            MorphPasswordField(
              key: const Key('library-unlock-password'),
              controller: _password,
              label: 'Profile password',
              autofillHints: const [AutofillHints.password],
              textInputAction: TextInputAction.done,
              onSubmitted: (_) => _submit(),
            ),
            if (_error != null) ...[
              const SizedBox(height: 8),
              Text(
                _error!,
                key: const Key('library-unlock-error'),
                style: TextStyle(color: theme.colorScheme.error),
              ),
            ],
          ],
        ),
      ),
      actions: [
        TextButton(
          key: const Key('library-unlock-skip'),
          onPressed: _busy ? null : widget.onSkip,
          child: const Text('Not now'),
        ),
        FilledButton(
          key: const Key('library-unlock-submit'),
          onPressed: _busy ? null : _submit,
          child: const Text('Unlock'),
        ),
      ],
    );
  }
}
