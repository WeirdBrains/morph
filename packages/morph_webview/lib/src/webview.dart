import 'dart:async';

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';

import 'controller.dart';
import 'events.dart';

/// Renders a platform web-view texture and routes Wayland-first input to the backend.
class MorphWebView extends StatefulWidget {
  const MorphWebView({
    super.key,
    required this.controller,
    this.userScript = '',
    this.profileId = 'default',
    this.placeholder,
  });

  final MorphWebViewController controller;
  final String userScript;

  /// Chrome-style profile that owns this view's cookies and website data.
  /// Defaults to the catalog's `default` profile.
  final String profileId;

  /// Shown in widget tests or before the texture id is available.
  final Widget? placeholder;

  @override
  State<MorphWebView> createState() => _MorphWebViewState();
}

class _MorphWebViewState extends State<MorphWebView>
    implements DeltaTextInputClient {
  static const Key surfaceKey = Key('morph-webview-surface');

  Size? _lastSize;
  double? _lastDpr;
  Object? _error;
  MouseCursor _cursor = SystemMouseCursors.basic;
  StreamSubscription<MorphCursorEvent>? _cursorSub;
  MorphPointerEvent? _pendingMove;
  bool _moveScheduled = false;
  int _pressedButtons = 0;
  TextInputConnection? _input;
  TextEditingValue _editingValue = TextEditingValue.empty;
  bool _composing = false;

  @override
  void initState() {
    super.initState();
    widget.controller.focusNode.addListener(_onFocusChange);
    widget.controller.cursorShape.addListener(_onCursorShape);
    _listenCursor();
  }

  @override
  void didUpdateWidget(MorphWebView oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.controller != widget.controller) {
      oldWidget.controller.focusNode.removeListener(_onFocusChange);
      oldWidget.controller.cursorShape.removeListener(_onCursorShape);
      widget.controller.focusNode.addListener(_onFocusChange);
      widget.controller.cursorShape.addListener(_onCursorShape);
      _onCursorShape();
      _listenCursor();
    }
  }

  void _onCursorShape() {
    final next = morphCursorForShape(widget.controller.cursorShape.value);
    if (mounted && next != _cursor) {
      setState(() => _cursor = next);
    }
  }

  @override
  void dispose() {
    widget.controller.focusNode.removeListener(_onFocusChange);
    widget.controller.cursorShape.removeListener(_onCursorShape);
    unawaited(_cursorSub?.cancel());
    _flushMove();
    _closeInput();
    super.dispose();
  }

  void _listenCursor() {
    unawaited(_cursorSub?.cancel());
    _cursorSub = widget.controller.cursor.listen((event) {
      if (!mounted) {
        return;
      }
      final next = morphCursorForShape(event.shape);
      if (next != _cursor) {
        setState(() => _cursor = next);
      }
    });
  }

  Future<void> _ensureAttached(BoxConstraints constraints, double dpr) async {
    final width = constraints.maxWidth.isFinite
        ? constraints.maxWidth.floor().clamp(1, 8192)
        : 800;
    final height = constraints.maxHeight.isFinite
        ? constraints.maxHeight.floor().clamp(1, 8192)
        : 600;
    final size = Size(width.toDouble(), height.toDouble());
    if (_lastSize == size && _lastDpr == dpr && widget.controller.isAttached) {
      return;
    }
    _lastSize = size;
    _lastDpr = dpr;
    try {
      await widget.controller.attach(
        width: width,
        height: height,
        devicePixelRatio: dpr,
        userScript: widget.userScript,
        profileId: widget.profileId,
      );
      if (mounted) {
        setState(() => _error = null);
        _listenCursor();
      }
    } catch (e) {
      if (mounted) {
        setState(() => _error = e);
      }
    }
  }

  void _onFocusChange() {
    if (widget.controller.focusNode.hasFocus) {
      _openInput();
      widget.controller.sendIme(const MorphImeEvent(phase: 'focus'));
    } else {
      if (_composing) {
        widget.controller.sendIme(const MorphImeEvent(phase: 'compose_end'));
        _composing = false;
      }
      widget.controller.sendIme(const MorphImeEvent(phase: 'blur'));
      _closeInput();
    }
  }

  void _openInput() {
    if (_input != null && _input!.attached) {
      _input!.show();
      return;
    }
    _editingValue = TextEditingValue.empty;
    _input = TextInput.attach(
      this,
      const TextInputConfiguration(
        inputType: TextInputType.multiline,
        inputAction: TextInputAction.newline,
        autocorrect: false,
        enableSuggestions: false,
        keyboardAppearance: Brightness.dark,
        enableDeltaModel: true,
      ),
    );
    _input!.setEditingState(_editingValue);
    _input!.show();
  }

  void _closeInput() {
    _input?.close();
    _input = null;
    _composing = false;
    _editingValue = TextEditingValue.empty;
  }

  void _sendPointer(MorphPointerEvent event) {
    if (event.phase == 'hover' || event.phase == 'move') {
      _pendingMove = event;
      if (!_moveScheduled) {
        _moveScheduled = true;
        SchedulerBinding.instance.scheduleFrameCallback((_) {
          _moveScheduled = false;
          final pending = _pendingMove;
          _pendingMove = null;
          if (pending != null) {
            widget.controller.sendPointer(pending);
          }
        });
      }
      return;
    }
    _flushMove();
    widget.controller.sendPointer(event);
  }

  void _flushMove() {
    final pending = _pendingMove;
    _pendingMove = null;
    if (pending != null) {
      widget.controller.sendPointer(pending);
    }
  }

  MorphPointerEvent _buildPointer(
    Offset local, {
    required String phase,
    int button = 0,
    int buttons = 0,
    double scrollDeltaX = 0,
    double scrollDeltaY = 0,
  }) {
    return MorphPointerEvent(
      phase: phase,
      x: local.dx,
      y: local.dy,
      button: button,
      buttons: buttons,
      scrollDeltaX: scrollDeltaX,
      scrollDeltaY: scrollDeltaY,
      ctrl: morphModifierPressed(
        left: LogicalKeyboardKey.controlLeft,
        right: LogicalKeyboardKey.controlRight,
      ),
      alt: morphModifierPressed(
        left: LogicalKeyboardKey.altLeft,
        right: LogicalKeyboardKey.altRight,
      ),
      shift: morphModifierPressed(
        left: LogicalKeyboardKey.shiftLeft,
        right: LogicalKeyboardKey.shiftRight,
      ),
      meta: morphModifierPressed(
        left: LogicalKeyboardKey.metaLeft,
        right: LogicalKeyboardKey.metaRight,
      ),
    );
  }

  void _onPointerDown(PointerDownEvent event) {
    widget.controller.focusNode.requestFocus();
    final buttons = morphButtonsFromFlutter(event.buttons);
    final button = morphButtonFromMask(buttons & ~_pressedButtons);
    _pressedButtons = buttons;
    _sendPointer(
      _buildPointer(
        event.localPosition,
        phase: 'down',
        button: button == 0 ? 1 : button,
        buttons: buttons,
      ),
    );
  }

  void _onPointerMove(PointerMoveEvent event) {
    final buttons = morphButtonsFromFlutter(event.buttons);
    _pressedButtons = buttons;
    _sendPointer(
      _buildPointer(event.localPosition, phase: 'move', buttons: buttons),
    );
  }

  void _onPointerUp(PointerUpEvent event) {
    final buttons = morphButtonsFromFlutter(event.buttons);
    final button = morphButtonFromMask(_pressedButtons & ~buttons);
    _pressedButtons = buttons;
    _sendPointer(
      _buildPointer(
        event.localPosition,
        phase: 'up',
        button: button == 0 ? 1 : button,
        buttons: buttons,
      ),
    );
  }

  void _onPointerCancel(PointerCancelEvent event) {
    final buttons = morphButtonsFromFlutter(event.buttons);
    final button = morphButtonFromMask(_pressedButtons & ~buttons);
    _pressedButtons = buttons;
    _sendPointer(
      _buildPointer(
        event.localPosition,
        phase: 'cancel',
        button: button,
        buttons: buttons,
      ),
    );
  }

  void _onPointerHover(PointerHoverEvent event) {
    _sendPointer(
      _buildPointer(event.localPosition, phase: 'hover', buttons: 0),
    );
  }

  void _onEnter(PointerEnterEvent event) {
    _sendPointer(
      _buildPointer(event.localPosition, phase: 'enter', buttons: 0),
    );
  }

  void _onExit(PointerExitEvent event) {
    _pressedButtons = 0;
    _sendPointer(
      _buildPointer(event.localPosition, phase: 'leave', buttons: 0),
    );
    if (widget.controller.cursorShape.value != 'default') {
      widget.controller.cursorShape.value = 'default';
    }
  }

  void _onPointerSignal(PointerSignalEvent event) {
    if (event is PointerScrollEvent) {
      _sendPointer(
        _buildPointer(
          event.localPosition,
          phase: 'scroll',
          buttons: _pressedButtons,
          scrollDeltaX: event.scrollDelta.dx,
          scrollDeltaY: event.scrollDelta.dy,
        ),
      );
    }
  }

  KeyEventResult _onKey(FocusNode node, KeyEvent event) {
    final phase = event is KeyUpEvent ? 'up' : 'down';
    final repeat = event is KeyRepeatEvent;
    // While IME preedit is active, printable keys are delivered as compose
    // updates. Still forward non-character keys (Backspace, arrows, Enter).
    final character = morphKeyText(event);
    final isPrintable = character.isNotEmpty;
    if (_composing && isPrintable && event is! KeyUpEvent) {
      return KeyEventResult.handled;
    }
    widget.controller.sendKey(
      MorphKeyEvent(
        phase: phase,
        key: morphKeyName(event.logicalKey),
        hardwareKeycode: event.physicalKey.usbHidUsage,
        ctrl: morphModifierPressed(
          left: LogicalKeyboardKey.controlLeft,
          right: LogicalKeyboardKey.controlRight,
        ),
        alt: morphModifierPressed(
          left: LogicalKeyboardKey.altLeft,
          right: LogicalKeyboardKey.altRight,
        ),
        shift: morphModifierPressed(
          left: LogicalKeyboardKey.shiftLeft,
          right: LogicalKeyboardKey.shiftRight,
        ),
        meta: morphModifierPressed(
          left: LogicalKeyboardKey.metaLeft,
          right: LogicalKeyboardKey.metaRight,
        ),
        text: character,
        repeat: repeat,
      ),
    );
    return KeyEventResult.handled;
  }

  @override
  Widget build(BuildContext context) {
    return ValueListenableBuilder<List<MorphWebViewController>>(
      valueListenable: widget.controller.popups,
      builder: (context, popups, _) {
        final page = _page(context);
        if (popups.isEmpty) {
          return page;
        }
        return Stack(
          fit: StackFit.expand,
          children: [
            page,
            for (final popup in popups)
              MorphPopupLayer(
                key: ObjectKey(popup),
                controller: popup,
                userScript: widget.userScript,
                profileId: widget.profileId,
                onClose: () => unawaited(widget.controller.closePopup(popup)),
              ),
          ],
        );
      },
    );
  }

  Widget _page(BuildContext context) {
    final dpr = MediaQuery.devicePixelRatioOf(context);
    return LayoutBuilder(
      builder: (context, constraints) {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) {
            unawaitedAttach(constraints, dpr);
          }
        });
        final textureId = widget.controller.textureId;
        final platformView = widget.controller.platformView;
        Widget child;
        if (_error != null) {
          child = Center(child: Text('Web view error: $_error'));
        } else if (platformView != null) {
          // Platform views handle pointer, keyboard, IME, and cursor natively.
          child = platformView;
        } else if (textureId != null) {
          child = Focus(
            focusNode: widget.controller.focusNode,
            onKeyEvent: _onKey,
            includeSemantics: false,
            child: ValueListenableBuilder<String>(
              valueListenable: widget.controller.cursorShape,
              builder: (context, shape, child) {
                return MouseRegion(
                  key: surfaceKey,
                  cursor: morphCursorForShape(shape),
                  onEnter: _onEnter,
                  onExit: _onExit,
                  onHover: _onPointerHover,
                  child: Listener(
                    onPointerDown: _onPointerDown,
                    onPointerMove: _onPointerMove,
                    onPointerUp: _onPointerUp,
                    onPointerCancel: _onPointerCancel,
                    onPointerSignal: _onPointerSignal,
                    child: Texture(textureId: textureId),
                  ),
                );
              },
            ),
          );
        } else {
          child =
              widget.placeholder ??
              const Center(child: CircularProgressIndicator());
        }
        return child;
      },
    );
  }

  void unawaitedAttach(BoxConstraints constraints, double dpr) {
    // ignore: discarded_futures
    _ensureAttached(constraints, dpr);
  }

  // --- TextInputClient (IME / compose / paste of committed text) ---

  @override
  AutofillScope? get currentAutofillScope => null;

  @override
  TextEditingValue get currentTextEditingValue => _editingValue;

  @override
  void insertTextPlaceholder(Size size) {}

  @override
  void removeTextPlaceholder() {}

  @override
  void showToolbar() {}

  @override
  void insertContent(KeyboardInsertedContent content) {}

  @override
  bool onFocusReceived() => true;

  @override
  void showAutocorrectionPromptRect(int start, int end) {}

  @override
  void performAction(TextInputAction action) {
    if (action == TextInputAction.newline || action == TextInputAction.done) {
      widget.controller.sendKey(
        const MorphKeyEvent(phase: 'down', key: 'Enter'),
      );
      widget.controller.sendKey(const MorphKeyEvent(phase: 'up', key: 'Enter'));
    }
  }

  @override
  void performPrivateCommand(String action, Map<String, dynamic> data) {}

  @override
  void performSelector(String selectorName) {}

  @override
  void updateEditingValue(TextEditingValue value) {
    _applyEditing(value);
  }

  @override
  void updateEditingValueWithDeltas(List<TextEditingDelta> textEditingDeltas) {
    var next = _editingValue;
    for (final delta in textEditingDeltas) {
      next = delta.apply(next);
    }
    _applyEditing(next);
  }

  void _applyEditing(TextEditingValue value) {
    final wasComposing =
        _editingValue.composing.isValid && !_editingValue.composing.isCollapsed;
    final isComposing = value.composing.isValid && !value.composing.isCollapsed;
    if (isComposing) {
      final preedit = value.composing.textInside(value.text);
      if (!wasComposing) {
        widget.controller.sendIme(const MorphImeEvent(phase: 'compose_start'));
      }
      widget.controller.sendIme(
        MorphImeEvent(phase: 'compose_update', text: preedit),
      );
      _composing = true;
      _editingValue = value;
      return;
    }
    if (wasComposing || _composing) {
      widget.controller.sendIme(
        MorphImeEvent(phase: 'commit', text: value.text),
      );
      widget.controller.sendIme(const MorphImeEvent(phase: 'compose_end'));
      _composing = false;
      _editingValue = TextEditingValue.empty;
      _input?.setEditingState(_editingValue);
      return;
    }
    // Multi-character insertions (paste) are committed. Single characters
    // are already sent as key events.
    if (value.text.length > _editingValue.text.length + 1) {
      final inserted = value.text.substring(_editingValue.text.length);
      if (inserted.isNotEmpty) {
        widget.controller.sendIme(
          MorphImeEvent(phase: 'commit', text: inserted),
        );
      }
    }
    _editingValue = TextEditingValue.empty;
    _input?.setEditingState(_editingValue);
  }

  @override
  void updateFloatingCursor(RawFloatingCursorPoint point) {}

  @override
  void connectionClosed() {
    _input = null;
  }

  @override
  void didChangeInputControl(
    TextInputControl? oldControl,
    TextInputControl? newControl,
  ) {}
}

/// One window a page opened, shown over the tab (or the popup beneath it)
/// with its address and a close button. Closing it returns to the opener.
class MorphPopupLayer extends StatelessWidget {
  const MorphPopupLayer({
    super.key,
    required this.controller,
    required this.onClose,
    this.userScript = '',
    this.profileId = 'default',
  });

  final MorphWebViewController controller;
  final VoidCallback onClose;
  final String userScript;
  final String profileId;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Material(
      key: const Key('morph-popup'),
      color: theme.colorScheme.surface,
      elevation: 8,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          SizedBox(
            height: 40,
            child: Row(
              children: [
                const SizedBox(width: 12),
                Expanded(
                  child: StreamBuilder<MorphNavigationEvent>(
                    stream: controller.navigation,
                    initialData: controller.currentNavigation,
                    builder: (context, snapshot) {
                      final nav = snapshot.data;
                      final host = Uri.tryParse(nav?.url ?? '')?.host ?? '';
                      final title = nav?.title ?? '';
                      return Text(
                        title.isNotEmpty && host.isNotEmpty
                            ? '$title · $host'
                            : (host.isNotEmpty ? host : title),
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: theme.textTheme.bodyMedium,
                      );
                    },
                  ),
                ),
                IconButton(
                  key: const Key('morph-popup-close'),
                  tooltip: 'Close window',
                  icon: const Icon(Icons.close, size: 18),
                  onPressed: onClose,
                ),
              ],
            ),
          ),
          Expanded(
            child: MorphWebView(
              controller: controller,
              userScript: userScript,
              profileId: profileId,
            ),
          ),
        ],
      ),
    );
  }
}
