import 'package:flutter/material.dart';
import 'package:morph_webview/morph_webview.dart';

import 'src/app.dart';
import 'src/incoming_links.dart';
import 'src/ffi_api.dart';
import 'src/morph_home.dart';
import 'src/rust/api/control.dart' as rust_control;
import 'src/rust/frb_generated.dart';
import 'src/window/morph_desktop_window.dart';

Future<void> main(List<String> arguments) async {
  WidgetsFlutterBinding.ensureInitialized();
  MorphDesktopWindow.applyChromeless();
  await RustLib.init();
  MorphWebViewControl.start = _startControl;
  final home = await defaultMorphHome();
  runApp(
    MorphApp(
      api: const FfiMorphApi(),
      home: home,
      links: MorphIncomingLinks(arguments: arguments),
    ),
  );
}

Future<MorphControlBindings> _startControl() async {
  final events = rust_control.controlBackendEvents();
  var endpoint = await rust_control.startWebviewControl();
  if (!MorphControlClient.canOpen(endpoint)) {
    await rust_control.stopWebviewControl();
    endpoint = await rust_control.startWebviewControlLoopback();
  }
  return MorphControlBindings(
    endpoint: endpoint,
    backendEvents: events.map(
      (event) => MorphControlCommand(
        kind: event.kind,
        viewId: _asInt(event.viewId),
        url: event.url,
        requestId: _asInt(event.requestId),
        script: event.script,
      ),
    ),
    emitNavigation: rust_control.controlEmitNavigation,
    emitPageMessage: rust_control.controlEmitPageMessage,
    completeEvaluateJs: rust_control.controlCompleteEvaluateJs,
  );
}

int _asInt(Object value) {
  if (value is int) {
    return value;
  }
  if (value is BigInt) {
    return value.toInt();
  }
  return int.parse(value.toString());
}
