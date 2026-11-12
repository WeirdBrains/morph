/// The Rust surface the shell needs. Widget tests use a fake so they can run
/// on the Dart VM without loading generated FFI bindings or `libmorph_bridge.so`.
abstract class MorphApi {
  String coreVersion();
  Future<IdentityStatusView> identityDirInfo({String? home});
  Stream<int> eventTicks();
  Future<EphemeralUser> newEphemeralUser();
  Future<String> handlePageMessage({required String json});
  Future<String> loadTabPosition({String? home});
  Future<void> saveTabPosition(String position, {String? home});
  Future<bool> loadSidebarCollapsed({String? home});
  Future<void> saveSidebarCollapsed(bool collapsed, {String? home});
  Future<int> loadSidebarWidth({String? home});
  Future<void> saveSidebarWidth(int width, {String? home});
  Future<ProfileCatalog> listProfiles({String? home});
  Future<ProfileCatalog> createProfile(
    String name, {
    String? home,
    String? color,
  });
  Future<ProfileCatalog> renameProfile(String id, String name, {String? home});
  Future<ProfileCatalog> deleteProfile(String id, {String? home});
  Future<ProfileCatalog> setCurrentProfile(String id, {String? home});
  Future<ProfilePaths> profilePaths(String id, {String? home});
  Future<ProfileTabSnapshot> loadProfileTabs({String? home, String? profileId});
  Future<void> saveProfileTabs(
    ProfileTabSnapshot snapshot, {
    String? home,
    String? profileId,
  });

  /// Bookmarks for [profileId], newest first.
  Future<List<MorphBookmark>> listBookmarks(String profileId, {String? home});
  Future<List<MorphBookmark>> addBookmark(
    String profileId,
    String url,
    String title, {
    String? home,
  });
  Future<List<MorphBookmark>> removeBookmark(
    String profileId,
    String url, {
    String? home,
  });

  /// [profileId]'s folders and bookmarks.
  Future<MorphBookmarkTree> bookmarkTree(String profileId, {String? home});
  Future<MorphBookmarkTree> createBookmarkFolder(
    String profileId,
    String name, {
    String? parentId,
    String? home,
  });
  Future<MorphBookmarkTree> renameBookmarkFolder(
    String profileId,
    String folderId,
    String name, {
    String? home,
  });

  /// Deletes the folder with everything inside it.
  Future<MorphBookmarkTree> deleteBookmarkFolder(
    String profileId,
    String folderId, {
    String? home,
  });

  /// Moves a bookmark or folder into [parentId] (null: top level) at slot
  /// [index] among what is already there.
  Future<MorphBookmarkTree> moveBookmarkItem(
    String profileId,
    String itemId, {
    String? parentId,
    required int index,
    String? home,
  });

  /// Null fields stay as they are.
  Future<MorphBookmarkTree> updateBookmark(
    String profileId,
    String bookmarkId, {
    String? title,
    String? url,
    List<String>? tags,
    String? home,
  });
  Future<MorphBookmarkTree> deleteBookmark(
    String profileId,
    String bookmarkId, {
    String? home,
  });

  /// Bookmarks matching every word of [query] in title, URL or tags, and
  /// carrying [tag] when given.
  Future<List<MorphBookmark>> searchBookmarks(
    String profileId, {
    String query = '',
    String? tag,
    String? home,
  });
  Future<List<MorphBookmarkTag>> bookmarkTags(String profileId, {String? home});

  /// Browsing history for [profileId], newest first.
  Future<void> recordVisit(
    String profileId,
    String url,
    String title, {
    String? home,
  });
  Future<List<MorphHistoryEntry>> listHistory(
    String profileId, {
    String query = '',
    int limit = 0,
    String? home,
  });
  Future<void> deleteHistoryUrl(String profileId, String url, {String? home});
  Future<void> clearHistory(String profileId, {String? home});

  /// [profileId]'s own Weirdnet identity, or null before onboarding.
  Future<MorphIdentity?> profileIdentity(String profileId, {String? home});

  /// Creates [profileId]'s identity sealed under [password]. The result holds
  /// the recovery words, to show once.
  Future<MorphNewIdentity> createProfileIdentity(
    String profileId,
    String password, {
    String? home,
  });

  /// Restores an identity from its recovery [words] into [profileId].
  Future<MorphIdentity> importProfileIdentity(
    String profileId,
    String words,
    String password, {
    String? home,
  });

  /// [profileId]'s recovery words, after checking its password.
  Future<String> profileRecoveryWords(
    String profileId,
    String password, {
    String? home,
  });

  // --- Login over a QR code (weirdnet-device-link) ---------------------
  //
  // The new device listens and shows a QR; the existing device scans it,
  // both show the same six digits, and only after the user confirms them the
  // identity moves over, sealed. Seeds, keys and passwords stay in Rust.

  /// New device: listen and make a fresh offer (valid two minutes).
  /// [deviceName] is shown on the other device.
  Future<MorphLinkOffer> linkNewDeviceStart(String deviceName);
  Future<MorphLinkStatus> linkNewDeviceStatus(int session);

  /// New device: the user tapped "Codes match".
  Future<void> linkNewDeviceConfirm(int session);

  /// New device: store the identity that arrived in [profileId], sealed
  /// under [password].
  Future<MorphIdentity> linkNewDeviceFinish(
    String profileId,
    int session,
    String password, {
    String? home,
  });
  Future<void> linkNewDeviceCancel(int session);

  /// Existing device: check [profileId]'s [password], read [link] (scanned
  /// or pasted) and pair with the new device.
  Future<MorphLinkPairing> linkAddDeviceStart(
    String profileId,
    String profileName,
    String password,
    String link, {
    String? home,
  });

  /// Existing device: the codes match; send the identity.
  Future<void> linkAddDeviceSend(int session);
  Future<void> linkAddDeviceCancel(int session);
}

/// A new device's offer: [link] is what the QR code encodes.
class MorphLinkOffer {
  const MorphLinkOffer({
    required this.session,
    required this.link,
    required this.expires,
  });

  final int session;
  final String link;
  final DateTime expires;
}

enum MorphLinkState {
  waiting,
  paired,
  received,
  done,
  cancelled,
  expired,
  failed,
}

/// Where a new device's link stands. [sas] is set while paired or received.
class MorphLinkStatus {
  const MorphLinkStatus(this.state, {this.sas, this.reason});

  final MorphLinkState state;
  final String? sas;
  final String? reason;

  bool get ended => switch (state) {
    MorphLinkState.done ||
    MorphLinkState.cancelled ||
    MorphLinkState.expired ||
    MorphLinkState.failed => true,
    _ => false,
  };
}

/// An existing device paired with a scanned link.
class MorphLinkPairing {
  const MorphLinkPairing({
    required this.session,
    required this.sas,
    required this.device,
  });

  final int session;

  /// Six digits; the user compares them with the new device's screen.
  final String sas;

  /// What the new device calls itself.
  final String device;
}

/// Public facts about a profile's identity.
class MorphIdentity {
  const MorphIdentity({
    required this.userId,
    required this.fingerprint,
    this.created,
    this.imported = false,
    this.linked = false,
  });

  final String userId;
  final String fingerprint;
  final DateTime? created;
  final bool imported;

  /// Moved here from another device over a QR link.
  final bool linked;
}

/// A just-created identity and its recovery words.
class MorphNewIdentity {
  const MorphNewIdentity({required this.identity, required this.words});

  final MorphIdentity identity;

  /// 24 words. Show once; never log or store them.
  final String words;
}

class MorphBookmark {
  const MorphBookmark({
    required this.url,
    this.id = '',
    this.title = '',
    this.added,
    this.updated,
    this.folderId,
    this.order = '',
    this.tags = const [],
  });

  final String id;
  final String url;
  final String title;
  final DateTime? added;
  final DateTime? updated;

  /// Containing folder; null is the top level.
  final String? folderId;

  /// Sort key among the folder's items (folders and bookmarks together).
  final String order;
  final List<String> tags;

  MorphBookmark copyWith({
    String? url,
    String? title,
    String? Function()? folderId,
    String? order,
    List<String>? tags,
  }) => MorphBookmark(
    id: id,
    url: url ?? this.url,
    title: title ?? this.title,
    added: added,
    updated: updated,
    folderId: folderId == null ? this.folderId : folderId(),
    order: order ?? this.order,
    tags: tags ?? this.tags,
  );
}

class MorphBookmarkFolder {
  const MorphBookmarkFolder({
    required this.id,
    required this.name,
    this.parentId,
    this.order = '',
  });

  final String id;
  final String name;
  final String? parentId;
  final String order;

  MorphBookmarkFolder copyWith({
    String? name,
    String? Function()? parentId,
    String? order,
  }) => MorphBookmarkFolder(
    id: id,
    name: name ?? this.name,
    parentId: parentId == null ? this.parentId : parentId(),
    order: order ?? this.order,
  );
}

/// One entry inside a folder: a folder or a bookmark.
class MorphBookmarkNode {
  const MorphBookmarkNode.folder(MorphBookmarkFolder this.folder)
    : bookmark = null;
  const MorphBookmarkNode.bookmark(MorphBookmark this.bookmark) : folder = null;

  final MorphBookmarkFolder? folder;
  final MorphBookmark? bookmark;

  String get id => folder?.id ?? bookmark!.id;
  String get order => folder?.order ?? bookmark!.order;
  bool get isFolder => folder != null;
}

class MorphBookmarkTree {
  const MorphBookmarkTree({this.folders = const [], this.bookmarks = const []});

  static const empty = MorphBookmarkTree();

  final List<MorphBookmarkFolder> folders;
  final List<MorphBookmark> bookmarks;

  MorphBookmarkFolder? folder(String? id) {
    for (final f in folders) {
      if (f.id == id) {
        return f;
      }
    }
    return null;
  }

  /// What sits directly in [parentId], in order.
  List<MorphBookmarkNode> children(String? parentId) {
    final nodes = [
      for (final f in folders)
        if (f.parentId == parentId) MorphBookmarkNode.folder(f),
      for (final b in bookmarks)
        if (b.folderId == parentId) MorphBookmarkNode.bookmark(b),
    ];
    nodes.sort((a, b) {
      final byOrder = a.order.compareTo(b.order);
      return byOrder != 0 ? byOrder : a.id.compareTo(b.id);
    });
    return nodes;
  }

  /// Folders from the top level down to [folderId].
  List<MorphBookmarkFolder> path(String? folderId) {
    final out = <MorphBookmarkFolder>[];
    var at = folder(folderId);
    while (at != null && !out.contains(at)) {
      out.insert(0, at);
      at = folder(at.parentId);
    }
    return out;
  }

  /// [folderId] and every folder inside it.
  Set<String> subtree(String folderId) {
    final out = {folderId};
    var grew = true;
    while (grew) {
      grew = false;
      for (final f in folders) {
        if (f.parentId != null && out.contains(f.parentId) && out.add(f.id)) {
          grew = true;
        }
      }
    }
    return out;
  }

  /// Bookmarks anywhere inside [folderId].
  int bookmarksIn(String folderId) {
    final ids = subtree(folderId);
    return bookmarks.where((b) => ids.contains(b.folderId)).length;
  }
}

class MorphBookmarkTag {
  const MorphBookmarkTag({required this.name, required this.count});

  final String name;
  final int count;
}

class MorphHistoryEntry {
  const MorphHistoryEntry({
    required this.url,
    this.title = '',
    required this.visited,
  });

  final String url;
  final String title;
  final DateTime visited;
}

class IdentityStatusView {
  const IdentityStatusView({
    required this.home,
    required this.homeExists,
    required this.profile,
  });

  final String home;
  final bool homeExists;
  final String profile;
}

class EphemeralUser {
  const EphemeralUser({required this.userId, required this.fingerprint});

  final String userId;
  final String fingerprint;
}

class ProfileInfo {
  const ProfileInfo({required this.id, required this.name, this.color = ''});

  final String id;
  final String name;

  /// Optional `#rrggbb` avatar color. Empty means the UI picks a stable color
  /// from the profile id.
  final String color;
}

class ProfileTab {
  const ProfileTab({required this.id, this.url = '', this.title = ''});

  final int id;
  final String url;
  final String title;
}

class ProfileTabSnapshot {
  const ProfileTabSnapshot({
    required this.activeId,
    required this.nextId,
    required this.tabs,
  });

  final int activeId;
  final int nextId;
  final List<ProfileTab> tabs;

  static const empty = ProfileTabSnapshot(activeId: 1, nextId: 2, tabs: []);
}

class ProfileCatalog {
  const ProfileCatalog({required this.current, required this.profiles});

  final String current;
  final List<ProfileInfo> profiles;
}

class ProfilePaths {
  const ProfilePaths({
    required this.id,
    required this.profileDir,
    required this.settingsPath,
    required this.tabsPath,
    required this.webkitDir,
    required this.webkitDataDir,
    required this.webkitCacheDir,
    required this.cookiesPath,
  });

  final String id;
  final String profileDir;
  final String settingsPath;
  final String tabsPath;
  final String webkitDir;
  final String webkitDataDir;
  final String webkitCacheDir;
  final String cookiesPath;
}
