# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

- Windows: `scripts/build-windows.ps1` builds the release app and zips it.
  The build uses a short `TEMP` so vendored OpenSSL's object files fit
  under Windows' path limit, and `nuget.exe` fetches the WebView2 SDK.
  MSVC 14.51's removal of `<experimental/coroutine>` is silenced so the
  WebView2 plugin still compiles.
- Linux: on NVIDIA Wayland, start through XWayland so Flutter Impeller can
  create a GL context. This now also overrides session defaults such as
  Omarchy/Hyprland's `GDK_BACKEND=wayland,x11,*`, which previously left
  Wayland first and Morph aborted (sometimes after the first frame) with
  "GDK is not able to create a GL context: The current backend does not
  support OpenGL". `MORPH_ALLOW_WAYLAND=1` skips the override;
  `MORPH_GDK_BACKEND` forces a backend. `run-morph.sh` restores the
  terminal if Morph aborts.
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
- Windows isolates each Morph profile with a WebView2
  `WebViewEnvironment` whose `userDataFolder` is
  `<morph home>/profiles/<id>/webview2`. macOS 14+ uses
  `WKWebsiteDataStore(forIdentifier:)` via a creation hook; earlier
  macOS keeps the default store. Android still uses the process-wide
  WebView store (`dataDirectorySuffix` is process-global;
  `androidx.webkit.Profile` is not exposed by flutter_inappwebview).
- A session check covers a cross-host OAuth-style redirect (client state
  cookie set, hop to a different-host IdP, return with `code=`) so the
  callback still receives the client cookie after a simulated restart.
- Chrome-style user profiles are unlimited. Each profile keeps an isolated
  WebKit session under `<morph home>/profiles/<id>/webkit/` (cookies,
  localStorage, IndexedDB, cache). Tabs in the same profile share that
  session; cookies and localStorage set in one profile are invisible to
  the others. Shell settings live in the current profile's
  `settings.json`. Existing Morph data is migrated into the `default`
  profile. The profile catalog is available over the FRB bridge. A Glitch
  avatar on the trailing end of the address row (under the window
  buttons, on every desktop layout including macOS) opens a profile
  menu to switch, add (name and color), rename, or delete — never the
  last profile. The same picker sits in the mobile address sheet.
  Switching restores that profile's saved tabs and recreates its web
  views. Open-in-new-window is not available yet (window_manager 0.6
  is single-window).
- Shell shortcuts (Ctrl/Cmd+T, W, Tab, Shift+Tab, L, R, and 1–9) are
  handled by the shell before they are forwarded to a focused web view,
  so a page cannot swallow new-tab, close-tab, or the other chrome keys.
- When desktop tabs overflow the strip or a left/right list, Morph keeps
  the tabs that fit at their minimum size and puts the rest in a dropdown
  after the last visible tab, before the + group. Horizontal tabs share
  the strip evenly between 50 and 150 px and only move into the dropdown
  once every visible tab is at 50 px; the title hides first, then the
  close control (it stays on the active tab or on hover) so the favicon
  can stay centered. Side lists still use the sidebar width. Choosing an
  entry activates that tab and brings it into the visible set. Each
  entry has a close control.
- Favicon, title, and close sit on the vertical center of each desktop
  tab's visible body, including flipped bottom tabs, left/right list
  tiles, and overflow dropdown entries.
- The selected desktop tab is marked with a 2 px Glitch cyan (#00E5FF)
  border and a short magenta (#FF2BD6) offset: the outer top edge (and
  corners) of top tabs, the outer bottom edge of flipped bottom tabs,
  and the inner edge of a side list (right edge of the left rail, left
  edge of the right rail). Inactive tabs have no border.
- The expanded left and right tab lists have a draggable inner edge
  (resize cursor, 160–480 px, and never wider than the window).
  Double-click resets to 220 px. The width is stored in settings.json
  next to tab position and the collapsed state. Collapsing still uses
  the 48 px rail, and expanding restores the saved width.
- Linux web views present through a Flutter GL texture by default: frames
  come from the already-painted offscreen surface (BGRA upload, no CPU
  swizzle) at the display refresh rate, with pointer-move and scroll
  events coalesced. `webkit_web_view_get_snapshot` is the automatic
  fallback when that surface is missing. On NVIDIA the proprietary
  driver's WebKit DMA-BUF renderer stays disabled; true zero-copy
  DMA-BUF import remains the WPE path on Mesa when WPE is built.
  Override with `MORPH_WEBVIEW_RENDER=gl|snapshot|dmabuf`. Log fps and
  input lag with `MORPH_WEBVIEW_STATS=1`.
- Fix Linux web views rendering black on NVIDIA. Flutter populates GL
  textures in an OpenGL ES context, which rejects the `GL_RGBA8` + `GL_BGRA`
  upload (Mesa tolerated it). Uploads now use `GL_BGRA_EXT` on GLES (or an
  RGBA swizzle), and a self-check reads the first frames back: a GL error or
  mismatch drops to the RGBA upload, and an offscreen surface that stays
  blank while a WebKit snapshot has content switches the view to snapshots.
  Fallbacks are logged; `MORPH_WEBVIEW_STATS=1` adds read-back stats.
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
