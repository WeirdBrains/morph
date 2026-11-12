import 'package:morph/src/api.dart';

/// In-memory [MorphApi] for widget tests.
class FakeMorphApi implements MorphApi {
  @override
  String coreVersion() => '0.1.0';

  @override
  Future<IdentityStatusView> identityDirInfo({String? home}) async {
    return IdentityStatusView(
      home: home ?? '/tmp/morph-fake',
      homeExists: false,
      profile: 'weirdnet/keys/v1',
    );
  }

  @override
  Stream<int> eventTicks() => const Stream<int>.empty();

  @override
  Future<EphemeralUser> newEphemeralUser() async {
    return const EphemeralUser(
      userId: '0123456789abcdef0123456789abcdef',
      fingerprint: 'abcd 1234',
    );
  }

  @override
  Future<String> handlePageMessage({required String json}) async {
    return '{"id":"1","type":"pong"}';
  }

  String tabPosition = 'top';
  bool sidebarCollapsed = false;
  int sidebarWidth = 220;

  @override
  Future<String> loadTabPosition({String? home}) async => tabPosition;

  @override
  Future<void> saveTabPosition(String position, {String? home}) async {
    tabPosition = position;
  }

  @override
  Future<bool> loadSidebarCollapsed({String? home}) async => sidebarCollapsed;

  @override
  Future<void> saveSidebarCollapsed(bool collapsed, {String? home}) async {
    sidebarCollapsed = collapsed;
  }

  @override
  Future<int> loadSidebarWidth({String? home}) async => sidebarWidth;

  @override
  Future<void> saveSidebarWidth(int width, {String? home}) async {
    sidebarWidth = width;
  }

  String currentProfileId = 'default';
  final List<ProfileInfo> profiles = [
    const ProfileInfo(id: 'default', name: 'Default'),
  ];
  final Map<String, ProfileTabSnapshot> tabsByProfile = {};
  int _nextProfile = 1;

  ProfileCatalog _catalog() =>
      ProfileCatalog(current: currentProfileId, profiles: List.of(profiles));

  ProfileInfo _require(String id) {
    return profiles.firstWhere(
      (profile) => profile.id == id,
      orElse: () => throw StateError('unknown profile $id'),
    );
  }

  String _home(String? home) => home ?? '/tmp/morph-fake';

  @override
  Future<ProfileCatalog> listProfiles({String? home}) async => _catalog();

  @override
  @override
  Future<ProfileCatalog> createProfile(
    String name, {
    String? home,
    String? color,
  }) async {
    final profile = ProfileInfo(
      id: 'p${_nextProfile++}',
      name: name,
      color: color ?? '',
    );
    profiles.add(profile);
    currentProfileId = profile.id;
    return _catalog();
  }

  @override
  Future<ProfileCatalog> renameProfile(
    String id,
    String name, {
    String? home,
  }) async {
    final index = profiles.indexWhere((profile) => profile.id == id);
    if (index < 0) {
      throw StateError('unknown profile $id');
    }
    final existing = profiles[index];
    profiles[index] = ProfileInfo(id: id, name: name, color: existing.color);
    return _catalog();
  }

  @override
  Future<ProfileCatalog> deleteProfile(String id, {String? home}) async {
    if (profiles.length == 1) {
      throw StateError('cannot delete the last profile');
    }
    _require(id);
    profiles.removeWhere((profile) => profile.id == id);
    tabsByProfile.remove(id);
    if (currentProfileId == id) {
      currentProfileId = profiles.any((profile) => profile.id == 'default')
          ? 'default'
          : profiles.first.id;
    }
    return _catalog();
  }

  @override
  Future<ProfileCatalog> setCurrentProfile(String id, {String? home}) async {
    _require(id);
    currentProfileId = id;
    return _catalog();
  }

  @override
  Future<ProfilePaths> profilePaths(String id, {String? home}) async {
    _require(id);
    final root = '${_home(home)}/profiles/$id';
    return ProfilePaths(
      id: id,
      profileDir: root,
      settingsPath: '$root/settings.json',
      tabsPath: '$root/tabs.json',
      webkitDir: '$root/webkit',
      webkitDataDir: '$root/webkit/data',
      webkitCacheDir: '$root/webkit/cache',
      cookiesPath: '$root/webkit/cookies.sqlite',
    );
  }

  String _tabKey(String? profileId) => profileId ?? currentProfileId;

  @override
  Future<ProfileTabSnapshot> loadProfileTabs({
    String? home,
    String? profileId,
  }) async {
    return tabsByProfile[_tabKey(profileId)] ?? ProfileTabSnapshot.empty;
  }

  @override
  Future<void> saveProfileTabs(
    ProfileTabSnapshot snapshot, {
    String? home,
    String? profileId,
  }) async {
    tabsByProfile[_tabKey(profileId)] = snapshot;
  }

  final bookmarksByProfile = <String, List<MorphBookmark>>{};
  final historyByProfile = <String, List<MorphHistoryEntry>>{};

  @override
  Future<List<MorphBookmark>> listBookmarks(
    String profileId, {
    String? home,
  }) async => List.of(bookmarksByProfile[profileId] ?? const []);

  @override
  Future<List<MorphBookmark>> addBookmark(
    String profileId,
    String url,
    String title, {
    String? home,
  }) async {
    final list = bookmarksByProfile.putIfAbsent(profileId, () => []);
    list.removeWhere((b) => b.url == url);
    list.insert(0, MorphBookmark(url: url, title: title));
    return List.of(list);
  }

  @override
  Future<List<MorphBookmark>> removeBookmark(
    String profileId,
    String url, {
    String? home,
  }) async {
    final list = bookmarksByProfile.putIfAbsent(profileId, () => []);
    list.removeWhere((b) => b.url == url);
    return List.of(list);
  }

  @override
  Future<void> recordVisit(
    String profileId,
    String url,
    String title, {
    String? home,
  }) async {
    final list = historyByProfile.putIfAbsent(profileId, () => []);
    if (list.isNotEmpty && list.first.url == url) {
      list.removeAt(0);
    }
    list.insert(
      0,
      MorphHistoryEntry(url: url, title: title, visited: DateTime.now()),
    );
  }

  @override
  Future<List<MorphHistoryEntry>> listHistory(
    String profileId, {
    String query = '',
    int limit = 0,
    String? home,
  }) async {
    final q = query.toLowerCase();
    final list = [
      for (final e
          in historyByProfile[profileId] ?? const <MorphHistoryEntry>[])
        if (q.isEmpty ||
            e.url.toLowerCase().contains(q) ||
            e.title.toLowerCase().contains(q))
          e,
    ];
    return limit > 0 && list.length > limit ? list.sublist(0, limit) : list;
  }

  @override
  Future<void> deleteHistoryUrl(
    String profileId,
    String url, {
    String? home,
  }) async {
    historyByProfile[profileId]?.removeWhere((e) => e.url == url);
  }

  @override
  Future<void> clearHistory(String profileId, {String? home}) async {
    historyByProfile.remove(profileId);
  }

  final identitiesByProfile = <String, MorphIdentity>{};
  final _sealed = <String, ({String words, String password})>{};
  var _nextUser = 1;

  /// Fake words: the user id they restore is encoded in the first word.
  static String fakeWords(String userId) =>
      [userId, for (var i = 1; i < 24; i++) 'word$i'].join(' ');

  /// When true, a profile without a stored identity reports a stock one, so
  /// shell tests that are not about onboarding never see it.
  bool onboarded = true;

  @override
  Future<MorphIdentity?> profileIdentity(
    String profileId, {
    String? home,
  }) async =>
      identitiesByProfile[profileId] ??
      (onboarded
          ? MorphIdentity(
              userId: 'stock-$profileId'.padRight(32, '0'),
              fingerprint: 'fp-stock-$profileId',
            )
          : null);

  void _checkNew(String profileId, String password) {
    if (identitiesByProfile.containsKey(profileId)) {
      throw StateError('this profile already has an identity');
    }
    if (password.length < 8) {
      throw StateError('password must be at least 8 characters');
    }
  }

  @override
  Future<MorphNewIdentity> createProfileIdentity(
    String profileId,
    String password, {
    String? home,
  }) async {
    _checkNew(profileId, password);
    final userId = 'user${_nextUser++}'.padRight(32, '0');
    final identity = MorphIdentity(
      userId: userId,
      fingerprint: 'fp-$userId',
      created: DateTime.now(),
    );
    identitiesByProfile[profileId] = identity;
    final words = fakeWords(userId);
    _sealed[profileId] = (words: words, password: password);
    return MorphNewIdentity(identity: identity, words: words);
  }

  @override
  Future<MorphIdentity> importProfileIdentity(
    String profileId,
    String words,
    String password, {
    String? home,
  }) async {
    _checkNew(profileId, password);
    final parts = words.trim().split(RegExp(r'\s+'));
    if (parts.length != 24) {
      throw StateError('expected 24 recovery words');
    }
    final identity = MorphIdentity(
      userId: parts.first,
      fingerprint: 'fp-${parts.first}',
      created: DateTime.now(),
      imported: true,
    );
    identitiesByProfile[profileId] = identity;
    _sealed[profileId] = (words: words.trim(), password: password);
    return identity;
  }

  @override
  Future<String> profileRecoveryWords(
    String profileId,
    String password, {
    String? home,
  }) async {
    final sealed = _sealed[profileId];
    if (sealed == null || sealed.password != password) {
      throw StateError('wrong password');
    }
    return sealed.words;
  }
}
