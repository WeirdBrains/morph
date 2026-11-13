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
  }) async {
    listBookmarksCalls++;
    return List.of(bookmarksByProfile[profileId] ?? const []);
  }

  int listBookmarksCalls = 0;

  final foldersByProfile = <String, List<MorphBookmarkFolder>>{};
  var _nextBookmarkId = 1;

  /// Rewrites the order keys of everything in [parent] to follow [ids].
  void _reorder(String profileId, String? parent, List<String> ids) {
    final bookmarks = bookmarksByProfile.putIfAbsent(profileId, () => []);
    final folders = foldersByProfile.putIfAbsent(profileId, () => []);
    for (var i = 0; i < ids.length; i++) {
      final key = (i + 1).toString().padLeft(6, '0');
      final b = bookmarks.indexWhere((b) => b.id == ids[i]);
      if (b >= 0) {
        bookmarks[b] = bookmarks[b].copyWith(
          order: key,
          folderId: () => parent,
        );
      }
      final f = folders.indexWhere((f) => f.id == ids[i]);
      if (f >= 0) {
        folders[f] = folders[f].copyWith(order: key, parentId: () => parent);
      }
    }
  }

  MorphBookmarkTree _tree(String profileId) => MorphBookmarkTree(
    folders: List.of(foldersByProfile[profileId] ?? const []),
    bookmarks: List.of(bookmarksByProfile[profileId] ?? const []),
  );

  @override
  Future<List<MorphBookmark>> addBookmark(
    String profileId,
    String url,
    String title, {
    String? home,
  }) async {
    final list = bookmarksByProfile.putIfAbsent(profileId, () => []);
    final existing = list.indexWhere((b) => b.url == url);
    if (existing >= 0) {
      list[existing] = list[existing].copyWith(title: title);
      return List.of(list);
    }
    final id = 'b${_nextBookmarkId++}';
    final root = _tree(profileId).children(null).map((n) => n.id).toList();
    list.insert(0, MorphBookmark(id: id, url: url, title: title));
    _reorder(profileId, null, [id, ...root]);
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
  Future<MorphBookmarkTree> bookmarkTree(
    String profileId, {
    String? home,
  }) async => _tree(profileId);

  @override
  Future<MorphBookmarkTree> createBookmarkFolder(
    String profileId,
    String name, {
    String? parentId,
    String? home,
  }) async {
    final id = 'f${_nextBookmarkId++}';
    final siblings = _tree(profileId).children(parentId).map((n) => n.id);
    foldersByProfile
        .putIfAbsent(profileId, () => [])
        .add(MorphBookmarkFolder(id: id, name: name, parentId: parentId));
    _reorder(profileId, parentId, [...siblings, id]);
    return _tree(profileId);
  }

  @override
  Future<MorphBookmarkTree> renameBookmarkFolder(
    String profileId,
    String folderId,
    String name, {
    String? home,
  }) async {
    final folders = foldersByProfile[profileId] ?? [];
    final i = folders.indexWhere((f) => f.id == folderId);
    folders[i] = folders[i].copyWith(name: name);
    return _tree(profileId);
  }

  @override
  Future<MorphBookmarkTree> deleteBookmarkFolder(
    String profileId,
    String folderId, {
    String? home,
  }) async {
    final ids = _tree(profileId).subtree(folderId);
    foldersByProfile[profileId]?.removeWhere((f) => ids.contains(f.id));
    bookmarksByProfile[profileId]?.removeWhere((b) => ids.contains(b.folderId));
    return _tree(profileId);
  }

  @override
  Future<MorphBookmarkTree> moveBookmarkItem(
    String profileId,
    String itemId, {
    String? parentId,
    required int index,
    String? home,
  }) async {
    final tree = _tree(profileId);
    if (tree.folder(itemId) != null &&
        parentId != null &&
        tree.subtree(itemId).contains(parentId)) {
      throw StateError('a folder cannot move into itself');
    }
    final siblings = tree
        .children(parentId)
        .map((n) => n.id)
        .where((id) => id != itemId)
        .toList();
    siblings.insert(index.clamp(0, siblings.length), itemId);
    _reorder(profileId, parentId, siblings);
    return _tree(profileId);
  }

  @override
  Future<MorphBookmarkTree> updateBookmark(
    String profileId,
    String bookmarkId, {
    String? title,
    String? url,
    List<String>? tags,
    String? home,
  }) async {
    final list = bookmarksByProfile[profileId] ?? [];
    final i = list.indexWhere((b) => b.id == bookmarkId);
    final clean = tags == null
        ? null
        : <String>{
            for (final t in tags)
              if (t.trim().isNotEmpty) t.trim().toLowerCase(),
          }.toList();
    list[i] = list[i].copyWith(title: title, url: url, tags: clean);
    return _tree(profileId);
  }

  @override
  Future<MorphBookmarkTree> deleteBookmark(
    String profileId,
    String bookmarkId, {
    String? home,
  }) async {
    bookmarksByProfile[profileId]?.removeWhere((b) => b.id == bookmarkId);
    return _tree(profileId);
  }

  @override
  Future<List<MorphBookmark>> searchBookmarks(
    String profileId, {
    String query = '',
    String? tag,
    String? home,
  }) async {
    final words = query.toLowerCase().split(RegExp(r'\s+'))
      ..removeWhere((w) => w.isEmpty);
    return [
      for (final b in bookmarksByProfile[profileId] ?? const <MorphBookmark>[])
        if ((tag == null || b.tags.contains(tag)) &&
            words.every(
              (w) =>
                  b.title.toLowerCase().contains(w) ||
                  b.url.toLowerCase().contains(w) ||
                  b.tags.any((t) => t.contains(w)),
            ))
          b,
    ];
  }

  @override
  Future<List<MorphBookmarkTag>> bookmarkTags(
    String profileId, {
    String? home,
  }) async {
    final counts = <String, int>{};
    for (final b in bookmarksByProfile[profileId] ?? const <MorphBookmark>[]) {
      for (final t in b.tags) {
        counts[t] = (counts[t] ?? 0) + 1;
      }
    }
    final names = counts.keys.toList()..sort();
    return [
      for (final n in names) MorphBookmarkTag(name: n, count: counts[n]!),
    ];
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

  /// Profiles whose library the keystore cannot open (they need the password),
  /// with the reason to report.
  final libraryNeedsPassword = <String, String?>{};

  /// The password that unlocks each locked library.
  final libraryPasswords = <String, String>{};

  /// Whether an unlock keeps the key in the (fake) keystore.
  bool keystoreRemembers = true;
  final openLibraryCalls = <String>[];
  final unlockLibraryCalls = <String>[];

  @override
  Future<MorphLibraryOpen> openLibrary(String profileId, {String? home}) async {
    openLibraryCalls.add(profileId);
    if (!libraryNeedsPassword.containsKey(profileId)) {
      return const MorphLibraryOpen();
    }
    return MorphLibraryOpen(
      ready: false,
      needsPassword: true,
      reason: libraryNeedsPassword[profileId],
    );
  }

  @override
  Future<MorphLibraryUnlock> unlockLibrary(
    String profileId,
    String password, {
    String? home,
  }) async {
    unlockLibraryCalls.add(profileId);
    if (libraryPasswords[profileId] != password) {
      throw StateError('wrong password');
    }
    if (keystoreRemembers) {
      libraryNeedsPassword.remove(profileId);
    }
    return MorphLibraryUnlock(
      remembered: keystoreRemembers,
      reason: keystoreRemembers ? null : 'no keyring',
    );
  }

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

  // --- QR device linking ---------------------------------------------
  //
  // Tests drive the new device's side through [linkStatus] and see what the
  // UI did in [linkCalls].

  final linkCalls = <String>[];
  MorphLinkStatus linkStatus = const MorphLinkStatus(MorphLinkState.waiting);
  String linkSas = '042917';

  /// The identity a finished link stores (the other device's).
  MorphIdentity linkedIdentity = const MorphIdentity(
    userId: 'linkeduser0000000000000000000000',
    fingerprint: 'fp-linked',
    linked: true,
  );

  @override
  Future<MorphLinkOffer> linkNewDeviceStart(String deviceName) async {
    linkCalls.add('start $deviceName');
    return MorphLinkOffer(
      session: 7,
      link: 'weirdnet-morph://link/fake-offer',
      expires: DateTime.now().add(const Duration(minutes: 2)),
    );
  }

  @override
  Future<MorphLinkStatus> linkNewDeviceStatus(int session) async => linkStatus;

  @override
  Future<void> linkNewDeviceConfirm(int session) async =>
      linkCalls.add('confirm $session');

  @override
  Future<MorphIdentity> linkNewDeviceFinish(
    String profileId,
    int session,
    String password, {
    String? home,
  }) async {
    if (password.length < 8) {
      throw StateError('password must be at least 8 characters');
    }
    linkCalls.add('finish $profileId $session');
    identitiesByProfile[profileId] = linkedIdentity;
    return linkedIdentity;
  }

  @override
  Future<void> linkNewDeviceCancel(int session) async =>
      linkCalls.add('cancel $session');

  @override
  Future<MorphLinkPairing> linkAddDeviceStart(
    String profileId,
    String profileName,
    String password,
    String link, {
    String? home,
  }) async {
    final sealed = _sealed[profileId];
    if (sealed != null && sealed.password != password) {
      throw StateError('wrong password');
    }
    if (!link.startsWith('weirdnet-morph://link/')) {
      throw StateError('not a Morph link');
    }
    linkCalls.add('pair $profileId $link');
    return MorphLinkPairing(
      session: 9,
      sas: linkSas,
      device: 'Morph on laptop',
    );
  }

  @override
  Future<void> linkAddDeviceSend(int session) async =>
      linkCalls.add('send $session');

  @override
  Future<void> linkAddDeviceCancel(int session) async =>
      linkCalls.add('pair-cancel $session');
}
