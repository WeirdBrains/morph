// This is a generated file - do not edit.
//
// Generated from morph.webview.v1.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:async' as $async;
import 'dart:core' as $core;

import 'package:grpc/service_api.dart' as $grpc;
import 'package:protobuf/protobuf.dart' as $pb;

import 'morph.webview.v1.pb.dart' as $0;

export 'morph.webview.v1.pb.dart';

/// In-process control channel between Dart (or any client) and the web-view backend.
/// One bidirectional stream keeps pointer, key, and IME events in order.
@$pb.GrpcServiceName('morph.webview.v1.WebViewControl')
class WebViewControlClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  WebViewControlClient(super.channel, {super.options, super.interceptors});

  $grpc.ResponseStream<$0.ServerMessage> session(
    $async.Stream<$0.ClientMessage> request, {
    $grpc.CallOptions? options,
  }) {
    return $createStreamingCall(_$session, request, options: options);
  }

  // method descriptors

  static final _$session =
      $grpc.ClientMethod<$0.ClientMessage, $0.ServerMessage>(
          '/morph.webview.v1.WebViewControl/Session',
          ($0.ClientMessage value) => value.writeToBuffer(),
          $0.ServerMessage.fromBuffer);
}

@$pb.GrpcServiceName('morph.webview.v1.WebViewControl')
abstract class WebViewControlServiceBase extends $grpc.Service {
  $core.String get $name => 'morph.webview.v1.WebViewControl';

  WebViewControlServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.ClientMessage, $0.ServerMessage>(
        'Session',
        session,
        true,
        true,
        ($core.List<$core.int> value) => $0.ClientMessage.fromBuffer(value),
        ($0.ServerMessage value) => value.writeToBuffer()));
  }

  $async.Stream<$0.ServerMessage> session(
      $grpc.ServiceCall call, $async.Stream<$0.ClientMessage> request);
}
