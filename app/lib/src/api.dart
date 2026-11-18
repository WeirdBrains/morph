/// The Rust surface the shell needs. Widget tests use a fake so they can run
/// on the Dart VM without loading generated FFI bindings or `libmorph_bridge.so`.
abstract class MorphApi
    implements MorphRelaySource, MorphNetworkSource, MorphDeviceSource {
  String coreVersion();

  /// What typing [input] into the address bar opens (`morph-core`'s rule,
  /// shared with the desktop shell): an address, a search, or null if blank.
  String? omniboxTarget(String input);

  /// [omniboxTarget] with phrases sent to the search engine [engine] (an id
  /// from [searchEngines]).
  String? omniboxTargetFor(String input, String engine);

  /// The search engines for the selector beside the address, default first.
  List<SearchEngineOption> searchEngines();

  /// The search engine new tabs use (the profile's setting, as on desktop).
  Future<String> defaultSearchEngine({String? home});

  /// What Morph opens itself for [url] (as [omniboxTargetFor] gave it): one
  /// of its `morph://` pages, a `weirdnet-<app>://` link to call over
  /// Weirdnet ([OwnAddress.app], then [openApp]), or its error page for an
  /// unknown page or a link that can't be called. Null for web addresses.
  /// None of them is ever a search or handed to the OS.
  OwnAddress? ownAddress(String url);

  /// Calls the app a `weirdnet-<app>://` address names over Weirdnet (off
  /// the UI thread) and returns its page, or the error page saying why not.
  Future<MorphAppPage> openApp(String url);
  Future<IdentityStatusView> identityDirInfo({String? home});
  Stream<int> eventTicks();
  Future<EphemeralUser> newEphemeralUser();
  Future<String> handlePageMessage({required String json});
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

  /// Opens [profileId]'s encrypted bookmarks and history with the key kept in
  /// the system keystore, without a prompt. `needsPassword` means the keystore
  /// had no usable key: ask for the profile password ([unlockLibrary]).
  Future<MorphLibraryOpen> openLibrary(String profileId, {String? home});

  /// Unlocks [profileId]'s library with its password and keeps the key in the
  /// system keystore for next time.
  Future<MorphLibraryUnlock> unlockLibrary(
    String profileId,
    String password, {
    String? home,
  });

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

  /// What a file offered for restore holds, without any password.
  Future<MorphBackupKind> identityBackupKind(List<int> contents);

  /// A backup of [profileId]'s identity sealed under [backupPassword] (text:
  /// an armored age file of the recovery words), after checking [password].
  Future<String> exportProfileBackup(
    String profileId,
    String password,
    String backupPassword, {
    String? home,
  });

  /// Restore the identity in a backup or recovery-phrase file into
  /// [profileId], sealed under [password]. A sealed backup needs
  /// [backupPassword].
  Future<MorphIdentity> restoreProfileBackup(
    String profileId,
    List<int> contents, {
    String? backupPassword,
    required String password,
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

  /// New device: answer the existing device's [invite]
  /// (`weirdnet-morph://invite/…`, scanned or pasted) with this session's
  /// offer. The existing device then pairs as if it had scanned this
  /// device's code; [linkNewDeviceStatus] goes on as usual.
  Future<void> linkNewDeviceAnswer(int session, String invite);

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

/// The relays device linking uses, and whether Morph is connected to them
/// now. The state comes from the running link nodes or a relay check (never
/// guessed); with none running, [MorphRelayReport.nodes] is 0.
/// One search engine in the address selector.
class SearchEngineOption {
  const SearchEngineOption({
    required this.id,
    required this.name,
    required this.mark,
  });

  final String id;
  final String name;

  /// Short mark for the selector button (`G`).
  final String mark;
}

/// Morph's own handling of an address (see [MorphApi.ownAddress]).
class OwnAddress {
  const OwnAddress({
    this.page,
    this.app = false,
    this.title = '',
    this.body = '',
  });

  /// One of Morph's pages (`settings`, `history`, `relays`, …) to open.
  final String? page;

  /// A `weirdnet-<app>://` address to call ([MorphApi.openApp]).
  final bool app;

  /// Otherwise the error page's title and explanation.
  final String title;
  final String body;
}

/// A Weirdnet app's answer (see [MorphApi.openApp]).
class MorphAppPage {
  const MorphAppPage({
    this.dataUrl,
    this.title = '',
    this.body = '',
    this.status = 0,
  });

  /// The `data:` address to load, or null when the call failed.
  final String? dataUrl;

  /// The page's title, or the error page's.
  final String title;

  /// The error page's explanation.
  final String body;

  /// The app's status (0 when it did not answer).
  final int status;
}

/// The token marking an app page's `data:` address (the bridge puts
/// `morph-app=<hex>` among its parameters), or null for any other address.
/// Read in Dart: the address can be megabytes long.
String? morphAppPageToken(String url) {
  if (!url.startsWith('data:')) {
    return null;
  }
  final comma = url.indexOf(',');
  final head = url.substring(5, comma < 0 ? url.length : comma);
  for (final part in head.split(';')) {
    final p = part.trim();
    if (p.startsWith('morph-app=')) {
      final token = p.substring('morph-app='.length);
      return RegExp(r'^[0-9a-fA-F]+$').hasMatch(token) ? token : null;
    }
  }
  return null;
}

abstract class MorphRelaySource {
  /// The relays now. Cheap; never waits on the network.
  MorphRelayReport relayReport();

  /// Reach the relays for 30 s (a node like a link's), so the report shows
  /// real state while no link runs.
  Future<void> relayCheckStart();
  Future<void> relayCheckStop();
}

/// Settings → Linked devices: the profile's devices (`morph-services` sync,
/// shared with the desktop), their key state after a rotation, and revoking
/// one. A revoke rotates the sync key, so the revoked device cannot read
/// anything synced afterwards.
abstract class MorphDeviceSource {
  /// This device first. Empty when the profile does not sync yet.
  Future<List<MorphLinkedDevice>> linkedDevices(
    String profileId, {
    String? home,
  });

  /// What to say before revoking [peerId] (other devices that have not synced
  /// since updating would be left out of the new key), or null.
  Future<String?> linkedDeviceRevokeWarning(
    String profileId,
    String peerId, {
    String? home,
  });

  /// Revoke [peerId]; returns the list after the revoke.
  Future<List<MorphLinkedDevice>> revokeLinkedDevice(
    String profileId,
    String peerId, {
    String? home,
  });
}

/// One linked device in Settings → Linked devices.
class MorphLinkedDevice {
  const MorphLinkedDevice({
    required this.peerId,
    required this.label,
    required this.state,
    this.issuedMs = 0,
    this.revokedMs,
    this.thisDevice = false,
    this.note,
    this.hasKey = true,
  });

  final String peerId;
  final String label;

  /// "This device", "Linked" or "Revoked".
  final String state;
  final int issuedMs;
  final int? revokedMs;
  final bool thisDevice;

  /// "Needs re-admission from another device" (this device, restored after
  /// keys rotated) or "Waiting for its first sync" (another device, linked
  /// here after keys rotated).
  final String? note;

  /// It has synced since updating Morph (its device key is published).
  final bool hasKey;

  bool get revoked => revokedMs != null;
}

/// Shown after restoring an identity from its recovery words or a backup:
/// the same words as `morph_services::sync::RESTORED_NOTE` on desktop.
const morphRestoredNote =
    'If you use this identity on other devices, link this one again from one '
    'of them so it can read what they sync; until then Linked devices shows '
    'it waiting for re-admission.';

/// Settings → Relays: Weirdnet discovery and manually added peers for every
/// profile (`network.json` in the Morph home, the same file the desktop
/// uses). Rust validates, stores and restarts the running nodes; turning
/// discovery off keeps sharing, relays and sync with added peers.
abstract class MorphNetworkSource {
  Future<MorphNetworkSettings> loadNetwork({String? home});

  /// Turn discovery on or off. A locked setting comes back unchanged.
  Future<MorphNetworkSettings> setDiscovery(bool enabled, {String? home});

  /// Add a peer by multiaddr (ending in `/p2p/<id>`) or `<id> <addr>`.
  /// Throws [MorphNetworkError] saying what is wrong with [spec].
  Future<MorphNetworkSettings> addNetworkPeer(String spec, {String? home});
  Future<MorphNetworkSettings> removeNetworkPeer(String peerId, {String? home});
}

/// Why a network settings change failed, for the screen.
class MorphNetworkError implements Exception {
  const MorphNetworkError(this.message);

  final String message;

  @override
  String toString() => message;
}

/// One manually added peer: an address hint, not trust.
class MorphNetworkPeer {
  const MorphNetworkPeer({required this.peerId, required this.addrs});

  final String peerId;
  final List<String> addrs;
}

class MorphNetworkSettings {
  const MorphNetworkSettings({
    required this.discoveryEnabled,
    required this.discoveryLocked,
    required this.discoveryHelp,
    required this.peers,
    this.lockSource,
  });

  /// Whether discovery runs (the preference, unless an admin locked it off).
  final bool discoveryEnabled;

  /// An admin override (`MORPH_DISABLE_WEIRDNET_DISCOVERY` or a policy
  /// file) forces discovery off; the switch cannot turn it back on.
  final bool discoveryLocked;

  /// `env` or `policy` when locked.
  final String? lockSource;

  /// What the screen says under the switch (why it is locked, if it is).
  final String discoveryHelp;
  final List<MorphNetworkPeer> peers;
}

enum MorphRelayState {
  /// No node runs: Morph is not connected to any relay.
  noNode,
  notConnected,
  connected,

  /// Connected with a circuit reservation: other devices can reach this one.
  reserved;

  /// The bridge's word (`no-node`, `not-connected`, `connected`, `reserved`).
  static MorphRelayState parse(String word) => switch (word) {
    'reserved' => reserved,
    'connected' => connected,
    'not-connected' => notConnected,
    _ => noNode,
  };
}

/// One configured relay.
class MorphRelay {
  const MorphRelay({
    required this.peer,
    required this.addrs,
    required this.title,
    required this.state,
    required this.label,
    this.host,
    this.rttMs,
  });

  final String peer;

  /// Its configured multiaddrs.
  final List<String> addrs;

  /// Its display name (from its well-known info), else its host.
  final String title;
  final String? host;
  final MorphRelayState state;

  /// What a screen says about [state].
  final String label;

  /// Ping round trip, when the network layer reports it.
  final int? rttMs;
}

class MorphRelayReport {
  const MorphRelayReport({
    required this.nodes,
    required this.checking,
    required this.summary,
    required this.relays,
  });

  /// Running nodes the state came from (0: nothing runs).
  final int nodes;

  /// A relay check runs.
  final bool checking;

  /// One line for a link screen.
  final String summary;
  final List<MorphRelay> relays;
}

/// The prefix of an existing device's invite (Add a device on the desktop).
const morphLinkInvitePrefix = 'weirdnet-morph://invite/';

/// The prefix of a new device's link (Link from another device).
const morphLinkOfferPrefix = 'weirdnet-morph://link/';

/// Whether [text] is an existing device's invite rather than a new device's
/// link.
bool morphIsLinkInvite(String text) =>
    text.trim().toLowerCase().startsWith(morphLinkInvitePrefix);

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

/// What opening a profile's library at launch found.
class MorphLibraryOpen {
  const MorphLibraryOpen({
    this.ready = true,
    this.needsPassword = false,
    this.reason,
  });

  /// Open, or nothing to unlock (no identity yet).
  final bool ready;
  final bool needsPassword;

  /// Why the system keystore could not help. Never secret.
  final String? reason;
}

/// The outcome of unlocking a library with the profile password.
class MorphLibraryUnlock {
  const MorphLibraryUnlock({this.remembered = true, this.reason});

  /// The key is in the system keystore, so the next launch needs no password.
  final bool remembered;
  final String? reason;
}

/// A just-created identity and its recovery words.
/// What a file offered for identity restore holds.
enum MorphBackupKind {
  /// A password-sealed backup: needs its backup password.
  sealed,

  /// An unsealed recovery phrase.
  phrase,

  /// Neither.
  unknown,
}

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
