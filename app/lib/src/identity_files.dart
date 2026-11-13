import 'package:file_selector/file_selector.dart';
import 'package:flutter/foundation.dart';

/// A file picked to restore an identity from.
typedef MorphPickedFile = ({String name, List<int> bytes});

/// Picking identity backup and recovery-phrase files.
abstract final class MorphIdentityFiles {
  /// Backups and phrase files are a few hundred bytes; anything over this is
  /// not one, and is not read.
  static const maxBytes = 64 * 1024;

  /// Opens a file picker; null when cancelled. Throws [FormatException] for a
  /// file too large to be a backup. Tests replace it.
  @visibleForTesting
  static Future<MorphPickedFile?> Function() pickBackup = _pick;

  /// Opens a file picker ([pickBackup]).
  static Future<MorphPickedFile?> pick() => pickBackup();

  /// The name to show for a picked file. Android's picker copies the file and
  /// replaces its extension with one guessed from the MIME type, so a backup
  /// saved as `application/octet-stream` comes back as `.bin`: show the name
  /// without it rather than a wrong extension.
  static String shownName(String name, {required bool android}) {
    if (android && name.toLowerCase().endsWith('.bin') && name.length > 4) {
      return name.substring(0, name.length - 4);
    }
    return name;
  }

  static Future<MorphPickedFile?> _pick() async {
    // Android's picker filters by MIME type, and backups have none it knows:
    // show every file there.
    final android = !kIsWeb && defaultTargetPlatform == TargetPlatform.android;
    final file = await openFile(
      acceptedTypeGroups: android
          ? const []
          : const [
              XTypeGroup(
                label: 'Morph backup or recovery phrase',
                extensions: ['age', 'txt'],
              ),
            ],
    );
    if (file == null) {
      return null;
    }
    if (await file.length() > maxBytes) {
      throw const FormatException('too large');
    }
    return (
      name: shownName(file.name, android: android),
      bytes: await file.readAsBytes(),
    );
  }
}
