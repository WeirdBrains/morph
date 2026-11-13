# Morph architecture

Planning document for Morph v1. It sits next to `PLAN.md` and `PRIORITIES.md`. Adopted 2026-11-11 together with `PRIORITIES.md`. Crate facts below were read from the published sources on 2026-11-11.
Updated 2026-11-11 with Michael's second round of feedback: Dart verdict (section 7),
mobile reachability and names (5b), public HTTPS and `weirdnet-tls` (6b), out-of-process
extensions (7) and Student classifiers (8). Later the same day: Salty Gull as the update
service Morph uses (9a), a Weirdnet-relay wake path in phase 1 that is opt-in and off
by default (5b), a modified Flutter engine fork as later work (9a), and relay abuse
policy (6b). Later still the same day: Linux web view decisions in section 2
(GTK 3 now / GTK 4 later, DMA-BUF default with snapshot fallback, Wayland first).
Later still: a Weirdnet URL-scheme registry tying schemes such as `weirdnet-morph://` to apps
(section 5c), and access policy moved upstream to `weirdnet-access` (section 6).

## 1. Shape of the app

Morph is a Flutter app (Windows, macOS, Linux, iOS, Android) with a Rust 2024 core. Pages
render in the system web view. Morph's own value (identity, AI, notes, projects, peers)
lives in the Rust core.

```
┌──────────────────────────── Flutter (Dart) ───────────────────────────────────────┐
│ Shell: tabs, address bar, panels  │ Components (core set)  │ Themes               │
│ morph_webview: WebKitGTK (own plugin) / flutter_inappwebview + bridge             │
├──────────────── flutter_rust_bridge v2 (async, streams) ──────────────────────────┤
│                           morph-core (Rust)                                       │
│ identity │ store │ ai │ projects/notes │ ddns │ peer │ sites │ ext                │
├───────────────────────────────────────────────────────────────────────────────────┤
│ weirdnet-keys · weirdnet-storage · weirdnet-net · grindvakt-core                  │
│ weirdnet-model · weirdnet-tls · weirdnet-access · weirdnet-schemes · weirdnet-ext │
└───────────────────────────────────────────────────────────────────────────────────┘
```

Rules:
- Dart draws and routes input. Rust owns state, secrets, network and inference. The UI
  never holds a key.
- One Rust runtime (tokio) per process, started by the bridge. Long-lived things (peer
  node, inference, DNS sync) are services with typed commands and event streams to Dart.
- Peer content and extensions are untrusted. They reach Morph only through the page
  bridge and the permission check (section 7). Third-party extensions run in their own
  sandboxed process (section 7).

## 2. Web view

Decision: `morph_webview` is a Morph-owned federated plugin with one Dart interface and a
backend per platform.

- **Linux: our own WebKit plugin (`morph_webview_linux`, M1).** _(changed 2026-11-11)_
  - Flutter's Linux embedder is GTK 3 and has **no platform-view API**. Upstream work is
    still at the design and prototype stage (flutter/flutter#188375).
  - **GTK 3 now, GTK 4 later.** Phase 1 stays on the GTK 3 embedder. The snapshot
    fallback and the GTK widget overlay use **webkit2gtk-4.1**. **WebKitGTK 6.0
    (GTK 4)** comes in later priorities, once Flutter Linux can live in a GTK 4
    window; `GdkDmabufTexture` may then also be a GTK-port import path
    ([GdkDmabufTexture](https://docs.gtk.org/gdk4/class.DmabufTexture.html)).
  - **Wayland first.** Native X11 support is later priorities. Until then, X11
    sessions run through **XWayland**.
  - **Primary design: render the page into a texture, with a hidden text layer for
    copying and a control layer for interacting with JavaScript and Rust scripts.**
    - *Texture layer:* each tab's page is composited offscreen and shown as a
      Flutter external texture (`FlTextureGL`, or `FlPixelBufferTexture` on the
      copy fallback), updating only damaged regions.
      - **Default: DMA-BUF sharing (zero-copy).** WebKitGTK's DMA-BUF renderer
        (since 2.42; the only renderer since 2.44, when the X11 and WPE
        renderers were removed) is **internal**: the web process composites into
        GBM-allocated DMA-BUF buffers and sends them to the UI process over IPC
        (`AcceleratedBackingStoreDMABuf`). That is not a public API an app can
        use to pull those file descriptors out of webkit2gtk-4.1
        ([graphics model](https://docs.webkit.org/Ports/WebKitGTK%20and%20WPE%20WebKit/Graphics.html),
        [2.42 release](https://webkitgtk.org/2023/09/15/webkitgtk2.42.0-released.html),
        [2.44 release](https://www.webkitgtk.org/2024/03/16/webkitgtk2.44.0-released.html),
        [bug 252988](https://bugs.webkit.org/show_bug.cgi?id=252988)).
        The viable public path is **WPE WebKit**, the same engine's WPE port:
        WPE Platform (default in WPE WebKit 2.50+) exposes `WPEBufferDMABuf`
        file descriptors, formats and modifiers
        ([WPE Platform](https://wpewebkit.org/blog/2026-10-06-wpe-platform.html),
        [WPEBufferDMABuf](https://wpewebkit.org/reference/2.52.0/wpe-platform-2.0/class.BufferDMABuf.html));
        the older WPEBackend-fdo exportable API hands the app an `EGLImage` or
        linux-dmabuf resource
        ([exportable EGL API](https://github.com/Igalia/WPEBackend-fdo),
        [1.0 release](https://wpewebkit.org/release/wpebackend-fdo-1.0.0.html)).
        Those buffers are imported in `FlTextureGL.populate` (Flutter's EGL/GL
        context is already current) with `EGL_LINUX_DMA_BUF_EXT` /
        `eglCreateImageKHR` and `glEGLImageTargetTexture2DOES`, then returned
        as a `GL_TEXTURE_2D`. Flutter documents `GL_RGBA8` as the expected
        format
        ([FlTextureGL](https://api.flutter.dev/linux-embedder/fl__texture__gl_8h_source.html));
        this import path is used in the wild
        ([flutter_wgpu_texture](https://github.com/flowsai/flutter_wgpu_texture/blob/main/doc/platforms.md)).
        M1 still has to confirm format, modifier and Y-inversion handling.
      - **Fallback: offscreen snapshot copying.** If DMA-BUF export or EGL
        import isn't available, a webkit2gtk-4.1 view in an offscreen GTK 3
        window is snapshotted into `FlPixelBufferTexture` or uploaded with
        `glTexImage2D`. This costs CPU; video, WebGL and fast scrolling are
        where it will show.
      - **Requirements (DMA-BUF path):** a DRM render node; a driver with GBM
        and `EGL_EXT_image_dma_buf_import` (and ideally
        `EGL_EXT_image_dma_buf_import_modifiers`). Mesa (Intel, AMD, nouveau
        and similar) is the well-supported case. The target display is
        Wayland. Feature-detect at runtime and fall back to the snapshot path
        when import fails.
      - **NVIDIA caveat:** the proprietary driver has a history of DMA-BUF
        failures in WebKitGTK: Wayland protocol error 71 (explicit sync
        without an acquire point), GBM buffer creation failures, and
        "AcceleratedSurfaceDMABuf was unable to construct a complete
        framebuffer". WebKit has disabled the DMA-BUF renderer for NVIDIA in
        some releases. Workarounds exist (`__NV_DISABLE_EXPLICIT_SYNC=1`,
        `WEBKIT_DISABLE_DMABUF_RENDERER=1`, `nvidia_drm.modeset=1`) but Morph
        should not assume NVIDIA works: detect and use the snapshot fallback
        ([bug 262607](https://bugs.webkit.org/show_bug.cgi?id=262607),
        [bug 280210](https://bugs.webkit.org/show_bug.cgi?id=280210),
        [bug 281279](https://bugs.webkit.org/show_bug.cgi?id=281279),
        [Tauri Linux graphics](https://v2.tauri.app/develop/debug/linux-graphics/)).
        The Flutter shell has a separate NVIDIA Wayland issue: GTK 3's
        Wayland backend cannot create the GL context Impeller needs, and
        Morph aborts at startup ("The current backend does not support
        OpenGL"). When the proprietary driver is present and an X11 display
        is available, the runner sets `GDK_BACKEND=x11` so the window runs
        on XWayland. An explicit `GDK_BACKEND` is left alone. Wayland stays
        the default on other GPUs.
    - *Hidden text layer:* an invisible mirror of the page's text, positioned over the
      texture from layout boxes the page bridge reports. Native selection and copying
      work on it, and it doubles as the accessibility tree. It is updated as the page
      changes. Early **drift is an accepted risk** (drift is better than no
      usability); alignment work and tests stay planned.
    - *Control layer:* the interaction layer between the user, page JavaScript and
      Rust/WASM extension scripts. It talks over a **gRPC channel**, the same
      design as `weirdnet-ext`: protobuf as the contract, tonic over a local Unix
      socket (named pipe on Windows, when this pattern is reused).
      - It hit-tests input and routes it: to the page (as synthesized events, in
        page coordinates), to the text layer for selection, or to a script hook.
      - It dispatches page events to Rust and extension hooks.
      - It hosts overlays that scripts draw.
      - Text input and IME sit under it and feed the focused page element.
      - **Latency and ordering:** pointer motion and IME are latency-sensitive
        and must stay in order. Use a single ordered stream (a bidirectional
        or client-streaming RPC with sequence numbers), not concurrent unary
        calls that can complete out of order. The control layer itself should
        stay **in-process** with the Flutter embedder and the WPE/WebKit UI
        process side, so synthesized input isn't an extra process hop on top
        of WebKit's own IPC. Out-of-process extension hooks already go through
        this contract and can tolerate more latency than raw pointer or IME
        events.
    - Flutter chrome (menus, popups, find bar) is ordinary Flutter UI above all of this.
  - Last-resort UI fallback: a native `WebKitWebView` in a `GtkOverlay` above the
    `FlView`, positioned by a Dart placeholder. It is simpler, but Flutter can't
    draw over the page.
  - Native APIs cover the bridge (user-content scripts at document start, message
    handlers, custom-scheme registration for `weirdnet-morph://`, and declarative content
    filters) on both the WPE and webkit2gtk-4.1 backends.
- **Windows, macOS, iOS, Android: `flutter_inappwebview` (6.x) behind the same interface.**
  - Why this rather than separate native plugins: one maintained API already gives us
    everything the bridge needs on these four platforms:
    - user scripts at document start, and JS↔Dart handlers;
    - custom-scheme loading (`resourceCustomSchemes`) on Apple and Android, and
      WebView2 on Windows;
    - downloads, cookies, and real platform views on Android, iOS and macOS.
  - The official `webview_flutter` has no Windows or Linux support and a narrower API.
    Writing four native plugins would cost months for no gain in phase 1.
  - Because Morph's interface wraps it, any platform can later switch to a Morph-owned
    backend without touching the app. Windows is the most likely candidate if WebView2
    custom-scheme support through the plugin falls short (checked in M7).
  - We don't use its beta Linux (WPE) backend.
  - _(changed 2026-11-12)_ **Profile storage on Android:** every Morph profile
    except `default` gets its own androidx.webkit `Profile` (`MULTI_PROFILE`,
    WebView 120+), named `morph-<id>`, so cookies, local and session storage,
    IndexedDB, Cache Storage, HTTP cache and service workers never mix.
    `default` stays on the WebView's own `Default` profile so existing data is
    kept. The profile has to be set before the WebView is used, so Dart names
    each native view with a flutter_inappwebview keep-alive id, registers
    `id -> profile` over `org.weirdbrains.morph_webview/profile`, and
    `MainActivity.registerForContextMenu` (called from the InAppWebView
    constructor, before settings, scripts or loads) applies it. A surface
    rebuild gets a fresh id and native view. Deleting a Morph profile deletes
    its WebView profile; one still in use is swept on the next start.
    `setDataDirectorySuffix` is not a fallback: it is once per process. On a
    WebView without `MULTI_PROFILE`, non-default profiles show an
    update-your-WebView page and never load sites, rather than share storage.
- _(new 2026-11-12)_ **Android versionCode grows with every commit.** It was
  pubspec's `+1`, while an earlier split-ABI build had shipped 2001, so newer
  APKs were refused as downgrades. `build.gradle.kts` now uses
  10000 + `git rev-list --count HEAD` unless `--build-number` is given. All
  APKs stay signed with the same debug key until a release key exists.
- _(new 2026-11-12)_ **Control socket on Android.** The in-process web view
  control server listens on a Unix socket under `XDG_RUNTIME_DIR`, `TMPDIR`
  or `/tmp`. Android apps get neither variable and cannot write `/tmp`, so
  every tab failed with "io: Permission denied". On Android the socket now
  lives in `<app support dir>/run` (`start_webview_control_in`, a 0700 dir
  only the app can reach). Loopback TCP is not used as a fallback there
  because any app on the device could connect to it.
- _(new 2026-11-12)_ **Tab crash isolation.** A tab's renderer crashing,
  being killed or hanging must never take down the shell or other tabs; the
  tab shows its own "page crashed" panel (Reload, plus Wait for a hang). The
  session reports `MorphRendererEvent`s, the controller keeps a per-tab
  `rendererFault` and stops auto-healing a faulted tab (no crash loops), and
  `recoverFromFault` ends a hung renderer first, then brings the page back.
  What each engine gives us:
  - **Linux (webkit2gtk):** WebKit already runs every view in its own web
    process (no shared process model to opt out of). We forward
    `web-process-terminated` and `is-web-process-responsive` per view and use
    `webkit_web_view_terminate_web_process`. Checked in CI by aborting one
    tab's real web process in `session_check`.
  - **macOS/iOS (WKWebView):** one web content process per view already;
    `WKProcessPool` per tab changes nothing on current systems.
    `webContentProcessDidTerminate` marks only that tab.
  - **Android (system WebView):** an app gets one renderer process for its
    WebViews, so a renderer crash or terminate reaches every tab at once.
    `onRenderProcessGone` keeps the app alive, and the shell survives; the
    visible tab shows the panel and background tabs reload when opened.
    Real per-tab processes would need each tab in its own Android process,
    which a Flutter platform view cannot do. Hangs come from
    `onRenderProcessUnresponsive`; Reload answers TERMINATE.
  - **Windows (WebView2):** Chromium already isolates sites per process.
    flutter_inappwebview 6.1 exposes no `ProcessFailed` event, so Morph shows
    WebView2's own error page there for now; wiring `ProcessFailed` needs a
    plugin patch or a Morph-owned Windows backend.
- _(new 2026-11-12)_ **Default browser and incoming links.** Settings has
  "Make default browser" on every platform, with the current status. Each
  platform keeps the final say with the user, and nothing needs admin rights:
  - **Android:** http/https `VIEW` + `BROWSABLE` filters (and `APP_BROWSER`)
    make Morph eligible; Android 10+ asks with
    `RoleManager.ROLE_BROWSER`, older versions (or a refused role) open the
    default-apps screen.
  - **Linux:** the desktop entry declares `x-scheme-handler/http(s)` and
    `Exec=morph %u`; Morph runs `xdg-settings set default-web-browser`
    (falling back to `xdg-mime default`), user level only. Without an
    installed entry (a dev build) Morph writes its own to
    `$XDG_DATA_HOME/applications`, marked `X-Morph-Generated`, and never
    overwrites one the user or a package installed.
  - **macOS:** `CFBundleURLTypes` for http/https and
    `NSWorkspace.setDefaultApplication`, which shows the system prompt.
    Not compiled in CI (no macOS job yet).
  - **Windows:** Morph registers capabilities under HKCU
    (`Software\Clients\StartMenuInternet\Morph`, `RegisteredApplications`,
    a `MorphURL` ProgId) and opens `ms-settings:defaultapps`; Windows does
    not let apps set the default themselves.
  Links arrive as arguments (Linux, Windows) or over the
  `org.weirdbrains.morph/links` channel (Android intents, macOS Apple
  Events) and open in a new tab of the active profile; an empty start tab
  is reused. A second launch on Linux/Windows is still a second instance
  until Morph is single-instance.
- _(new 2026-11-12)_ **Library opens wait for the panel.** Opening a bookmark
  or history entry navigates only after the library panel's close animation
  has finished, then checks the surface. Surface checks asked for while one
  is running queue one follow-up instead of reusing the running result,
  which may have probed the page mid-navigation (the Android white page
  after opening a bookmark).
- **Page bridge:** a script injected at document start (`morph-bridge.js`) exposes a
  narrow message channel. DOM queries, injection and page events go through it as JSON.
  Rust never touches the DOM. The bridge contract is identical on every backend and is
  tested against each one.

**Risk (top of the list):** Linux texture rendering. _(changed 2026-11-11: decided)_
- **DMA-BUF is viable as the default**, with offscreen snapshot copying as the
  fallback, as decided above. webkit2gtk-4.1 still has no public zero-copy
  export, so the default uses WPE WebKit's public DMA-BUF/EGLImage API and
  `FlTextureGL`. M1 still has to measure frame rate on the DMA-BUF path and on
  the copy fallback (video, WebGL, fast scrolling), and to confirm NVIDIA and
  other non-Mesa drivers fall back cleanly.
- Synthesized input must match native behavior: IME, drag and drop, context menus
  and HiDPI scaling. **Wayland is first**; native X11 is later, with XWayland as
  the phase 1 stand-in.
- Keeping the hidden text layer aligned with the rendered page (scrolling, reflow,
  zoom, fonts) adds bridge traffic, and any drift shows up as selection landing on
  the wrong text. **Early drift is accepted** (better than no usability);
  alignment work and tests stay planned.
- The control layer's routing must match what the page expects: focus, pointer
  capture and the order scripts see events in. It uses a gRPC channel (see
  above) so ordering and in-process vs out-of-process latency are part of the
  design, not an afterthought.

M1 exists to prove this design, with a frame-rate and input check. If the texture
paths fall short, the overlay fallback above is ready to use.

## 3. Rust workspace (proposed)

```
morph/
  Cargo.toml                 # workspace, edition 2024
  crates/
    morph-core/              # service wiring, runtime, config, event bus
    morph-bridge/            # flutter_rust_bridge API surface only (thin)
    morph-identity/          # user key, node key, certs, device linking, trust list
    morph-store/             # SQLCipher schema + migrations (weirdnet-storage), keyring
    morph-ai/                # thin: chat sessions, catalog policy over `weirdnet-model`
    morph-projects/          # projects, Code Spaces (+ sharing over W2), templates, notes
    morph-ddns/              # adapter over grindvakt-core
    morph-peer/              # thin: configures the weirdnet-net node, Morph wire types
    morph-sites/             # thin: weirdnet-morph:// URLs → weirdnet-net content, site metadata
    morph-access/            # thin Morph glue over weirdnet-access (section 6)
    morph-ext/               # Morph's extension services (morph.ext.v1) + policy over weirdnet-ext
  app/                       # Flutter app
  packages/                  # Dart packages (below)
```

Flutter side:

```
app/                         # morph app: routing, shell, platform setup
packages/
  morph_bridge/              # generated frb bindings + Dart-friendly wrappers
  morph_ui/                  # core component set (tabs, address bar, panel, list, editor)
  morph_webview/             # web view interface, page bridge, scheme handling
  morph_webview_linux/       # Morph-owned WebKit backend: WPE DMA-BUF default,
                             # webkit2gtk-4.1 snapshot fallback; GTK 4 later
                             # other platforms: flutter_inappwebview behind the interface
  morph_chat/                # chat component
  morph_notes/               # notes + projects views
  morph_theme/               # theme tokens (later fed by weirdnet-themes hex output)
  # Dart extension SDK: later, after Rust-only extensions ship
```

`morph-bridge` stays thin so the generated surface is small and stable. Each domain crate
is testable without Flutter.

Shared outside Morph: **`weirdnet-model`** (section 8), used by Mouser and Morph.

**Rule: Morph consumes Weirdnet, it doesn't extend it locally.** Any transport or
networking capability Morph needs that Weirdnet lacks (streaming, content transfer,
membership on the wire, access policy, relay and bootstrap pieces, mobile transport
support, real-network tests) is contributed upstream to the Weirdnet crates and released
there first. Morph's peer crates only configure Weirdnet and add Morph meaning (site
paths, UI, and Morph-specific role mappings).
Section 11 marks every gap as **Weirdnet** or **Morph**. Weirdnet contributions go
**straight to main** in the Weirdnet repos, with no PRs, since we're core contributors.
During development Morph tracks Weirdnet main through a git dependency (or a path
dependency to a local checkout). Morph releases pin published crates.io versions.

**Naming convention:** shared functionality moves into Weirdnet over time. Anything
reusable beyond Morph becomes a `weirdnet-<thing>` crate (like `weirdnet-model`), and
`morph-*` crates hold only Morph-specific code. Candidates to become `weirdnet-*` crates:

| Proposed crate | Reusable part | Possible Weirdnet crate |
|---|---|---|
| `morph-identity` | User/node keys, device linking, trust list, impostor checks | `weirdnet-identity` |
| `morph-store` | Keyring + platform keystore unlock around SQLCipher | `weirdnet-vault` (or into `weirdnet-storage` + `weirdnet-keys`) |
| `morph-access` | Visibility × membership policy, configurable roles, host-side enforcement | **`weirdnet-access`** (decided 2026-11-11, W17). `morph-access` stays as thin Morph glue. |
| `morph-ext` | Out-of-process sandboxed extension host (native + wasm), gRPC/protobuf contract, packages, signing | `weirdnet-ext` + `weirdnet-ext-proto` + `weirdnet-ext-sdk` (planned as Weirdnet from the start, W13) |
| `morph-ddns` | Glue between Grindvakt and Weirdnet (`_dnsaddr`, `external` addresses) | Into `weirdnet-net::addr` or a `weirdnet-dns` crate over Grindvakt |
| `morph-ai` (chat sessions) | Chat sessions and history over `weirdnet-model` | Into `weirdnet-model` or a `weirdnet-chat` crate |
| `morph_webview` + `morph_webview_linux` (Dart) | Cross-platform web view with the Linux texture backend | `weirdnet_webview` (Dart package) |

`morph-core`, `morph-bridge`, `morph-peer`, `morph-sites` and `morph-projects` stay Morph
code: wiring, the frb surface, Morph's wire types, `weirdnet-morph://` and Morph's project model.
`weirdnet-access` moves now, because Muninn already needs the same behaviour. Other
candidates still move once a second app needs them, or sooner if the API is clearly
general.

## 4. What Weirdnet provides

All nine crates are `0.1.0`, BSD-3-Clause, edition 2024, published 2026-11-11, extracted
from Muninn, and none have been audited. Every crate except `weirdnet-plugin` needs
rust-version 1.99.

| Crate | Repo | Use in Morph |
|---|---|---|
| `weirdnet-keys` 0.1.0 | github.com/WeirdBrains/weirdnet-keys | Identity root: `Key32`, 24 BIP39 words, Shamir shards, Argon2id/HKDF, XChaCha20-Poly1305, X25519, envelope keyring (DEK wrap, rotate, rekey, recover). **Phase 1.** |
| `weirdnet-storage` 0.1.0 | github.com/WeirdBrains/weirdnet-storage | Local store: SQLCipher with a caller-supplied key, migrations, savepoints, snapshots, row change log + three-way merge (`sync`). **Phase 1.** |
| `weirdnet-net` 0.1.0 | github.com/WeirdBrains/weirdnet-net | libp2p 0.57 node (TCP, QUIC, Noise, Yamux, mDNS, Kademlia, relay client + opt-in relay server, DCUtR hole punching, AutoNAT). App-defined CBOR request/response. Signed offers/answers with a 10-digit short code, invitation links, sealed messages, node certs, user cards, link grants, TOFU pinning. **Phase 1.** |
| `weirdnet-themes` 0.1.0 | github.com/WeirdBrains/weirdnet-themes | TOML themes, 21 colour roles, contrast checks, `to_hex` for Dart. **Phase 2** (turn off `terminal-background` on mobile). |
| `weirdnet-rules` 0.1.0 | github.com/WeirdBrains/weirdnet-rules | Rules over event history + `every`/cron schedules. Candidate for Researcher/Student triggers. Phase 2. |
| `weirdnet-pipelines` 0.1.0 | github.com/WeirdBrains/weirdnet-pipelines | DAG runner. Candidate for research pipelines. Phase 2. |
| `weirdnet-plugin` 0.1.0 | github.com/WeirdBrains/weirdnet-plugin | Native cdylib C ABI v2. **Not for Morph extensions** (no sandbox, no mobile). |
| `weirdnet-code-spaces` 0.1.0 | github.com/WeirdBrains/weirdnet-code-spaces | Compiles math/Polars/Rust/Python into native plugins. **Different idea from Morph's Code Spaces** (below); not used. |
| `weirdnet-render` 0.1.0 | github.com/WeirdBrains/weirdnet-render | Tables/charts to PNG. Not needed (Flutter draws). |
| `weirdnet-model` (planned) | github.com/WeirdBrains/weirdnet-model (to create) | HF search/download, model registry, llama.cpp inference, extracted from Mouser and shared by Mouser and Morph (section 8). **Phase 1, before M4.** |
| `weirdnet-tls` (planned) | github.com/WeirdBrains/weirdnet-tls (to create) | ACME/Let's Encrypt (TLS-ALPN-01, HTTP-01, DNS-01 via grindvakt-core), cert store, SNI resolver, renewal; aggregates Mouser/Muninn/itcmdr/TAUR code (section 6b, W11). **Phase 1, before M9.** |
| `weirdnet-ext` (planned) | github.com/WeirdBrains/weirdnet-ext (to create) | Out-of-process extension host, per-OS sandbox, gRPC over local IPC, wasm fallback (section 7, W13). **Phase 2.** |

Naming: Morph's **Code Spaces** means a collection of code within a project, a grouping of
repos and code folders. `weirdnet-code-spaces` is a plugin compiler. Keep the names apart
in docs and code (for example, `morph_projects::CodeSpace`).

## 5. Identity, peers, relays and weirdnet-morph://

**Identity (`morph-identity`, M3).**
- *User key*: an Ed25519 keypair, the person. `user::user_id(pubkey)` is the user id. It's
  backed up as 24 words, with optional Shamir shards for trusted custodians, using
  `weirdnet-keys`.
- *Node key*: one per device (`identity::load_or_create` through a Morph `IdentityStore`
  inside the SQLCipher store). The user key signs a `NodeCert` for each device and can
  revoke it.
- *Card*: user id + node cert, shown to peers. `check_card` proves the node speaks for
  the user.
- *Same keys as Muninn*: a person has one user key across Weirdnet apps. A Morph install
  can link to an existing Muninn identity, or the reverse, as another node of the same
  user. Morph never creates a second identity for someone who already has one.
- *Linking devices*: a link-scope exchange plus a link grant (`check_grant`) moves the user
  key to the new device, sealed. Data sync between your own devices uses `sync_key`,
  `seal_sync` and the `weirdnet-storage` change log (phase 2 for full sync; phase 1 links
  identity only).
- *Store unlock*: the `weirdnet-keys` keyring wraps the database DEK. The service key sits
  in the platform keystore (Keychain, Android Keystore, DPAPI/Credential Locker, Secret
  Service) so the app opens without a password, with the password as a fallback.

- _(new 2026-11-12)_ **One identity per Morph profile.** Each profile is its
  own persona, so each gets its own user key, created or restored in a
  first-use onboarding screen (desktop dialog, phone sheet) the first time
  the profile is active. "Not now" waits until the next run; the Identity
  entry in the menu or sheet starts it any time. The key lives only in
  `profiles/<id>/identity/` (`morph-identity::profile_identity`):
  `user.key` holds a 32-byte seed sealed with `weirdnet_keys::password_seal`
  (Argon2id, XChaCha20-Poly1305) under a password of at least 8 characters,
  and `identity.json` holds only the public user id and fingerprint. The
  Ed25519 user key is derived from the seed and the seed is what the 24
  `weirdnet_keys::words` encode, so the words alone restore the same identity
  in another profile or device. Deleting a profile deletes its identity.
  Muninn linking and platform-keystore unlock (above) still come in M3 and
  will move the seal into the profile's keyring; "never a second identity"
  applies per profile.

- _(new 2026-11-12)_ **Identity recovery and sync (designed, not built).**
  See [docs/identity-recovery.md](docs/identity-recovery.md). Per profile:
  always-encrypted backups (`weirdnet-backup/v1`), restore in onboarding
  (words, backup file, another device, peers), dark shards (Shamir split of
  the seed, each shard password-sealed, trusted holders only, as in Muninn),
  QR device linking (no secret in the QR, ephemeral X25519, 6-digit SAS,
  120 s single use, Weirdnet transport), and end-to-end encrypted sync of
  opt-in data types with per-record HLC versions, LWW plus OR-set tags,
  kept conflicts, device revocation and key epochs. Shared logic goes into
  new `weirdnet-backup`, `weirdnet-shards`, `weirdnet-device-link` and
  (if `weirdnet-storage` is not enough) `weirdnet-sync` crates for Muninn
  to adopt too.

**Trust.** A *trusted peer* is a user you completed an exchange with (invite link or
code, short code compared out of band) and pinned. The trust list stores user id, pinned
node ids, label, when the user was added and when they were last seen.
`invite::impostor` warnings show in the UI. Revoked node certs drop out automatically.

**Peer node (`morph-peer`, M7).** Morph **shares Muninn's Weirdnet network**. It uses the
default `WEIRDNET` profile (`/weirdnet/net/1`, Kademlia `/weirdnet/kad/1`, port 4747,
app-prefixed invite links such as `weirdnet-morph://join/`), so Morph and Muninn nodes share the DHT, relays and invites.
- Morph identifies itself as `morph/<version>` in identify. Its own traffic (sites,
  content, Code Spaces, membership) runs on separate stream protocol ids
  (`/morph/...`) over W1, so a Muninn node never receives a Morph message it can't
  decode, and the reverse. Several app protocols on one node is an upstream need (W9).
- Morph registers as a handler for `weirdnet-morph://` links, including
  `weirdnet-morph://join/` and `weirdnet-morph://answer/`, through the scheme
  registry in section 5c. `weirdnet://` is not registered with the OS.
- **Relays: both.**
  - WeirdBrains runs public relay and bootstrap nodes (the W6 binary, deployed by ops).
    Every Morph node starts with them as `bootstrap` and `relays`.
  - Running your own relay is optional and only possible when the desktop has a domain
    tied to it through Grindvakt dynamic DNS (M6). Only then can it set
    `relay_server: true`. Your own relay is preferred for your own devices and trusted
    peers.
- Desktop: listens on TCP and QUIC, IPv4 and IPv6, with mDNS on. `external` comes from
  the DDNS name (`/dns4/<name>/tcp/<port>`, `/dns6/...`) when there is one.
- Mobile: dial-out only. It reserves a slot on your own relay if you have one, otherwise
  on WeirdBrains relays, then on relays found in Kademlia (`relay_key`). That relay is its
  *home relay*: it publishes a signed peer record there and to trusted peers, so peers find
  it by name or user id without DNS address updates (section 5b). DCUtR upgrades to a
  direct connection when NAT allows. It runs only while the app is in the foreground. On
  Android, hosting needs a foreground service. iOS can't host in the background.
- `Node` is a blocking handle on its own thread. An async handle and a `Status` event
  stream (NAT state, reservations, connections) are proposed upstream (W5). `morph-peer`
  only forwards that stream to Dart.

**weirdnet-morph:// (`morph-sites`, M7).**
- Address forms: `weirdnet-morph://<name>` (a DDNS-backed domain), `weirdnet-morph://<peer-id>`, and
  `weirdnet-morph://<user-id>/<project>`.
- `weirdnet-morph://` links open in Morph, and any Weirdnet app can resolve them through the
  scheme registry (section 5c). _(changed 2026-11-11)_ Public + Open projects can also be
  served over plain `https://<name>` to ordinary browsers from the owner's own dynamic DNS
  name, routed through the owner's relay or trusted peers when needed (section 6b). There
  is still no WeirdBrains-run gateway.
- Name resolution through DNS: the host publishes `_dnsaddr.<name> TXT
  "dnsaddr=/dns4/<name>/tcp/<port>/p2p/<peer-id>"`. That is libp2p's standard, and
  `weirdnet-net` enables libp2p's `dns` feature. This is why DDNS comes before p2p: a
  domain becomes a stable, verifiable pointer to a peer. Without DNS, the resolver falls
  back to the trust list and then the signed peer records of section 5b (trusted peers,
  home relay rendezvous, Kademlia).
- Transfer: today `weirdnet-net` only has request/response, with hard-coded caps in
  `node.rs` (`REQUEST_MAX` 8 MiB, `RESPONSE_MAX` 256 MiB, `REQUEST_TIMEOUT` 60 s).
  Morph needs real streaming. Section 5a proposes it as a Weirdnet contribution.
  `morph-sites` only maps `weirdnet-morph://` paths onto that content layer and adds Morph
  metadata (content types, site title, visibility header).
- Web view side: each site gets its own origin, so storage is isolated per site. It has
  no page-bridge privileges and a strict CSP by default. Per platform:
  - Apple: `WKURLSchemeHandler` (plugin `resourceCustomSchemes`).
  - Windows: WebView2 custom scheme registration.
  - Linux: `webkit_web_context_register_uri_scheme`, with the scheme marked secure in
    `WebKitSecurityManager` (our plugin).
  - Android: custom schemes get opaque origins, so load an intercepted
    `https://<site>.weirdnet-morph.invalid/` origin and show `weirdnet-morph://` in the address bar.
  Milestone 7 checks each platform.

## 5a. Weirdnet contribution: streaming and content transfer

Two designs were considered:

- **A. Manifest + chunks over request/response.** This fits under today's caps:
  1 MiB chunks fetched one request each by BLAKE3 hash. It's simple and works now. But
  every chunk costs a round trip, there's no backpressure, live data (chat media, video,
  model sharing) can't flow, and the caps stay baked in.
- **B. A real stream protocol.** A libp2p stream protocol (through `libp2p-stream` or a
  dedicated `NetworkBehaviour`) on Yamux, with backpressure from Yamux flow control and
  bounded buffers. Data flows as frames, a stream can resume from an offset, and the
  limits are configurable.

**Recommendation: B as the transport, with A's content model on top. Both go upstream in
`weirdnet-net`.**
- `weirdnet-net::stream` (W1): `Node::open_stream(peer, protocol)` and
  `Handler::stream(from, protocol, stream)`, with these properties:
  - **No size caps.** A stream can carry any amount of data. Per-protocol `Limits`
    replace the constants and control *how* data flows, not how much: max frame size,
    idle timeout, max concurrent streams per peer and in total, and an optional byte
    rate.
  - Defaults that prevent DoS without capping size: about 1 MiB frames, a 30 s idle
    timeout instead of a whole-transfer timeout, 8 streams per peer, 256 in total, and
    backpressure so a slow reader stalls only its own stream. Peers that break a limit
    are reset and can be scored down. Hosts only open streams for peers the app's
    policy allows.
  - Request/response keeps working for small messages. Its caps move into `NodeConfig`
    with today's values as defaults, so Muninn doesn't change. Anything large uses
    streams.
  - Sealed streams: frames sealed with the exchange key, with the stream id and a
    sequence number in the associated data, so they can't be reordered or replayed.
    `sealed::serve` gets the same for streams.
- `weirdnet-net::content` (W2), built on `stream`:
  - A signed **manifest**: entries of path → BLAKE3 root, size and app metadata (opaque
    bytes), signed by a user key. It has versions and a revocation list.
  - **rsync-based chunking:** files are split at content-defined boundaries by a
    rolling checksum (rsync-style weak hash, with BLAKE3 as the strong hash), so an
    edit only changes the chunks around it. A peer that holds an older version sends
    its chunk signatures, and only the differing chunks cross the network.
  - Chunks are addressed by hash, verified as they arrive, and resumable from any
    verified chunk. Range reads support media. There is no file or chunk-count size
    cap.
  - Fetching from several peers, provider records in Kademlia for Public + Open content,
    and a store trait so the app keeps the bytes.

Why this split: the streaming layer helps every Weirdnet app (chat media, sync, model
sharing), and the content layer gives Morph caching, multi-peer fetch and integrity
without Morph owning any transport code.

## 5b. Mobile reachability and names _(new 2026-11-11)_

Michael's question: can dynamic DNS work for phones once we can hole punch, and can the
decentralized net track which peer currently has a name? Short answer: **yes, but not by
putting phone IPs in DNS.** DNS binds the name to a user once; Weirdnet carries the
current address as a signed record; relays host the lookup and carry the traffic when
punching fails.

**What the evidence says**
- Phones on cellular sit behind carrier-grade NAT (CGNAT). Inbound connections are
  impossible, and the public IPv4 is shared and changes, so an A record for a phone is
  useless. Port mapping (UPnP, NAT-PMP, PCP) can't open the carrier's NAT. Cellular IPv6
  usually gives a global address, but carriers often firewall inbound traffic.
  ([Tailscale: How NAT traversal works](https://tailscale.com/blog/how-nat-traversal-works))
- Hole punching works most of the time, not always:
  - libp2p DCUtR, which weirdnet-net already uses, succeeded about **70%** of the time
    over 6.25M attempts ([ProbeLab RFM15](https://github.com/probe-lab/network-measurements/blob/main/results/rfm15-nat-hole-punching.md)).
    About 97% of successes came on the first try.
  - A 2026 study found **82.9%** for port-restricted cone NAT and about **40%** for
    symmetric NAT, which is the usual carrier case. It also found that about 36% of NATed
    peers never got a relay reservation, and without one there's nothing to punch from
    ([ProbeLab: Can libp2p punch through NATs?](https://probelab.io/blog/can-libp2p-punch-through-nats/)).
  - Iroh reports "roughly 9 out of 10" direct connections and names cellular networks
    as a place where that fails ([iroh NAT traversal](https://docs.iroh.computer/concepts/nat-traversal)).
    Its issue tracker shows phone cases that stay relayed or switch back and forth between
    IPv4 and IPv6 under load
    ([#2317](https://github.com/n0-computer/iroh/issues/2317), [#4644](https://github.com/n0-computer/iroh/issues/4644)).
  - Tailscale relays whenever both sides have hard NAT, or one hard and one easy
    ([device connectivity](https://tailscale.com/docs/reference/device-connectivity)).
- **Caution:** these numbers come from desktop-heavy IPFS and Tailscale populations.
  Phone-to-phone over two carriers will do worse. We'll only know Morph's real rate from
  W4 measurements.

**Answers to the sub-questions (item 6)**
1. *Good candidates and examples for hole punching:*
   - **libp2p DCUtR**: relay-coordinated simultaneous open over TCP and QUIC. We already
     have it.
   - **iroh**: QUIC with a relay first, then an upgrade to a direct path.
   - **Tailscale**: DERP relays plus "disco" pings and CallMeMaybe messages sent through
     DERP.
   - **WebRTC ICE**: STUN candidates, with TURN as the relay
     ([RFC 8445](https://www.rfc-editor.org/rfc/rfc8445)).
   - **Syncthing**: global discovery servers, public relays, plus QUIC and UPnP
     ([Syncthing relaying](https://docs.syncthing.net/users/relaying.html)).

   All of these use the same physics. None of them reliably punches symmetric NAT to
   symmetric NAT, and all of them fall back to a relay.
2. *Iroh:* it has worked out the **ergonomics**, not a new way through NAT.
   - It dials by key: an endpoint publishes a signed [pkarr](https://github.com/pubky/pkarr)
     packet (its home relay plus direct addresses) to a DNS server.
   - Peers look it up as `_iroh.<z32-id>.<origin> TXT`, with the Mainline DHT as an
     option ([iroh address lookup](https://docs.iroh.computer/concepts/discovery)).
   - Relays carry data by default over HTTPS/WebSocket, so it works where UDP is blocked
     ([iroh relays](https://docs.iroh.computer/concepts/relay)).
   - In 1.0, the relay path and direct paths are paths of one QUIC **multipath**
     connection, so moving between them doesn't break streams
     ([iroh 1.0](https://www.iroh.computer/blog/v1), [QUIC multipath](https://www.iroh.computer/blog/iroh-on-QUIC-multipath)).
   - It has first-party Swift and Kotlin bindings ([iroh-ffi](https://github.com/n0-computer/iroh-ffi)).

   What weirdnet-net (libp2p 0.57) lacks by comparison:
   - (a) seamless path switching inside one connection. DCUtR opens a second, direct
     connection beside the relayed one.
   - (b) relays meant for bulk data. Circuit relay v2 is designed for *limited* circuits
     ([spec](https://github.com/libp2p/specs/blob/master/relay/circuit-v2.md)), and
     rust-libp2p's defaults cap each circuit at a couple of minutes and about 128 KiB
     (to verify against 0.57).
   - (c) a relay transport over TLS/WebSocket on 443 by default.
   - (d) dial-by-key out of the box. libp2p has Kademlia peer routing instead.

   **Decision: stay on libp2p** _(agreed by Michael 2026-11-11)_. That keeps Muninn's
   shared network, Kademlia and the existing code. We contribute iroh's three strengths
   to Weirdnet:
   - **key-only dialing**: (d), the signed peer record and lookup (W10);
   - **data-capable relays**: (b), plus the TLS/WebSocket relay transport (c) (W6);
   - **seamless relay/direct path switching**: (a), so app streams survive the move
     from relay to direct and back (W15).

   We revisit iroh only if W4 shows libp2p is much worse on phones. Switching would split
   the network from Muninn.
3. *Could relays help?* Yes. They're the fallback that always works, as long as the
   phone can make one outbound connection.
   - The cost is bandwidth: every relayed byte crosses the relay twice, in and out.
   - _(changed 2026-11-11)_ In phase 1, WeirdBrains public relays carry relayed data and
     public HTTPS without rate limits or bandwidth caps, for compatibility. Michael:
     bandwidth isn't a Priorities 1 goal. Rate limits and bandwidth controls come in
     phase 2 or later. Only the W1 protocol guards (idle timeouts, concurrency limits)
     apply.
   - Your own relay is unlimited for your own devices and trusted peers.
4. *Can a DNS tracking system run through the decentralized net?* Yes, as described in
   the design below. DNS holds only a stable binding from the name to a user. The current
   peer id and addresses move through Weirdnet as signed, versioned records, so a phone
   can change networks every few minutes without touching DNS.
5. *Could relays do the lookup?* Yes. The [libp2p rendezvous protocol](https://github.com/libp2p/specs/blob/master/rendezvous/README.md)
   runs on the relay: a device holding a reservation registers its signed peer record
   there, and peers query it. Relays also run as Kademlia servers. This is the role of
   iroh's DNS server and of Tailscale's home DERP.

**Design (W10 `weirdnet-net::names`, plus W6 relay changes)**
- **Name binding (DNS, rarely changes):** `_weirdnet.<name> TXT "v=1 user=<user-id>"`.
  - Written once with grindvakt-core, by the desktop or by the phone with your DNS token.
  - It changes only if the name changes owner.
  - On a desktop with a public address, the existing `_dnsaddr` and A/AAAA records stay
    as a fast path (W7).
- **Peer record (Weirdnet, changes often):**
  - `PeerRecord { user_id, node_cert, addrs[], home_relay, seq, expires }`, signed by the
    node key. The node cert chains it to the user key.
  - A phone's `addrs` hold its relay circuit address (`.../p2p/<relay>/p2p-circuit/p2p/<phone>`)
    plus any direct IPv6 or IPv4 addresses it has seen.
  - A higher `seq` wins. Records expire in hours, and the device republishes on network
    change and on resume.
- **Where records live, in lookup order:**
  1. Trusted peers: pushed on change and exchanged when peers connect.
  2. Your home relay's rendezvous point (namespace = hash of the user id).
  3. Kademlia, **only while hosting a site** (public or private, below), with a validator
     that rejects bad signatures, revoked certs, lower `seq` and withdrawn keys.
  4. `_dnsaddr` for publicly reachable desktops.

  **When records go into the DHT _(changed 2026-11-11)_:**
  - A device publishes DHT records **only while it hosts a site, including private
    sites, and only for as long as it hosts.**
  - When it stops hosting, it **withdraws** them with a signed tombstone (below).
  - When it hosts nothing, it publishes only to trusted peers and its home relay, so
    nobody can see it's online.

- **Withdrawal (tombstone) _(new 2026-11-11)_:** removal is part of the protocol, not just
  addition.
  - The record: `Withdrawal { key, seq, issued_at, expires }`. It's signed by the **same
    key** that signed the record it removes, so only the publisher can withdraw.
  - Its `seq` is higher than any record published under that key.
  - **Rules for every storing node** (the W10 validator, running rust-libp2p Kademlia in
    `StoreInserts::FilterBoth` mode so the app decides what gets stored):
    - A higher `seq` always wins, so a withdrawal supersedes earlier records.
    - A node keeps the withdrawal until its `expires`, which is at least the longest
      record lifetime. Until then it rejects any put with a lower `seq`, so stale
      republishes and replication can't bring the record back.
    - A lookup that finds the withdrawal returns "withdrawn", not the old record.
  - **How it spreads:** the publisher puts the withdrawal to the same K closest nodes it
    published to, sends an `UNREGISTER` to its home relay's rendezvous point, and pushes
    it to trusted peers. Re-hosting later publishes a fresh record with a higher `seq`.
  - **Backstop:** every record expires within hours (republished about hourly while
    hosting). If a withdrawal misses some nodes, the old record still lapses on its own.
  - **Honest limit:** no DHT can force deletion. A malicious node can keep serving an
    old record. Clients reject anything expired, anything below the highest `seq` they
    have seen, and anything without a valid signature, so a stale record leads to a
    failed dial, not a wrong peer.
- **Public sites:** the plain `PeerRecord` is stored under `hash("weirdnet/peer/" +
  user_id)`, plus Kademlia provider records for Public + Open content (W2). Anyone can
  read them, which is what Public means.
- **Private sites: a record only trusted peers can find or read _(new 2026-11-11)_:**
  - Each private site gets a random Ed25519 **locator key** and a symmetric **locator
    secret**. The owner gives both only to trusted peers and members who may see the
    site. They're sent sealed in the exchange or with the W3 membership.
  - DHT key = `hash("weirdnet/locator/v1" + locator_pubkey + epoch)`, where the epoch is
    the current day. The key changes every day, so a stranger can't link records over
    time (like Tor v3 onion services' blinded keys).
  - The value is `{ seq, expires, ciphertext }`, where `ciphertext` is the `PeerRecord`
    (and site id) sealed with XChaCha20-Poly1305 under a key derived from the locator
    secret and the epoch. The whole thing is signed by the locator key, never the user
    key.
  - Storing nodes can still check signatures, `seq` and withdrawals, but they learn
    nothing about the owner, the site or the addresses.
  - Trusted peers derive the same key each day, fetch the record, decrypt it and dial.
    The host still enforces section 6: Noise-authenticated peer id → user, trust list
    and membership. So even someone who learns the address gets "not found".
  - **Feasibility:** feasible with standard primitives. It's the same pattern as Tor
    onion-service descriptors and pkarr's signed packets, and it doesn't need any change
    to Kademlia itself, only the W10 validator.
  - **Trade-offs:**
    1. **Network metadata:** DHT nodes that receive the put see the publishing peer's
       connection (IP or relay). Sending puts through the home relay hides the device
       IP behind the relay, at the cost of trusting the relay with timing.
    2. **Revocation:** removing a trusted peer means rotating the locator key and secret
       and re-sharing them with everyone who remains.
    3. **Trust:** a trusted peer can leak the decrypted address. Access control still
       holds, but the address doesn't stay secret.
    4. **Volume:** record size and update timing are visible, which makes one opaque
       entry per site per day observable.

  Recommendation: do it, with puts sent through the home relay by default.
- **Resolve and dial:**
  1. `weirdnet-morph://home.example.com` → DNS `_weirdnet` TXT → user id.
  2. Get the freshest PeerRecords → verify them.
  3. Dial the relay circuit and direct addresses in parallel.
  4. DCUtR upgrades to direct when it can.
  5. `weirdnet-morph://<user-id>/...` skips step 1.
- **Phone wake limits (honest):**
  - iOS suspends apps within seconds in the background. The relay reservation drops, so
    peers see the phone as "offline, last seen" and can't wake it without Apple push
    ([background execution](https://developer.apple.com/documentation/backgroundtasks)).
  - Android keeps a connection only through a foreground service with a visible
    notification, and Play policy limits which service types are allowed.
  - So phones are clients that host only while open, and anything always-on (sites, Code
    Spaces) is hosted by a desktop or your own relay.
  - Messages to an offline phone wait, sealed, on your relay or desktop (store and forward)
    until the phone comes back to the foreground.
- **Push wake-up _(changed 2026-11-11: decided)_:**
  - **Users can turn it off _(new 2026-11-11)_:**
    - **APNs/FCM push is on by default**, with a clear **per-device toggle** in
      Settings → Devices, also offered at first run. The toggle states what push
      reveals: timing only, to Apple/Google and WeirdBrains.
    - Turning APNs/FCM off unregisters the device token from the push service and
      revokes the device's wake handle. Trusted peers and the home relay are told
      that the handle is gone.
    - The device then falls back to **"reachable only while open"**: messages wait,
      sealed, on the home relay or desktop until the app is opened. Turning it on
      again registers a new handle, never the old one.
    - The **Weirdnet-relay wake path** (a persistent home-relay connection via an
      Android foreground service) is a separate control: **opt-in, off by default**,
      with a clear explanation of the battery cost shown before it is turned on.
  - **What the push providers see:**
    - APNs/FCM and the WeirdBrains push service only ever handle an **encrypted, minimal
      wake payload with no content**: a fixed-size ciphertext sealed to the device, plus
      the routing token or handle needed to deliver it.
    - No message text, sender identity, peer ids, addresses or site names ever go to
      Apple, Google or WeirdBrains.
    - The iOS alert text is generated on the device after decryption. The push request
      itself carries only a generic placeholder.
  - **Phase 1:** a WeirdBrains-run, self-hostable push service in `weirdnet-push` (W14)
    wakes phones. Any Weirdnet app's phone needs it. Delivery paths:
    - **APNs** for iOS. **iOS background wake still requires APNs.** A persistent
      connection to a relay cannot keep a normal iOS app runnable in the background:
      iOS suspends it within seconds, and VoIP/PushKit is reserved for reported
      VoIP apps. Sideloaded iOS builds get APNs only when signed with the push
      entitlement (developer or enterprise team yes; free AltStore signing no).
    - **FCM** for Android Play builds.
    - **A Weirdnet-relay wake path** for builds that can't use Google services
      (F-Droid, and sideloaded Android if the user prefers it). **This path is
      opt-in and off by default.** When the user turns it on, after a clear
      explanation of the battery cost, the phone keeps a connection to its home
      relay (and can receive wake announcements originating from WeirdBrains,
      relayed by any relay, so devices behind NAT are reachable). On Android this
      needs a **foreground service** with a visible notification. Battery cost is
      a real trade-off versus FCM: keepalives, doze/app-standby and Android's
      foreground-service type rules (including time limits on some types from
      Android 14+) all apply, and the service can still be killed. Play policy
      also limits which service types store builds may use. Turning it off
      unregisters the relay wake registration and, if APNs/FCM is also off,
      leaves the device reachable only while open.
    The same `weirdnet-push` server binary is self-hostable in phase 1 (ops runs one;
    anyone can run the same binary). The APNs/FCM off switch already gives
    "reachable only while open" on store builds.
  - **Phase 2:** UnifiedPush (the open push standard) as a user-selectable wake
    option, alongside the phase 1 paths.
  - **Minimal, encrypted content:**
    - At registration a phone with APNs or FCM gives the push service its token and
      gets back an opaque **wake handle**. A phone on the Weirdnet-relay path does
      the same only after the user opts in. It shares the handle only with its home
      relay and trusted peers (sealed).
    - A sender (usually the home relay, holding a sealed message for the phone) posts
      `{ handle, ciphertext }`. The ciphertext is sealed to the phone's push X25519 key
      and holds only a reason code (message waiting / connection requested), a random
      nonce and a timestamp. It never contains message content, sender identity or
      addresses.
    - The push service sees the handle, token (if any) and timing. Apple and Google see
      only timing and a fixed-size ciphertext. Relays that carry a wake announcement
      see the handle and timing, not the plaintext reason.
    - On iOS, user-visible wakes use an alert push with `mutable-content` plus a
      Notification Service Extension that decrypts on the device. Silent
      (`content-available`) pushes are throttled by iOS and not guaranteed. On Android
      Play builds, it's a high-priority FCM data message. On the relay path, the home
      relay (or another relay carrying a WeirdBrains wake announcement) delivers the
      same sealed payload over the existing connection.
    - On wake, the phone reconnects to its home relay if needed, collects the waiting
      sealed messages (store and forward), and republishes its peer record.
  - **Limits:**
    - F-Droid builds can't use FCM, because they must not depend on Google Play
      Services. In phase 1 they can use the Weirdnet-relay wake path if the user
      opts in (off by default, with a battery-cost explanation). Without that they
      are reachable only while open. UnifiedPush comes later.
    - Sideloaded iOS builds get APNs only when they're signed with the push
      entitlement: a developer or enterprise team yes, free AltStore signing no.
      Without APNs, iOS has no background wake.
    - Abuse control in phase 1 is limited to handle revocation and rotation, with no
      bandwidth limits (Michael: those are Priorities 2).

## 5c. Custom protocol (URL scheme) registry _(changed 2026-11-11: decided)_

Michael: Weirdnet should register custom protocols such as `weirdnet-morph://` to
applications such as Morph, so those links can be broadcast across the network
without confusion and every Weirdnet app resolves them the same way.

This is a **Weirdnet** capability, not a Morph-only one. It lives in a new crate,
`weirdnet-schemes` (W16). Libp2p stream protocol ids (`/morph/...`, W9) stay
separate; this crate is only for URL schemes.

**Prefixes.** Every app registers a **required prefix** for its scheme or schemes.
Prefix registration is **signed**. Any prefix that begins with `weirdnet` is
**blacklisted for third parties** and reserved for WeirdBrains.
- First-party apps use `weirdnet-<app>://`: Morph uses `weirdnet-morph://`, Muninn
  uses `weirdnet-muninn://`, and later first-party apps follow the same pattern.
- Third parties register their own signed prefix, which must not begin with
  `weirdnet`, and must be a valid URI scheme name that does not collide with
  IANA-registered schemes (`https`, `http`, `file`, `mailto`, and so on).
- `weirdnet://` itself is reserved for **internal Weirdnet messages only**, to
  be decided later. It is **not** registered with the OS, and nothing routes
  through it. There is no `weirdnet://open/` resolver. Invite links live on the
  app's own scheme: `weirdnet-morph://join/` and `weirdnet-morph://answer/` for
  Morph, `weirdnet-muninn://join/` and `weirdnet-muninn://answer/` for Muninn.

**Registry entry.** A `SchemeRecord` is signed by the app's **publisher key**
(first-party reserved prefixes are also signed by the WeirdBrains key):
scheme (for example `weirdnet-morph`), the registered prefix, a stable app id
(for example `org.weirdbrains.morph`), publisher identity, a monotonic `seq` and
an expiry, display name, handler capabilities (browse sites, join invites, and
so on), and a fallback / install hint (a Salty Gull app id, a store or F-Droid
listing, or an HTTPS page). Updates and withdrawals use a higher `seq`, the
same tombstone pattern as section 5b. Clients reject stale, lower-`seq`, expired
or unsigned records, and reject any third-party prefix that begins with
`weirdnet`.

**Signing key.** Michael holds and rotates the WeirdBrains signing key through
a new `weirdnetctl` CLI (W18). That CLI signs reserved-prefix registrations and
the canonical list.

**Distribution.**
- A **WeirdBrains-hosted canonical list**, signed with `weirdnetctl`, ships in
  app defaults and can be refreshed. It is authoritative for reserved
  first-party prefixes and schemes.
- Relays cache and serve records, so devices behind NAT can fetch them.
- The DHT can hold third-party prefix and scheme records in a later phase, under
  a key derived from the prefix or scheme, with the same expiry and withdrawal
  rules as other DHT records.
- Each device keeps a local cache, including records for apps installed on it.

**Conflicts and transfers.**
- Reserved `weirdnet…` prefixes cannot be claimed by anyone else. They are
  published only on the canonical list.
- A prefix or scheme moves to a new publisher only with a signature from the
  previous one (or, for reserved prefixes, a `weirdnetctl` rotation).
- When records conflict, clients prefer, in order: a user pin, the canonical
  reserved list, a locally installed app whose publisher signature matches, then
  a conflict UI. They never silently pick a stranger's record.
- Because each app has its own scheme, Morph and Muninn invite links do not
  compete for a single OS handler.

**Local resolution and OS-level registration** (every platform in the AGENTS.md
matrix; BSD later, with the engine fork):
- Linux: a `.desktop` `x-scheme-handler/<scheme>` entry and `xdg-mime` /
  `xdg-settings` default-handler registration (user-level).
- macOS: `CFBundleURLTypes` in the app's Info.plist.
- Windows: per-user `HKCU\Software\Classes\<scheme>` (no admin).
- Android: intent filters with `android:scheme` in the manifest.
- iOS: `CFBundleURLTypes`. Universal Links and Android App Links are HTTPS-only
  and complement, rather than replace, custom schemes (public sites already have
  `https://<name>` in section 6b).
- On install or first run the app registers **its own** prefixed schemes only
  (Morph registers `weirdnet-morph://`, including `join/` and `answer/`).
  **`weirdnet://` is not registered with the OS.**

**When the target app isn't installed.** The resolver shows the publisher, app
name and capabilities from the record, then offers the fallback hint. Nothing is
downloaded or installed until the user opts in. Desktop and sideloaded hints can
point at a **Salty Gull** signed full update; store builds get a store notice or
store listing; F-Droid builds point at the F-Droid listing (updates remain off
until opt-in, section 9a). If no trusted record exists, the UI says the scheme
is unknown and does not fetch a random handler on its own.

**Phase 1:** signed prefix registration, first-party `weirdnet-<app>://` schemes,
the canonical list signed with `weirdnetctl`, the in-process resolver, OS
registration of app schemes (not `weirdnet://`), and missing-app install hints.
DHT-backed third-party prefixes come later. The internal `weirdnet://` message
format is later as well.

## 6. Hosting: visibility and membership

_(changed 2026-11-11)_ This policy lives in Weirdnet, in a new crate `weirdnet-access`
(W17), so every Weirdnet app enforces it the same way. `morph-access` becomes thin
Morph-specific glue (resource types and role mappings, including forums later) on top
of it.

There are two independent settings, used for projects (phase 1) and, in Morph only,
forums (phase 2):

| | **Open** (no membership needed) | **Gated** (membership required) |
|---|---|---|
| **Private** (not visible outside trusted peers) | Any trusted peer can find and read it. | A trusted peer who is also a member. |
| **Public** (visible to anyone with the link and to all peers; also over HTTPS, section 6b) | Anyone with the link, any peer. Listed in Kademlia. | Anyone sees the title, owner and description, and can **request to join**. Content needs membership. |

Defaults are Private + Open, matching PRIORITIES.

**Membership.** Weirdnet has invite scopes (`invite_scope(to, role)`) but no lasting group
membership. Membership is a network-level credential, so it goes **upstream (W3)**, in
`weirdnet-net::user` or a new `member` module:
- `Membership { resource, user_id, role, issued_at, expires_at?, issuer }`, signed by the
  resource owner's user key (same pattern as `NodeCert`), plus signed revocations. It
  comes with verification, presentation on a stream or request, and a revocation list
  carried in the W2 manifest.

- `JoinRequest { resource, user card, message?, at }`, signed by the requester's user
  key, with the owner's answer: an issued `Membership` or a signed denial. It includes
  per-requester rate limits and a host-side queue trait, so the app keeps the requests.

`weirdnet-access` holds the shared policy: the two axes, a **configurable** role and
permission model, and the enforcement order below. **`Gated` is the name on the
wire everywhere**; Muninn may keep `members` and `closed` only as CLI aliases.
Roles are not hard-coded to forums or to any one app. The crate ships a **small
shared permission core** (membership and role administration, plus `read`) that
apps extend with their own catalog. **Muninn's rank rules and permission
implications are the default model.** An app registers its extensions and a
default role set; custom roles can be added at runtime. W3's `Membership.role`
is an identifier `weirdnet-access` interprets against that catalog.

- **Invite:** the owner sends an invite (`invite_scope(<resource>, <role>)`, as a link or
  code). Completing the exchange issues the membership and adds both sides to each
  other's trust list.
- **Request to join (Public + Gated):** a non-member sees the title, owner and
  description, then sends a W3 `JoinRequest`. The owner moderates a queue: approve
  issues a W3 `Membership` and adds the requester to the trust list, deny sends a
  signed denial. Requests from denied or rate-limited users are dropped quietly.
- Open resources skip the membership step.

**Roles.** The crate models a role as a name, a rank, a permission set and a builtin
flag, plus implication rules (a stronger permission includes weaker ones), using
Muninn's rank and implication behaviour as the default. Actors are the resource
owner, a member acting under a role, or a guest. Morph's default set is `owner`
and `member`; a `moderator` role is a Morph forum mapping in `morph-access`
(forums stay Morph-only for now). Muninn plugs in its richer default roles and
extends the shared core with its org, resource and code permissions (see
"Muninn today" below).

**Enforcement** happens on the host, per request, in this order (in `weirdnet-access`):
1. Noise has already authenticated the peer id. Resolve it to a user through the node
   cert in its card and reject revoked nodes.
2. Visibility. Private: the user must be on the host's trust list, otherwise the answer
   is "not found", which leaks nothing. Public: continue.
3. Membership. Gated: require a valid, unrevoked membership for this resource. Open:
   continue.
4. Role. Check that the actor's role allows this action, using the app's catalog
   (implications and rank rules included). Guests never receive managing permissions.
5. Private and Gated content goes over sealed streams (W1, bound to the exchange and both
   peers). Public + Open content is streamed plainly, and its integrity comes from the
   signed W2 manifest.

Caches on other peers only ever hold Public + Open chunks, or chunks encrypted to members.
Morph never shares private chunks.

**Muninn today** (read 2026-11-11 from the alien-blue checkout of `muninn`,
`crates/muninn-core/src/org/mod.rs` and `store/orgs.rs`; that tree is 6 commits behind
`origin/main`, but those commits do not touch org roles; other uncommitted files in
the checkout were not used). Orgs already have two axes, close to this model:
`Visibility::{Public, Private}` (default private) and `Access::{Members, Open}`
(default members). `Access::Members` is Morph's Gated (`members`, `member` and
`closed` are accepted aliases). A separate boolean `Org.gated` is *not* that axis:
it means a private org's hosted site shows guests a landing, sign-in and join page,
and stays Muninn-specific. Built-in roles are `owner` (rank 100, `*`), `admin`
(80, every listed permission), `user` (50: `members.list`, `read`, `records.read`,
`records.write`, `call`, `resources.transfer`, `code.view`, `code.run`) and
`viewer` (10: `members.list`, `read`, `records.read`, `code.view`). Admins can add
named custom roles with a rank and permissions. The permission catalog is Muninn-
specific (`org.edit`, `roles.manage`, `members.*`, `publish`, `host`,
`projects.manage`, `resources.*`, `read`, `records.*`, `call`, `code.*`), with
implication rules (`code.manage` includes `code.edit`/`run`/`view`, and so on) and
rank checks (you may only grant or change roles ranked below your own; only the
owner grants `owner`). Defaults: approved applicants and invitees get `user`.
Actors are `Owner`, `Member` (with a role) or `Guest` (only in an open org, and
never managing permissions). Muninn also has a separate, local MCP client role
(`read`, `write`, `run`, `full`) that is not org membership and stays in Muninn.

**Muninn adoption.** After W17 ships, Muninn switches its org policy onto
`weirdnet-access` in a no-behavior-change release: on the wire, `Access::Members`
becomes `Gated` (`members` and `closed` remain CLI aliases only); the built-in
roles stay Muninn's defaults, extending the shared permission core; rank rules
and implications come from the crate; and `Org.gated` stays a Muninn site flag.
Morph uses the same crate with a smaller catalog; `morph-access` only names Morph
resources (projects, sites, Code Spaces, forums) and maps Morph roles (including
forum `moderator`) onto them.

## 6a. Code Spaces and sharing

A **Code Space** is a named collection of local folders within a project (repos or plain
code folders). In v1 they are local folders, shared with peers.

- **Model** (`morph-projects`): `CodeSpace { id, project, name, folders[], ignore }`.
  Folders are paths on this device. Each folder's `.gitignore` is honored, and a default
  ignore list (`target/`, `build/`, `.dart_tool/`, `node_modules/`, `.env*`, key files)
  keeps build output and secrets out.
- **Publishing** (on top of W2): a snapshot turns each folder into a W2 manifest tree
  (path → BLAKE3 root, size, mode) with rsync-based chunks of the file contents. It is signed by the
  owner's user key and versioned. A new snapshot changes only the manifest and the
  chunks that changed.
- **Syncing metadata:** Code Space and note records travel with `weirdnet-storage` change
  log pages, sealed per exchange (W1 sealed streams). Peers apply changes with the
  existing three-way merge decision.
- **Fetching:** peers pull the latest manifest and only the missing chunks, into a
  read-only checkout under the project. Resuming and multi-peer fetch come from W2.
- **Permissions follow the project.** A Code Space has no visibility setting of its own.
  Project visibility and membership (section 6) decide who can see the manifest and
  fetch chunks, enforced by the same host check.
- **v1 limits:** only the owner publishes, and peers get read-only copies. Members sending
  changes back to the owner comes **later (phase 2)**. Updates resend only changed chunks
  (W2 rsync-based chunking), and there is no size cap on a Code Space.
- **Code reviews (later):** reviewing changes in a Code Space, such as members' sent-back
  changes before the owner accepts them, is planned for a later date and isn't designed yet.

## 6b. Public HTTPS for hosted projects _(new 2026-11-11)_

> **Changed decision (2026-11-11).** The earlier rule was "no web gateway: Public means
> Morph users only." Michael's new item 7.1, confirmed: **Public projects can also be
> served over HTTPS to anyone with an ordinary browser, at the owner's own dynamic DNS
> name, optionally routed through Weirdnet peers.** What stays from the old rule: there is
> **no WeirdBrains-run gateway** and no shared WeirdBrains domain, and nobody but the host
> terminates TLS.
>
> _(changed 2026-11-11)_ WeirdBrains public relays now also *forward* public HTTPS in
> phase 1 for users without their own relay. This still isn't a gateway:
> - the site keeps the owner's own domain (no WeirdBrains namespace);
> - the relay only passes encrypted bytes, by SNI, under the owner's signed grant;
> - TLS terminates on the host, and the relay never holds a key or sees content.

**What we already have (Let's Encrypt and ACME across WeirdBrains, read 2026-11-11):**

| Project | Where | Challenges | Notable |
|---|---|---|---|
| Mouser | `mouser-core/src/mcp_acme.rs` (~1.1k lines, commit `da967fd`) | TLS-ALPN-01, HTTP-01, DNS-01 (hook command or manual) | Hot cert swap in the rustls resolver, renew 30 days before expiry, 1 h to 1 day failure backoff, forced-renewal guard, 0600 files, in-memory test CA; ring |
| Muninn | `muninn-core/src/org/host.rs`, `org/tls.rs`, `mcp/serve.rs` | TLS-ALPN-01, HTTP-01 | Per-org SNI certs, source order user cert → ACME → self-signed, keys kept in the encrypted store, expiry watcher, mock CA in tests; aws-lc-rs |
| itcmdr | `adminsvr/src/acme.rs` (~460 lines) | TLS-ALPN-01, HTTP-01 | SNI split between the ACME cert for public names and a pinned internal cert for enrolled devices, renewal alert (`tls-acme`); ring |
| TAUR | `taur-server/src/acme.rs` (~940 lines) | HTTP-01 | `CertificateIssuer` trait (testable), `ChallengeBoard`, renew loop, never falls back to plain HTTP on a failed order, same cert for gRPC; ring |
| grindvakt-core | (no ACME) | | DNS providers (DNSimple, DigitalOcean) with keyed TXT writes, so it can publish DNS-01 records |
| weirdnet-* | (no ACME) | | |

All four use `instant-acme` 0.8 with rustls 0.23 and rcgen. They differ in crypto
provider (ring vs aws-lc-rs), storage and challenge coverage.

**Shared crate: `weirdnet-tls` (W11)** _(agreed by Michael 2026-11-11, together with
DNS-01 through grindvakt-core and encrypted ingress forwarding with TLS terminating only on
the host)_. The name follows the `weirdnet-<thing>`
convention and says what it does. Certificates aren't a transport, and the forwarding
part is a Weirdnet network protocol (W12), so `weirdnet-transports` isn't used.
- `Acme` client over instant-acme: account handling, orders, multiple SANs, staging and
  production, and custom directories.
- Challenges:
  - TLS-ALPN-01 answered by the resolver itself.
  - HTTP-01 through a token board plus a small listener or router hook.
  - **DNS-01 through a `DnsChallenge` trait.** A `grindvakt` feature implements it with
    grindvakt-core providers (keyed `_acme-challenge` TXT writes, removed after
    validation). There's also a hook command and a manual mode, taken from Mouser.
- A `CertStore` trait, with a file store (0600, from Mouser and TAUR) and an
  encrypted-store adapter (Muninn's settings keys over `weirdnet-storage`).
- An SNI `Resolver` (rustls `ResolvesServerCert`):
  - Per-name certs that are hot-swapped without a restart.
  - The source order user → ACME → self-signed (from Muninn).
  - Pinned-internal-cert routing (from itcmdr).
- Renewal:
  - Follows Let's Encrypt's ACME Renewal Information (ARI) window, falling back to
    two-thirds of the lifetime. Let's Encrypt moves to 64-day certs in February 2027 and
    45-day certs in February 2028 ([LE: 90 to 45 days](https://letsencrypt.org/2025/12/02/from-90-to-45.html),
    [ARI guide](https://letsencrypt.org/2024/04/25/guide-to-integrating-ari-into-existing-acme-clients.html)).
  - Failure backoff and a forced-renewal guard (from Mouser), a typed status for UIs and
    alerts (from itcmdr), and never downgrading to plain HTTP (from TAUR).
- A crypto provider feature (`ring` or `aws-lc-rs`), so Muninn (QUIC brings aws-lc-rs)
  and the ring-based projects can all use it. A mock ACME CA ships as a test utility.
- Out of scope: HTTP servers and forwarding.
- Mouser, Muninn, itcmdr and TAUR switch over later, each as a no-behavior-change release
  through its own release script (gap T).

**How a stranger's browser reaches `https://site.example.com`**
- **A. The host has a public address** (a desktop with a global IPv6 address or a
  forwarded IPv4 port):
  - grindvakt-core points A/AAAA at the host.
  - The host serves HTTPS itself with a `weirdnet-tls` cert (TLS-ALPN-01, HTTP-01 or
    DNS-01).
- **B. The host has no public address** (CGNAT, no port forwarding):
  - A/AAAA, or a CNAME to the ingress's hostname (not at the apex; see section 10), point at an
    **ingress peer**, in this order of preference:
    1. your own relay (it already needs a domain through Grindvakt);
    2. a trusted peer that opts in;
    3. **WeirdBrains public relays** _(changed 2026-11-11)_, for anyone without 1 or 2.
       They forward in phase 1 for compatibility, with the same SNI-passthrough rules.
  - The ingress peer listens on TCP 443 and reads **only the TLS ClientHello's SNI**,
    without decrypting.
  - It checks the name against a signed `IngressGrant { name, host_user, ingress_peer,
    expires }` from the host's user key.
  - It then opens a W12 stream `/weirdnet/ingress/1` to the host's peer id (direct, or
    through a relay circuit) and splices the bytes. A PROXY-protocol v2 header goes first
    so the host sees the client IP.
  - TLS terminates **on the host**. The ingress never holds a key or sees plaintext.
  - Port 80 on the ingress only answers HTTP-01 for granted names and redirects
    everything else to HTTPS.
  - Certificates in case B use **DNS-01 through grindvakt-core**, which needs no inbound
    path. TLS-ALPN-01 also works, because the passthrough forwards the `acme-tls/1`
    handshake to the host.
- **What's exposed follows section 6:**
  - Public + Open: the whole site.
  - Public + Gated: only the public card (title, owner, description, and "open in Morph
    to request membership").
  - Private: never. No HTTPS listener or grant exists for it.
- **Limits and risks:**
  - The ingress sees client IPs, SNI and traffic volume.
  - Ingress bandwidth is the operator's cost. _(changed 2026-11-11)_ In phase 1 there are
    no per-grant byte rates or bandwidth caps, on WeirdBrains relays too. Those come in
    phase 2 or later. Ops watches relay traffic in the meantime.
  - Content forwarded through WeirdBrains relay IPs can draw abuse reports, even though
    the relay can't read it. _(changed 2026-11-11: decided)_ A published contact address
    takes reports. WeirdBrains may revoke a grant per name on its own relays, and
    WeirdBrains makes that call for those relays. Every device (a relay or an ingress
    peer) can blacklist a forwarded site and refuse to forward it, and can turn off
    site forwarding entirely.
  - HTTP/3 isn't passed through in v1: QUIC SNI sits inside encrypted Initial packets.
  - Let's Encrypt rate limits are per registered domain, so one cert per site with SANs.
  - grindvakt-core writes A, AAAA, TXT and CNAME (`grindvakt-core` 1.1.0, section 10).
    A site can CNAME to an ingress hostname. A CNAME cannot share a name with A, AAAA or
    TXT (including `_weirdnet` and `_acme-challenge`), so the user's own domain should
    CNAME to their dynamic DNS host name, not at the apex.

## 7. Extensions _(redesigned 2026-11-11)_

Michael's direction for Priorities 2:
- not only WASM;
- platform-native binaries, downloaded and installed with the user's permission;
- each extension in its own sandboxed process;
- message-passing RPC with a gRPC design, so Dart or any compiled language works.

Confirmed: **WASM is the fallback where downloaded native code is forbidden** (iOS, and
Android).

**Shape.**
```
Morph (morph-ext host, broker) ──tonic gRPC over UDS / named pipe──► native extension process (sandboxed)
                               └─same protobuf messages over a WIT call/stream interface─► WASM extension (wasmtime)
```
- **One contract: protobuf.**
  - `weirdnet.ext.v1` covers lifecycle, handshake, capabilities, logging and health.
  - `morph.ext.v1` covers Morph's API:
    - page hooks: `PageLoaded`, `Navigated`, `Message`, `Request` where the platform has it;
    - page actions: `InjectScript`, `InjectStyle`, `Query`, `Selection`, `PageText`;
    - services: `ai.Chat/Embed`, `notes.*`, `projects.*`, per-extension `Storage`, and
      `Panel` (declarative UI rendered by `morph_ui`).
  - Events are server-streaming RPCs, and commands are unary.
- **Native transport:** tonic over local IPC.
  - Unix domain sockets on Linux and macOS, in a per-extension 0700 directory.
  - Named pipes on Windows (`tokio::net::windows::named_pipe` through a tonic connector),
    with an ACL for the extension's AppContainer SID.
  - Morph creates the socket and spawns the process with a one-time token on an inherited
    fd or in the environment. The extension connects back and presents the token. No TCP
    ports.
  - Precedent: TAUR already serves tonic gRPC with a bundled `protoc`
    (`protoc-bin-vendored`), and itcmdr keeps `.proto` contracts.
- **WASM transport:** the same prost-encoded messages cross a small WIT interface
  (`call(method, bytes) -> bytes` plus stream resources), so one schema serves both kinds.
  - Runtime: wasmtime with the component model.
  - Cranelift on desktop and Android. The Pulley interpreter on iOS (no JIT).
  - GC and exceptions are on by default since Wasmtime 47, and Pulley supports both
    ([GC and exceptions](https://bytecodealliance.org/articles/wasmtime-gc), [Pulley on iOS](https://github.com/bytecodealliance/wasmtime/issues/12251)),
    which matters for Dart and Kotlin WASM.
- **The broker is the only door.**
  - An extension process gets no file system, network or devices by default.
  - It reaches the page, the network (`Fetch`, checked against its site patterns) and
    Morph data only through RPC calls, and the host checks each one against granted
    capabilities.
  - The OS sandbox makes "no direct access" true even for malicious native code.
- **Lifecycle:**
  - Started on demand, stopped when idle, restarted with backoff on crash. A crash never
    takes Morph down.
  - Resource limits: cgroup v2 or rlimits on Linux, and Job Objects on Windows.

**Sandbox per platform (what is really possible):**

| Platform | Downloaded native binaries | Process sandbox | WASM |
|---|---|---|---|
| Linux (Arch/Omarchy, Debian, Ubuntu, NixOS) | **Yes** | Always: [Landlock](https://docs.kernel.org/userspace-api/landlock.html) (files; TCP bind/connect on kernel 6.7+) + seccomp-bpf, set by Morph before `exec`, no privileges needed. Plus [bubblewrap](https://github.com/containers/bubblewrap) namespaces where unprivileged user namespaces are allowed (Ubuntu 24.04+ restricts them through AppArmor, so detect and degrade) | Yes (Cranelift) |
| macOS | **Yes, with caveats**: arm64 code must be signed (ad-hoc is enough), and downloaded files carry quarantine | _(changed 2026-11-11: decided)_ Morph **re-signs each native extension ad hoc into the App Sandbox** (`com.apple.security.app-sandbox`, no other entitlements), validated in a spike before phase 2 ships it. Fallback: a `sandbox-exec`/`sandbox_init` profile (deprecated but still working, [apple/containerization#737](https://github.com/apple/containerization/issues/737)). If Morph itself ships App-Sandboxed, children inherit it | Yes (Cranelift) |
| Windows | **Yes** | AppContainer + Job Object (Rust: [`rappct`](https://crates.io/crates/rappct)); Low integrity level as a fallback | Yes (Cranelift) |
| Android | **No.** Apps targeting API 29+ can't `execve` files in their writable data directory (W^X, [Android 10 changes](https://developer.android.com/about/versions/10/behavior-changes-10#execute-permission)); only binaries bundled in the APK run. Same for F-Droid and sideloaded builds | WASM host in an `android:isolatedProcess` service | **Fallback: yes** (Cranelift) |
| iOS | **No.** [App Review 2.5.2](https://developer.apple.com/app-store/review/guidelines/#software-requirements) forbids downloading executable code, and apps can't spawn processes | No child processes: WASM runs in-process, isolated by wasmtime | **Fallback: yes** (Pulley). _(changed 2026-11-11: decided)_ The **App Store build allows downloadable WASM extensions**, as interpreted code under DPLA 3.3.1(B): no change of the app's purpose, no bypassing of signing or the sandbox, and no storefront for other apps (section 9a). Sideloading (AltStore, EU marketplaces, developer-signed `ipa`) is always the fallback |

So every package should include a WASM build to run on mobile. Native-only extensions are
desktop-only, and Morph says so before install.

**Packages, permissions and signing.**
- **Package (`.morphext`):**
  - A manifest: id, version, author user id, site patterns, capabilities, and the minimum
    Morph API version.
  - Per-target native binaries (linux/macos/windows × x86_64/aarch64) and an optional
    `extension.wasm`.
  - Every file listed with its BLAKE3 hash.
- **Signing:** the author's Weirdnet user key signs the manifest (same Ed25519 identity
  as everywhere else). Optional minisign for release pipelines. Updates must be signed by
  the same key.
- **Install flow:**
  1. Download from a link or a peer.
  2. Verify the signature and every hash.
  3. Show the author (trusted peer or not, pinned per extension id), the capabilities,
     the site patterns, and whether a native binary will run.
  4. The user allows or denies each capability.
  5. Install into a per-user read-only directory, following the user-level install rule.
- Hashes are checked again before every launch. An update that adds capabilities asks
  again. Revoking a capability takes effect on the next call.
- **Capabilities:** `page.read`, `page.inject`, `net.fetch(patterns)`, `ai`,
  `notes.read/write`, `projects.read/write`, `storage`, `panel`, `request` (Windows and
  Android GET only).

**Request interception still varies by platform:**
- Windows (WebView2): full.
- Android: GET only, no request bodies.
- iOS/macOS: none for http(s); only declarative blocking through `WKContentRuleList`.
- Linux (WebKitGTK): none for http(s); only declarative blocking through
  `WebKitUserContentFilterStore`.

The API exposes declarative block rules everywhere and a `Request` hook only where it is
real.

**Dart (Priorities 1 item 5 verdict).**
- *Embedding the Dart VM in the browser:* not possible. A system web view (WebView2,
  WKWebView, WebKitGTK, Android WebView) runs only JavaScript and WebAssembly and has no
  API for hosting another VM.
- *Dart running in the page:* possible, compiled by `dart2js` or by `dart2wasm` to
  WasmGC. WasmGC needs a recent engine (Chrome 137+, Safari 26.2+), so older WebKitGTK and
  Android WebView builds may lack it ([dart.dev/web/wasm](https://dart.dev/web/wasm)).
  This gives page scripts with page privileges only, not a sandboxed extension runtime.
  It isn't worth prioritizing.
- *Dart extensions, the real path:*
  - Desktop: `dart compile exe` produces a native binary that speaks the same gRPC
    contract (Dart `package:grpc` with protobuf stubs). It works as soon as native
    extensions ship.
  - Mobile WASM: `dart compile wasm --standalone` targets Wasmtime. It's still
    experimental and needs host imports that "are not stable"
    ([dart-lang/sdk#53884](https://github.com/dart-lang/sdk/issues/53884), [wasm.dart](https://github.com/simolus3/wasm.dart)).
    We watch it, not build on it.
- **So: Rust first.** The Rust SDK (`weirdnet-ext-sdk`) ships first, then the Dart
  package. Other compiled languages (Go, Zig, C, Kotlin) work through the protobuf
  contract without an official SDK.

**Phasing.**
- Phase 1: Morph's own features call the same `morph.ext.v1` services in-process, which
  proves the contract.
- Phase 2: third-party extensions:
  1. Linux native first, then Windows and macOS (after the macOS sandbox spike).
  2. WASM fallback on all platforms.
  3. The Dart SDK last.
- `weirdnet-plugin` (cdylib C ABI, in-process, no sandbox) stays for trusted first-party
  plugins in other apps. It is not used for Morph extensions.

**Crates:**
- `weirdnet-ext` (W13): supervisor, per-OS sandbox, IPC transport, broker skeleton,
  package format, signing.
- `weirdnet-ext-proto`: `weirdnet.ext.v1`.
- `weirdnet-ext-sdk`: the Rust author SDK.
- `morph-ext`: Morph's `morph.ext.v1` services and policy.

## 8. AI stack (`morph-ai`)

- **Local AI is optional _(new 2026-11-11)_.** It is **on by default**, and `morph-ai`
  owns two user controls:
  - **One toggle (Settings, and the AI panel header) turns it off.** Off means:
    - no model is loaded and no inference runs;
    - AI panels, "ask about this page" and AI extension services (`ai.Chat/Embed`)
      return "AI is off";
    - Student/Researcher jobs don't run.

    Downloaded model files stay on disk until the user removes them in the catalog.
    Turning it back on needs no re-download.
  - **One action, "Clear AI context and history", wipes everything the local model has
    stored:**
    - chat sessions and messages, and attached page context;
    - the memory/cache layer, embeddings, Student summaries and classifier labels;
    - Researcher journals' AI-generated entries;
    - the inference KV/prompt caches.

    It runs in one store transaction plus deletion of the cache files, then a SQLCipher
    `VACUUM` so nothing is left in free pages. It doesn't touch the user's notes,
    projects or model files. It asks for confirmation once, with no per-item steps.
  - This is separate from Student browsing-history collection, which stays **opt-in**
    (section 9). AI being on doesn't mean history is collected.
  - Both controls are core actions in `morph-ai`, exposed through `morph-bridge`, so UI,
    extensions and tests use the same path.
- **`weirdnet-model` (cross-project, before M4).** A new Weirdnet-family crate in its own
  repo (`github.com/WeirdBrains/weirdnet-model`), published on crates.io like the other
  `weirdnet-*` crates. It is extracted from Mouser, and Mouser and Morph both depend on
  it:
  - the HF client from `mouser-core/src/models/hf.rs` (search, truthful download sizes,
    quant/shard handling);
  - the non-UI part of the `hf pick` flow (listing models and quantizations to pick
    from);
  - resumable, verified downloads and the local model registry;
  - the llama.cpp inference wrapper from `mouser-llm` (`llama-cpp-2`: Vulkan on Linux,
    Metal on Apple, CPU on Windows; plus Android and iOS targets for Morph).

  Mouser keeps its terminal picker and UI. Morph keeps its chat UI. `morph-ai` stays a
  thin layer for sessions and device policy. Switching Mouser over is a Mouser change
  with no behavior change, shipped through Mouser's normal release process, including
  the TAUR step.
- Mobile: GGUF models of about 4B parameters or less, quantized to Q4. Metal on iOS, and
  CPU first on Android with Vulkan where stable. The catalog filters by device RAM and
  shows the download size before fetching.
- Desktop: any GGUF that fits, plus an optional remote endpoint later.
- Chat sessions live in the store and can attach page context through the bridge. The
  user starts that explicitly (for example with "ask about this page").
- Phase 2 (Student, Researcher): a memory layer with embeddings in SQLite tables, plus
  `weirdnet-rules` schedules for research jobs.
- **Student "classify" _(defined 2026-11-11)_:** run classifiers over collected history and
  summaries to label and structure them for later AI use (memory retrieval, Researcher,
  project feedback).
  - Labels: topic, content type (article, docs, code, video, shop...), named entities,
    project relevance and sensitivity (finance, health, login pages, to skip or keep
    local).
  - Classifiers run locally: embedding similarity against label prototypes, small
    fine-tuned or zero-shot classifiers via `weirdnet-model`, and the chat model as a slow
    fallback.
  - Each label is stored as `(item, label, classifier id + version, confidence)`, so a
    classifier can be upgraded and items re-labelled.
  - Users see and correct labels. Corrections are kept as local training examples.
  - The same opt-in, pause and delete rules as section 9 apply. Deleting an item removes
    its labels.

## 9. Data and privacy

- One SQLCipher database per user (`weirdnet-storage`) for settings, trust list,
  memberships, projects, Code Spaces, notes, chats and the model catalog. Large blobs
  (models, site chunks) go in private files. Models aren't encrypted; chunks are
  content-addressed and checked against their hashes.
- Nothing leaves the device unless the user publishes, syncs or invites. There is no
  telemetry.
- Browsing history for the Student layer is **opt-in** (off by default) and can be paused
  per site. It is inspectable as a list with summaries and deletable per item, per site
  or all at once. Deleting it also removes the derived summaries and embeddings.
- Export: an age-encrypted archive (`weirdnet-keys` `age` feature).
- _(new 2026-11-12)_ **Browser bookmarks and history** are kept per Morph
  profile in `profiles/<id>/bookmarks.json` and `history.json`
  (`morph-core::library`), written atomically, with history capped at 5000
  visits and repeat visits to the same page folded together. They never leave
  the device and are deletable per item or all at once from the Library and
  Settings. This is the ordinary browser back-list that shows in History, not
  the Student layer's opt-in history above, which stays off by default. The
  files move into the profile's `weirdnet-storage` database once that lands.
  Find in page is a page script (`window.find` plus a text match count), so it
  behaves the same on every backend. Tab position and the sidebar stay
  app-wide; Settings labels them that way.
- _(new 2026-11-12)_ **Bookmark folders and tags** (`morph-core::bookmarks`,
  file version 2). Folders nest without limit; folders and bookmarks share one
  ordering inside a parent, kept as fractional base-62 keys (rocicorp-style
  midpoints) so a move rewrites only the moved item. Every item has a UUIDv7
  id and created/updated times; older id-less bookmarks get a deterministic
  `legacy-` id until the file is next written. Deleting leaves a tombstone for
  90 days. Ids, keys, times and tombstones are what Weirdnet sync will merge
  on (docs/identity-recovery.md). Moves that would put a folder inside itself
  are refused in the core, and loading repairs missing parents and cycles.
  Deleting a folder deletes what is inside it, after a confirm that counts
  it. Tags are lowercase keywords (trimmed, `#` dropped, 40 chars max).
  Search needs every word to match the title, URL or a tag; a tag chip
  filters exactly. In the Library, desktop drags from a handle and phones
  long-press the row; folder rows take drops before, into (middle half) or
  after, breadcrumbs take drops too, and "Move to…" does the same without
  dragging. This stays in `morph-core` because it is browser data, not shared
  Weirdnet code.

## 9a. Updating the Flutter app over the air _(changed 2026-11-11: decided)_

App updates come from **Salty Gull**, a standalone WeirdBrains project. Morph uses
that service; this document does not design it. See **Salty Gull's plan** (written
by 0072; as read on 2026-11-11 from `/mnt/ext1/scratch/salty-gull/PLAN.md`) for the
update mechanism, channels, signing and later Dart-patch work. Morph does not use
Shorebird.

Requirements Morph relies on, agreed 2026-11-11:
- Updates are **signed** and distributed over Weirdnet.
- Every relay can carry update announcements originating from WeirdBrains, so devices
  behind NAT can receive them.
- Updates are **never forced**. The user opts in per update, or opts in to all updates.
- No fees beyond our own server costs. The service is self-hostable.

What each Morph build gets (from that plan):
- **Store builds** (App Store, Google Play): **asset** bundles, **downloaded UI**
  (Remote Flutter Widgets) and **WASM** logic modules (wasmtime; Pulley on iOS, no
  JIT). The Dart shell and the Rust core update only through a store release.
  **No Dart patches on iOS** until the engine fork (Salty Gull Phase 3).
- **Desktop and sideloaded** builds: **signed full updates** over Weirdnet, with
  an HTTPS fallback. Sideloading is always available: signed `apk` downloads on
  Android; AltStore, EU marketplaces and developer-signed `ipa` on iOS.
- **F-Droid:** an `fdroid` flavor that uses **stock Flutter** (no forked engine),
  with **updates off until the user opts in**. No Dart patching and no APK
  self-update; after opt-in, only asset and WASM updates, behind F-Droid consent
  wording.

Morph's **Rust core can't be patched** by a Dart-layer or WASM update. Native code
arrives through `flutter_rust_bridge` and needs a full app update or a store release.

A **modified Flutter engine fork** remains later work: an interpreter for Dart
patches on App Store builds (Salty Gull Phase 3) and a BSD port (FreeBSD and
OpenBSD). Salty Gull's plan owns the fork's design. Android and desktop AOT Dart
patches, if they ship at all, are also later (Salty Gull Phase 2) and are not a
Morph phase 1 goal.

The same DPLA 3.3.1(B) interpreted-code clause is why downloadable **WASM
extensions** on iOS should pass (section 7). The storefront condition means an
in-app extension catalog must not look like an app store.

## 10. Grindvakt: what Morph needs

The requirements below were the contract for App Maker's Grindvakt library work (the
app was v0.6.2 when first read on 2026-11-11). The published crate is what Morph pins;
do not change Grindvakt here.

**Status (2026-11-11, from App Maker, verified on crates.io):** `grindvakt-core` 1.1.0 is
published on crates.io
([crate](https://crates.io/crates/grindvakt-core), [docs](https://docs.rs/grindvakt-core/1.1.0/grindvakt_core/),
docs.rs built), repo WeirdBrains/grindvakt-core, BSD-3-Clause, imported as
`grindvakt_core`. Morph pins `=1.1.0`. 1.0.0 already covered requirements 1-9:
`Record::a/aaaa/txt` with keyed TXT matching, `plan`/`apply` (never deletes, refuses
ambiguous plans), a `DnsProvider` trait with DNSimple and DigitalOcean, `ip::discover`
with replaceable sources and `require_global` -> `Error::NoGlobalAddress`, typed
`Error::is_retryable()`, `SecretString` tokens, no path access, rustls only, Android/iOS
`cargo check` in CI, and no scheduler. Default features are dnsimple, digitalocean and
public-ip; `vault` and `standalone` are opt-in. Versions move in lockstep with the
Grindvakt app. Morph notes: on Android, Morph must initialise
`rustls-platform-verifier`; the `_dnsaddr` TXT text builder belongs in `weirdnet-net`
(W7), not in grindvakt-core. Verify against the real crate in M6.

**CNAME support (1.1.0; request done).** `Record::cname(name, target)` (trailing dot optional,
case-insensitive), with `RecordKind::Cname` and `RecordValue::Cname(String)`.
`record::normalize_target` and `RecordKind::ALL` are available. `DnsProvider::types_at(zone, name)`
has a default implementation. Matching: a name has one CNAME, left alone if correct,
retargeted in place if not, or created if missing. Two existing CNAMEs at one name make
the plan ambiguous. `apply` refuses the whole plan if it would put a CNAME where any
other type exists (including MX), an A/AAAA/TXT where a CNAME exists, or a CNAME and
another record at the same name in one plan. It never deletes anything. `InvalidRecord`
covers a CNAME at the apex, a target that isn't a full host name, a self-pointing CNAME,
and duplicate CNAMEs in a plan. DigitalOcean is given `target.`; DNSimple the bare name.
`plan` now makes one extra list call per name, so HTTP mocks must answer it.

Morph implication: a `_weirdnet` TXT record and an A/AAAA record cannot share a name
with a CNAME, so a user's own domain should CNAME to their dynamic DNS host name, not
at the apex.

Required:
1. **A library crate with no CLI or TTY in it.** No `clap`, `rpassword` or interactive
   prompts on the library path. Feature-gate `cli`, `vault` and `service`.
2. **Caller-owned secrets and paths.** The caller passes the token as `SecretString`, and
   there are no hard-coded `~/.grindvakt` paths. Morph keeps tokens in its own keyring.
3. **Record types A, AAAA, TXT and CNAME**, plus multiple records per run (apex + names +
   `_dnsaddr` TXT). A CNAME cannot share a name with any other type, and cannot be at
   the apex.
4. **Address discovery for IPv4 and IPv6 separately**, with injectable sources and the
   option to pass addresses in (from AutoNAT/`Status.external`). It should return a
   distinct `NoGlobalAddress` (for CGNAT or private-only addresses) so Morph can switch
   to relays.
5. **Plan/apply split**: `plan(records) -> Vec<DnsAction>`, `apply(actions)`, with dry
   runs, idempotent writes and the existing ambiguity checks.
6. **Typed errors** (`Auth`, `RateLimited`, `Ambiguous`, `NoGlobalAddress`, `Network`,
   `Provider`) instead of `anyhow`, so the UI can explain them.
7. **A provider trait**, so DNSimple and DigitalOcean are two implementations and more
   can be added.
8. **Async on tokio, no global state.** It should compile for Android and iOS targets
   (rustls only), even though Morph runs sync on desktop only.
9. **No schedulers inside the library.** Morph runs sync while open. The standalone
   systemd/launchd service stays a separate CLI concern. Morph should detect a
   standalone Grindvakt managing the same FQDN so the two don't fight.

Sketch of the surface Morph would call:

```rust
let provider = grindvakt_core::provider::digitalocean(token);        // or dnsimple(token, account_id)
let addrs = grindvakt_core::ip::discover(Family::Both, &sources).await?;  // or supplied
let want = [Record::a("home", v4), Record::aaaa("home", v6),
            Record::txt("_dnsaddr.home", weirdnet_net::dnsaddr_txt(...)),  // builder lives in weirdnet-net (W7)
            Record::cname("site", "home.example.com")];  // trailing dot optional; not at the apex, and not the same name as A/AAAA/TXT
let plan = grindvakt_core::plan(&provider, zone, &want).await?;
grindvakt_core::apply(&provider, &plan).await?;
```

Mobile: phones don't publish their own address records. A phone may write the stable
`_weirdnet.<name>` binding TXT (section 5b) with grindvakt-core and the user's token; its
changing addresses travel as Weirdnet peer records. Phones can manage DNS settings for the
user's desktop remotely (phase 2) and reach peers through relays. Public HTTPS (section 6b)
uses grindvakt-core for A/AAAA pointing at the host or ingress, for CNAME to an ingress
hostname (`Record::cname`, 1.1.0), and for DNS-01 `_acme-challenge` TXT records through
`weirdnet-tls`. A CNAME cannot share a name with `_weirdnet` TXT or A/AAAA, so the
user's own domain should CNAME to their dynamic DNS host name, not at the apex.

## 11. Gaps and who owns them

**Weirdnet** means an upstream contribution to the Weirdnet crates, released before Morph
uses it. **Morph** means work inside Morph. **Grindvakt** means App Maker's in-flight work
(section 10). **Weirdnet (model)** means `weirdnet-model`, shared with Mouser.

| # | Gap | Owner | Work |
|---|---|---|---|
| W1 | Hard caps (8 MiB / 256 MiB / 60 s) and no streaming | **Weirdnet** (`weirdnet-net`) | `stream` module: libp2p stream protocol with no size caps, backpressure, per-protocol flow `Limits` (frame, idle, concurrency, rate), sealed streams; request/response caps move into `NodeConfig` (section 5a) |
| W2 | No content transfer | **Weirdnet** (`weirdnet-net`) | `content` module: signed manifests, rsync-based (rolling checksum) chunking with deltas, BLAKE3-verified resumable chunks, ranges, multi-peer fetch, provider records |
| W3 | No lasting membership or roles on the wire | **Weirdnet** (`weirdnet-net`) | `Membership` credential (including a role id interpreted by W17) and revocations, verification, presentation |
| W4 | Real networks untested (TCP, QUIC, relays, hole punching, mDNS, AutoNAT) | **Weirdnet** | Multi-host test harness (network namespaces or containers with NAT), run in Weirdnet CI |
| W5 | Mobile and desktop platforms unverified; blocking handle | **Weirdnet** (`weirdnet-net`) | Android and iOS builds in CI; pause/resume and network-change handling for app lifecycle; async handle + `Status` stream; Windows/macOS runs |
| W6 | No relay/bootstrap pieces; relay v2 circuits limited by default | **Weirdnet** | A small relay/bootstrap binary (`weirdnet-relay`) on `relay_server`, relay preference (own devices first), reservation renewal. _(2026-11-11)_ Data-capable circuits: unlimited in phase 1 (rate limits and bandwidth controls for others are phase 2, changed 2026-11-11). A rendezvous server (W10). A WebSocket-over-TLS listener on 443 for UDP-blocked networks. An ingress listener (W12) |
| W7 | `/dnsaddr` dialing unverified; no helper for the TXT value | **Weirdnet** (`weirdnet-net::addr`) | `dnsaddr_record(name, port, peer)` helper + test that resolves and dials `/dnsaddr/<name>` |
| W8 | SQLCipher + vendored OpenSSL on Android/iOS, and binary size | **Weirdnet** (`weirdnet-storage`) | Cross builds in CI, size report, feature flags if needed |
| W9 | One node carrying several apps' protocols (Morph and Muninn on a shared network) | **Weirdnet** (`weirdnet-net`) | Register extra request/stream protocols per app on one `Node`; unknown protocols are refused cleanly |
| W10 | No way to find a moving peer by name or user id _(new 2026-11-11)_ | **Weirdnet** (`weirdnet-net::names`) | Signed `PeerRecord` (node cert → user key, seq, expiry, home relay, circuit + direct addrs); `_weirdnet.<name>` binding TXT builder/parser; rendezvous client + server; Kademlia record validator; push to and cache on trusted peers; DHT records only while hosting a site; signed `Withdrawal` tombstones that supersede by `seq` and expire as a backstop; encrypted, daily-blinded locator records for private sites (changed 2026-11-11) (section 5b) |
| W11 | ACME code duplicated in four projects _(new 2026-11-11)_ | **Weirdnet** (`weirdnet-tls`, new crate) | ACME (TLS-ALPN-01, HTTP-01, DNS-01 with a grindvakt-core feature), `CertStore` trait, SNI resolver with hot swap, ARI-based renewal, backoff, typed status, ring/aws-lc-rs features, mock CA (section 6b) |
| W12 | No public HTTPS path to hosts without a public address _(new 2026-11-11)_ | **Weirdnet** (`weirdnet-net::ingress`) | SNI-passthrough ingress: ClientHello SNI peek, signed `IngressGrant`, `/weirdnet/ingress/1` stream to the host, PROXY v2 header, HTTP-01-only port 80, per-grant byte and connection limits in phase 2 (changed 2026-11-11); no TLS termination; per-name grant revocation; per-device forwarded-site blacklist and a switch to turn off site forwarding (section 6b) |
| W13 | No sandboxed out-of-process extension host _(new 2026-11-11, phase 2)_ | **Weirdnet** (`weirdnet-ext`, `weirdnet-ext-proto`, `weirdnet-ext-sdk`) | Supervisor, per-OS sandbox (Landlock + seccomp + bubblewrap, macOS spike, AppContainer + Job Objects, Android isolated process), tonic over UDS/named pipes, WIT bridge for wasm with the same protobuf messages, `.morphext` packages, signing (section 7) |
| W15 | Relay → direct upgrade opens a second connection; app streams don't move _(new 2026-11-11)_ | **Weirdnet** (`weirdnet-net`) | Seamless path switching (iroh-style): keep a logical session across relayed and direct connections, migrate open W1 streams when DCUtR succeeds and fall back to the relay without resetting them, prefer direct while keeping the relay path warm; evaluate libp2p QUIC multipath support as it lands (section 5b) |
| W14 | No way to wake a suspended phone _(new 2026-11-11)_ | **Weirdnet** (`weirdnet-push`, new crate) + **WeirdBrains ops** | Wake-handle registration, sealed fixed-size wake payloads (reason code only), sender API used by relays and peers, handle revocation and rotation; unregister API for the per-device APNs/FCM off switch (on by default); self-hostable server binary in phase 1 with APNs (required for iOS background wake), FCM (Play builds), and a Weirdnet-relay wake path that is opt-in and off by default (F-Droid and other builds without Google services; Android foreground service, with a clear battery-cost explanation before opt-in); relays carry wake announcements originating from WeirdBrains; UnifiedPush in phase 2 (section 5b) |
| W17 | Access policy lives only in Morph _(new 2026-11-11)_ | **Weirdnet** (`weirdnet-access`, new crate) | Private/Public and Gated/Open as independent axes (`Gated` on the wire; Muninn keeps `members`/`closed` as CLI aliases only); a small shared permission core that apps extend; Muninn's rank rules and permission implications as the default model; configurable roles (name, rank, permissions, builtin flag); actors (owner, member, guest); host-side enforcement order (auth, visibility, membership, role, sealed vs plain streams); used by Morph and Muninn; no forum-specific types here (section 6) |
| W16 | No shared registry tying URL schemes to apps _(new 2026-11-11)_ | **Weirdnet** (`weirdnet-schemes`, new crate) + **WeirdBrains ops** | Signed prefix registration (required per app); prefixes beginning with `weirdnet` reserved for WeirdBrains and blacklisted for third parties; first-party `weirdnet-<app>://` (`weirdnet-morph://`, `weirdnet-muninn://`); signed `SchemeRecord` (scheme, prefix, app id, publisher key, seq, expiry, capabilities, install hint); canonical list signed with W18; local resolver; OS handler registration of *app* schemes only (`weirdnet://` is not registered); `join`/`answer` on the app scheme; missing-app fallback through Salty Gull, store or F-Droid hints (never auto-install); DHT and third-party prefixes later (section 5c) |
| W18 | No CLI to hold or rotate the WeirdBrains signing key _(new 2026-11-11)_ | **Weirdnet** (`weirdnetctl`, new CLI) + **ops** | A `weirdnetctl` CLI that Michael uses to hold and rotate the WeirdBrains signing key, and to sign reserved-prefix registrations and the canonical scheme list (section 5c) |
| W3+ | Request-to-join | **Weirdnet** (`weirdnet-net`, with W3) | `JoinRequest`, signed approval/denial, rate limits, host queue trait |
| S | HF client + downloads + llama.cpp shared between Mouser and Morph | **Weirdnet** (`weirdnet-model`, cross-project) | Extract from Mouser, switch Mouser over, release (section 8) |
| M-a | Morph-specific access glue | **Morph** (`morph-access`) over W17 | Resource types (projects, sites, Code Spaces, forums) and Morph role mappings (`owner`, `member`, later forum `moderator`) on top of `weirdnet-access`; uses W1, W3 and W17 |
| M-b | `weirdnet-morph://` mapping, per-site origins, scheme handlers | **Morph** (`morph-sites`, `morph_webview`) | Uses W2 + W16: register `weirdnet-morph://` (including `join/` and `answer/`) with the OS; do not register `weirdnet://`; in-page origins stay in `morph_webview` (section 5, 5c) |
| M-c | Mobile background limits (Android foreground service; no iOS hosting) | **Morph** | Platform integration, UI copy ("offline, last seen"), store-and-forward through own relay/desktop; phase 1 APNs/FCM push wake-up through W14 (on by default, per-device toggle in Settings, off = reachable only while open) with a Notification Service Extension on iOS; opt-in Weirdnet-relay wake path (off by default, battery-cost explanation, Android foreground service) for F-Droid; iOS background wake still requires APNs; phase 2 adds UnifiedPush (section 5b) |
| M-d | Extension system; `weirdnet-plugin` unsuitable | **Morph** (`morph-ext`) over W13 | `morph.ext.v1` services, capability policy, install UI; native + wasm fallback (section 7) |
| M-e | No Flutter platform views on Linux | **Morph** (`morph_webview_linux`) | Own WebKit plugin: DMA-BUF texture default (WPE public export → `FlTextureGL`) with webkit2gtk-4.1 offscreen snapshot fallback; hidden text layer (copying; early drift accepted, alignment tests planned); control layer over in-process gRPC; GTK 3 now / GTK 4 later; Wayland first, X11 later via XWayland; overlay last resort (section 2) |
| M-g | Code Spaces sharing semantics | **Morph** (`morph-projects`) | Uses W2 + storage sync; permissions from project (section 6a) |
| M-f | MSRV 1.99 pins Morph to latest stable | **Morph** | Track stable in CI |
| M-h | Public HTTPS hosting UI and policy _(new 2026-11-11)_ | **Morph** (`morph-sites`, `morph-access`) | Serve Public + Open sites over hyper + rustls with `weirdnet-tls`; Public + Gated card page; choose ingress (own relay / trusted peer / WeirdBrains relay) and issue grants; per-device forwarded-site blacklist and a switch to turn off site forwarding; uses W11 + W12 + grindvakt-core |
| M-i | Over-the-air updates _(changed 2026-11-11: decided)_ | **Morph** (client) + **Salty Gull** (service) | Morph consumes Salty Gull (plan by 0072, section 9a). Signed updates over Weirdnet, relayed announcements, opt-in per update or to all updates, no fees beyond server costs, self-hostable. Store builds: asset, downloaded-UI (RFW) and WASM updates; no Dart patches on iOS until the engine fork. Desktop and sideloaded builds: signed full updates. F-Droid uses stock Flutter, with updates off until the user opts in |
| U | Switch Muninn org policy to `weirdnet-access` _(new 2026-11-11)_ | **Muninn** | No behavior change: `Visibility` stays; `Access::Members` is `Gated` on the wire, with `members`/`closed` as CLI aliases only; built-in roles extend the shared permission core; rank rules and implications come from W17; `Org.gated` stays a Muninn site flag; MCP client roles stay local. Released through Muninn's release script after W17 ships |
| T | Switch existing ACME users to `weirdnet-tls` _(new 2026-11-11)_ | **Mouser, Muninn, itcmdr, TAUR** | Each project switches with no behavior change, released through the shared release script; after W11 ships |
| G | Grindvakt: no library boundary, IPv4 A only, TTY secrets, no TXT, `anyhow` errors | **Grindvakt** | Delivered as `grindvakt-core` 1.1.0 with CNAME (section 10); pin `=1.1.0`; verify in M6 |
| Ops | Public relay/bootstrap nodes, HTTPS forwarding, push service, scheme list | **WeirdBrains ops** | Deploy and monitor the W6 binary (including W12 ingress for public HTTPS, phase 1, with per-name revocation and a contact address for abuse reports) and the W14 push server (APNs/FCM credentials plus the relay wake path); host the W16 canonical scheme list, signed with `weirdnetctl` (W18); addresses and the reserved-prefix list ship in Morph's defaults |

## 12. Milestones (phase 1)

Each milestone ends with a demo on the listed platforms and CI building them. W-rows are
upstream Weirdnet releases; the Morph milestone after each one only consumes them.

| # | Milestone | Exit criteria |
|---|---|---|
| M1 | **Shell**: Flutter app + `morph_webview` (own WebKit plugin on Linux, `flutter_inappwebview` on Android) + `flutter_rust_bridge` | On Linux and Android: loads an https URL, back/forward/reload work, the address bar works, a Dart→Rust call returns `morph-core` version, a Rust→Dart event stream ticks, and a page→Rust→page message round-trips through `morph-bridge.js`, with the same bridge tests passing on both backends. Linux: pages render through the texture layer at a usable frame rate on the DMA-BUF path (measured on a scroll-heavy page and a video); when DMA-BUF is unavailable the snapshot-copy fallback still renders, and a written note records both costs plus NVIDIA/Mesa results and a go/no-go on the overlay last resort. Hidden-text-layer alignment tests cover scroll, reflow and zoom; residual early drift is accepted and is not a go/no-go. The control layer uses an in-process gRPC channel, routes clicks, keys and IME to the page in order, fires a page event into a Rust hook and back, and shows a script-drawn overlay. Input works on Wayland; X11 is later (XWayland is acceptable in phase 1). CI builds both. |
| M2 | **Navigation + core components** | Tabs, address bar with search, history list (session only), and panel host. The core component set lives in `morph_ui` with golden tests. Windows and macOS builds join CI. |
| M3 | **Key identity + store** | First run either creates the user key and node key and shows the 24 words, or links to an existing Weirdnet (Muninn) identity with the same user key. The SQLCipher store opens with the keyring + platform keystore. Recovery from words works on a clean install. Android and iOS cross builds of `weirdnet-storage` succeed and binary size is recorded. |
| S-a | **`weirdnet-model`** (cross-project, before M4) | `WeirdBrains/weirdnet-model` repo created, crate extracted and published on crates.io. Mouser switched over with its tests passing and no behavior change, shipped through Mouser's release process. Android and iOS targets build. |
| M4 | **Local AI chat** | HF search shows sizes and filters by device. Download is resumable. A chat streams tokens on Linux (Vulkan) and Android (CPU, small Q4 model). "Ask about this page" takes page text through the bridge only after the user acts. _(2026-11-11)_ Local AI is on by default on a fresh install. One toggle turns it off: the model unloads, inference and AI panels stop, and models stay on disk. One "Clear AI context and history" action removes all chats, page context, embeddings, memory and caches (test: store tables empty and cache files gone after `VACUUM`), and leaves notes, projects and models intact. |
| M5 | **Projects, Code Spaces, notes** | Create a project from a Rust template (Dart templates come later), add Code Spaces (local folders with ignore rules), and write notes linked to projects and pages. Everything persists in the store and is searchable. Sharing Code Spaces comes in M8. |
| W-a | **Weirdnet groundwork** (upstream, can run alongside M2–M5) | W4 real-network harness green in Weirdnet CI. W5 Android/iOS builds + async handle released. W8 storage cross builds released (M3 depends on it). |
| M6 | **Dynamic DNS** | Using the Grindvakt library: configure a provider, and Morph keeps A, AAAA and `_dnsaddr` TXT records current on desktop. A dry-run preview works. `NoGlobalAddress` shows "will use relays". The conflict with a standalone Grindvakt is detected. The `_weirdnet.<name>` binding TXT is written and verified. |
| W-b | **Weirdnet streaming + content** (upstream, before M7) | W1 `stream` with `Limits` and sealed streams, W2 `content`, W6 relay binary, W7 `dnsaddr` and W9 multi-protocol nodes, W10 names (with withdrawals and private locators) and W14 push, W15 path switching, W16 scheme registry and W18 `weirdnetctl` released; WeirdBrains push service deployed (`weirdnet-push`: APNs, FCM and the relay wake path); the canonical scheme list published, signed with `weirdnetctl`. WeirdBrains public relays/bootstrap deployed by ops. Request/response defaults unchanged, Muninn unaffected. Abuse tests (oversize frame, stream flood, slow reader, idle hold) pass. A multi-GB transfer completes, and a one-line edit to a large file resends only the changed chunks. |
| M7 | **Peers + weirdnet-morph://** | Two desktops on different networks and a phone exchange `weirdnet-morph://join/` invites (short codes match) and pin each other. With no own relay, a phone reaches a desktop through WeirdBrains relays. With a Grindvakt domain, the desktop's own relay is preferred. Direct hole punching happens when possible. A Muninn node on the same network is unaffected. `weirdnet-morph://<name>` resolves via `_dnsaddr` and renders a signed static site with its own origin on all platforms. _(2026-11-11)_ A phone is reached by name with no DNS address change: `_weirdnet` TXT → user id → `PeerRecord` from its home relay rendezvous. After it switches Wi-Fi → cellular, peers re-resolve within one republish interval. W4 records hole-punch rates for desktop↔phone and phone↔phone on real carriers. A backgrounded iPhone and an Android (Play) phone are woken by push when a peer sends them a message, and a captured push payload holds only the fixed-size ciphertext. An Android F-Droid build (no Google Play Services) is not woken over the Weirdnet-relay path on a fresh install: that path is opt-in and off by default. After the user opts in, having been shown a clear explanation of the battery cost, it is woken over that path. A backgrounded iPhone without APNs is not woken. _(2026-11-11)_ APNs/FCM push is on by default on a fresh install. With the per-device APNs/FCM toggle off, the device's token and handle are revoked at the push service, a peer's wake attempt does nothing, and the message is delivered when the app is next opened. Turning it on registers a new handle. Inspecting what APNs/FCM and the WeirdBrains push service receive shows only the fixed-size ciphertext and the delivery token or handle: no content, sender, peer id, address or site name. A transfer that starts relayed and upgrades to direct (W15) keeps its streams open. A `weirdnet-morph://` link (including `join/` and `answer/`) opened in another Weirdnet app launches Morph when it is installed; if it is not, the install hint is shown and nothing is installed until the user opts in (W16). `weirdnet://` is not registered with the OS. |
| W-c | **Weirdnet membership + access** (upstream, before M8) | W3 `Membership`, revocation and `JoinRequest` (approve/deny, rate limits) released with tests. W17 `weirdnet-access` published: both visibility/membership axes with `Gated` on the wire, a shared permission core that apps extend, Muninn rank and implication defaults, enforcement-order tests (stranger, trusted non-member, member, revoked member, guest on open vs gated). |
| M8 | **Project hosting + Code Space sharing** | All four visibility/membership combinations are enforced as in section 6, through `weirdnet-access`. Tests cover a stranger, a trusted non-member, a member and a revoked member. Gated content uses sealed streams only. Public + Gated: request to join → owner approves → content opens; deny → stays closed. A Code Space is published, fetched read-only by a member, and updated incrementally. A non-member can't fetch it. _(2026-11-11)_ DHT: while a site is hosted its record is findable; a trusted peer derives, fetches and decrypts a private site's locator record, and a stranger's node storing it can't link it to the user or site; when hosting stops, the withdrawal supersedes the record, and a replayed older record is rejected. |
| W-d | **Weirdnet TLS + ingress** (upstream, before M9) _(new 2026-11-11)_ | W11 `weirdnet-tls` published on crates.io with mock-CA tests for all three challenges and a DNS-01 test through grindvakt-core against the DigitalOcean/DNSimple sandbox. W12 ingress released with W6 ingress listener. Test: the relay process never has the certificate key, and a captured relay trace contains only TLS records. |
| M9 | **Public HTTPS** _(new 2026-11-11)_ | A Public + Open project is served to a stock browser at `https://<name>`: (a) from a desktop with a public address, directly; (b) from a host behind CGNAT through the user's own relay (SNI passthrough, DNS-01 cert); (c) the same through a WeirdBrains public relay for a user with no relay of their own. A Let's Encrypt staging cert issues, then a production cert, and it renews without a restart. Public + Gated shows only the card; Private has no listener; revoking a grant stops forwarding. A published contact address is documented for abuse reports about sites on WeirdBrains relays; ops can revoke forwarding for one name on those relays. A device can blacklist a forwarded name and refuse it, and can turn off site forwarding entirely. |

Phase 2 (order to be set later):
- third-party extensions (section 7): out-of-process native, with the wasm fallback on
  mobile; Rust SDK first, Dart later;
- drag-and-drop design mode;
- theming via `weirdnet-themes`;
- component polish;
- Student (with classifiers, section 8) and Researcher;
- device data sync;
- Code Space changes sent back by members;
- Code Space code reviews;
- peer chat and forums (same visibility/membership model);
- UnifiedPush as a user-selectable wake option (phase 1 already has APNs/FCM on by
  default, an opt-in Weirdnet-relay path that is off by default, self-hosting the
  same binary, and "reachable only while open");
- a modified Flutter engine fork (Dart interpreter for App Store patches, used by
  Salty Gull, and a FreeBSD/OpenBSD port); Salty Gull's plan (0072) owns the design
  (section 9a);
- Linux web view: GTK 4 / WebKitGTK 6.0 (and `GdkDmabufTexture` if useful), and
  native X11 beyond XWayland (section 2);
- relay bandwidth controls and rate limits (WeirdBrains relays, ingress grants, data
  circuits).

## 13. Open questions

The five questions from the first 2026-11-11 round are answered and written in above:
- WeirdBrains relays forward HTTPS (6b);
- the push service is phase 1 (5b, W14);
- DHT records only while hosting, with withdrawal (5b);
- the iOS App Store build allows WASM extensions (7, 9a);
- the macOS re-signed App Sandbox (7).

The later 2026-11-11 questions are also answered:
1. **F-Droid push:** write our own Weirdnet-relay wake path in phase 1 (self-hostable,
   no Google services). It is opt-in and off by default, with a clear explanation of
   the battery cost. UnifiedPush comes later. iOS background wake still requires
   APNs (5b, W14).
2. **Update path:** no Shorebird. Morph uses Salty Gull (plan by 0072). Store
   builds get asset, downloaded-UI (RFW) and WASM updates, with no Dart patches
   on iOS until the engine fork. Desktop and sideloaded builds get signed full
   updates. The F-Droid build uses stock Flutter, with updates off until the user
   opts in. Signed updates over Weirdnet, relayed announcements, opt-in per
   update or to all updates, no fees beyond server costs, self-hostable (9a, M-i).
3. **Abuse on WeirdBrains relay IPs:** a contact address, and per-site revocation by
   WeirdBrains on its own relays (WeirdBrains makes that call). Every device can
   blacklist a forwarded site or turn off site forwarding (6b, W12, M9).

The scheme-registry and access questions (4–10) are also answered:
4. **Third-party schemes:** every app registers a required, signed prefix. Prefixes
   beginning with `weirdnet` are reserved for WeirdBrains and blacklisted for
   third parties. First-party apps use `weirdnet-<app>://` (`weirdnet-morph://`,
   `weirdnet-muninn://`) (5c, W16).
5. **Publisher keys:** Michael holds and rotates the WeirdBrains signing key
   through a new `weirdnetctl` CLI (5c, W18).
6. **Several apps for one capability:** each app has its own prefixed scheme, so
   Morph invites are `weirdnet-morph://join/` and Muninn's are
   `weirdnet-muninn://join/`. They do not share an OS handler (5c).
7. **`weirdnet://` OS owner:** `weirdnet://` is reserved for internal Weirdnet
   messages only, to be decided later. It is not registered with the OS, and
   nothing routes through it. `weirdnet://open/` is dropped (5c).
8. **Naming on the wire:** `Gated` everywhere. Muninn keeps `members` and
   `closed` as CLI aliases only (6, W17).
9. **Permission catalogs:** `weirdnet-access` ships a small shared permission
   core that apps extend (6, W17).
10. **Ranks and implications:** Muninn's rank rules and permission implications
    are the crate's default model (6, W17).

Other questions will come out of M1 (DMA-BUF vs snapshot frame cost, NVIDIA/Mesa
fallback, format and Y-inversion), W-b/W4 (real-network and phone hole-punch
results), the macOS sandbox spike, Salty Gull's plan for the engine fork (Dart
interpreter and BSD port), and the later design of internal `weirdnet://`
messages. The exact names in the shared permission core can be settled when W17
is specified.

### 2026-11-12 — Real popup windows on Android (sign-in popups)

Google sign-in (GSI One Tap / "Continue with Google") calls `window.open`
and expects the popup to `postMessage` its opener and then `window.close()`.
Morph never enabled multiple windows, so Android WebView loaded the popup's
URL (`accounts.google.com/gsi/select?...`) into the tab itself. The opener
page was gone, nothing received the result, and the tab stayed blank white.

Decision: tabs set `supportMultipleWindows` and
`javaScriptCanOpenWindowsAutomatically`. `onCreateWindow` builds a second
`InAppWebView(windowId:)` in the **same profile** (its own keep-alive, so
`WebViewProfiles` assigns the tab's androidx.webkit profile before the
transport is set). It sits in a `MorphPopupStack` (at most 4) drawn by
`MorphPopupOverlay` over the opener, which stays mounted, so `window.opener`
and `postMessage` work. A bar shows "Title · host" and a close button.
`onCloseWindow` (`window.close()`) or the button removes it and the tab is
back. Nested popups stack. Rebuilding the tab's surface or closing the tab
closes its popups. A popup's renderer crash goes through the tab's renderer
watch, the same as the tab's own. Off Android (non-isolated sessions) we
return false and the engine keeps its default.

Verified on the emulator with a window.open + postMessage + window.close
fixture (before: same-tab, message lost; after: opener got the message, the
popup closed) and with linkedin.com → "Continue with Google", which now
opens accounts.google.com in a popup over LinkedIn.


### 2026-11-12 — Android share target and "Search in Morph"

Morph accepts `ACTION_SEND` and `ACTION_SEND_MULTIPLE` with `text/plain` (the
share sheet) and `ACTION_PROCESS_TEXT` through an activity-alias labelled
"Search in Morph" (the text-selection menu). `IncomingLinks.kt` pulls out
the text (`EXTRA_TEXT`, joined lines for SEND_MULTIPLE, `EXTRA_SUBJECT` as a
fallback, `EXTRA_PROCESS_TEXT`) and hands it to Dart on the existing links
channel: `initialShare` for the launch intent (cold start), `share` pushes
for `onNewIntent` (running; the activity is singleTop).

Dart owns the decision (`MorphIncomingLinks.sharedTarget`): the first
http(s) link in the text, with trailing punctuation trimmed; else the text
as a lone `host.tld/path`; else a web search. The result goes through the
same path as ACTION_VIEW links, so it opens in a new tab of the active
profile and reuses the tab if it is still an empty start page.

Morph has no search-engine setting yet; `searchUrl` uses DuckDuckGo
(`https://duckduckgo.com/?q=`, query whitespace-collapsed, capped at 2000
characters) until one exists. Non-text shares (images, files) are not
accepted.

### 2026-11-12 — Linux: the WPE path is opt-in until it has its own module

On a machine with wpewebkit installed, `morph_webview_linux_plugin` linked
both libWPEWebKit-2.0 and libwebkit2gtk-4.1. They export the same `webkit_*`
names, so the GTK web view's calls bound to WPE's copies and the first tab
crashed (SIGSEGV in `webkit_user_content_manager_register_script_message_handler`,
a few seconds after launch). CI never saw it: Ubuntu 22.04 has no WPE.

Decision: the WPE DMA-BUF path is built only with `-DMORPH_WEBVIEW_WPE=ON`.
By default the plugin links webkit2gtk alone and reports `wpe-not-built`
(flutter-gl, then snapshot). Step 3 (zero-copy WPE) must put the WPE renderer
in a separate library (or dlopen it with RTLD_LOCAL) so the two WebKits never
share a symbol namespace, then turn the option back on.

### 2026-11-12 — Address bar: phrases are searches

The shell prefixed `https://` to anything without `://`, so a phrase loaded
as a percent-encoded URL and WebKit showed "The URL can't be shown".
Decision: `omniboxTarget` (shared by every platform) opens text as an address
only when it looks like one: http(s) with a host and no spaces, `about:`,
`file:`, `data:`, `view-source:`, `localhost[:port]`, an IPv4 address, or a
bare `host.tld[:port][/path]`. Everything else, including any input with a
space and unknown schemes such as `javascript:`, is a DuckDuckGo search
(`MorphIncomingLinks.searchUrl`) until a search-engine setting exists.

### 2026-11-12 — Linux: Morph error pages, popups, hang grace

- *Error pages.* The plugin handles `load-failed` and
  `load-failed-with-tls-errors`, returns TRUE (no WebKit plain-text page) and
  sends a `load-error` event (domain, code, message, tls). Cancelled loads,
  downloads (policy change) and plugin handoffs keep the current page. Dart
  maps errors to kinds (certificate, not found, unreachable, timed out,
  other); `MorphLoadErrorGate` shows a Morph page with "Try again" (and
  "Search instead" for an unknown host). There is no "proceed anyway" for bad
  certificates. Any navigation clears it. Android keeps its engine page for
  now.
- *Popups.* WebKit's `create` already made a related view, but Dart never
  showed it, so `window.open` loaded invisibly (sign-in popups, "Log in with
  …"). The `new-window` event now carries the opener, texture and render
  path; the Linux session wraps it as a session of its own and the
  controller stacks it in `popups` (cap 4, same as Android; the native side
  refuses a fifth). `MorphPopupLayer` draws it over the tab with "Title ·
  host" and a close button; `window.close()` sends `closed`. Nested popups
  stack on the same tab. Same profile and context (related view), so
  `window.opener` and `postMessage` work. Windows and macOS: no popup support
  yet (native views, to do with their backends).
- *Hang grace.* WebKit flags a renderer unresponsive after ~3 s, which heavy
  pages (a YouTube watch page starting its player) hit routinely. The tab now
  says "isn't responding" only if it is still unresponsive after 10 s
  (`MorphWebViewController.unresponsiveGrace`); a crash still shows at once.

### 2026-11-12 — Linux: WebKit's DMA-BUF renderer stays on; video crash fixed

The plugin set `WEBKIT_DISABLE_DMABUF_RENDERER=1` on NVIDIA. With it,
WebKitGTK 2.54 aborts the web process (SIGABRT from a DOM binding called by
the player's JS) on YouTube watch pages and Facebook stories; reproduced in
plain WebKitGTK (no Morph) with only that variable: crash in 13-38 s, while
without it the same page decodes (readyState 4, 1280 px). GStreamer was
complete (gst-libav, good/bad/ugly, nvcodec); ranking the nv/vulkan decoders
to 0 did not help. Decision: Morph no longer sets it (WebKit has its own
NVIDIA fallbacks); `MORPH_WEBKIT_DISABLE_DMABUF=1` opts back in. A crash
event carries a hint: missing GStreamer formats (registry checked through a
run-time `dlopen` of libgstreamer, no build dependency) or that variable being
set. Package list in `docs/linux-media.md`.

Crash pages were also 3-9 s late: the web process is only gone once
systemd-coredump has written its core (gigabytes for a video page). The plugin
sets `RLIMIT_CORE` to 0 at startup, inherited by web processes;
`MORPH_KEEP_CORE_DUMPS=1` keeps them.

### 2026-11-12 — Linux: NVIDIA Wayland startup abort

On a Hyprland/NVIDIA session, Morph aborted during `FlView` realize:

`GDK is not able to create a GL context: The current backend does not
support OpenGL`. Flutter 3.47 Impeller (OpenGLES) needs that context;
GTK 3's Wayland backend on the proprietary driver does not provide one.
The runner now, before `gtk_init`, sets `GDK_BACKEND=x11` when
`WAYLAND_DISPLAY` and `DISPLAY` are both set, the NVIDIA proprietary
driver is present (`/proc/driver/nvidia`), and `GDK_BACKEND` is unset.
X11-only sessions and non-NVIDIA Wayland are unchanged. Override with
`GDK_BACKEND`. `run-morph.sh` stays in the foreground and restores the
tty if Morph aborts.

### 2026-11-12 — Linux: passkeys fail fast

WebKitGTK ships without WebAuthn (`PublicKeyCredential` and
`navigator.credentials` are undefined), and Facebook's "Use face scan,
fingerprint or screen lock" step spun forever. A document-start shim (all
frames, secure contexts, only where the engine lacks the API) defines them:
`get`/`create` with `publicKey` reject at once with `NotAllowedError` (what a
dismissed prompt gives), and platform/conditional availability report false,
so sites offer their other methods. Real passkeys need a native
authenticator bridge: CTAP2 over USB HID (libfido2 or
`webauthn-authenticator-rs`) and hybrid/caBLE v2 (QR with the FIDO URI, BLE
advert via BlueZ, the tunnel WebSocket, Noise handshake), exposed to pages
through this shim; phone-as-authenticator then works like Chrome's QR flow.

### 2026-11-12 — Linux: damage-driven presents, no copies for hidden tabs

Measured in Xvfb (dpr 2, 5120×2500 texture): one present copies 51 MB and
took 18.9 ms on the main thread, and every tab presented on every tick once
live, hidden tabs included; the main thread also waited on the raster
thread's upload lock during its copy. Decisions: (1) a view copies only when
Flutter drew its last frame (`frame_consumed`), so Offstage tabs stop copying
until shown; (2) live presents follow the offscreen window's `damage-event`
once one has been seen (unchanged frames are not copied; views that never
report damage keep presenting every tick); (3) the copy into the back buffer
runs without the lock, which now only covers resize and the swap. Navigations
paint a dark backdrop from load start until 400 ms after commit (or load
finish), so reloads and sign-in redirects no longer flash white. Zero-copy
(WPE DMA-BUF import) remains the real fix for the per-frame copy and upload
at HiDPI sizes.

### 2026-11-12 — Android: pre-raster for the visible tab, idle buffering for hidden ones

Fast flings on long pages reached unpainted tiles. The visible tab now asks
WebView to pre-raster beyond the viewport (`offscreenPreRaster`) at an
IMPORTANT, never-waived renderer priority. With texture-layer composition the
system always considers every WebView visible, so it never lowers a hidden
tab by itself: a tab hidden for 45 s (`MorphWebViewController.idleAfter`)
drops to `MorphBuffering.idle` (no pre-raster, waived priority,
`WebView.onPause`; timers keep running, so background audio continues) and
gets full buffering back the moment it is shown again. Measured on
emulator-5600 (API 37, swiftshader, arm64 translation), 12 flings over the
scroll fixture, then four tabs with three hidden for 65 s or more: delivered
frames 176/176 → 223/195, p50 frame interval 48.5 → 46 ms; total PSS
(app + renderer) at t0 430–432 → 435–441 MB (pre-raster cost), after the
hidden tabs went idle 419–421 → 422–423 MB, so idle hands back what
pre-raster costs. The emulator renders in software; checkerboarding and
real frame pacing need a device run.

### 2026-11-12 — Android: Morph error pages

Android showed Chromium's own error pages, while Linux showed Morph's.
`disableDefaultErrorPage` is now on and main-frame `onReceivedError` maps
to the shared `MorphLoadErrorKind`s (`load_errors.dart`), so
`MorphLoadErrorGate` shows the same pages everywhere. Cancelled loads and
schemes handed to other apps show nothing. Android reports a bad certificate
only through `onReceivedSslError`, so that hook (Android only; Apple
platforms ask it for every TLS connection) raises a certificate page when
the failing host is the page's own, and still cancels the load.

### 2026-11-12 — Android: hybrid composition for web views

Flings stuttered on a Pixel 9 Pro XL (1e9edbb): the page repeated a frame and
then jumped ahead. Frame-by-frame analysis of that 120 Hz recording: in fast
flings 44% of displayed frames had the page out of step with elapsed time
(23% stale, 21% catch-up jumps up to 4-5 frames' worth), while the Flutter
chrome stayed in place in every frame and nothing stalled (the end of the
recording is the fling settling, then a still screen). The cause is
texture-layer composition: the WebView draws into a SurfaceTexture on
Android's UI thread and Flutter's raster thread samples it on its own clock.
Web views now use hybrid composition (the WebView in the Android view
hierarchy, composited by SurfaceFlinger) with no forced hardware layer;
`LAYER_TYPE_HARDWARE` is what froze hybrid views after a resize, a second
load or a resume on Flutter 3.47, and without it those cases keep painting
(checked on the emulator, along with tab switching and tab previews).
emulator-5600 (API 37, swiftshader, x86_64 release builds), six scripted
flings over a long article, frame motion from `screenrecord`: single-frame
jumps over 2.5× the expected step 5.7–11.8% (5 runs) → 1.7–2.4% (3 runs);
frames whose content moved too far to match 13–74 → 5–19; total PSS
232 → 237 MB. Flutter 3.47 ignores `EnableImpeller=false` on Android, so
Skia is not an option. While a hybrid view is on screen Flutter rasters on
the platform thread; Hybrid Composition++ (`initHybridAndroidView`, Vulkan
and API 34+, `EnableHcpp`) avoids that, but flutter_inappwebview 6.1 only
creates texture-layer or hybrid views, so it needs a plugin change. On Android 9 and older, hybrid composition
copies each Flutter frame, which is the accepted cost.

### 2026-11-12 — Login over QR (device linking) is built

`weirdnet-device-link` (in-tree until WeirdBrains/weirdnet-device-link
exists) carries the protocol of `docs/identity-recovery.md` section 4;
`morph-identity::device_link` ties it to profiles: the new device stores the
seed as `Source::Linked` plus its own password-sealed node key and the node
cert the user key signed. The new device checks the password and that the
profile has no identity before it takes the transfer, so a typo does not
spend the single-use link. UI: "Link from another device" in onboarding (QR
from the pure-Dart `qr` package plus the link text), "Add device" in
Identity. Android scans with CameraX and ZXing core (FOSS, in-app activity,
no Play Services; about 0.3 MB). The bigger cost is the
network stack: libp2p in `libmorph_bridge.so` took the arm64 APK from
11.6 MB (5adb4a2) to 14.8 MB, still under the 25 MB limit. Transport is direct TCP/QUIC only for now:
mDNS is off and no relays are set, so both devices need to reach each other.

On Android, every real-network Weirdnet node failed with "the node did not
start in time": libp2p's DNS transport (hickory-resolver) reads Android's DNS
servers through ndk-context and panics when nobody initialised it, and the
panic killed the node thread before it reported ready. MainActivity now hands
the application Context to the bridge (`RustContext.kt`,
`morph-bridge/src/android.rs`), and the app holds `ACCESS_NETWORK_STATE`,
which that lookup needs. Checked end to end: emulator (existing device)
scanned/pasted the link of a desktop bundle (new device), both showed the same
code, and the desktop profile ended with the same user id, source "linked".

### 2026-11-12 — Android: surface checks stay out of scrolling

After hybrid composition (5adb4a2) a Pixel 9 Pro XL scroll had 22 janky
frames in 4,001 (0.55%, p99 14 ms) and a handful of 73-200 ms frames. Every
one lined up with `Bitmap.compress` on the main thread, which in hybrid
composition also draws the web view: the blank-surface check's screenshot.
The checks came in pairs: a first capture that encoded in 3-5 ms (a flat
frame, taken mid-fling) and, after the confirm delay, a second that took
29-153 ms as a quality-100 PNG of the real page. Three changes: a check waits
until the page has gone 1 s without a scroll step (`onScrollChanged`, which
the plugin sends anyway) before each look, and gives up on a page that keeps
scrolling (it is evidently painting); the shell only asks for a check after
a real load (a loading event, then the finished page), not for the repeated
"progress 100", title and same-page URL updates Android sends while reading;
the capture is a JPEG, which keeps a flat frame flat within the tolerance
and any text visible. The flat-frame test itself stays strict.

