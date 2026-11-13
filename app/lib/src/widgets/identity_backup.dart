import 'package:flutter/material.dart';

import '../recovery_phrase.dart';
import '../theme/morph_theme.dart';
import 'identity_onboarding.dart' show morphMinPasswordLength;
import 'password_field.dart';
import 'recovery_phrase_view.dart';

/// Shows a profile's recovery phrase after its password, with Copy, Share
/// and Download. The words live only in this dialog.
class MorphRecoveryPhraseDialog extends StatefulWidget {
  const MorphRecoveryPhraseDialog({
    super.key,
    required this.profileName,
    required this.userId,
    required this.reveal,
  });

  final String profileName;
  final String userId;

  /// The recovery words for the profile password, or throws.
  final Future<String> Function(String password) reveal;

  @override
  State<MorphRecoveryPhraseDialog> createState() =>
      _MorphRecoveryPhraseDialogState();
}

class _MorphRecoveryPhraseDialogState extends State<MorphRecoveryPhraseDialog> {
  final _password = TextEditingController();
  String? _words;
  String? _error;
  bool _busy = false;

  @override
  void dispose() {
    _password.dispose();
    _words = null;
    super.dispose();
  }

  Future<void> _reveal() async {
    if (_password.text.isEmpty) {
      setState(() => _error = 'Enter the profile password.');
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final words = await widget.reveal(_password.text);
      _password.clear();
      if (mounted) {
        setState(() => _words = words);
      }
    } catch (_) {
      if (mounted) {
        setState(() => _error = 'That password does not open this identity.');
      }
    } finally {
      if (mounted) {
        setState(() => _busy = false);
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final words = _words;
    return AlertDialog(
      key: const Key('recovery-phrase-dialog'),
      title: Text('${widget.profileName} recovery phrase'),
      content: SizedBox(
        width: 420,
        child: SingleChildScrollView(
          child: words == null
              ? Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    const Text(
                      'Anyone with these 24 words can restore this identity. '
                      'Enter the profile password to see them.',
                    ),
                    const SizedBox(height: 12),
                    MorphPasswordField(
                      key: const Key('recovery-phrase-password'),
                      controller: _password,
                      label: 'Profile password',
                      onSubmitted: (_) => _busy ? null : _reveal(),
                    ),
                    if (_error != null) ...[
                      const SizedBox(height: 8),
                      Text(
                        _error!,
                        key: const Key('recovery-phrase-error'),
                        style: const TextStyle(color: MorphColors.magenta),
                      ),
                    ],
                  ],
                )
              : Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    const Text(
                      'Write these words down, in order, and keep them '
                      'offline.',
                    ),
                    const SizedBox(height: 12),
                    MorphRecoveryPhraseView(
                      profileName: widget.profileName,
                      userId: widget.userId,
                      words: words,
                    ),
                  ],
                ),
        ),
      ),
      actions: [
        if (words == null)
          FilledButton(
            key: const Key('recovery-phrase-show'),
            onPressed: _busy ? null : _reveal,
            child: const Text('Show phrase'),
          ),
        TextButton(
          key: const Key('recovery-phrase-close'),
          onPressed: () => Navigator.of(context).pop(),
          child: const Text('Close'),
        ),
      ],
    );
  }
}

/// Makes a password-sealed backup of a profile's identity and saves or
/// shares it. The backup password may differ from the profile password; it
/// is the only way to open the backup.
class MorphIdentityBackupDialog extends StatefulWidget {
  const MorphIdentityBackupDialog({
    super.key,
    required this.profileName,
    required this.export,
  });

  final String profileName;

  /// The sealed backup for (profile password, backup password), or throws.
  final Future<String> Function(String password, String backupPassword) export;

  @override
  State<MorphIdentityBackupDialog> createState() =>
      _MorphIdentityBackupDialogState();
}

class _MorphIdentityBackupDialogState extends State<MorphIdentityBackupDialog> {
  final _password = TextEditingController();
  final _backupPassword = TextEditingController();
  final _confirm = TextEditingController();
  String? _backup;
  String? _error;
  String? _status;
  bool _busy = false;

  String get _fileName => morphBackupFileName(widget.profileName);

  @override
  void dispose() {
    _password.dispose();
    _backupPassword.dispose();
    _confirm.dispose();
    _backup = null;
    super.dispose();
  }

  String? _problem() {
    if (_password.text.isEmpty) {
      return 'Enter the profile password.';
    }
    if (_backupPassword.text.length < morphMinPasswordLength) {
      return 'Use at least $morphMinPasswordLength characters for the backup '
          'password.';
    }
    if (_backupPassword.text != _confirm.text) {
      return 'The backup passwords do not match.';
    }
    return null;
  }

  Future<void> _create() async {
    final problem = _problem();
    if (problem != null) {
      setState(() => _error = problem);
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final backup = await widget.export(_password.text, _backupPassword.text);
      _password.clear();
      _backupPassword.clear();
      _confirm.clear();
      if (mounted) {
        setState(() => _backup = backup);
      }
    } catch (e) {
      if (mounted) {
        setState(
          () => _error = '$e'.contains('password must')
              ? 'Use at least $morphMinPasswordLength characters for the '
                    'backup password.'
              : 'That password does not open this identity.',
        );
      }
    } finally {
      if (mounted) {
        setState(() => _busy = false);
      }
    }
  }

  Future<void> _save() async {
    final result = await MorphRecoveryPhrase.save(
      _fileName,
      _backup!,
      mimeType: 'application/octet-stream',
    );
    if (mounted) {
      setState(
        () => _status = switch (result) {
          MorphPhraseSaveResult.saved => 'Saved $_fileName.',
          MorphPhraseSaveResult.cancelled => 'Not saved.',
          MorphPhraseSaveResult.failed => 'Could not save the file.',
        },
      );
    }
  }

  Future<void> _share() async {
    final opened = await MorphRecoveryPhrase.share(
      _backup!,
      subject: 'Morph identity backup (${widget.profileName})',
    );
    if (mounted) {
      final seconds = MorphRecoveryPhrase.clipboardLifetime.inSeconds;
      setState(
        () => _status = opened
            ? 'Shared. It opens only with the backup password.'
            : 'No share sheet here, so it was copied. The clipboard clears '
                  'in $seconds seconds.',
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final made = _backup != null;
    return AlertDialog(
      key: const Key('identity-backup-dialog'),
      title: Text('Back up ${widget.profileName}'),
      content: SizedBox(
        width: 420,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: made
                ? [
                    const Text(
                      'The backup is ready. It holds this identity sealed '
                      'under the backup password: keep both, apart. Restore '
                      'it from "Restore from a backup file" when setting up '
                      'a profile.',
                    ),
                    const SizedBox(height: 12),
                    Wrap(
                      spacing: 8,
                      runSpacing: 4,
                      alignment: WrapAlignment.center,
                      children: [
                        FilledButton.icon(
                          key: const Key('backup-save'),
                          onPressed: _save,
                          icon: const Icon(Icons.download),
                          label: const Text('Download'),
                        ),
                        OutlinedButton.icon(
                          key: const Key('backup-share'),
                          onPressed: _share,
                          icon: const Icon(Icons.share),
                          label: const Text('Share'),
                        ),
                      ],
                    ),
                    if (_status != null) ...[
                      const SizedBox(height: 8),
                      Text(
                        _status!,
                        key: const Key('backup-status'),
                        textAlign: TextAlign.center,
                        style: MorphTheme.monoStyle(
                          color: colors.onSurface.withValues(alpha: 0.8),
                          size: 12,
                        ),
                      ),
                    ],
                  ]
                : [
                    const Text(
                      'A backup file restores this identity on any device. '
                      'Choose a backup password: without it the file cannot '
                      'be opened, by anyone.',
                    ),
                    const SizedBox(height: 12),
                    MorphPasswordField(
                      key: const Key('backup-profile-password'),
                      controller: _password,
                      label: 'Profile password',
                      textInputAction: TextInputAction.next,
                    ),
                    const SizedBox(height: 12),
                    MorphPasswordField(
                      key: const Key('backup-password'),
                      controller: _backupPassword,
                      label: 'Backup password',
                      autofillHints: const [AutofillHints.newPassword],
                      textInputAction: TextInputAction.next,
                    ),
                    const SizedBox(height: 12),
                    MorphPasswordField(
                      key: const Key('backup-confirm'),
                      controller: _confirm,
                      label: 'Confirm backup password',
                      onSubmitted: (_) => _busy ? null : _create(),
                    ),
                    if (_error != null) ...[
                      const SizedBox(height: 8),
                      Text(
                        _error!,
                        key: const Key('backup-error'),
                        style: const TextStyle(color: MorphColors.magenta),
                      ),
                    ],
                  ],
          ),
        ),
      ),
      actions: [
        if (!made)
          FilledButton(
            key: const Key('backup-create'),
            onPressed: _busy ? null : _create,
            child: _busy
                ? const SizedBox(
                    width: 18,
                    height: 18,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : const Text('Create backup'),
          ),
        TextButton(
          key: const Key('backup-close'),
          onPressed: () => Navigator.of(context).pop(),
          child: Text(made ? 'Done' : 'Cancel'),
        ),
      ],
    );
  }
}
