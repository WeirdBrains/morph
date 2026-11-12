import 'api.dart';
import 'rust/api/events.dart' as rust_events;
import 'rust/api/identity.dart' as rust_identity;
import 'rust/api/info.dart' as rust_info;
import 'rust/api/page.dart' as rust_page;
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
}
