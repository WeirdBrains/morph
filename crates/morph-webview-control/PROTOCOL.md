# WebView control protocol

The in-process gRPC `WebViewControl.Session` stream is the shared contract
between the Flutter shell and every platform web-view backend.

This document is the note for macOS (WKWebView) and Windows (WebView2)
implementers. Linux (`morph_webview_linux`, webkit2gtk snapshot and the WPE
path when it shares the same dispatch code) already implements the messages
below.

## Pointer

`PointerEvent` phases: `down`, `move`, `up`, `cancel`, `scroll`, `enter`,
`leave`, `hover`.

- Coordinates are view-logical pixels, origin at the top-left of the web-view
  widget (toolbar offset already removed). Multiply by the view's device pixel
  ratio before hitting the native view. Keep this mapping correct across
  resizes and DPR changes.
- `hover` is pointer motion with no buttons pressed. `move` is motion with one
  or more buttons held (drag). Do not treat hover as a left-button drag.
- `enter` / `leave` are crossing events. **`leave` must clear CSS `:hover`**
  (and typically reset the cursor to `default`).
- `button` is the button that changed on down/up (1=left, 2=middle, 3=right,
  4=back, 5=forward). `buttons` is the mask of buttons currently held
  (1=left, 2=middle, 4=right, 8=back, 16=forward).
- `ctrl` / `alt` / `shift` / `meta` travel with every pointer event.
- `scroll` carries smooth/trackpad deltas in `scroll_delta_x` / `scroll_delta_y`
  (Flutter logical pixels). A typical wheel tick is ~40–120.
- Dart coalesces `hover`/`move` to one event per frame. Backends should still
  tolerate bursts without blocking the stream.

## Cursor (server → client)

Emit `CursorEvent` whenever the page cursor changes. `shape` is a CSS cursor
keyword: `default`, `pointer`, `text`, `wait`, `progress`, `crosshair`,
`move`, `not-allowed`, `grab`, `grabbing`, `help`, `none`, `cell`,
`context-menu`, `alias`, `copy`, `vertical-text`, `zoom-in`, `zoom-out`,
`col-resize`, `row-resize`, `n-resize`, `s-resize`, `e-resize`, `w-resize`,
`ne-resize`, `nw-resize`, `se-resize`, `sw-resize`, `ew-resize`, `ns-resize`,
`nesw-resize`, `nwse-resize`, `all-scroll`.

The Flutter shell maps those names onto `SystemMouseCursors`. On Linux this
is `morph_control_emit_cursor`. On macOS/Windows, call the morph-bridge
`control_emit_cursor` helper (or the equivalent FFI) from the WKWebView /
WebView2 cursor-change callback.

Platform views that already composite natively (current `morph_webview_inapp`
path) may ignore forwarded pointer/key events, but a texture-hosted backend
must implement both directions: input in, cursor out.

## Keyboard

`KeyEvent` phases: `down`, `up`.

- `key` is a Flutter-style label (`A`, `Enter`, `Backspace`, `Arrow Left`,
  `Tab`, `Escape`, …).
- `text` is the produced character when there is one (`a`, `A`, `@`). Use it
  for printable input. Empty for modifiers and nav keys.
- `repeat` is set for key-repeat downs.
- `ctrl` / `alt` / `shift` / `meta` must be honored so Copy, Paste, Select
  All, and Undo (`C`/`V`/`A`/`Z`, Cmd on macOS) reach the page, along with
  Backspace, Enter, Tab, Escape, and arrows.
- `hardware_keycode` is the platform scan code when the client knows it; 0
  means derive it from the key.

Clicking the web view must give it keyboard focus. The address bar and the
web view must not steal keys from each other: keys go to whichever has
focus. When the web view is focused, Tab and Escape are sent to the page
(not used for Flutter focus traversal or window chrome).

## IME

`ImeEvent` phases: `focus`, `blur`, `compose_start`, `compose_update`,
`compose_end`, `commit`.

Feed preedit (`compose_*`) and `commit` into the platform input-method
context (WKTextInput / WebView2 composition / `WebKitInputMethodContext`).
`focus` / `blur` follow Flutter focus, so a click in a page text field can
type, and clicking the address bar stops dispatching keys into the page.

## Compatibility

New proto fields are proto3-optional and default to 0/false/empty. Older
clients still work. The Linux C ABI (`morph_control_pointer_fn`,
`morph_control_key_fn`) was extended with the new fields and
`morph_control_emit_cursor` was added; update any out-of-tree callers.
