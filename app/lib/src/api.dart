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
  Future<ProfileCatalog> listProfiles({String? home});
  Future<ProfileCatalog> createProfile(String name, {String? home});
  Future<ProfileCatalog> renameProfile(String id, String name, {String? home});
  Future<ProfileCatalog> deleteProfile(String id, {String? home});
  Future<ProfileCatalog> setCurrentProfile(String id, {String? home});
  Future<ProfilePaths> profilePaths(String id, {String? home});
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
  const ProfileInfo({required this.id, required this.name});

  final String id;
  final String name;
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
