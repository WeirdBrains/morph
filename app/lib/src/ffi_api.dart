import 'api.dart';
import 'rust/api/events.dart' as rust_events;
import 'rust/api/identity.dart' as rust_identity;
import 'rust/api/info.dart' as rust_info;
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
  Future<ProfileCatalog> listProfiles({String? home}) async {
    return _catalog(await rust_profiles.listProfiles(home: home));
  }

  @override
  Future<ProfileCatalog> createProfile(String name, {String? home}) async {
    return _catalog(await rust_profiles.createProfile(home: home, name: name));
  }

  @override
  Future<ProfileCatalog> renameProfile(
    String id,
    String name, {
    String? home,
  }) async {
    return _catalog(
      await rust_profiles.renameProfile(home: home, id: id, name: name),
    );
  }

  @override
  Future<ProfileCatalog> deleteProfile(String id, {String? home}) async {
    return _catalog(await rust_profiles.deleteProfile(home: home, id: id));
  }

  @override
  Future<ProfileCatalog> setCurrentProfile(String id, {String? home}) async {
    return _catalog(await rust_profiles.setCurrentProfile(home: home, id: id));
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
}
