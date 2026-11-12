# Priorities 1

Adopted 2026-11-11, replacing the earlier list. Same items, reordered so each step lands in a working
app. Changes are marked _(changed)_. Items from Michael's 2026-11-11 feedback are marked
_(changed 2026-11-11)_ or _(new 2026-11-11)_; details are in ARCHITECTURE.md (sections 5b,
6b, 7 and 8).

Implementation started 2026-11-11 on M1's first slice: the Rust workspace, Flutter shell,
flutter_rust_bridge wiring, and Weirdnet 0.1.0 identity helpers. As of 2026-11-12 the
Linux `morph_webview` plugin loads pages through the webkit2gtk-4.1 snapshot texture
fallback (WPE DMA-BUF is feature-detected and compiled when the libraries are present).
The Android in-app web view, DMA-BUF frame-rate measurements, hidden-text-layer
alignment tests, and a fully wired in-process gRPC control stream are still ahead in M1.

1. Webview _(moved up: everything else plugs into it)_
    - System web views: WebView2 (Windows), WKWebView (Apple), WebKit (Linux: WPE + webkit2gtk), Android WebView _(changed 2026-11-11)_
        - Linux: build our own WebKit plugin _(changed 2026-11-11)_
            - GTK 3 (webkit2gtk-4.1) now; GTK 4 (WebKitGTK 6.0) later
            - Render the page into a texture: DMA-BUF sharing is the default, with
              offscreen snapshot copying as the fallback (ARCHITECTURE.md section 2)
            - Wayland first; X11 later, initially via XWayland
            - Hidden text layer, for copying (early drift is accepted; alignment
              work and tests stay planned)
            - Control layer, to interact with JavaScript and Rust scripts, over a
              gRPC channel
        - Other platforms: flutter_inappwebview behind the same Morph interface _(changed)_
    - Navigation components
2. UI Components (a plugin system)
    - Build a core set of components for the browser, chat, notes and projects _(changed: core set first)_
3. Key Identity (a plugin system)
    - Use key identities (Weirdnet: `weirdnet-keys`, `weirdnet-net` identities and user certificates)
    - Same keys as Muninn: one identity across Weirdnet apps _(changed)_
    - Recovery words and linking your own devices
4. Local AI models
    - Optional: on by default, with one toggle to turn it off _(new 2026-11-11)_
        - Off means no model loaded, no inference, no AI panels or suggestions; downloaded models stay on disk unless removed
        - One "Clear AI context and history" action wipes the local model's stored context, chats, embeddings and memory
        - Separate from browsing-history collection for the Student layer, which stays opt-in (off by default)
    - Integration
        - Share Mouser's Hugging Face and model code through `weirdnet-model` (new Weirdnet crate) _(changed)_
        - Search Hugging Face
        - Download models
        - Embed inference engine
        - Mobile: small quantized models only _(changed)_
    - Chat
        - build chat component
5. Projects
    - Manage projects
        - Support Rust first; Dart later _(changed 2026-11-11)_
            - A system web view can't host the Dart VM, so Dart can't run "inside the browser" as a
              Morph runtime. Dart reaches the page only as compiled JavaScript or WasmGC, with
              page privileges only (ARCHITECTURE.md section 7, "Dart")
            - Dart arrives with out-of-process extensions in Priorities 2 (`dart compile exe`)
        - Templates (Rust first)
        - Code Spaces: a collection of code within a project (a grouping of repos and code folders) _(defined)_
            - Local folders, shared with peers _(defined)_
            - Sharing follows the project's visibility and membership _(changed)_
            - Peers get read-only copies; sending changes back comes later _(changed)_
        - Notes
6. Be decentralized
    - Dynamic dns (Grindvakt) _(before peer to peer: stable names are the groundwork for sharing)_
        - Manage dns entries against
            - DNSimple
            - Digital Ocean
        - A and AAAA (IPv6) records _(changed)_
        - Desktop: address records; phones and carrier NAT go through relays _(changed)_
        - Mobile: bind the name, don't track the address _(new 2026-11-11)_
            - DNS holds a rarely-changing `_weirdnet.<name>` TXT record that binds the name to
              your user key, not the phone's IP address
            - The phone's current address (direct addresses + its relay address) travels
              through Weirdnet as a signed peer record: trusted peers, your home relay
              (rendezvous) and, only while you host a site, the Kademlia DHT. Peers resolve the
              name, then dial the peer id
            - DHT records only while a site is hosted, private sites included, and withdrawn
              with a signed removal record when hosting stops _(changed 2026-11-11)_
                - Private sites: encrypted records only trusted peers can find and read
            - Hole punching first (DCUtR; roughly 70-80% success, much less behind symmetric
              carrier NAT), relays as the fallback that always works
            - Phones are reachable only while Morph is open (iOS) or a foreground service runs
              (Android)
            - A WeirdBrains push service wakes phones, carrying only small encrypted wake-ups
              _(changed 2026-11-11)_
                - APNs/FCM push is on by default; a clear per-device toggle in Settings turns
                  it off, falling back to reachable only while open
                - Apple/Google and our service see only an encrypted, minimal wake payload with
                  no content
                - Phase 1 also offers wake-ups over Weirdnet relays (self-hostable, no Google
                  services) so F-Droid builds can stay reachable. That keeps a background
                  connection to the home relay through an Android foreground service; it is
                  **opt-in, off by default**, with a clear explanation of the battery cost.
                  iOS background wake still needs Apple's push service (APNs). UnifiedPush
                  comes later
    - Peer to peer web client and server (Weirdnet)
        - Share Muninn's Weirdnet network _(changed)_
        - Stay on libp2p; contribute key-only dialing, data-capable relays and seamless
          relay/direct path switching to Weirdnet _(agreed 2026-11-11)_
        - Relays for peers DNS can't reach _(changed)_
            - WeirdBrains runs public relays. In Priorities 1 they also forward public HTTPS for
              users without their own relay, with no bandwidth limits yet _(changed 2026-11-11)_
            - Relays also host the name lookup (rendezvous) for devices holding a slot there
              _(new 2026-11-11)_
            - Running your own relay is optional, and needs a domain tied to your browser (Grindvakt)
        - Missing transports get contributed to Weirdnet, not built in Morph _(changed)_
            - Straight to main, no PRs; Morph tracks Weirdnet main while developing and pins crates.io versions for releases
        - Streaming with rsync-based chunking and no size caps (Weirdnet) _(changed)_
    - Anything reusable beyond Morph becomes a weirdnet-* crate; morph-* stays Morph-specific _(changed)_
        - weirdnet-morph:// protocol for browsing peer sites _(moved from Webview)_
        - A Weirdnet scheme registry ties signed, prefixed URL schemes
          (`weirdnet-morph://`, `weirdnet-muninn://`) to apps so every Weirdnet app
          resolves and broadcasts them the same way. `weirdnet://` is reserved for
          internal messages and is not registered with the OS
          _(changed 2026-11-11; ARCHITECTURE.md section 5c)_
7. Host projects
    - Private (default) or Public
        - Private: not visible outside trusted peers
        - Public: visible to anyone with the link and to all peers
        - Public projects can also be served over HTTPS to anyone with a normal browser, at your
          own dynamic DNS name _(changed 2026-11-11: replaces "Morph users only, no web gateway")_
            - Let's Encrypt certificates through `weirdnet-tls`, a new shared crate that
              combines the ACME code from Mouser, Muninn, itcmdr and TAUR, with DNS-01 through
              grindvakt-core _(agreed 2026-11-11)_
            - When the host has no public address, traffic is routed through your own relay, a
              trusted Weirdnet peer or a WeirdBrains public relay _(changed 2026-11-11)_, which
              reads only the TLS server name and forwards the encrypted bytes (TLS stays end to
              end; the relay never holds the key)
            - Abuse reports about sites forwarded through WeirdBrains relays go to a contact
              address; WeirdBrains may revoke forwarding per site on its own relays. Every
              device can blacklist a forwarded site or turn off site forwarding
              _(new 2026-11-11)_
            - Still no WeirdBrains-run gateway or shared WeirdBrains domain: the site keeps your
              domain and TLS terminates on your host _(changed 2026-11-11)_
            - Public + Open: the full site; Public + Gated: only the public card; Private: never
    - Gated or Open (default)
        - Gated: requires membership
        - Open: no membership needed
    - Visibility and membership are separate settings _(defined)_
    - Public + Gated: non-members see the title, owner and description, and can ask to become a member, moderated by the owner _(changed)_
    - This access policy lives in `weirdnet-access`, so Muninn and Morph behave the
      same way: `Gated` on the wire (Muninn keeps `members`/`closed` as CLI aliases
      only), a small shared permission core that apps extend, and Muninn's rank
      rules and permission implications as the default model. Morph-only glue
      (including forums later) stays in `morph-access`
      _(changed 2026-11-11)_
    - Add Notes to projects
8. App updates _(changed 2026-11-11)_
    - Updates come from Salty Gull (plan by 0072); Morph uses it, rather than
      designing its own
    - Signed updates, distributed over Weirdnet; every relay can carry update
      announcements from WeirdBrains so devices behind NAT can receive them
    - Never forced: the user opts in per update, or opts in to all updates
    - No fees beyond our own server costs; self-hostable
    - Store builds: asset, downloaded-UI (Remote Flutter Widgets) and WASM updates;
      no Dart patches on iOS until the engine fork
    - Desktop and sideloaded builds: signed full updates
    - F-Droid uses stock Flutter, with updates off until the user opts in
    - Sideloading is always a fallback on Android and iOS

# Priorities 2

- Extensions _(new; redesigned 2026-11-11)_
    - Not only WASM: platform-native binaries too _(changed 2026-11-11)_
        - Downloaded and installed only with your permission, signed by the author's key
        - Each extension runs in its own sandboxed process
        - Talks to Morph over message-passing RPC with a gRPC design: protobuf schema as the
          contract, tonic over a local Unix socket or named pipe
        - So Dart or any compiled language can be used
    - WASM is the fallback where native code can't be downloaded (iOS, Android) _(changed 2026-11-11)_
        - The iOS App Store build allows downloadable WASM extensions; sideloading is always a
          fallback _(changed 2026-11-11)_
    - macOS: native extensions are re-signed into the App Sandbox (spike first), with
      `sandbox-exec` as the fallback _(changed 2026-11-11)_
    - Rust first; the Rust SDK ships before the Dart SDK _(changed)_
    - Phase 1 builds the same hooks for Morph's own features first
- Linux web view: GTK 4 (WebKitGTK 6.0), and native X11 beyond XWayland
  _(moved later, 2026-11-11)_
- Relays: bandwidth controls and rate limits for WeirdBrains relays and HTTPS forwarding
  _(moved from Priorities 1, 2026-11-11)_
- Phone wake: UnifiedPush support _(moved later, 2026-11-11; phase 1 uses APNs/FCM on
  by default, and an opt-in Weirdnet-relay wake path that is off by default)_
- Modified Flutter engine fork (later): a Dart interpreter for App Store patches (used
  by Salty Gull) and a BSD (FreeBSD/OpenBSD) port; design owned by Salty Gull's plan
  _(new 2026-11-11)_
- Themeing
    - Allow all components to be themed
- UI Components
    - Build a drag a drop design mode to allow the user to layout where they want components, and what to hide/show by default _(moved from Priorities 1)_
    - Polish
- AI integration
    - Student
        - Add a layer of memory/cache in front of the inference engine
        - Collect browsing history and summaries: opt-in, inspectable, deletable _(changed)_
            - Classify this: run classifiers over the data to label and structure it for later AI
              use (topic, content type, entities, project relevance, sensitivity), with labels
              you can see and correct _(defined 2026-11-11)_
    - Researcher
        - Start a research journal
        - AI researches across
            - The web
            - Peers
            - Chats
                - AI
                - Peers
            - Resource from student layer
        - Keeps entries and footnote notes in journals dedicated to this
            - Started by user as a project
            - Feeds back data to project for actionable items
- Projects
    - Code Spaces: members send changes back to the owner _(moved from Priorities 1)_
    - Code Spaces: code reviews on changes sent back by members _(added 2026-11-11, later date)_
- Decentralization
    - Chat
        - Chat with other peers you know or discover
        - Supports media types
        - Timestamped
        - Searchable
    - Forums
        - Support for forum style system
        - All users are peers
        - Can be public (default) or private (same meaning as hosting)
        - Gated or Open (default) (same meaning as hosting)
        - Forums stay Morph-only: `morph-access` maps forum resources and roles
          (including moderator) onto `weirdnet-access` _(new 2026-11-11)_
