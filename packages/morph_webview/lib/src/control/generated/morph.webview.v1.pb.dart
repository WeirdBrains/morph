// This is a generated file - do not edit.
//
// Generated from morph.webview.v1.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:fixnum/fixnum.dart' as $fixnum;
import 'package:protobuf/protobuf.dart' as $pb;

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

enum ClientMessage_Body {
  navigate,
  goBack,
  goForward,
  reload,
  pointer,
  key,
  ime,
  evaluateJavascript,
  pageMessage,
  notSet
}

class ClientMessage extends $pb.GeneratedMessage {
  factory ClientMessage({
    $fixnum.Int64? seq,
    $fixnum.Int64? viewId,
    Navigate? navigate,
    GoBack? goBack,
    GoForward? goForward,
    Reload? reload,
    PointerEvent? pointer,
    KeyEvent? key,
    ImeEvent? ime,
    EvaluateJavaScript? evaluateJavascript,
    PageMessage? pageMessage,
  }) {
    final result = ClientMessage._();
    if (seq != null) result.seq = seq;
    if (viewId != null) result.viewId = viewId;
    if (navigate != null) result.navigate = navigate;
    if (goBack != null) result.goBack = goBack;
    if (goForward != null) result.goForward = goForward;
    if (reload != null) result.reload = reload;
    if (pointer != null) result.pointer = pointer;
    if (key != null) result.key = key;
    if (ime != null) result.ime = ime;
    if (evaluateJavascript != null)
      result.evaluateJavascript = evaluateJavascript;
    if (pageMessage != null) result.pageMessage = pageMessage;
    return result;
  }

  ClientMessage._();

  factory ClientMessage.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ClientMessage()..mergeFromBuffer(data, registry);
  factory ClientMessage.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ClientMessage()..mergeFromJson(json, registry);

  static const $core.Map<$core.int, ClientMessage_Body>
      _ClientMessage_BodyByTag = {
    3: ClientMessage_Body.navigate,
    4: ClientMessage_Body.goBack,
    5: ClientMessage_Body.goForward,
    6: ClientMessage_Body.reload,
    7: ClientMessage_Body.pointer,
    8: ClientMessage_Body.key,
    9: ClientMessage_Body.ime,
    10: ClientMessage_Body.evaluateJavascript,
    11: ClientMessage_Body.pageMessage,
    0: ClientMessage_Body.notSet
  };
  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ClientMessage',
      package:
          const $pb.PackageName(_omitMessageNames ? '' : 'morph.webview.v1'),
      createEmptyInstance: ClientMessage.$_createMessage)
    ..oo(0, [3, 4, 5, 6, 7, 8, 9, 10, 11])
    ..a<$fixnum.Int64>(1, _omitFieldNames ? '' : 'seq', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aInt64(2, _omitFieldNames ? '' : 'viewId')
    ..aOM<Navigate>(3, _omitFieldNames ? '' : 'navigate',
        subBuilder: Navigate.$_createMessage)
    ..aOM<GoBack>(4, _omitFieldNames ? '' : 'goBack',
        subBuilder: GoBack.$_createMessage)
    ..aOM<GoForward>(5, _omitFieldNames ? '' : 'goForward',
        subBuilder: GoForward.$_createMessage)
    ..aOM<Reload>(6, _omitFieldNames ? '' : 'reload',
        subBuilder: Reload.$_createMessage)
    ..aOM<PointerEvent>(7, _omitFieldNames ? '' : 'pointer',
        subBuilder: PointerEvent.$_createMessage)
    ..aOM<KeyEvent>(8, _omitFieldNames ? '' : 'key',
        subBuilder: KeyEvent.$_createMessage)
    ..aOM<ImeEvent>(9, _omitFieldNames ? '' : 'ime',
        subBuilder: ImeEvent.$_createMessage)
    ..aOM<EvaluateJavaScript>(10, _omitFieldNames ? '' : 'evaluateJavascript',
        subBuilder: EvaluateJavaScript.$_createMessage)
    ..aOM<PageMessage>(11, _omitFieldNames ? '' : 'pageMessage',
        subBuilder: PageMessage.$_createMessage)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ClientMessage clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ClientMessage copyWith(void Function(ClientMessage) updates) =>
      super.copyWith((message) => updates(message as ClientMessage))
          as ClientMessage;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use ClientMessage() / ClientMessage.new instead')
  static ClientMessage create() => ClientMessage._();
  static $pb.GeneratedMessage $_createMessage() => ClientMessage._();
  @$core.override
  ClientMessage createEmptyInstance() => ClientMessage._();
  @$core.pragma('dart2js:noInline')
  static ClientMessage getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<ClientMessage>(
          ClientMessage.$_createMessage);
  static ClientMessage? _defaultInstance;

  @$pb.TagNumber(3)
  @$pb.TagNumber(4)
  @$pb.TagNumber(5)
  @$pb.TagNumber(6)
  @$pb.TagNumber(7)
  @$pb.TagNumber(8)
  @$pb.TagNumber(9)
  @$pb.TagNumber(10)
  @$pb.TagNumber(11)
  ClientMessage_Body whichBody() => _ClientMessage_BodyByTag[$_whichOneof(0)]!;
  @$pb.TagNumber(3)
  @$pb.TagNumber(4)
  @$pb.TagNumber(5)
  @$pb.TagNumber(6)
  @$pb.TagNumber(7)
  @$pb.TagNumber(8)
  @$pb.TagNumber(9)
  @$pb.TagNumber(10)
  @$pb.TagNumber(11)
  void clearBody() => $_clearField($_whichOneof(0));

  @$pb.TagNumber(1)
  $fixnum.Int64 get seq => $_getI64(0);
  @$pb.TagNumber(1)
  set seq($fixnum.Int64 value) => $_setInt64(0, value);
  @$pb.TagNumber(1)
  $core.bool hasSeq() => $_has(0);
  @$pb.TagNumber(1)
  void clearSeq() => $_clearField(1);

  @$pb.TagNumber(2)
  $fixnum.Int64 get viewId => $_getI64(1);
  @$pb.TagNumber(2)
  set viewId($fixnum.Int64 value) => $_setInt64(1, value);
  @$pb.TagNumber(2)
  $core.bool hasViewId() => $_has(1);
  @$pb.TagNumber(2)
  void clearViewId() => $_clearField(2);

  @$pb.TagNumber(3)
  Navigate get navigate => $_getN(2);
  @$pb.TagNumber(3)
  set navigate(Navigate value) => $_setField(3, value);
  @$pb.TagNumber(3)
  $core.bool hasNavigate() => $_has(2);
  @$pb.TagNumber(3)
  void clearNavigate() => $_clearField(3);
  @$pb.TagNumber(3)
  Navigate ensureNavigate() => $_ensure(2);

  @$pb.TagNumber(4)
  GoBack get goBack => $_getN(3);
  @$pb.TagNumber(4)
  set goBack(GoBack value) => $_setField(4, value);
  @$pb.TagNumber(4)
  $core.bool hasGoBack() => $_has(3);
  @$pb.TagNumber(4)
  void clearGoBack() => $_clearField(4);
  @$pb.TagNumber(4)
  GoBack ensureGoBack() => $_ensure(3);

  @$pb.TagNumber(5)
  GoForward get goForward => $_getN(4);
  @$pb.TagNumber(5)
  set goForward(GoForward value) => $_setField(5, value);
  @$pb.TagNumber(5)
  $core.bool hasGoForward() => $_has(4);
  @$pb.TagNumber(5)
  void clearGoForward() => $_clearField(5);
  @$pb.TagNumber(5)
  GoForward ensureGoForward() => $_ensure(4);

  @$pb.TagNumber(6)
  Reload get reload => $_getN(5);
  @$pb.TagNumber(6)
  set reload(Reload value) => $_setField(6, value);
  @$pb.TagNumber(6)
  $core.bool hasReload() => $_has(5);
  @$pb.TagNumber(6)
  void clearReload() => $_clearField(6);
  @$pb.TagNumber(6)
  Reload ensureReload() => $_ensure(5);

  @$pb.TagNumber(7)
  PointerEvent get pointer => $_getN(6);
  @$pb.TagNumber(7)
  set pointer(PointerEvent value) => $_setField(7, value);
  @$pb.TagNumber(7)
  $core.bool hasPointer() => $_has(6);
  @$pb.TagNumber(7)
  void clearPointer() => $_clearField(7);
  @$pb.TagNumber(7)
  PointerEvent ensurePointer() => $_ensure(6);

  @$pb.TagNumber(8)
  KeyEvent get key => $_getN(7);
  @$pb.TagNumber(8)
  set key(KeyEvent value) => $_setField(8, value);
  @$pb.TagNumber(8)
  $core.bool hasKey() => $_has(7);
  @$pb.TagNumber(8)
  void clearKey() => $_clearField(8);
  @$pb.TagNumber(8)
  KeyEvent ensureKey() => $_ensure(7);

  @$pb.TagNumber(9)
  ImeEvent get ime => $_getN(8);
  @$pb.TagNumber(9)
  set ime(ImeEvent value) => $_setField(9, value);
  @$pb.TagNumber(9)
  $core.bool hasIme() => $_has(8);
  @$pb.TagNumber(9)
  void clearIme() => $_clearField(9);
  @$pb.TagNumber(9)
  ImeEvent ensureIme() => $_ensure(8);

  @$pb.TagNumber(10)
  EvaluateJavaScript get evaluateJavascript => $_getN(9);
  @$pb.TagNumber(10)
  set evaluateJavascript(EvaluateJavaScript value) => $_setField(10, value);
  @$pb.TagNumber(10)
  $core.bool hasEvaluateJavascript() => $_has(9);
  @$pb.TagNumber(10)
  void clearEvaluateJavascript() => $_clearField(10);
  @$pb.TagNumber(10)
  EvaluateJavaScript ensureEvaluateJavascript() => $_ensure(9);

  @$pb.TagNumber(11)
  PageMessage get pageMessage => $_getN(10);
  @$pb.TagNumber(11)
  set pageMessage(PageMessage value) => $_setField(11, value);
  @$pb.TagNumber(11)
  $core.bool hasPageMessage() => $_has(10);
  @$pb.TagNumber(11)
  void clearPageMessage() => $_clearField(11);
  @$pb.TagNumber(11)
  PageMessage ensurePageMessage() => $_ensure(10);
}

enum ServerMessage_Body {
  navigation,
  pageMessage,
  evaluateJavascriptResult,
  ack,
  error,
  cursor,
  notSet
}

class ServerMessage extends $pb.GeneratedMessage {
  factory ServerMessage({
    $fixnum.Int64? seq,
    $fixnum.Int64? viewId,
    NavigationEvent? navigation,
    PageMessage? pageMessage,
    EvaluateJavaScriptResult? evaluateJavascriptResult,
    Ack? ack,
    Error? error,
    CursorEvent? cursor,
  }) {
    final result = ServerMessage._();
    if (seq != null) result.seq = seq;
    if (viewId != null) result.viewId = viewId;
    if (navigation != null) result.navigation = navigation;
    if (pageMessage != null) result.pageMessage = pageMessage;
    if (evaluateJavascriptResult != null)
      result.evaluateJavascriptResult = evaluateJavascriptResult;
    if (ack != null) result.ack = ack;
    if (error != null) result.error = error;
    if (cursor != null) result.cursor = cursor;
    return result;
  }

  ServerMessage._();

  factory ServerMessage.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ServerMessage()..mergeFromBuffer(data, registry);
  factory ServerMessage.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ServerMessage()..mergeFromJson(json, registry);

  static const $core.Map<$core.int, ServerMessage_Body>
      _ServerMessage_BodyByTag = {
    3: ServerMessage_Body.navigation,
    4: ServerMessage_Body.pageMessage,
    5: ServerMessage_Body.evaluateJavascriptResult,
    6: ServerMessage_Body.ack,
    7: ServerMessage_Body.error,
    8: ServerMessage_Body.cursor,
    0: ServerMessage_Body.notSet
  };
  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ServerMessage',
      package:
          const $pb.PackageName(_omitMessageNames ? '' : 'morph.webview.v1'),
      createEmptyInstance: ServerMessage.$_createMessage)
    ..oo(0, [3, 4, 5, 6, 7, 8])
    ..a<$fixnum.Int64>(1, _omitFieldNames ? '' : 'seq', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aInt64(2, _omitFieldNames ? '' : 'viewId')
    ..aOM<NavigationEvent>(3, _omitFieldNames ? '' : 'navigation',
        subBuilder: NavigationEvent.$_createMessage)
    ..aOM<PageMessage>(4, _omitFieldNames ? '' : 'pageMessage',
        subBuilder: PageMessage.$_createMessage)
    ..aOM<EvaluateJavaScriptResult>(
        5, _omitFieldNames ? '' : 'evaluateJavascriptResult',
        subBuilder: EvaluateJavaScriptResult.$_createMessage)
    ..aOM<Ack>(6, _omitFieldNames ? '' : 'ack', subBuilder: Ack.$_createMessage)
    ..aOM<Error>(7, _omitFieldNames ? '' : 'error',
        subBuilder: Error.$_createMessage)
    ..aOM<CursorEvent>(8, _omitFieldNames ? '' : 'cursor',
        subBuilder: CursorEvent.$_createMessage)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ServerMessage clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ServerMessage copyWith(void Function(ServerMessage) updates) =>
      super.copyWith((message) => updates(message as ServerMessage))
          as ServerMessage;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use ServerMessage() / ServerMessage.new instead')
  static ServerMessage create() => ServerMessage._();
  static $pb.GeneratedMessage $_createMessage() => ServerMessage._();
  @$core.override
  ServerMessage createEmptyInstance() => ServerMessage._();
  @$core.pragma('dart2js:noInline')
  static ServerMessage getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<ServerMessage>(
          ServerMessage.$_createMessage);
  static ServerMessage? _defaultInstance;

  @$pb.TagNumber(3)
  @$pb.TagNumber(4)
  @$pb.TagNumber(5)
  @$pb.TagNumber(6)
  @$pb.TagNumber(7)
  @$pb.TagNumber(8)
  ServerMessage_Body whichBody() => _ServerMessage_BodyByTag[$_whichOneof(0)]!;
  @$pb.TagNumber(3)
  @$pb.TagNumber(4)
  @$pb.TagNumber(5)
  @$pb.TagNumber(6)
  @$pb.TagNumber(7)
  @$pb.TagNumber(8)
  void clearBody() => $_clearField($_whichOneof(0));

  @$pb.TagNumber(1)
  $fixnum.Int64 get seq => $_getI64(0);
  @$pb.TagNumber(1)
  set seq($fixnum.Int64 value) => $_setInt64(0, value);
  @$pb.TagNumber(1)
  $core.bool hasSeq() => $_has(0);
  @$pb.TagNumber(1)
  void clearSeq() => $_clearField(1);

  @$pb.TagNumber(2)
  $fixnum.Int64 get viewId => $_getI64(1);
  @$pb.TagNumber(2)
  set viewId($fixnum.Int64 value) => $_setInt64(1, value);
  @$pb.TagNumber(2)
  $core.bool hasViewId() => $_has(1);
  @$pb.TagNumber(2)
  void clearViewId() => $_clearField(2);

  @$pb.TagNumber(3)
  NavigationEvent get navigation => $_getN(2);
  @$pb.TagNumber(3)
  set navigation(NavigationEvent value) => $_setField(3, value);
  @$pb.TagNumber(3)
  $core.bool hasNavigation() => $_has(2);
  @$pb.TagNumber(3)
  void clearNavigation() => $_clearField(3);
  @$pb.TagNumber(3)
  NavigationEvent ensureNavigation() => $_ensure(2);

  @$pb.TagNumber(4)
  PageMessage get pageMessage => $_getN(3);
  @$pb.TagNumber(4)
  set pageMessage(PageMessage value) => $_setField(4, value);
  @$pb.TagNumber(4)
  $core.bool hasPageMessage() => $_has(3);
  @$pb.TagNumber(4)
  void clearPageMessage() => $_clearField(4);
  @$pb.TagNumber(4)
  PageMessage ensurePageMessage() => $_ensure(3);

  @$pb.TagNumber(5)
  EvaluateJavaScriptResult get evaluateJavascriptResult => $_getN(4);
  @$pb.TagNumber(5)
  set evaluateJavascriptResult(EvaluateJavaScriptResult value) =>
      $_setField(5, value);
  @$pb.TagNumber(5)
  $core.bool hasEvaluateJavascriptResult() => $_has(4);
  @$pb.TagNumber(5)
  void clearEvaluateJavascriptResult() => $_clearField(5);
  @$pb.TagNumber(5)
  EvaluateJavaScriptResult ensureEvaluateJavascriptResult() => $_ensure(4);

  @$pb.TagNumber(6)
  Ack get ack => $_getN(5);
  @$pb.TagNumber(6)
  set ack(Ack value) => $_setField(6, value);
  @$pb.TagNumber(6)
  $core.bool hasAck() => $_has(5);
  @$pb.TagNumber(6)
  void clearAck() => $_clearField(6);
  @$pb.TagNumber(6)
  Ack ensureAck() => $_ensure(5);

  @$pb.TagNumber(7)
  Error get error => $_getN(6);
  @$pb.TagNumber(7)
  set error(Error value) => $_setField(7, value);
  @$pb.TagNumber(7)
  $core.bool hasError() => $_has(6);
  @$pb.TagNumber(7)
  void clearError() => $_clearField(7);
  @$pb.TagNumber(7)
  Error ensureError() => $_ensure(6);

  @$pb.TagNumber(8)
  CursorEvent get cursor => $_getN(7);
  @$pb.TagNumber(8)
  set cursor(CursorEvent value) => $_setField(8, value);
  @$pb.TagNumber(8)
  $core.bool hasCursor() => $_has(7);
  @$pb.TagNumber(8)
  void clearCursor() => $_clearField(8);
  @$pb.TagNumber(8)
  CursorEvent ensureCursor() => $_ensure(7);
}

class Navigate extends $pb.GeneratedMessage {
  factory Navigate({
    $core.String? url,
  }) {
    final result = Navigate._();
    if (url != null) result.url = url;
    return result;
  }

  Navigate._();

  factory Navigate.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      Navigate()..mergeFromBuffer(data, registry);
  factory Navigate.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      Navigate()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'Navigate',
      package:
          const $pb.PackageName(_omitMessageNames ? '' : 'morph.webview.v1'),
      createEmptyInstance: Navigate.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'url')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Navigate clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Navigate copyWith(void Function(Navigate) updates) =>
      super.copyWith((message) => updates(message as Navigate)) as Navigate;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use Navigate() / Navigate.new instead')
  static Navigate create() => Navigate._();
  static $pb.GeneratedMessage $_createMessage() => Navigate._();
  @$core.override
  Navigate createEmptyInstance() => Navigate._();
  @$core.pragma('dart2js:noInline')
  static Navigate getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<Navigate>(Navigate.$_createMessage);
  static Navigate? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get url => $_getSZ(0);
  @$pb.TagNumber(1)
  set url($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasUrl() => $_has(0);
  @$pb.TagNumber(1)
  void clearUrl() => $_clearField(1);
}

class GoBack extends $pb.GeneratedMessage {
  factory GoBack() => GoBack._();

  GoBack._();

  factory GoBack.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      GoBack()..mergeFromBuffer(data, registry);
  factory GoBack.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      GoBack()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'GoBack',
      package:
          const $pb.PackageName(_omitMessageNames ? '' : 'morph.webview.v1'),
      createEmptyInstance: GoBack.$_createMessage)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GoBack clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GoBack copyWith(void Function(GoBack) updates) =>
      super.copyWith((message) => updates(message as GoBack)) as GoBack;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use GoBack() / GoBack.new instead')
  static GoBack create() => GoBack._();
  static $pb.GeneratedMessage $_createMessage() => GoBack._();
  @$core.override
  GoBack createEmptyInstance() => GoBack._();
  @$core.pragma('dart2js:noInline')
  static GoBack getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<GoBack>(GoBack.$_createMessage);
  static GoBack? _defaultInstance;
}

class GoForward extends $pb.GeneratedMessage {
  factory GoForward() => GoForward._();

  GoForward._();

  factory GoForward.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      GoForward()..mergeFromBuffer(data, registry);
  factory GoForward.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      GoForward()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'GoForward',
      package:
          const $pb.PackageName(_omitMessageNames ? '' : 'morph.webview.v1'),
      createEmptyInstance: GoForward.$_createMessage)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GoForward clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GoForward copyWith(void Function(GoForward) updates) =>
      super.copyWith((message) => updates(message as GoForward)) as GoForward;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use GoForward() / GoForward.new instead')
  static GoForward create() => GoForward._();
  static $pb.GeneratedMessage $_createMessage() => GoForward._();
  @$core.override
  GoForward createEmptyInstance() => GoForward._();
  @$core.pragma('dart2js:noInline')
  static GoForward getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<GoForward>(GoForward.$_createMessage);
  static GoForward? _defaultInstance;
}

class Reload extends $pb.GeneratedMessage {
  factory Reload() => Reload._();

  Reload._();

  factory Reload.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      Reload()..mergeFromBuffer(data, registry);
  factory Reload.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      Reload()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'Reload',
      package:
          const $pb.PackageName(_omitMessageNames ? '' : 'morph.webview.v1'),
      createEmptyInstance: Reload.$_createMessage)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Reload clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Reload copyWith(void Function(Reload) updates) =>
      super.copyWith((message) => updates(message as Reload)) as Reload;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use Reload() / Reload.new instead')
  static Reload create() => Reload._();
  static $pb.GeneratedMessage $_createMessage() => Reload._();
  @$core.override
  Reload createEmptyInstance() => Reload._();
  @$core.pragma('dart2js:noInline')
  static Reload getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<Reload>(Reload.$_createMessage);
  static Reload? _defaultInstance;
}

/// Pointer or wheel event in view-logical coordinates. The backend multiplies
/// x/y by the view's device pixel ratio. Phases: down, move, up, cancel,
/// scroll, enter, leave, hover.
///
/// `button` is the button that changed for down/up (1=left, 2=middle, 3=right,
/// 4=back, 5=forward); 0 otherwise.
/// `buttons` is the mask of buttons currently held: bit 0 (1)=left, bit 1 (2)=
/// middle, bit 2 (4)=right, bit 3 (8)=back, bit 4 (16)=forward.
class PointerEvent extends $pb.GeneratedMessage {
  factory PointerEvent({
    $core.String? phase,
    $core.double? x,
    $core.double? y,
    $core.int? button,
    $core.double? scrollDeltaX,
    $core.double? scrollDeltaY,
    $core.int? buttons,
    $core.bool? ctrl,
    $core.bool? alt,
    $core.bool? shift,
    $core.bool? meta,
  }) {
    final result = PointerEvent._();
    if (phase != null) result.phase = phase;
    if (x != null) result.x = x;
    if (y != null) result.y = y;
    if (button != null) result.button = button;
    if (scrollDeltaX != null) result.scrollDeltaX = scrollDeltaX;
    if (scrollDeltaY != null) result.scrollDeltaY = scrollDeltaY;
    if (buttons != null) result.buttons = buttons;
    if (ctrl != null) result.ctrl = ctrl;
    if (alt != null) result.alt = alt;
    if (shift != null) result.shift = shift;
    if (meta != null) result.meta = meta;
    return result;
  }

  PointerEvent._();

  factory PointerEvent.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      PointerEvent()..mergeFromBuffer(data, registry);
  factory PointerEvent.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      PointerEvent()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PointerEvent',
      package:
          const $pb.PackageName(_omitMessageNames ? '' : 'morph.webview.v1'),
      createEmptyInstance: PointerEvent.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'phase')
    ..aD(2, _omitFieldNames ? '' : 'x')
    ..aD(3, _omitFieldNames ? '' : 'y')
    ..aI(4, _omitFieldNames ? '' : 'button')
    ..aD(5, _omitFieldNames ? '' : 'scrollDeltaX')
    ..aD(6, _omitFieldNames ? '' : 'scrollDeltaY')
    ..aI(7, _omitFieldNames ? '' : 'buttons')
    ..aOB(8, _omitFieldNames ? '' : 'ctrl')
    ..aOB(9, _omitFieldNames ? '' : 'alt')
    ..aOB(10, _omitFieldNames ? '' : 'shift')
    ..aOB(11, _omitFieldNames ? '' : 'meta')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PointerEvent clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PointerEvent copyWith(void Function(PointerEvent) updates) =>
      super.copyWith((message) => updates(message as PointerEvent))
          as PointerEvent;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use PointerEvent() / PointerEvent.new instead')
  static PointerEvent create() => PointerEvent._();
  static $pb.GeneratedMessage $_createMessage() => PointerEvent._();
  @$core.override
  PointerEvent createEmptyInstance() => PointerEvent._();
  @$core.pragma('dart2js:noInline')
  static PointerEvent getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<PointerEvent>(
          PointerEvent.$_createMessage);
  static PointerEvent? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get phase => $_getSZ(0);
  @$pb.TagNumber(1)
  set phase($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasPhase() => $_has(0);
  @$pb.TagNumber(1)
  void clearPhase() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.double get x => $_getN(1);
  @$pb.TagNumber(2)
  set x($core.double value) => $_setDouble(1, value);
  @$pb.TagNumber(2)
  $core.bool hasX() => $_has(1);
  @$pb.TagNumber(2)
  void clearX() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.double get y => $_getN(2);
  @$pb.TagNumber(3)
  set y($core.double value) => $_setDouble(2, value);
  @$pb.TagNumber(3)
  $core.bool hasY() => $_has(2);
  @$pb.TagNumber(3)
  void clearY() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.int get button => $_getIZ(3);
  @$pb.TagNumber(4)
  set button($core.int value) => $_setSignedInt32(3, value);
  @$pb.TagNumber(4)
  $core.bool hasButton() => $_has(3);
  @$pb.TagNumber(4)
  void clearButton() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.double get scrollDeltaX => $_getN(4);
  @$pb.TagNumber(5)
  set scrollDeltaX($core.double value) => $_setDouble(4, value);
  @$pb.TagNumber(5)
  $core.bool hasScrollDeltaX() => $_has(4);
  @$pb.TagNumber(5)
  void clearScrollDeltaX() => $_clearField(5);

  @$pb.TagNumber(6)
  $core.double get scrollDeltaY => $_getN(5);
  @$pb.TagNumber(6)
  set scrollDeltaY($core.double value) => $_setDouble(5, value);
  @$pb.TagNumber(6)
  $core.bool hasScrollDeltaY() => $_has(5);
  @$pb.TagNumber(6)
  void clearScrollDeltaY() => $_clearField(6);

  @$pb.TagNumber(7)
  $core.int get buttons => $_getIZ(6);
  @$pb.TagNumber(7)
  set buttons($core.int value) => $_setSignedInt32(6, value);
  @$pb.TagNumber(7)
  $core.bool hasButtons() => $_has(6);
  @$pb.TagNumber(7)
  void clearButtons() => $_clearField(7);

  @$pb.TagNumber(8)
  $core.bool get ctrl => $_getBF(7);
  @$pb.TagNumber(8)
  set ctrl($core.bool value) => $_setBool(7, value);
  @$pb.TagNumber(8)
  $core.bool hasCtrl() => $_has(7);
  @$pb.TagNumber(8)
  void clearCtrl() => $_clearField(8);

  @$pb.TagNumber(9)
  $core.bool get alt => $_getBF(8);
  @$pb.TagNumber(9)
  set alt($core.bool value) => $_setBool(8, value);
  @$pb.TagNumber(9)
  $core.bool hasAlt() => $_has(8);
  @$pb.TagNumber(9)
  void clearAlt() => $_clearField(9);

  @$pb.TagNumber(10)
  $core.bool get shift => $_getBF(9);
  @$pb.TagNumber(10)
  set shift($core.bool value) => $_setBool(9, value);
  @$pb.TagNumber(10)
  $core.bool hasShift() => $_has(9);
  @$pb.TagNumber(10)
  void clearShift() => $_clearField(10);

  @$pb.TagNumber(11)
  $core.bool get meta => $_getBF(10);
  @$pb.TagNumber(11)
  set meta($core.bool value) => $_setBool(10, value);
  @$pb.TagNumber(11)
  $core.bool hasMeta() => $_has(10);
  @$pb.TagNumber(11)
  void clearMeta() => $_clearField(11);
}

/// Key down or up. `key` is a Flutter-style label ("A", "Enter", "Arrow Left").
/// `text` is the produced character when there is one ("a", "A", "@").
/// `hardware_keycode` is the platform scan code when known (X11/GDK keycode on
/// Linux); 0 means the backend should derive it from the keyval.
class KeyEvent extends $pb.GeneratedMessage {
  factory KeyEvent({
    $core.String? phase,
    $core.String? key,
    $core.int? hardwareKeycode,
    $core.bool? ctrl,
    $core.bool? alt,
    $core.bool? shift,
    $core.bool? meta,
    $core.String? text,
    $core.bool? repeat,
  }) {
    final result = KeyEvent._();
    if (phase != null) result.phase = phase;
    if (key != null) result.key = key;
    if (hardwareKeycode != null) result.hardwareKeycode = hardwareKeycode;
    if (ctrl != null) result.ctrl = ctrl;
    if (alt != null) result.alt = alt;
    if (shift != null) result.shift = shift;
    if (meta != null) result.meta = meta;
    if (text != null) result.text = text;
    if (repeat != null) result.repeat = repeat;
    return result;
  }

  KeyEvent._();

  factory KeyEvent.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      KeyEvent()..mergeFromBuffer(data, registry);
  factory KeyEvent.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      KeyEvent()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'KeyEvent',
      package:
          const $pb.PackageName(_omitMessageNames ? '' : 'morph.webview.v1'),
      createEmptyInstance: KeyEvent.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'phase')
    ..aOS(2, _omitFieldNames ? '' : 'key')
    ..aI(3, _omitFieldNames ? '' : 'hardwareKeycode',
        fieldType: $pb.PbFieldType.OU3)
    ..aOB(4, _omitFieldNames ? '' : 'ctrl')
    ..aOB(5, _omitFieldNames ? '' : 'alt')
    ..aOB(6, _omitFieldNames ? '' : 'shift')
    ..aOB(7, _omitFieldNames ? '' : 'meta')
    ..aOS(8, _omitFieldNames ? '' : 'text')
    ..aOB(9, _omitFieldNames ? '' : 'repeat')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  KeyEvent clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  KeyEvent copyWith(void Function(KeyEvent) updates) =>
      super.copyWith((message) => updates(message as KeyEvent)) as KeyEvent;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use KeyEvent() / KeyEvent.new instead')
  static KeyEvent create() => KeyEvent._();
  static $pb.GeneratedMessage $_createMessage() => KeyEvent._();
  @$core.override
  KeyEvent createEmptyInstance() => KeyEvent._();
  @$core.pragma('dart2js:noInline')
  static KeyEvent getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<KeyEvent>(KeyEvent.$_createMessage);
  static KeyEvent? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get phase => $_getSZ(0);
  @$pb.TagNumber(1)
  set phase($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasPhase() => $_has(0);
  @$pb.TagNumber(1)
  void clearPhase() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get key => $_getSZ(1);
  @$pb.TagNumber(2)
  set key($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasKey() => $_has(1);
  @$pb.TagNumber(2)
  void clearKey() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.int get hardwareKeycode => $_getIZ(2);
  @$pb.TagNumber(3)
  set hardwareKeycode($core.int value) => $_setUnsignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasHardwareKeycode() => $_has(2);
  @$pb.TagNumber(3)
  void clearHardwareKeycode() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.bool get ctrl => $_getBF(3);
  @$pb.TagNumber(4)
  set ctrl($core.bool value) => $_setBool(3, value);
  @$pb.TagNumber(4)
  $core.bool hasCtrl() => $_has(3);
  @$pb.TagNumber(4)
  void clearCtrl() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.bool get alt => $_getBF(4);
  @$pb.TagNumber(5)
  set alt($core.bool value) => $_setBool(4, value);
  @$pb.TagNumber(5)
  $core.bool hasAlt() => $_has(4);
  @$pb.TagNumber(5)
  void clearAlt() => $_clearField(5);

  @$pb.TagNumber(6)
  $core.bool get shift => $_getBF(5);
  @$pb.TagNumber(6)
  set shift($core.bool value) => $_setBool(5, value);
  @$pb.TagNumber(6)
  $core.bool hasShift() => $_has(5);
  @$pb.TagNumber(6)
  void clearShift() => $_clearField(6);

  @$pb.TagNumber(7)
  $core.bool get meta => $_getBF(6);
  @$pb.TagNumber(7)
  set meta($core.bool value) => $_setBool(6, value);
  @$pb.TagNumber(7)
  $core.bool hasMeta() => $_has(6);
  @$pb.TagNumber(7)
  void clearMeta() => $_clearField(7);

  @$pb.TagNumber(8)
  $core.String get text => $_getSZ(7);
  @$pb.TagNumber(8)
  set text($core.String value) => $_setString(7, value);
  @$pb.TagNumber(8)
  $core.bool hasText() => $_has(7);
  @$pb.TagNumber(8)
  void clearText() => $_clearField(8);

  @$pb.TagNumber(9)
  $core.bool get repeat => $_getBF(8);
  @$pb.TagNumber(9)
  set repeat($core.bool value) => $_setBool(8, value);
  @$pb.TagNumber(9)
  $core.bool hasRepeat() => $_has(8);
  @$pb.TagNumber(9)
  void clearRepeat() => $_clearField(9);
}

/// IME / compose: compose_start, compose_update, compose_end, commit, focus, blur.
class ImeEvent extends $pb.GeneratedMessage {
  factory ImeEvent({
    $core.String? phase,
    $core.String? text,
  }) {
    final result = ImeEvent._();
    if (phase != null) result.phase = phase;
    if (text != null) result.text = text;
    return result;
  }

  ImeEvent._();

  factory ImeEvent.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ImeEvent()..mergeFromBuffer(data, registry);
  factory ImeEvent.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ImeEvent()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ImeEvent',
      package:
          const $pb.PackageName(_omitMessageNames ? '' : 'morph.webview.v1'),
      createEmptyInstance: ImeEvent.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'phase')
    ..aOS(2, _omitFieldNames ? '' : 'text')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ImeEvent clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ImeEvent copyWith(void Function(ImeEvent) updates) =>
      super.copyWith((message) => updates(message as ImeEvent)) as ImeEvent;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use ImeEvent() / ImeEvent.new instead')
  static ImeEvent create() => ImeEvent._();
  static $pb.GeneratedMessage $_createMessage() => ImeEvent._();
  @$core.override
  ImeEvent createEmptyInstance() => ImeEvent._();
  @$core.pragma('dart2js:noInline')
  static ImeEvent getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ImeEvent>(ImeEvent.$_createMessage);
  static ImeEvent? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get phase => $_getSZ(0);
  @$pb.TagNumber(1)
  set phase($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasPhase() => $_has(0);
  @$pb.TagNumber(1)
  void clearPhase() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get text => $_getSZ(1);
  @$pb.TagNumber(2)
  set text($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasText() => $_has(1);
  @$pb.TagNumber(2)
  void clearText() => $_clearField(2);
}

/// CSS cursor keyword reported by the web view (default, pointer, text, wait, …).
/// macOS and Windows backends must emit this whenever the page cursor changes
/// so the Flutter shell can apply SystemMouseCursors over a texture-hosted view.
class CursorEvent extends $pb.GeneratedMessage {
  factory CursorEvent({
    $core.String? shape,
  }) {
    final result = CursorEvent._();
    if (shape != null) result.shape = shape;
    return result;
  }

  CursorEvent._();

  factory CursorEvent.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      CursorEvent()..mergeFromBuffer(data, registry);
  factory CursorEvent.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      CursorEvent()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'CursorEvent',
      package:
          const $pb.PackageName(_omitMessageNames ? '' : 'morph.webview.v1'),
      createEmptyInstance: CursorEvent.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'shape')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CursorEvent clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CursorEvent copyWith(void Function(CursorEvent) updates) =>
      super.copyWith((message) => updates(message as CursorEvent))
          as CursorEvent;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use CursorEvent() / CursorEvent.new instead')
  static CursorEvent create() => CursorEvent._();
  static $pb.GeneratedMessage $_createMessage() => CursorEvent._();
  @$core.override
  CursorEvent createEmptyInstance() => CursorEvent._();
  @$core.pragma('dart2js:noInline')
  static CursorEvent getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<CursorEvent>(
          CursorEvent.$_createMessage);
  static CursorEvent? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get shape => $_getSZ(0);
  @$pb.TagNumber(1)
  set shape($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasShape() => $_has(0);
  @$pb.TagNumber(1)
  void clearShape() => $_clearField(1);
}

class EvaluateJavaScript extends $pb.GeneratedMessage {
  factory EvaluateJavaScript({
    $fixnum.Int64? requestId,
    $core.String? script,
  }) {
    final result = EvaluateJavaScript._();
    if (requestId != null) result.requestId = requestId;
    if (script != null) result.script = script;
    return result;
  }

  EvaluateJavaScript._();

  factory EvaluateJavaScript.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      EvaluateJavaScript()..mergeFromBuffer(data, registry);
  factory EvaluateJavaScript.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      EvaluateJavaScript()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'EvaluateJavaScript',
      package:
          const $pb.PackageName(_omitMessageNames ? '' : 'morph.webview.v1'),
      createEmptyInstance: EvaluateJavaScript.$_createMessage)
    ..a<$fixnum.Int64>(
        1, _omitFieldNames ? '' : 'requestId', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aOS(2, _omitFieldNames ? '' : 'script')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  EvaluateJavaScript clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  EvaluateJavaScript copyWith(void Function(EvaluateJavaScript) updates) =>
      super.copyWith((message) => updates(message as EvaluateJavaScript))
          as EvaluateJavaScript;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use EvaluateJavaScript() / EvaluateJavaScript.new instead')
  static EvaluateJavaScript create() => EvaluateJavaScript._();
  static $pb.GeneratedMessage $_createMessage() => EvaluateJavaScript._();
  @$core.override
  EvaluateJavaScript createEmptyInstance() => EvaluateJavaScript._();
  @$core.pragma('dart2js:noInline')
  static EvaluateJavaScript getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<EvaluateJavaScript>(
          EvaluateJavaScript.$_createMessage);
  static EvaluateJavaScript? _defaultInstance;

  @$pb.TagNumber(1)
  $fixnum.Int64 get requestId => $_getI64(0);
  @$pb.TagNumber(1)
  set requestId($fixnum.Int64 value) => $_setInt64(0, value);
  @$pb.TagNumber(1)
  $core.bool hasRequestId() => $_has(0);
  @$pb.TagNumber(1)
  void clearRequestId() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get script => $_getSZ(1);
  @$pb.TagNumber(2)
  set script($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasScript() => $_has(1);
  @$pb.TagNumber(2)
  void clearScript() => $_clearField(2);
}

class EvaluateJavaScriptResult extends $pb.GeneratedMessage {
  factory EvaluateJavaScriptResult({
    $fixnum.Int64? requestId,
    $core.String? result,
    $core.String? error,
  }) {
    final result$ = EvaluateJavaScriptResult._();
    if (requestId != null) result$.requestId = requestId;
    if (result != null) result$.result = result;
    if (error != null) result$.error = error;
    return result$;
  }

  EvaluateJavaScriptResult._();

  factory EvaluateJavaScriptResult.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      EvaluateJavaScriptResult()..mergeFromBuffer(data, registry);
  factory EvaluateJavaScriptResult.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      EvaluateJavaScriptResult()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'EvaluateJavaScriptResult',
      package:
          const $pb.PackageName(_omitMessageNames ? '' : 'morph.webview.v1'),
      createEmptyInstance: EvaluateJavaScriptResult.$_createMessage)
    ..a<$fixnum.Int64>(
        1, _omitFieldNames ? '' : 'requestId', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aOS(2, _omitFieldNames ? '' : 'result')
    ..aOS(3, _omitFieldNames ? '' : 'error')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  EvaluateJavaScriptResult clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  EvaluateJavaScriptResult copyWith(
          void Function(EvaluateJavaScriptResult) updates) =>
      super.copyWith((message) => updates(message as EvaluateJavaScriptResult))
          as EvaluateJavaScriptResult;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated(
      'Use EvaluateJavaScriptResult() / EvaluateJavaScriptResult.new instead')
  static EvaluateJavaScriptResult create() => EvaluateJavaScriptResult._();
  static $pb.GeneratedMessage $_createMessage() => EvaluateJavaScriptResult._();
  @$core.override
  EvaluateJavaScriptResult createEmptyInstance() =>
      EvaluateJavaScriptResult._();
  @$core.pragma('dart2js:noInline')
  static EvaluateJavaScriptResult getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<EvaluateJavaScriptResult>(
          EvaluateJavaScriptResult.$_createMessage);
  static EvaluateJavaScriptResult? _defaultInstance;

  @$pb.TagNumber(1)
  $fixnum.Int64 get requestId => $_getI64(0);
  @$pb.TagNumber(1)
  set requestId($fixnum.Int64 value) => $_setInt64(0, value);
  @$pb.TagNumber(1)
  $core.bool hasRequestId() => $_has(0);
  @$pb.TagNumber(1)
  void clearRequestId() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get result => $_getSZ(1);
  @$pb.TagNumber(2)
  set result($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasResult() => $_has(1);
  @$pb.TagNumber(2)
  void clearResult() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get error => $_getSZ(2);
  @$pb.TagNumber(3)
  set error($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasError() => $_has(2);
  @$pb.TagNumber(3)
  void clearError() => $_clearField(3);
}

class PageMessage extends $pb.GeneratedMessage {
  factory PageMessage({
    $core.String? json,
    $core.bool? handled,
  }) {
    final result = PageMessage._();
    if (json != null) result.json = json;
    if (handled != null) result.handled = handled;
    return result;
  }

  PageMessage._();

  factory PageMessage.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      PageMessage()..mergeFromBuffer(data, registry);
  factory PageMessage.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      PageMessage()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PageMessage',
      package:
          const $pb.PackageName(_omitMessageNames ? '' : 'morph.webview.v1'),
      createEmptyInstance: PageMessage.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'json')
    ..aOB(2, _omitFieldNames ? '' : 'handled')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PageMessage clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PageMessage copyWith(void Function(PageMessage) updates) =>
      super.copyWith((message) => updates(message as PageMessage))
          as PageMessage;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use PageMessage() / PageMessage.new instead')
  static PageMessage create() => PageMessage._();
  static $pb.GeneratedMessage $_createMessage() => PageMessage._();
  @$core.override
  PageMessage createEmptyInstance() => PageMessage._();
  @$core.pragma('dart2js:noInline')
  static PageMessage getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<PageMessage>(
          PageMessage.$_createMessage);
  static PageMessage? _defaultInstance;

  /// JSON envelope from or to morph-bridge.js.
  @$pb.TagNumber(1)
  $core.String get json => $_getSZ(0);
  @$pb.TagNumber(1)
  set json($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasJson() => $_has(0);
  @$pb.TagNumber(1)
  void clearJson() => $_clearField(1);

  /// When true, the control layer already ran the Rust hook and delivered any reply to the page.
  @$pb.TagNumber(2)
  $core.bool get handled => $_getBF(1);
  @$pb.TagNumber(2)
  set handled($core.bool value) => $_setBool(1, value);
  @$pb.TagNumber(2)
  $core.bool hasHandled() => $_has(1);
  @$pb.TagNumber(2)
  void clearHandled() => $_clearField(2);
}

class NavigationEvent extends $pb.GeneratedMessage {
  factory NavigationEvent({
    $core.String? url,
    $core.String? title,
    $core.bool? canGoBack,
    $core.bool? canGoForward,
    $core.bool? isLoading,
    $core.double? progress,
  }) {
    final result = NavigationEvent._();
    if (url != null) result.url = url;
    if (title != null) result.title = title;
    if (canGoBack != null) result.canGoBack = canGoBack;
    if (canGoForward != null) result.canGoForward = canGoForward;
    if (isLoading != null) result.isLoading = isLoading;
    if (progress != null) result.progress = progress;
    return result;
  }

  NavigationEvent._();

  factory NavigationEvent.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      NavigationEvent()..mergeFromBuffer(data, registry);
  factory NavigationEvent.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      NavigationEvent()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'NavigationEvent',
      package:
          const $pb.PackageName(_omitMessageNames ? '' : 'morph.webview.v1'),
      createEmptyInstance: NavigationEvent.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'url')
    ..aOS(2, _omitFieldNames ? '' : 'title')
    ..aOB(3, _omitFieldNames ? '' : 'canGoBack')
    ..aOB(4, _omitFieldNames ? '' : 'canGoForward')
    ..aOB(5, _omitFieldNames ? '' : 'isLoading')
    ..aD(6, _omitFieldNames ? '' : 'progress')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  NavigationEvent clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  NavigationEvent copyWith(void Function(NavigationEvent) updates) =>
      super.copyWith((message) => updates(message as NavigationEvent))
          as NavigationEvent;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use NavigationEvent() / NavigationEvent.new instead')
  static NavigationEvent create() => NavigationEvent._();
  static $pb.GeneratedMessage $_createMessage() => NavigationEvent._();
  @$core.override
  NavigationEvent createEmptyInstance() => NavigationEvent._();
  @$core.pragma('dart2js:noInline')
  static NavigationEvent getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<NavigationEvent>(
          NavigationEvent.$_createMessage);
  static NavigationEvent? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get url => $_getSZ(0);
  @$pb.TagNumber(1)
  set url($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasUrl() => $_has(0);
  @$pb.TagNumber(1)
  void clearUrl() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get title => $_getSZ(1);
  @$pb.TagNumber(2)
  set title($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasTitle() => $_has(1);
  @$pb.TagNumber(2)
  void clearTitle() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.bool get canGoBack => $_getBF(2);
  @$pb.TagNumber(3)
  set canGoBack($core.bool value) => $_setBool(2, value);
  @$pb.TagNumber(3)
  $core.bool hasCanGoBack() => $_has(2);
  @$pb.TagNumber(3)
  void clearCanGoBack() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.bool get canGoForward => $_getBF(3);
  @$pb.TagNumber(4)
  set canGoForward($core.bool value) => $_setBool(3, value);
  @$pb.TagNumber(4)
  $core.bool hasCanGoForward() => $_has(3);
  @$pb.TagNumber(4)
  void clearCanGoForward() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.bool get isLoading => $_getBF(4);
  @$pb.TagNumber(5)
  set isLoading($core.bool value) => $_setBool(4, value);
  @$pb.TagNumber(5)
  $core.bool hasIsLoading() => $_has(4);
  @$pb.TagNumber(5)
  void clearIsLoading() => $_clearField(5);

  @$pb.TagNumber(6)
  $core.double get progress => $_getN(5);
  @$pb.TagNumber(6)
  set progress($core.double value) => $_setDouble(5, value);
  @$pb.TagNumber(6)
  $core.bool hasProgress() => $_has(5);
  @$pb.TagNumber(6)
  void clearProgress() => $_clearField(6);
}

class Ack extends $pb.GeneratedMessage {
  factory Ack({
    $fixnum.Int64? clientSeq,
  }) {
    final result = Ack._();
    if (clientSeq != null) result.clientSeq = clientSeq;
    return result;
  }

  Ack._();

  factory Ack.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      Ack()..mergeFromBuffer(data, registry);
  factory Ack.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      Ack()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'Ack',
      package:
          const $pb.PackageName(_omitMessageNames ? '' : 'morph.webview.v1'),
      createEmptyInstance: Ack.$_createMessage)
    ..a<$fixnum.Int64>(
        1, _omitFieldNames ? '' : 'clientSeq', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Ack clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Ack copyWith(void Function(Ack) updates) =>
      super.copyWith((message) => updates(message as Ack)) as Ack;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use Ack() / Ack.new instead')
  static Ack create() => Ack._();
  static $pb.GeneratedMessage $_createMessage() => Ack._();
  @$core.override
  Ack createEmptyInstance() => Ack._();
  @$core.pragma('dart2js:noInline')
  static Ack getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<Ack>(Ack.$_createMessage);
  static Ack? _defaultInstance;

  @$pb.TagNumber(1)
  $fixnum.Int64 get clientSeq => $_getI64(0);
  @$pb.TagNumber(1)
  set clientSeq($fixnum.Int64 value) => $_setInt64(0, value);
  @$pb.TagNumber(1)
  $core.bool hasClientSeq() => $_has(0);
  @$pb.TagNumber(1)
  void clearClientSeq() => $_clearField(1);
}

class Error extends $pb.GeneratedMessage {
  factory Error({
    $fixnum.Int64? clientSeq,
    $core.String? message,
  }) {
    final result = Error._();
    if (clientSeq != null) result.clientSeq = clientSeq;
    if (message != null) result.message = message;
    return result;
  }

  Error._();

  factory Error.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      Error()..mergeFromBuffer(data, registry);
  factory Error.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      Error()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'Error',
      package:
          const $pb.PackageName(_omitMessageNames ? '' : 'morph.webview.v1'),
      createEmptyInstance: Error.$_createMessage)
    ..a<$fixnum.Int64>(
        1, _omitFieldNames ? '' : 'clientSeq', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aOS(2, _omitFieldNames ? '' : 'message')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Error clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Error copyWith(void Function(Error) updates) =>
      super.copyWith((message) => updates(message as Error)) as Error;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use Error() / Error.new instead')
  static Error create() => Error._();
  static $pb.GeneratedMessage $_createMessage() => Error._();
  @$core.override
  Error createEmptyInstance() => Error._();
  @$core.pragma('dart2js:noInline')
  static Error getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<Error>(Error.$_createMessage);
  static Error? _defaultInstance;

  @$pb.TagNumber(1)
  $fixnum.Int64 get clientSeq => $_getI64(0);
  @$pb.TagNumber(1)
  set clientSeq($fixnum.Int64 value) => $_setInt64(0, value);
  @$pb.TagNumber(1)
  $core.bool hasClientSeq() => $_has(0);
  @$pb.TagNumber(1)
  void clearClientSeq() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get message => $_getSZ(1);
  @$pb.TagNumber(2)
  set message($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasMessage() => $_has(1);
  @$pb.TagNumber(2)
  void clearMessage() => $_clearField(2);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
