# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

- `v0.2.0-alpha.10`, desktop chrome: preferences are a full page in a tab
  (Ctrl+, / Cmd+,) with search, sections and every setting; a search engine
  selector left of the address (Google by default, DuckDuckGo; each tab keeps
  its pick) with "Default search engine settings…"; clicking or focusing the
  address opens a dropdown that matches the mobile sheet (back, forward, copy
  link, refresh, bookmark, find, history, bookmarks, identity, settings,
  profiles), driven by arrows, Enter and Escape. Tab bar buttons switch the
  tab position (top, bottom, left, right) and collapse the tab list to icons
  with tooltips. Favicons in tabs. Pages' `<input type=file>` opens the
  system file dialog. Closing the last tab quits Morph. The star fills at
  once with a "Bookmarked" toast. morph://about and the hamburger menu.
  Linux: `<select>` lists drawn by the shell on Wayland and placed under the
  control on X11; dark sites get dark controls.
- macOS and Windows: no system title bar. The top row (tabs, or the toolbar
  when tabs are elsewhere) is the caption, with the traffic lights inset on
  macOS and Morph's own minimize, maximize and close on Windows. Morph can be
  made the default browser on both (NSWorkspace on macOS; the Default Apps
  settings page on Windows, with Morph registered under StartMenuInternet).
  The MSI has the usual install screens (licence, folder).
- Device linking works from either side: Add a device on the device that
  has the identity now shows its own QR code, link and Copy link (an invite);
  the new device pastes or scans it on its Link from another device screen,
  which also keeps its own QR. Both screens then show the same six-digit code
  as before, and nothing moves until it is confirmed on both. Pasting the new
  device's link under Add a device still works. The Xvfb, Wayland and
  macOS/Windows smoke checks decode the QR codes from screenshots.
- Back from the About or Settings page gives the tab its page's title again.
- `weirdnet-morph://` is registered on every OS (desktop file, Info.plist,
  the MSI and the per-user registry) and links go to the running Morph
  (single instance on Linux and Windows, Apple Events on macOS).
- Shortcuts: macOS uses Cmd throughout, plus Cmd+Y history (Cmd+H hides the
  app), Cmd+[ and Cmd+] back and forward, Cmd+Option+arrows to switch tabs
  and Ctrl+Cmd+F full screen. Windows and Linux add Ctrl+PgUp/PgDn,
  Ctrl+F4, F6 and Alt+D; Ctrl+Shift+R, Shift+F5 and Ctrl+F5 reload. On
  Windows, WebView2's own accelerators (Ctrl+D favourites and the like) are
  off so Morph's shortcuts reach the shell.

## [0.2.0-alpha.9] - 2026-11-15

- `v0.2.0-alpha.9`, identity: the recovery words get Copy (the clipboard
  clears itself after a minute if it still holds them) and Download (a
  native Save dialog, a file only you can read, with a note to keep it
  offline). New "Split into shards": 5 paper shards, any 3 of which restore
  the identity (weirdnet-keys Shamir shards, so Muninn reads them too), each
  with Copy, Download and a QR code, plus "Download all shards" (one file per
  shard, "shard N of 5, any 3"). Restore takes shards too: pasted into the
  words field, or chosen as files. File fields in the Identity panel open
  native dialogs (the desktop portal on Linux); the link field and the words
  field have "Paste from clipboard". Tab and Shift+Tab move between the
  panel's fields (password fields included). An Identity button in the
  toolbar, and page shortcuts on macOS: a Cmd shortcut pressed while a page
  has the keyboard now reaches the shell (Cmd+Shift+L opened nothing).
  Linux: "Make Morph the default browser" writes `mimeapps.list` itself
  (and each existing `<desktop>-mimeapps.list` for `$XDG_CURRENT_DESKTOP`,
  which wins over it) instead of calling `xdg-settings`, which failed on
  Hyprland and left the list half-written; `xdg-mime` checks every type.
  The old Flutter build's `org.weirdbrains.morph.desktop` and its
  mimeapps entries are removed at start-up when they carry Morph's marker.

- `v0.2.0-alpha.8` (phase 7), GPUI desktop shell on macOS and Windows: pages
  are WKWebView (macOS) and WebView2 (Windows) child views behind the same
  web view trait as Linux, with the page bridge, find in page, shell
  shortcuts while a page has the keyboard (and Cmd for every Ctrl shortcut on
  macOS). Downloads: a universal Morph.app (arm64 + x86_64) as a zip and a
  dmg, and a Windows zip and MSI, all unsigned (Gatekeeper and SmartScreen
  steps in `docs/install-macos-windows.md`). Linux is unchanged from alpha.7.
  The release script (weirdnet-project-helpers 0.3.0) now handles these
  pre-releases.

- `v0.2.0-alpha.7`, GPUI desktop shell: fixes for alpha.6 on a real GPU
  (NVIDIA GPU, nvidia-open, Hyprland). Native Wayland no longer aborts with
  "GDK is not able to create a GL context": GTK 3's off-screen windows have
  no GL, so off-screen pages get WebKit's shared-memory buffers (WebKit still
  renders with the GPU). Black pages under X11 ("Failed to create GBM buffer
  of size 1x1"): the start-up check now loads a real test page in a child
  process and keeps the first WebKit setting that paints it (defaults,
  `WEBKIT_DMABUF_RENDERER_DISABLE_GBM`, `..._FORCE_SHM`,
  `WEBKIT_DISABLE_DMABUF_RENDERER`), falling back to XWayland when nothing
  paints natively. It runs where a DRM device exists (`MORPH_DMABUF_PROBE=1`
  forces it, `=0` skips the page check). No vendor checks.

- `v0.2.0-alpha.6` (phase 6), GPUI desktop shell: native Wayland. Pages are
  rendered off screen (WebKitGTK in a GTK offscreen window) and drawn by the
  shell at the window's scale, with pointer, wheel, keys and input-method text
  forwarded; shell shortcuts never reach the page. `MORPH_BACKEND=x11|wayland`
  overrides the automatic choice; XWayland is the fallback. The key suite
  also runs on a headless sway (`scripts/xvfb-check.py --wayland`, new
  `morph-wl-input`), at scale 1 and 2.
  Fixed: black pages where the GPU refuses the buffers WebKit's DMA-BUF
  renderer allocates ("Failed to create GBM buffer"): Morph makes the same
  allocation in a child process at start and, only if it fails, uses
  WebKit's GL-texture export (`WEBKIT_DMABUF_RENDERER_DISABLE_GBM=1`) or
  shared-memory fallback (`MORPH_DMABUF_PROBE=0` skips the check). Pages
  twice the shell's size with `GDK_SCALE=2`: pages follow the shell's scale.
  Sign-in popups that call `window.close()` close (and the popup count goes
  down). Video plays on native Wayland. VA-API decoders, when GStreamer's `va` plugin has them, are ranked
  first for video (`MORPH_VAAPI=0` leaves ranks alone).

- `v0.2.0-alpha.5` (phase 5), GPUI desktop shell: the page bridge
  (`window.MorphBridge`) works on Linux; zip, deb, rpm and Nix flake packages,
  each with and without the desktop entry. Fixed: the window stopped
  answering while `xdg-settings` ran.

- `v0.2.0-alpha.4` (phase 4), GPUI desktop shell: the identity panel
  (Ctrl+Shift+L; create, restore from recovery words or a backup file, show the
  words, save a backup), device linking (a link with its QR code, a code to
  compare on both devices), "Make default browser" in settings
  (`xdg-settings`), single instance (a second `morph URL` opens the URL in the
  running window). Fixed: the page lost the first key after a panel closed;
  after unlocking the library the keyboard was left nowhere.

- `v0.2.0-alpha.3` (phase 3), GPUI desktop shell: find in page (a find bar
  beside the page, Ctrl+F, F3/Ctrl+G), Morph error pages for failed loads, a
  crash and hang panel per tab, the library panel (bookmarks with folders, tags
  and search; history; Ctrl+D, Ctrl+Shift+O, Ctrl+H; the password prompt for a
  locked library), the profiles panel (switch, create, rename, delete;
  Ctrl+Shift+M), the settings panel (tab position, side tab list width and
  collapse, per profile; Ctrl+,) and a frameless window with its own caption and
  buttons (close only on tiling window managers; `MORPH_WINDOW_BUTTONS`).

- `v0.2.0-alpha.2` (phase 2): the Flutter desktop app (Linux, macOS, Windows
  runners, `morph_webview_linux`, window chrome, `window_manager`,
  `scripts/build-windows.ps1`) is removed; the desktop is the GPUI shell and
  `app/` is the Android/iOS app. New `morph-services` crate with the logic both
  shells share; the mobile app uses the Rust address-bar rule; the desktop opens
  an encrypted library from the system keystore. Android Gradle Plugin 9.1.1.

- Linux builds on Flutter 3.50 / Dart 3.14. `window_manager` 0.7 brings
  `nativeapi` 0.6, whose ordinary import no longer compiles `Isolate.create`
  (removed in Dart 3.14). `nativeapi` 0.5.10, locked for Flutter 3.47, does.

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
