import 'package:flutter/material.dart';

import '../recovery_phrase.dart';
import '../theme/morph_theme.dart';

/// A profile's 24 recovery words in a numbered grid, with Copy (the
/// clipboard clears after a minute), Share (the Android share sheet, else
/// copy) and Download (a save dialog or the Android document picker).
///
/// The words stay in this widget; they are never logged.
class MorphRecoveryPhraseView extends StatefulWidget {
  const MorphRecoveryPhraseView({
    super.key,
    required this.profileName,
    required this.userId,
    required this.words,
  });

  final String profileName;
  final String userId;
  final String words;

  @override
  State<MorphRecoveryPhraseView> createState() =>
      _MorphRecoveryPhraseViewState();
}

class _MorphRecoveryPhraseViewState extends State<MorphRecoveryPhraseView> {
  String? _status;

  String get _fileText => morphRecoveryFileText(
    profileName: widget.profileName,
    userId: widget.userId,
    words: widget.words,
  );

  Future<void> _run(Future<String> Function() action) async {
    final status = await action();
    if (mounted) {
      setState(() => _status = status);
    }
  }

  Future<String> _copy() async {
    await MorphRecoveryPhrase.copy(widget.words);
    final seconds = MorphRecoveryPhrase.clipboardLifetime.inSeconds;
    return 'Copied. The clipboard clears in $seconds seconds.';
  }

  Future<String> _share() async {
    final opened = await MorphRecoveryPhrase.share(
      _fileText,
      subject: 'Morph recovery phrase (${widget.profileName})',
    );
    if (opened) {
      return 'Only share it with somewhere you trust.';
    }
    final seconds = MorphRecoveryPhrase.clipboardLifetime.inSeconds;
    return 'No share sheet here, so it was copied. '
        'The clipboard clears in $seconds seconds.';
  }

  Future<String> _save() async {
    final name = morphRecoveryFileName(widget.profileName);
    return switch (await MorphRecoveryPhrase.save(name, _fileText)) {
      MorphPhraseSaveResult.saved => 'Saved $name. Keep it offline.',
      MorphPhraseSaveResult.cancelled => 'Not saved.',
      MorphPhraseSaveResult.failed => 'Could not save the file.',
    };
  }

  @override
  Widget build(BuildContext context) {
    final words = widget.words.trim().split(RegExp(r'\s+'));
    final colors = Theme.of(context).colorScheme;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      mainAxisSize: MainAxisSize.min,
      children: [
        Container(
          key: const Key('recovery-word-grid'),
          padding: const EdgeInsets.all(12),
          decoration: BoxDecoration(
            border: Border.all(color: MorphColors.cyan.withValues(alpha: 0.6)),
            borderRadius: BorderRadius.circular(12),
          ),
          child: Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              for (var i = 0; i < words.length; i++)
                SizedBox(
                  width: 120,
                  child: Text(
                    '${(i + 1).toString().padLeft(2)}. ${words[i]}',
                    style: MorphTheme.monoStyle(
                      color: colors.onSurface,
                      size: 13,
                    ),
                  ),
                ),
            ],
          ),
        ),
        const SizedBox(height: 8),
        Wrap(
          spacing: 8,
          runSpacing: 4,
          alignment: WrapAlignment.center,
          children: [
            TextButton.icon(
              key: const Key('recovery-copy'),
              onPressed: () => _run(_copy),
              icon: const Icon(Icons.copy),
              label: const Text('Copy'),
            ),
            TextButton.icon(
              key: const Key('recovery-share'),
              onPressed: () => _run(_share),
              icon: const Icon(Icons.share),
              label: const Text('Share'),
            ),
            TextButton.icon(
              key: const Key('recovery-save'),
              onPressed: () => _run(_save),
              icon: const Icon(Icons.download),
              label: const Text('Download'),
            ),
          ],
        ),
        if (_status != null)
          Text(
            _status!,
            key: const Key('recovery-status'),
            textAlign: TextAlign.center,
            style: MorphTheme.monoStyle(
              color: colors.onSurface.withValues(alpha: 0.8),
              size: 12,
            ),
          ),
      ],
    );
  }
}
