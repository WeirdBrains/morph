import 'package:flutter/material.dart';

import '../api.dart';
import '../identity_files.dart';
import '../theme/morph_theme.dart';
import 'password_field.dart';
import 'recovery_phrase_view.dart';

/// Shortest password Morph accepts for sealing a profile's user key. Mirrors
/// `morph_identity::profile_identity::MIN_PASSWORD_LEN`.
const morphMinPasswordLength = 8;

enum _Step { welcome, create, import, backup, words, link }

/// First-use onboarding for one profile's Weirdnet identity: create a new one
/// (and write down its 24 recovery words), restore one from its words or
/// from a backup file, or link one from another device.
///
/// The words and passwords only live in this widget's controllers and are
/// never logged or stored by it. The words leave it only when the user picks
/// Copy (cleared from the clipboard after a minute), Share or Download.
class MorphIdentityOnboarding extends StatefulWidget {
  const MorphIdentityOnboarding({
    super.key,
    required this.profileName,
    required this.onCreate,
    required this.onImport,
    required this.onDone,
    required this.onSkip,
    this.linkFromDevice,
    this.backupKind,
    this.onRestoreBackup,
  });

  final String profileName;
  final Future<MorphNewIdentity> Function(String password) onCreate;
  final Future<MorphIdentity> Function(String words, String password) onImport;
  final ValueChanged<MorphIdentity> onDone;
  final VoidCallback onSkip;

  /// Builds the "Link from another device" step (QR login); null hides it.
  /// The built widget calls [onDone] itself when the identity is stored.
  final WidgetBuilder? linkFromDevice;

  /// What a picked file holds. With [onRestoreBackup], shows "Restore from a
  /// backup file"; null hides it.
  final Future<MorphBackupKind> Function(List<int> contents)? backupKind;

  /// Restores from a backup (with its [backupPassword]) or phrase file.
  final Future<MorphIdentity> Function(
    List<int> contents,
    String? backupPassword,
    String password,
  )?
  onRestoreBackup;

  @override
  State<MorphIdentityOnboarding> createState() =>
      _MorphIdentityOnboardingState();
}

class _MorphIdentityOnboardingState extends State<MorphIdentityOnboarding> {
  _Step _step = _Step.welcome;
  final _password = TextEditingController();
  final _confirm = TextEditingController();
  final _words = TextEditingController();
  final _backupPassword = TextEditingController();
  MorphNewIdentity? _created;
  MorphPickedFile? _backupFile;
  MorphBackupKind? _backupFileKind;
  bool _saved = false;
  bool _busy = false;
  String? _error;

  @override
  void dispose() {
    _password.dispose();
    _confirm.dispose();
    _words.dispose();
    _backupPassword.dispose();
    _created = null;
    _backupFile = null;
    super.dispose();
  }

  String? _passwordProblem() {
    if (_password.text.length < morphMinPasswordLength) {
      return 'Use at least $morphMinPasswordLength characters.';
    }
    if (_password.text != _confirm.text) {
      return 'The passwords do not match.';
    }
    return null;
  }

  void _go(_Step step) {
    setState(() {
      _step = step;
      _error = null;
      if (step != _Step.backup) {
        _backupFile = null;
        _backupFileKind = null;
        _backupPassword.clear();
      }
    });
  }

  Future<void> _chooseBackup() async {
    setState(() => _error = null);
    MorphPickedFile? file;
    try {
      file = await MorphIdentityFiles.pick();
    } on FormatException {
      setState(() => _error = 'That file is too large to be a Morph backup.');
      return;
    } catch (_) {
      setState(() => _error = 'Could not open the file.');
      return;
    }
    if (file == null || !mounted) {
      return;
    }
    final kind = await widget.backupKind!(file.bytes);
    if (!mounted) {
      return;
    }
    setState(() {
      _backupFile = kind == MorphBackupKind.unknown ? null : file;
      _backupFileKind = kind;
      _backupPassword.clear();
      if (kind == MorphBackupKind.unknown) {
        _error = '${file!.name} is not a Morph backup or recovery phrase.';
      }
    });
  }

  Future<void> _submit() async {
    final problem = _passwordProblem();
    if (_step == _Step.backup && _backupFile == null) {
      setState(() => _error = 'Choose a backup or recovery phrase file.');
      return;
    }
    if (_step == _Step.backup &&
        _backupFileKind == MorphBackupKind.sealed &&
        _backupPassword.text.isEmpty) {
      setState(() => _error = 'Enter the backup password.');
      return;
    }
    if (_step == _Step.import &&
        _words.text.trim().split(RegExp(r'\s+')).length != 24) {
      setState(() => _error = 'Enter all 24 recovery words.');
      return;
    }
    if (problem != null) {
      setState(() => _error = problem);
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      if (_step == _Step.create) {
        final created = await widget.onCreate(_password.text);
        if (!mounted) {
          return;
        }
        setState(() {
          _created = created;
          _step = _Step.words;
        });
      } else if (_step == _Step.backup) {
        final identity = await widget.onRestoreBackup!(
          _backupFile!.bytes,
          _backupFileKind == MorphBackupKind.sealed
              ? _backupPassword.text
              : null,
          _password.text,
        );
        _backupPassword.clear();
        _backupFile = null;
        widget.onDone(identity);
      } else {
        final identity = await widget.onImport(
          _words.text.trim(),
          _password.text,
        );
        _words.clear();
        widget.onDone(identity);
      }
      _password.clear();
      _confirm.clear();
    } catch (e) {
      if (mounted) {
        setState(() => _error = _friendly(e));
      }
    } finally {
      if (mounted) {
        setState(() => _busy = false);
      }
    }
  }

  static String _friendly(Object e) {
    final text = '$e';
    if (text.contains('backup password')) {
      return 'Wrong backup password, or the backup is damaged.';
    }
    if (text.contains('word')) {
      return 'Those recovery words are not valid. Check each word and try again.';
    }
    return text.replaceFirst(RegExp(r'^(Bad state|Exception): '), '');
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      key: const Key('identity-onboarding'),
      appBar: AppBar(
        title: Text('Identity · ${widget.profileName}'),
        automaticallyImplyLeading: false,
        leading:
            _step == _Step.create ||
                _step == _Step.import ||
                _step == _Step.backup ||
                _step == _Step.link
            ? IconButton(
                tooltip: 'Back',
                icon: const Icon(Icons.arrow_back),
                onPressed: _busy ? null : () => _go(_Step.welcome),
              )
            : null,
      ),
      body: SafeArea(
        child: SingleChildScrollView(
          padding: const EdgeInsets.fromLTRB(20, 8, 20, 20),
          child: switch (_step) {
            _Step.welcome => _welcome(context),
            _Step.create || _Step.import || _Step.backup => _form(context),
            _Step.words => _wordsStep(context),
            _Step.link => widget.linkFromDevice!(context),
          },
        ),
      ),
    );
  }

  Widget _welcome(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const SizedBox(height: 12),
        const Icon(Icons.fingerprint, size: 56, color: MorphColors.cyan),
        const SizedBox(height: 16),
        Text(
          'Give ${widget.profileName} its own identity',
          textAlign: TextAlign.center,
          style: Theme.of(context).textTheme.titleLarge,
        ),
        const SizedBox(height: 8),
        Text(
          'Each Morph profile has its own Weirdnet identity and keys. '
          'Nothing is shared with your other profiles.',
          textAlign: TextAlign.center,
          style: TextStyle(color: colors.onSurface.withValues(alpha: 0.75)),
        ),
        const SizedBox(height: 28),
        FilledButton.icon(
          key: const Key('onboarding-create'),
          onPressed: () => _go(_Step.create),
          icon: const Icon(Icons.add),
          label: const Text('Create a new identity'),
        ),
        const SizedBox(height: 12),
        OutlinedButton.icon(
          key: const Key('onboarding-import'),
          onPressed: () => _go(_Step.import),
          icon: const Icon(Icons.restore),
          label: const Text('Restore from recovery words'),
        ),
        if (widget.backupKind != null && widget.onRestoreBackup != null) ...[
          const SizedBox(height: 12),
          OutlinedButton.icon(
            key: const Key('onboarding-backup'),
            onPressed: () => _go(_Step.backup),
            icon: const Icon(Icons.file_open_outlined),
            label: const Text('Restore from a backup file'),
          ),
        ],
        if (widget.linkFromDevice != null) ...[
          const SizedBox(height: 12),
          OutlinedButton.icon(
            key: const Key('onboarding-link'),
            onPressed: () => _go(_Step.link),
            icon: const Icon(Icons.qr_code_2),
            label: const Text('Link from another device'),
          ),
        ],
        const SizedBox(height: 12),
        TextButton(
          key: const Key('onboarding-skip'),
          onPressed: widget.onSkip,
          child: const Text('Not now'),
        ),
      ],
    );
  }

  Widget _form(BuildContext context) {
    final importing = _step == _Step.import;
    final fromBackup = _step == _Step.backup;
    final sealed = _backupFileKind == MorphBackupKind.sealed;
    return AutofillGroup(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(
            fromBackup
                ? 'Choose a Morph identity backup (.age) or a saved recovery '
                      'phrase (.txt), then a password for it in '
                      '${widget.profileName}.'
                : importing
                ? 'Enter the 24 recovery words of the identity to restore, '
                      'then choose a password for it in ${widget.profileName}.'
                : 'Choose a password. It seals ${widget.profileName}\'s keys '
                      'on this device.',
          ),
          const SizedBox(height: 16),
          if (importing) ...[
            TextField(
              key: const Key('onboarding-words'),
              controller: _words,
              minLines: 3,
              maxLines: 6,
              autocorrect: false,
              enableSuggestions: false,
              enableIMEPersonalizedLearning: false,
              keyboardType: TextInputType.visiblePassword,
              style: MorphTheme.monoStyle(
                color: Theme.of(context).colorScheme.onSurface,
                size: 14,
              ),
              decoration: const InputDecoration(
                labelText: 'Recovery words',
                alignLabelWithHint: true,
              ),
            ),
            const SizedBox(height: 12),
          ],
          if (fromBackup) ...[
            OutlinedButton.icon(
              key: const Key('onboarding-backup-choose'),
              onPressed: _busy ? null : _chooseBackup,
              icon: const Icon(Icons.folder_open),
              label: Text(_backupFile?.name ?? 'Choose file'),
            ),
            if (_backupFile != null) ...[
              const SizedBox(height: 6),
              Text(
                sealed
                    ? 'A password-sealed backup. Enter the password it was '
                          'made with.'
                    : 'A recovery phrase file.',
                key: const Key('onboarding-backup-kind'),
              ),
            ],
            if (sealed) ...[
              const SizedBox(height: 12),
              MorphPasswordField(
                key: const Key('onboarding-backup-password'),
                controller: _backupPassword,
                label: 'Backup password',
                textInputAction: TextInputAction.next,
              ),
            ],
            const SizedBox(height: 12),
          ],
          MorphPasswordField(
            key: const Key('onboarding-password'),
            controller: _password,
            label: 'Password',
            autofillHints: const [AutofillHints.newPassword],
            textInputAction: TextInputAction.next,
          ),
          const SizedBox(height: 12),
          MorphPasswordField(
            key: const Key('onboarding-confirm'),
            controller: _confirm,
            label: 'Confirm password',
            onSubmitted: (_) => _busy ? null : _submit(),
          ),
          if (_error != null) ...[
            const SizedBox(height: 12),
            Text(
              _error!,
              key: const Key('onboarding-error'),
              style: const TextStyle(color: MorphColors.magenta),
            ),
          ],
          const SizedBox(height: 20),
          FilledButton(
            key: const Key('onboarding-submit'),
            onPressed: _busy ? null : _submit,
            child: _busy
                ? const SizedBox(
                    width: 18,
                    height: 18,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : Text(
                    importing || fromBackup
                        ? 'Restore identity'
                        : 'Create identity',
                  ),
          ),
        ],
      ),
    );
  }

  Widget _wordsStep(BuildContext context) {
    final created = _created!;
    final colors = Theme.of(context).colorScheme;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const Text(
          'Write these 24 words down, in order, and keep them offline. '
          'They are the only way to restore this identity. Morph will not '
          'show them again without the password.',
        ),
        const SizedBox(height: 16),
        MorphRecoveryPhraseView(
          profileName: widget.profileName,
          userId: created.identity.userId,
          words: created.words,
        ),
        const SizedBox(height: 12),
        Text(
          'User ${created.identity.userId}',
          key: const Key('onboarding-user-id'),
          style: MorphTheme.monoStyle(
            color: colors.onSurface.withValues(alpha: 0.7),
            size: 12,
          ),
        ),
        CheckboxListTile(
          key: const Key('onboarding-saved'),
          contentPadding: EdgeInsets.zero,
          value: _saved,
          activeColor: MorphColors.cyan,
          title: const Text('I have written down my recovery words'),
          onChanged: (value) => setState(() => _saved = value ?? false),
        ),
        const SizedBox(height: 8),
        FilledButton(
          key: const Key('onboarding-done'),
          onPressed: _saved ? () => widget.onDone(created.identity) : null,
          child: const Text('Done'),
        ),
      ],
    );
  }
}
