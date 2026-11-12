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
