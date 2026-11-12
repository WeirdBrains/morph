import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// Whether Morph is the system's default web browser.
enum MorphDefaultBrowserState { isDefault, notDefault, unknown }

/// What asking to become the default did.
enum MorphDefaultBrowserRequest {
  /// Morph is the default now.
  madeDefault,

  /// The system's own default-apps screen opened; the user picks there.
  openedSettings,

  /// The user said no in the system prompt.
  declined,

  /// Nothing could be changed on this system.
  failed,
}

typedef MorphProcessRunner = Future<ProcessResult> Function(
  String executable,
  List<String> arguments,
);

Future<ProcessResult> _run(String executable, List<String> arguments) =>
    Process.run(executable, arguments);

/// Reads and sets the default browser. Android and macOS ask the platform
/// (RoleManager, Launch Services) over [channel]; Linux uses `xdg-settings`
/// and Windows registers per-user (HKCU) capabilities, so neither needs admin
/// rights.
abstract class MorphDefaultBrowser {
  const MorphDefaultBrowser();

  static const channel = MethodChannel('org.weirdbrains.morph/default_browser');

  /// The desktop entry id and Windows names Morph registers under.
  static const desktopId = 'org.weirdbrains.morph.desktop';
  static const windowsApp = 'Morph';
  static const windowsProgId = 'MorphURL';

  factory MorphDefaultBrowser.forPlatform() {
    if (kIsWeb) {
      return const _Unsupported();
    }
    switch (defaultTargetPlatform) {
      case TargetPlatform.android:
      case TargetPlatform.macOS:
        return const MorphChannelDefaultBrowser();
      case TargetPlatform.linux:
        return MorphLinuxDefaultBrowser();
      case TargetPlatform.windows:
        return MorphWindowsDefaultBrowser();
      default:
        return const _Unsupported();
    }
  }

  bool get supported => true;

  Future<MorphDefaultBrowserState> status();

  Future<MorphDefaultBrowserRequest> makeDefault();
}

class _Unsupported extends MorphDefaultBrowser {
  const _Unsupported();

  @override
  bool get supported => false;

  @override
  Future<MorphDefaultBrowserState> status() async =>
      MorphDefaultBrowserState.unknown;

  @override
  Future<MorphDefaultBrowserRequest> makeDefault() async =>
      MorphDefaultBrowserRequest.failed;
}

/// Android (RoleManager.ROLE_BROWSER, or the default-apps screen) and macOS
/// (NSWorkspace / Launch Services), implemented natively.
class MorphChannelDefaultBrowser extends MorphDefaultBrowser {
  const MorphChannelDefaultBrowser();

  @override
  Future<MorphDefaultBrowserState> status() async {
    try {
      final value = await MorphDefaultBrowser.channel.invokeMethod<bool>(
        'status',
      );
      if (value == null) {
        return MorphDefaultBrowserState.unknown;
      }
      return value
          ? MorphDefaultBrowserState.isDefault
          : MorphDefaultBrowserState.notDefault;
    } on PlatformException {
      return MorphDefaultBrowserState.unknown;
    } on MissingPluginException {
      return MorphDefaultBrowserState.unknown;
    }
  }

  @override
  Future<MorphDefaultBrowserRequest> makeDefault() async {
    try {
      final value = await MorphDefaultBrowser.channel.invokeMethod<String>(
        'request',
      );
      return MorphDefaultBrowserRequest.values.firstWhere(
        (r) => r.name == value,
        orElse: () => MorphDefaultBrowserRequest.failed,
      );
    } on PlatformException {
      return MorphDefaultBrowserRequest.failed;
    } on MissingPluginException {
      return MorphDefaultBrowserRequest.failed;
    }
  }
}

/// Linux: `xdg-settings`, which every desktop (and Omarchy's Hyprland, via
/// the generic `xdg-mime` path) honours, at user level only.
class MorphLinuxDefaultBrowser extends MorphDefaultBrowser {
  MorphLinuxDefaultBrowser({
    MorphProcessRunner? run,
    String? executable,
    Map<String, String>? environment,
  }) : _runner = run ?? _run,
       _executable = executable ?? Platform.resolvedExecutable,
       _env = environment ?? Platform.environment;

  final MorphProcessRunner _runner;
  final String _executable;
  final Map<String, String> _env;

  /// Marks a desktop entry Morph wrote itself (and may rewrite).
  static const generatedKey = 'X-Morph-Generated';

  String get _dataHome {
    final xdg = _env['XDG_DATA_HOME'];
    if (xdg != null && xdg.isNotEmpty) {
      return xdg;
    }
    return '${_env['HOME'] ?? ''}/.local/share';
  }

  List<String> get _dataDirs {
    final dirs = _env['XDG_DATA_DIRS'];
    return (dirs == null || dirs.isEmpty)
        ? const ['/usr/local/share', '/usr/share']
        : dirs.split(':').where((d) => d.isNotEmpty).toList();
  }

  /// The user-level desktop entry Morph writes when no installed one exists.
  String get userEntryPath =>
      '$_dataHome/applications/${MorphDefaultBrowser.desktopId}';

  static String desktopEntry(String executable) {
    final exec = executable.contains(' ') ? '"$executable"' : executable;
    return '''[Desktop Entry]
Name=Morph
Comment=A distributed web browser with local AI
Exec=$exec %u
Icon=org.weirdbrains.morph
Terminal=false
Type=Application
Categories=Network;WebBrowser;
MimeType=x-scheme-handler/http;x-scheme-handler/https;text/html;
StartupWMClass=org.weirdbrains.morph
$generatedKey=true
''';
  }

  @override
  Future<MorphDefaultBrowserState> status() async {
    try {
      final result = await _runner('xdg-settings', [
        'get',
        'default-web-browser',
      ]);
      if (result.exitCode != 0) {
        return MorphDefaultBrowserState.unknown;
      }
      return '${result.stdout}'.trim() == MorphDefaultBrowser.desktopId
          ? MorphDefaultBrowserState.isDefault
          : MorphDefaultBrowserState.notDefault;
    } on ProcessException {
      return MorphDefaultBrowserState.unknown;
    }
  }

  /// Writes (or refreshes) Morph's own user-level desktop entry, unless a
  /// packaged one is installed. Returns the path it wrote, or null.
  Future<String?> ensureDesktopEntry() async {
    final user = File(userEntryPath);
    if (await user.exists()) {
      final text = await user.readAsString();
      if (!text.contains('$generatedKey=true')) {
        return null; // The user's own entry: leave it alone.
      }
    } else {
      for (final dir in _dataDirs) {
        if (await File('$dir/applications/${MorphDefaultBrowser.desktopId}')
            .exists()) {
          return null;
        }
      }
    }
    await user.parent.create(recursive: true);
    await user.writeAsString(desktopEntry(_executable));
    try {
      await _runner('update-desktop-database', [user.parent.path]);
    } on ProcessException {
      // Optional: xdg-mime reads the entry directly.
    }
    return user.path;
  }

  @override
  Future<MorphDefaultBrowserRequest> makeDefault() async {
    try {
      await ensureDesktopEntry();
      final set = await _runner('xdg-settings', [
        'set',
        'default-web-browser',
        MorphDefaultBrowser.desktopId,
      ]);
      if (set.exitCode != 0) {
        // Some minimal sessions lack a working xdg-settings backend; the
        // scheme handlers are what browsers-opening-links actually use.
        final mime = await _runner('xdg-mime', [
          'default',
          MorphDefaultBrowser.desktopId,
          'x-scheme-handler/http',
          'x-scheme-handler/https',
          'text/html',
        ]);
        if (mime.exitCode != 0) {
          return MorphDefaultBrowserRequest.failed;
        }
      }
    } on ProcessException {
      return MorphDefaultBrowserRequest.failed;
    } on FileSystemException {
      return MorphDefaultBrowserRequest.failed;
    }
    return await status() == MorphDefaultBrowserState.isDefault
        ? MorphDefaultBrowserRequest.madeDefault
        : MorphDefaultBrowserRequest.failed;
  }
}

/// Windows: registers Morph's capabilities under HKCU (no admin rights) and
/// opens Settings > Default apps on Morph's page, where Windows makes the
/// user confirm. Windows does not let apps set the default themselves.
class MorphWindowsDefaultBrowser extends MorphDefaultBrowser {
  MorphWindowsDefaultBrowser({MorphProcessRunner? run, String? executable})
    : _runner = run ?? _run,
      _executable = executable ?? Platform.resolvedExecutable;

  final MorphProcessRunner _runner;
  final String _executable;

  static const _clientKey =
      r'HKCU\Software\Clients\StartMenuInternet\'
      '${MorphDefaultBrowser.windowsApp}';
  static const _classKey =
      r'HKCU\Software\Classes\'
      '${MorphDefaultBrowser.windowsProgId}';

  /// The `reg add` argument lists that register Morph for the current user.
  List<List<String>> registrationCommands() {
    final exe = '"$_executable"';
    const capabilities = '$_clientKey\\Capabilities';
    List<String> value(String key, String? name, String data) => [
      'add',
      key,
      if (name == null) '/ve' else ...['/v', name],
      '/t',
      'REG_SZ',
      '/d',
      data,
      '/f',
    ];
    return [
      value(_classKey, null, 'Morph URL'),
      value(_classKey, 'URL Protocol', ''),
      value('$_classKey\\DefaultIcon', null, '$exe,0'),
      value('$_classKey\\shell\\open\\command', null, '$exe "%1"'),
      value(_clientKey, null, MorphDefaultBrowser.windowsApp),
      value('$_clientKey\\DefaultIcon', null, '$exe,0'),
      value('$_clientKey\\shell\\open\\command', null, exe),
      value(capabilities, 'ApplicationName', MorphDefaultBrowser.windowsApp),
      value(
        capabilities,
        'ApplicationDescription',
        'A distributed web browser with local AI',
      ),
      value(
        '$capabilities\\URLAssociations',
        'http',
        MorphDefaultBrowser.windowsProgId,
      ),
      value(
        '$capabilities\\URLAssociations',
        'https',
        MorphDefaultBrowser.windowsProgId,
      ),
      value(
        r'HKCU\Software\RegisteredApplications',
        MorphDefaultBrowser.windowsApp,
        r'Software\Clients\StartMenuInternet\'
            '${MorphDefaultBrowser.windowsApp}\\Capabilities',
      ),
    ];
  }

  @override
  Future<MorphDefaultBrowserState> status() async {
    try {
      final result = await _runner('reg', [
        'query',
        r'HKCU\Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice',
        '/v',
        'ProgId',
      ]);
      if (result.exitCode != 0) {
        return MorphDefaultBrowserState.notDefault;
      }
      return '${result.stdout}'.contains(MorphDefaultBrowser.windowsProgId)
          ? MorphDefaultBrowserState.isDefault
          : MorphDefaultBrowserState.notDefault;
    } on ProcessException {
      return MorphDefaultBrowserState.unknown;
    }
  }

  @override
  Future<MorphDefaultBrowserRequest> makeDefault() async {
    try {
      for (final args in registrationCommands()) {
        final result = await _runner('reg', args);
        if (result.exitCode != 0) {
          return MorphDefaultBrowserRequest.failed;
        }
      }
      await _runner('explorer.exe', [
        'ms-settings:defaultapps?registeredAppUser='
            '${MorphDefaultBrowser.windowsApp}',
      ]);
    } on ProcessException {
      return MorphDefaultBrowserRequest.failed;
    }
    return MorphDefaultBrowserRequest.openedSettings;
  }
}
