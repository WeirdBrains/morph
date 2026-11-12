import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/api.dart';
import 'package:morph/src/app.dart';
import 'package:morph/src/shell.dart';

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
}

void main() {
  testWidgets('shell shows the address bar and core version', (tester) async {
    const missingHome = '/tmp/morph-does-not-exist';
    final api = FakeMorphApi();
    await tester.pumpWidget(
      MaterialApp(
        home: MorphShell(api: api, home: missingHome, subscribeToTicks: false),
      ),
    );
    await tester.pump();

    expect(find.byKey(const Key('address-bar')), findsOneWidget);
    expect(find.textContaining('morph-core 0.1.0'), findsOneWidget);
    expect(find.textContaining('Home exists false'), findsOneWidget);

    await tester.tap(find.text('Ping page bridge'));
    await tester.pump();
    expect(find.textContaining('pong'), findsOneWidget);

    await tester.tap(find.text('New ephemeral user'));
    await tester.pump();
    expect(
      find.textContaining('0123456789abcdef0123456789abcdef'),
      findsOneWidget,
    );
  });

  testWidgets('MorphApp builds', (tester) async {
    await tester.pumpWidget(
      MorphApp(api: FakeMorphApi(), subscribeToTicks: false),
    );
    await tester.pump();
    expect(find.text('Morph'), findsOneWidget);
  });
}
