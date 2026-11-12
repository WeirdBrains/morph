# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

- Start the Morph workspace: Rust core, identity integration with Weirdnet 0.1.0,
  a Flutter shell wired through flutter_rust_bridge, and CI for the first M1 slice.
- Linux `morph_webview` / `morph_webview_linux`: webkit2gtk-4.1 offscreen
  snapshot texture, WPE DMA-BUF feature detection, live navigation, and
  page-bridge wiring on a real view.
- In-process gRPC control layer (`morph-webview-control`): a single ordered
  `Session` stream over a Unix domain socket, or a local named pipe on Windows
  (loopback TCP remains available). The Linux plugin keeps the method channel
  for create/dispose/resize/texture and routes navigation, input, and
  page-bridge traffic through the control server, which handles page messages
  in Rust.
- macOS (WKWebView) and Windows (WebView2) backends (`morph_webview_inapp`)
  use flutter_inappwebview 6.x behind the Morph web-view interface and talk
  to the in-process control server. Dart's gRPC client falls back to loopback
  TCP on Windows because package:grpc cannot open a named pipe.
- On mobile, back/forward/reload/go are hidden. Edge-swipe walks history
  (with a visual arrow), pull-to-refresh reloads, and the system back
  gesture or button goes back in web history when possible. Tapping the
  URL field selects all of its text on every platform. Tapping the
  address bar opens a Firefox-style sheet (no VPN row) with an editable
  URL at the top; unbuilt actions show a coming soon snackbar. A tab-count
  button opens a two-column tab grid with favicon, title, close, preview,
  swipe-to-close, and a bottom search/menu pill plus a new-tab button.
- Desktop tab chrome can sit on the top, bottom, left, or right of the web
  view. The address and back/forward/reload row always stays at the top.
  Left and right use a vertical list (favicon, truncated title, close) that
  can collapse to a 48px favicon rail, with new-tab and position trailing
  the last tab and an expand control that stays visible; bottom tab
  decorations flip downward so they attach to the content above. The last
  position and collapsed-rail state are stored in `settings.json` under
  the Morph home. New-tab and tab-position always follow the last tab
  (pinned at the end only when the strip overflows) so they never cover
  tabs or push window buttons off the top-right. Close stays visible in
  every tab position; minimize and maximize still appear on non-tiling
  desktops. Switching tabs restores that tab's URL, title, and web view.
- Linux web views share one persistent WebKit session per Morph profile,
  with cookies stored in SQLite under the Morph home so logins survive
  restarts and tab recreation. `window.open` / new-window requests open a
  related view instead of being dropped. macOS, Windows, and Android keep
  the shared, non-incognito cookie store used by flutter_inappwebview.
- Linux web views turn on MediaSource, encrypted media, WebGL, and always-on
  hardware acceleration so sites can play video. YouTube and Instagram still
  need the usual GStreamer codec plugins (`gst-plugins-good`,
  `gst-plugins-bad`, `gst-plugins-ugly`, and `gst-libav`).
- Desktop window buttons: Windows and Linux draw minimize, maximize, and
  close (close-only on tiling compositors such as Hyprland, Sway, i3, niri,
  and river). Override with `MORPH_WINDOW_BUTTONS=close` or
  `MORPH_WINDOW_BUTTONS=all`. macOS keeps the native traffic lights visible
  on the left of a hidden title bar; the tab strip pads to clear them.
- Android uses the same in-app backend and control layer, with the system
  WebView in a Flutter platform view. The main manifest grants INTERNET, and
  the shell can submit a typed URL before the view has finished attaching.
