// This is a generated file - do not edit.
//
// Generated from morph.webview.v1.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports
// ignore_for_file: unused_import

import 'dart:convert' as $convert;
import 'dart:core' as $core;
import 'dart:typed_data' as $typed_data;

@$core.Deprecated('Use clientMessageDescriptor instead')
const ClientMessage$json = {
  '1': 'ClientMessage',
  '2': [
    {'1': 'seq', '3': 1, '4': 1, '5': 4, '10': 'seq'},
    {'1': 'view_id', '3': 2, '4': 1, '5': 3, '10': 'viewId'},
    {
      '1': 'navigate',
      '3': 3,
      '4': 1,
      '5': 11,
      '6': '.morph.webview.v1.Navigate',
      '9': 0,
      '10': 'navigate'
    },
    {
      '1': 'go_back',
      '3': 4,
      '4': 1,
      '5': 11,
      '6': '.morph.webview.v1.GoBack',
      '9': 0,
      '10': 'goBack'
    },
    {
      '1': 'go_forward',
      '3': 5,
      '4': 1,
      '5': 11,
      '6': '.morph.webview.v1.GoForward',
      '9': 0,
      '10': 'goForward'
    },
    {
      '1': 'reload',
      '3': 6,
      '4': 1,
      '5': 11,
      '6': '.morph.webview.v1.Reload',
      '9': 0,
      '10': 'reload'
    },
    {
      '1': 'pointer',
      '3': 7,
      '4': 1,
      '5': 11,
      '6': '.morph.webview.v1.PointerEvent',
      '9': 0,
      '10': 'pointer'
    },
    {
      '1': 'key',
      '3': 8,
      '4': 1,
      '5': 11,
      '6': '.morph.webview.v1.KeyEvent',
      '9': 0,
      '10': 'key'
    },
    {
      '1': 'ime',
      '3': 9,
      '4': 1,
      '5': 11,
      '6': '.morph.webview.v1.ImeEvent',
      '9': 0,
      '10': 'ime'
    },
    {
      '1': 'evaluate_javascript',
      '3': 10,
      '4': 1,
      '5': 11,
      '6': '.morph.webview.v1.EvaluateJavaScript',
      '9': 0,
      '10': 'evaluateJavascript'
    },
    {
      '1': 'page_message',
      '3': 11,
      '4': 1,
      '5': 11,
      '6': '.morph.webview.v1.PageMessage',
      '9': 0,
      '10': 'pageMessage'
    },
  ],
  '8': [
    {'1': 'body'},
  ],
};

/// Descriptor for `ClientMessage`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List clientMessageDescriptor = $convert.base64Decode(
    'Cg1DbGllbnRNZXNzYWdlEhAKA3NlcRgBIAEoBFIDc2VxEhcKB3ZpZXdfaWQYAiABKANSBnZpZX'
    'dJZBI4CghuYXZpZ2F0ZRgDIAEoCzIaLm1vcnBoLndlYnZpZXcudjEuTmF2aWdhdGVIAFIIbmF2'
    'aWdhdGUSMwoHZ29fYmFjaxgEIAEoCzIYLm1vcnBoLndlYnZpZXcudjEuR29CYWNrSABSBmdvQm'
    'FjaxI8Cgpnb19mb3J3YXJkGAUgASgLMhsubW9ycGgud2Vidmlldy52MS5Hb0ZvcndhcmRIAFIJ'
    'Z29Gb3J3YXJkEjIKBnJlbG9hZBgGIAEoCzIYLm1vcnBoLndlYnZpZXcudjEuUmVsb2FkSABSBn'
    'JlbG9hZBI6Cgdwb2ludGVyGAcgASgLMh4ubW9ycGgud2Vidmlldy52MS5Qb2ludGVyRXZlbnRI'
    'AFIHcG9pbnRlchIuCgNrZXkYCCABKAsyGi5tb3JwaC53ZWJ2aWV3LnYxLktleUV2ZW50SABSA2'
    'tleRIuCgNpbWUYCSABKAsyGi5tb3JwaC53ZWJ2aWV3LnYxLkltZUV2ZW50SABSA2ltZRJXChNl'
    'dmFsdWF0ZV9qYXZhc2NyaXB0GAogASgLMiQubW9ycGgud2Vidmlldy52MS5FdmFsdWF0ZUphdm'
    'FTY3JpcHRIAFISZXZhbHVhdGVKYXZhc2NyaXB0EkIKDHBhZ2VfbWVzc2FnZRgLIAEoCzIdLm1v'
    'cnBoLndlYnZpZXcudjEuUGFnZU1lc3NhZ2VIAFILcGFnZU1lc3NhZ2VCBgoEYm9keQ==');

@$core.Deprecated('Use serverMessageDescriptor instead')
const ServerMessage$json = {
  '1': 'ServerMessage',
  '2': [
    {'1': 'seq', '3': 1, '4': 1, '5': 4, '10': 'seq'},
    {'1': 'view_id', '3': 2, '4': 1, '5': 3, '10': 'viewId'},
    {
      '1': 'navigation',
      '3': 3,
      '4': 1,
      '5': 11,
      '6': '.morph.webview.v1.NavigationEvent',
      '9': 0,
      '10': 'navigation'
    },
    {
      '1': 'page_message',
      '3': 4,
      '4': 1,
      '5': 11,
      '6': '.morph.webview.v1.PageMessage',
      '9': 0,
      '10': 'pageMessage'
    },
    {
      '1': 'evaluate_javascript_result',
      '3': 5,
      '4': 1,
      '5': 11,
      '6': '.morph.webview.v1.EvaluateJavaScriptResult',
      '9': 0,
      '10': 'evaluateJavascriptResult'
    },
    {
      '1': 'ack',
      '3': 6,
      '4': 1,
      '5': 11,
      '6': '.morph.webview.v1.Ack',
      '9': 0,
      '10': 'ack'
    },
    {
      '1': 'error',
      '3': 7,
      '4': 1,
      '5': 11,
      '6': '.morph.webview.v1.Error',
      '9': 0,
      '10': 'error'
    },
    {
      '1': 'cursor',
      '3': 8,
      '4': 1,
      '5': 11,
      '6': '.morph.webview.v1.CursorEvent',
      '9': 0,
      '10': 'cursor'
    },
  ],
  '8': [
    {'1': 'body'},
  ],
};

/// Descriptor for `ServerMessage`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List serverMessageDescriptor = $convert.base64Decode(
    'Cg1TZXJ2ZXJNZXNzYWdlEhAKA3NlcRgBIAEoBFIDc2VxEhcKB3ZpZXdfaWQYAiABKANSBnZpZX'
    'dJZBJDCgpuYXZpZ2F0aW9uGAMgASgLMiEubW9ycGgud2Vidmlldy52MS5OYXZpZ2F0aW9uRXZl'
    'bnRIAFIKbmF2aWdhdGlvbhJCCgxwYWdlX21lc3NhZ2UYBCABKAsyHS5tb3JwaC53ZWJ2aWV3Ln'
    'YxLlBhZ2VNZXNzYWdlSABSC3BhZ2VNZXNzYWdlEmoKGmV2YWx1YXRlX2phdmFzY3JpcHRfcmVz'
    'dWx0GAUgASgLMioubW9ycGgud2Vidmlldy52MS5FdmFsdWF0ZUphdmFTY3JpcHRSZXN1bHRIAF'
    'IYZXZhbHVhdGVKYXZhc2NyaXB0UmVzdWx0EikKA2FjaxgGIAEoCzIVLm1vcnBoLndlYnZpZXcu'
    'djEuQWNrSABSA2FjaxIvCgVlcnJvchgHIAEoCzIXLm1vcnBoLndlYnZpZXcudjEuRXJyb3JIAF'
    'IFZXJyb3ISNwoGY3Vyc29yGAggASgLMh0ubW9ycGgud2Vidmlldy52MS5DdXJzb3JFdmVudEgA'
    'UgZjdXJzb3JCBgoEYm9keQ==');

@$core.Deprecated('Use navigateDescriptor instead')
const Navigate$json = {
  '1': 'Navigate',
  '2': [
    {'1': 'url', '3': 1, '4': 1, '5': 9, '10': 'url'},
  ],
};

/// Descriptor for `Navigate`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List navigateDescriptor =
    $convert.base64Decode('CghOYXZpZ2F0ZRIQCgN1cmwYASABKAlSA3VybA==');

@$core.Deprecated('Use goBackDescriptor instead')
const GoBack$json = {
  '1': 'GoBack',
};

/// Descriptor for `GoBack`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List goBackDescriptor =
    $convert.base64Decode('CgZHb0JhY2s=');

@$core.Deprecated('Use goForwardDescriptor instead')
const GoForward$json = {
  '1': 'GoForward',
};

/// Descriptor for `GoForward`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List goForwardDescriptor =
    $convert.base64Decode('CglHb0ZvcndhcmQ=');

@$core.Deprecated('Use reloadDescriptor instead')
const Reload$json = {
  '1': 'Reload',
};

/// Descriptor for `Reload`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reloadDescriptor =
    $convert.base64Decode('CgZSZWxvYWQ=');

@$core.Deprecated('Use pointerEventDescriptor instead')
const PointerEvent$json = {
  '1': 'PointerEvent',
  '2': [
    {'1': 'phase', '3': 1, '4': 1, '5': 9, '10': 'phase'},
    {'1': 'x', '3': 2, '4': 1, '5': 1, '10': 'x'},
    {'1': 'y', '3': 3, '4': 1, '5': 1, '10': 'y'},
    {'1': 'button', '3': 4, '4': 1, '5': 5, '10': 'button'},
    {'1': 'scroll_delta_x', '3': 5, '4': 1, '5': 1, '10': 'scrollDeltaX'},
    {'1': 'scroll_delta_y', '3': 6, '4': 1, '5': 1, '10': 'scrollDeltaY'},
    {'1': 'buttons', '3': 7, '4': 1, '5': 5, '10': 'buttons'},
    {'1': 'ctrl', '3': 8, '4': 1, '5': 8, '10': 'ctrl'},
    {'1': 'alt', '3': 9, '4': 1, '5': 8, '10': 'alt'},
    {'1': 'shift', '3': 10, '4': 1, '5': 8, '10': 'shift'},
    {'1': 'meta', '3': 11, '4': 1, '5': 8, '10': 'meta'},
  ],
};

/// Descriptor for `PointerEvent`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List pointerEventDescriptor = $convert.base64Decode(
    'CgxQb2ludGVyRXZlbnQSFAoFcGhhc2UYASABKAlSBXBoYXNlEgwKAXgYAiABKAFSAXgSDAoBeR'
    'gDIAEoAVIBeRIWCgZidXR0b24YBCABKAVSBmJ1dHRvbhIkCg5zY3JvbGxfZGVsdGFfeBgFIAEo'
    'AVIMc2Nyb2xsRGVsdGFYEiQKDnNjcm9sbF9kZWx0YV95GAYgASgBUgxzY3JvbGxEZWx0YVkSGA'
    'oHYnV0dG9ucxgHIAEoBVIHYnV0dG9ucxISCgRjdHJsGAggASgIUgRjdHJsEhAKA2FsdBgJIAEo'
    'CFIDYWx0EhQKBXNoaWZ0GAogASgIUgVzaGlmdBISCgRtZXRhGAsgASgIUgRtZXRh');

@$core.Deprecated('Use keyEventDescriptor instead')
const KeyEvent$json = {
  '1': 'KeyEvent',
  '2': [
    {'1': 'phase', '3': 1, '4': 1, '5': 9, '10': 'phase'},
    {'1': 'key', '3': 2, '4': 1, '5': 9, '10': 'key'},
    {'1': 'hardware_keycode', '3': 3, '4': 1, '5': 13, '10': 'hardwareKeycode'},
    {'1': 'ctrl', '3': 4, '4': 1, '5': 8, '10': 'ctrl'},
    {'1': 'alt', '3': 5, '4': 1, '5': 8, '10': 'alt'},
    {'1': 'shift', '3': 6, '4': 1, '5': 8, '10': 'shift'},
    {'1': 'meta', '3': 7, '4': 1, '5': 8, '10': 'meta'},
    {'1': 'text', '3': 8, '4': 1, '5': 9, '10': 'text'},
    {'1': 'repeat', '3': 9, '4': 1, '5': 8, '10': 'repeat'},
  ],
};

/// Descriptor for `KeyEvent`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List keyEventDescriptor = $convert.base64Decode(
    'CghLZXlFdmVudBIUCgVwaGFzZRgBIAEoCVIFcGhhc2USEAoDa2V5GAIgASgJUgNrZXkSKQoQaG'
    'FyZHdhcmVfa2V5Y29kZRgDIAEoDVIPaGFyZHdhcmVLZXljb2RlEhIKBGN0cmwYBCABKAhSBGN0'
    'cmwSEAoDYWx0GAUgASgIUgNhbHQSFAoFc2hpZnQYBiABKAhSBXNoaWZ0EhIKBG1ldGEYByABKA'
    'hSBG1ldGESEgoEdGV4dBgIIAEoCVIEdGV4dBIWCgZyZXBlYXQYCSABKAhSBnJlcGVhdA==');

@$core.Deprecated('Use imeEventDescriptor instead')
const ImeEvent$json = {
  '1': 'ImeEvent',
  '2': [
    {'1': 'phase', '3': 1, '4': 1, '5': 9, '10': 'phase'},
    {'1': 'text', '3': 2, '4': 1, '5': 9, '10': 'text'},
  ],
};

/// Descriptor for `ImeEvent`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List imeEventDescriptor = $convert.base64Decode(
    'CghJbWVFdmVudBIUCgVwaGFzZRgBIAEoCVIFcGhhc2USEgoEdGV4dBgCIAEoCVIEdGV4dA==');

@$core.Deprecated('Use cursorEventDescriptor instead')
const CursorEvent$json = {
  '1': 'CursorEvent',
  '2': [
    {'1': 'shape', '3': 1, '4': 1, '5': 9, '10': 'shape'},
  ],
};

/// Descriptor for `CursorEvent`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List cursorEventDescriptor =
    $convert.base64Decode('CgtDdXJzb3JFdmVudBIUCgVzaGFwZRgBIAEoCVIFc2hhcGU=');

@$core.Deprecated('Use evaluateJavaScriptDescriptor instead')
const EvaluateJavaScript$json = {
  '1': 'EvaluateJavaScript',
  '2': [
    {'1': 'request_id', '3': 1, '4': 1, '5': 4, '10': 'requestId'},
    {'1': 'script', '3': 2, '4': 1, '5': 9, '10': 'script'},
  ],
};

/// Descriptor for `EvaluateJavaScript`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List evaluateJavaScriptDescriptor = $convert.base64Decode(
    'ChJFdmFsdWF0ZUphdmFTY3JpcHQSHQoKcmVxdWVzdF9pZBgBIAEoBFIJcmVxdWVzdElkEhYKBn'
    'NjcmlwdBgCIAEoCVIGc2NyaXB0');

@$core.Deprecated('Use evaluateJavaScriptResultDescriptor instead')
const EvaluateJavaScriptResult$json = {
  '1': 'EvaluateJavaScriptResult',
  '2': [
    {'1': 'request_id', '3': 1, '4': 1, '5': 4, '10': 'requestId'},
    {'1': 'result', '3': 2, '4': 1, '5': 9, '10': 'result'},
    {'1': 'error', '3': 3, '4': 1, '5': 9, '10': 'error'},
  ],
};

/// Descriptor for `EvaluateJavaScriptResult`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List evaluateJavaScriptResultDescriptor =
    $convert.base64Decode(
        'ChhFdmFsdWF0ZUphdmFTY3JpcHRSZXN1bHQSHQoKcmVxdWVzdF9pZBgBIAEoBFIJcmVxdWVzdE'
        'lkEhYKBnJlc3VsdBgCIAEoCVIGcmVzdWx0EhQKBWVycm9yGAMgASgJUgVlcnJvcg==');

@$core.Deprecated('Use pageMessageDescriptor instead')
const PageMessage$json = {
  '1': 'PageMessage',
  '2': [
    {'1': 'json', '3': 1, '4': 1, '5': 9, '10': 'json'},
    {'1': 'handled', '3': 2, '4': 1, '5': 8, '10': 'handled'},
  ],
};

/// Descriptor for `PageMessage`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List pageMessageDescriptor = $convert.base64Decode(
    'CgtQYWdlTWVzc2FnZRISCgRqc29uGAEgASgJUgRqc29uEhgKB2hhbmRsZWQYAiABKAhSB2hhbm'
    'RsZWQ=');

@$core.Deprecated('Use navigationEventDescriptor instead')
const NavigationEvent$json = {
  '1': 'NavigationEvent',
  '2': [
    {'1': 'url', '3': 1, '4': 1, '5': 9, '10': 'url'},
    {'1': 'title', '3': 2, '4': 1, '5': 9, '10': 'title'},
    {'1': 'can_go_back', '3': 3, '4': 1, '5': 8, '10': 'canGoBack'},
    {'1': 'can_go_forward', '3': 4, '4': 1, '5': 8, '10': 'canGoForward'},
    {'1': 'is_loading', '3': 5, '4': 1, '5': 8, '10': 'isLoading'},
    {'1': 'progress', '3': 6, '4': 1, '5': 1, '10': 'progress'},
  ],
};

/// Descriptor for `NavigationEvent`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List navigationEventDescriptor = $convert.base64Decode(
    'Cg9OYXZpZ2F0aW9uRXZlbnQSEAoDdXJsGAEgASgJUgN1cmwSFAoFdGl0bGUYAiABKAlSBXRpdG'
    'xlEh4KC2Nhbl9nb19iYWNrGAMgASgIUgljYW5Hb0JhY2sSJAoOY2FuX2dvX2ZvcndhcmQYBCAB'
    'KAhSDGNhbkdvRm9yd2FyZBIdCgppc19sb2FkaW5nGAUgASgIUglpc0xvYWRpbmcSGgoIcHJvZ3'
    'Jlc3MYBiABKAFSCHByb2dyZXNz');

@$core.Deprecated('Use ackDescriptor instead')
const Ack$json = {
  '1': 'Ack',
  '2': [
    {'1': 'client_seq', '3': 1, '4': 1, '5': 4, '10': 'clientSeq'},
  ],
};

/// Descriptor for `Ack`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List ackDescriptor =
    $convert.base64Decode('CgNBY2sSHQoKY2xpZW50X3NlcRgBIAEoBFIJY2xpZW50U2Vx');

@$core.Deprecated('Use errorDescriptor instead')
const Error$json = {
  '1': 'Error',
  '2': [
    {'1': 'client_seq', '3': 1, '4': 1, '5': 4, '10': 'clientSeq'},
    {'1': 'message', '3': 2, '4': 1, '5': 9, '10': 'message'},
  ],
};

/// Descriptor for `Error`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List errorDescriptor = $convert.base64Decode(
    'CgVFcnJvchIdCgpjbGllbnRfc2VxGAEgASgEUgljbGllbnRTZXESGAoHbWVzc2FnZRgCIAEoCV'
    'IHbWVzc2FnZQ==');
