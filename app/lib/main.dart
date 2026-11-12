import 'package:flutter/material.dart';
import 'package:morph_webview/morph_webview.dart';

import 'src/app.dart';
import 'src/ffi_api.dart';
import 'src/rust/api/control.dart' as rust_control;
import 'src/rust/frb_generated.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await RustLib.init();
  MorphWebViewControl.start = _startControl;
  runApp(const MorphApp(api: FfiMorphApi()));
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
