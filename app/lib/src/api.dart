/// The Rust surface the shell needs. Widget tests use a fake so they can run
/// on the Dart VM without loading generated FFI bindings or `libmorph_bridge.so`.
abstract class MorphApi {
  String coreVersion();
  Future<IdentityStatusView> identityDirInfo({String? home});
  Stream<int> eventTicks();
  Future<EphemeralUser> newEphemeralUser();
  Future<String> handlePageMessage({required String json});
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
