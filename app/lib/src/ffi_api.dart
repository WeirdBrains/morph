import 'dart:convert';
import 'dart:io';

import 'api.dart';
import 'rust/api/events.dart' as rust_events;
import 'rust/api/identity.dart' as rust_identity;
import 'rust/api/info.dart' as rust_info;
import 'rust/api/library.dart' as rust_library;
import 'rust/api/page.dart' as rust_page;
import 'rust/api/profiles.dart' as rust_profiles;
import 'rust/api/settings.dart' as rust_settings;

/// Production `MorphApi` backed by flutter_rust_bridge.
class FfiMorphApi implements MorphApi {
  const FfiMorphApi();

  @override
  String coreVersion() => rust_info.coreVersion();

  @override
  Future<IdentityStatusView> identityDirInfo({String? home}) async {
    final info = await rust_identity.identityDirInfo(home: home);
    return IdentityStatusView(
      home: info.home,
      homeExists: info.homeExists,
      profile: info.profile,
    );
  }

  @override
  Stream<int> eventTicks() => rust_events.eventTicks();

  @override
  Future<EphemeralUser> newEphemeralUser() async {
    final user = await rust_identity.newEphemeralUser();
    return EphemeralUser(userId: user.userId, fingerprint: user.fingerprint);
  }

  @override
  Future<String> handlePageMessage({required String json}) =>
      rust_page.handlePageMessage(json: json);

  @override
  Future<String> loadTabPosition({String? home}) async {
    final settings = await rust_settings.loadSettings(home: home);
    return settings.tabPosition;
  }

  @override
  Future<void> saveTabPosition(String position, {String? home}) async {
    await rust_settings.setTabPosition(home: home, position: position);
  }

  @override
  Future<bool> loadSidebarCollapsed({String? home}) async {
    final settings = await rust_settings.loadSettings(home: home);
    return settings.sidebarCollapsed;
  }

  @override
  Future<void> saveSidebarCollapsed(bool collapsed, {String? home}) async {
    await rust_settings.setSidebarCollapsed(home: home, collapsed: collapsed);
  }

  @override
  Future<int> loadSidebarWidth({String? home}) async {
    final settings = await rust_settings.loadSettings(home: home);
    return settings.sidebarWidth;
  }

  @override
  Future<void> saveSidebarWidth(int width, {String? home}) async {
    await rust_settings.setSidebarWidth(home: home, width: width);
  }

  @override
  Future<ProfileCatalog> listProfiles({String? home}) async {
    return _mergeColors(
      home,
      _catalog(await rust_profiles.listProfiles(home: home)),
    );
  }

  @override
  Future<ProfileCatalog> createProfile(
    String name, {
    String? home,
    String? color,
  }) async {
    final catalog = _catalog(
      await rust_profiles.createProfile(home: home, name: name),
    );
    if (color != null && color.isNotEmpty) {
      final created = catalog.profiles.firstWhere(
        (profile) => profile.id == catalog.current,
      );
      await _writeColor(home, created.id, color);
      return _mergeColors(home, catalog);
    }
    return _mergeColors(home, catalog);
  }

  @override
  Future<ProfileCatalog> renameProfile(
    String id,
    String name, {
    String? home,
  }) async {
    return _mergeColors(
      home,
      _catalog(
        await rust_profiles.renameProfile(home: home, id: id, name: name),
      ),
    );
  }

  @override
  Future<ProfileCatalog> deleteProfile(String id, {String? home}) async {
    final catalog = _catalog(
      await rust_profiles.deleteProfile(home: home, id: id),
    );
    return _mergeColors(home, catalog);
  }

  @override
  Future<ProfileCatalog> setCurrentProfile(String id, {String? home}) async {
    return _mergeColors(
      home,
      _catalog(await rust_profiles.setCurrentProfile(home: home, id: id)),
    );
  }

  @override
  Future<ProfilePaths> profilePaths(String id, {String? home}) async {
    final paths = await rust_profiles.profilePaths(home: home, id: id);
    return ProfilePaths(
      id: paths.id,
      profileDir: paths.profileDir,
      settingsPath: paths.settingsPath,
      tabsPath: paths.tabsPath,
      webkitDir: paths.webkitDir,
      webkitDataDir: paths.webkitDataDir,
      webkitCacheDir: paths.webkitCacheDir,
      cookiesPath: paths.cookiesPath,
    );
  }

  ProfileCatalog _catalog(rust_profiles.CatalogView view) {
    return ProfileCatalog(
      current: view.current,
      profiles: [
        for (final profile in view.profiles)
          ProfileInfo(id: profile.id, name: profile.name),
      ],
    );
  }

  Future<ProfileCatalog> _mergeColors(
    String? home,
    ProfileCatalog catalog,
  ) async {
    final colored = <ProfileInfo>[];
    for (final profile in catalog.profiles) {
      final color = await _readColor(home, profile.id);
      colored.add(
        ProfileInfo(id: profile.id, name: profile.name, color: color),
      );
    }
    return ProfileCatalog(current: catalog.current, profiles: colored);
  }

  Future<String> _readColor(String? home, String id) async {
    try {
      final paths = await profilePaths(id, home: home);
      final file = File('${paths.profileDir}/color');
      if (await file.exists()) {
        return (await file.readAsString()).trim();
      }
    } catch (_) {}
    return '';
  }

  Future<void> _writeColor(String? home, String id, String color) async {
    final paths = await profilePaths(id, home: home);
    final dir = Directory(paths.profileDir);
    if (!await dir.exists()) {
      await dir.create(recursive: true);
    }
    await File('${paths.profileDir}/color').writeAsString('$color\n');
  }

  @override
  Future<ProfileTabSnapshot> loadProfileTabs({
    String? home,
    String? profileId,
  }) async {
    final id =
        profileId ?? (await rust_profiles.listProfiles(home: home)).current;
    try {
      final paths = await profilePaths(id, home: home);
      final file = File(paths.tabsPath);
      if (!await file.exists()) {
        return ProfileTabSnapshot.empty;
      }
      return _decodeTabs(await file.readAsString());
    } catch (_) {
      return ProfileTabSnapshot.empty;
    }
  }

  @override
  Future<void> saveProfileTabs(
    ProfileTabSnapshot snapshot, {
    String? home,
    String? profileId,
  }) async {
    final id =
        profileId ?? (await rust_profiles.listProfiles(home: home)).current;
    final paths = await profilePaths(id, home: home);
    final file = File(paths.tabsPath);
    await file.parent.create(recursive: true);
    await file.writeAsString('${jsonEncode(_encodeTabs(snapshot))}\n');
  }

  Map<String, Object?> _encodeTabs(ProfileTabSnapshot snapshot) {
    return {
      'active_id': snapshot.activeId,
      'next_id': snapshot.nextId,
      'tabs': [
        for (final tab in snapshot.tabs)
          {'id': tab.id, 'url': tab.url, 'title': tab.title},
      ],
    };
  }

  ProfileTabSnapshot _decodeTabs(String raw) {
    final json = jsonDecode(raw);
    if (json is! Map) {
      return ProfileTabSnapshot.empty;
    }
    final tabs = <ProfileTab>[];
    final rawTabs = json['tabs'];
    if (rawTabs is List) {
      for (final item in rawTabs) {
        if (item is Map) {
          tabs.add(
            ProfileTab(
              id: (item['id'] as num?)?.toInt() ?? 0,
              url: item['url'] as String? ?? '',
              title: item['title'] as String? ?? '',
            ),
          );
        }
      }
    }
    return ProfileTabSnapshot(
      activeId: (json['active_id'] as num?)?.toInt() ?? 1,
      nextId: (json['next_id'] as num?)?.toInt() ?? 2,
      tabs: tabs,
    );
  }

  static List<MorphBookmark> _bookmarks(List<rust_library.BookmarkView> list) =>
      [
        for (final b in list)
          MorphBookmark(
            url: b.url,
            title: b.title,
            added: DateTime.fromMillisecondsSinceEpoch(b.addedMs.toInt()),
          ),
      ];

  @override
  Future<List<MorphBookmark>> listBookmarks(
    String profileId, {
    String? home,
  }) async => _bookmarks(
    await rust_library.listBookmarks(home: home, profileId: profileId),
  );

  @override
  Future<List<MorphBookmark>> addBookmark(
    String profileId,
    String url,
    String title, {
    String? home,
  }) async => _bookmarks(
    await rust_library.addBookmark(
      home: home,
      profileId: profileId,
      url: url,
      title: title,
    ),
  );

  @override
  Future<List<MorphBookmark>> removeBookmark(
    String profileId,
    String url, {
    String? home,
  }) async => _bookmarks(
    await rust_library.removeBookmark(
      home: home,
      profileId: profileId,
      url: url,
    ),
  );

  @override
  Future<void> recordVisit(
    String profileId,
    String url,
    String title, {
    String? home,
  }) => rust_library.recordVisit(
    home: home,
    profileId: profileId,
    url: url,
    title: title,
  );

  @override
  Future<List<MorphHistoryEntry>> listHistory(
    String profileId, {
    String query = '',
    int limit = 0,
    String? home,
  }) async => [
    for (final e in await rust_library.listHistory(
      home: home,
      profileId: profileId,
      query: query,
      limit: limit,
    ))
      MorphHistoryEntry(
        url: e.url,
        title: e.title,
        visited: DateTime.fromMillisecondsSinceEpoch(e.visitedMs.toInt()),
      ),
  ];

  @override
  Future<void> deleteHistoryUrl(String profileId, String url, {String? home}) =>
      rust_library.deleteHistoryUrl(home: home, profileId: profileId, url: url);

  @override
  Future<void> clearHistory(String profileId, {String? home}) =>
      rust_library.clearHistory(home: home, profileId: profileId);
}
