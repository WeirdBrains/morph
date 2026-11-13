import 'dart:async';
import 'dart:io';

import 'package:file_selector/file_selector.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

import 'share.dart';

String _profileSlug(String profileName) {
  final slug = profileName
      .toLowerCase()
      .replaceAll(RegExp(r'[^a-z0-9]+'), '-')
      .replaceAll(RegExp(r'^-+|-+$'), '');
  return slug.isEmpty ? 'profile' : slug;
}

/// `morph-<profile>-recovery-phrase.txt`, with the profile name reduced to
/// lowercase letters, digits and dashes.
String morphRecoveryFileName(String profileName) =>
    'morph-${_profileSlug(profileName)}-recovery-phrase.txt';

/// `morph-<profile>-identity-backup.age`: a password-sealed identity backup.
String morphBackupFileName(String profileName) =>
    'morph-${_profileSlug(profileName)}-identity-backup.age';

/// Plain-text body of a saved or shared recovery phrase.
String morphRecoveryFileText({
  required String profileName,
  required String userId,
  required String words,
}) {
  final list = words.trim().split(RegExp(r'\s+'));
  final b = StringBuffer()
    ..writeln('Morph recovery phrase')
    ..writeln('Profile: $profileName')
    ..writeln('User: $userId')
    ..writeln()
    ..writeln('Anyone with these 24 words can restore this identity.')
    ..writeln('Keep this file offline and private.')
    ..writeln();
  for (var i = 0; i < list.length; i++) {
    b.writeln('${(i + 1).toString().padLeft(2)}. ${list[i]}');
  }
  b
    ..writeln()
    ..writeln(list.join(' '));
  return b.toString();
}

enum MorphPhraseSaveResult { saved, cancelled, failed }

/// Copy, share and save for a recovery phrase. The text is never logged.
abstract final class MorphRecoveryPhrase {
  static const saveChannel = MethodChannel('org.weirdbrains.morph/save');

  /// How long a copied phrase stays on the clipboard.
  static const clipboardLifetime = Duration(seconds: 60);

  static Timer? _clearTimer;

  /// Picks where a desktop save goes; null means cancelled. Tests replace it.
  @visibleForTesting
  static Future<String?> Function(String suggestedName) pickSavePath =
      (name) async {
        final extension = name.contains('.') ? name.split('.').last : 'txt';
        return (await getSaveLocation(
          suggestedName: name,
          acceptedTypeGroups: [
            XTypeGroup(
              label: extension == 'age' ? 'Morph backup' : 'Text',
              extensions: [extension],
            ),
          ],
        ))?.path;
      };

  /// Copies [text] and clears the clipboard after [clipboardLifetime] if it
  /// still holds it.
  static Future<void> copy(String text) async {
    await Clipboard.setData(ClipboardData(text: text));
    _clearTimer?.cancel();
    _clearTimer = Timer(clipboardLifetime, () async {
      _clearTimer = null;
      final current = await Clipboard.getData(Clipboard.kTextPlain);
      if (current?.text == text) {
        await Clipboard.setData(const ClipboardData(text: ''));
      }
    });
  }

  /// Android share sheet; elsewhere copies instead. True when a sheet opened.
  static Future<bool> share(String text, {String subject = ''}) async {
    final opened = await MorphShare.shareText(text, subject: subject);
    if (!opened) {
      await copy(text);
    }
    return opened;
  }

  /// Saves [text] as [fileName]: the Android document picker, or a desktop
  /// save dialog. [mimeType] tells Android what it is (a backup is not
  /// `text/plain`, or the picker would add `.txt` to its name).
  static Future<MorphPhraseSaveResult> save(
    String fileName,
    String text, {
    String mimeType = 'text/plain',
  }) async {
    if (!kIsWeb && defaultTargetPlatform == TargetPlatform.android) {
      try {
        final saved = await saveChannel.invokeMethod<bool>('saveText', {
          'fileName': fileName,
          'text': text,
          'mimeType': mimeType,
        });
        return saved == true
            ? MorphPhraseSaveResult.saved
            : MorphPhraseSaveResult.cancelled;
      } on PlatformException {
        return MorphPhraseSaveResult.failed;
      } on MissingPluginException {
        return MorphPhraseSaveResult.failed;
      }
    }
    final String? path;
    try {
      path = await pickSavePath(fileName);
    } catch (_) {
      return MorphPhraseSaveResult.failed;
    }
    if (path == null) {
      return MorphPhraseSaveResult.cancelled;
    }
    try {
      await File(path).writeAsString(text, flush: true);
      return MorphPhraseSaveResult.saved;
    } on FileSystemException {
      return MorphPhraseSaveResult.failed;
    }
  }
}
