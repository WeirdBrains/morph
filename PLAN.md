# Morph v1

Morph is a distributed web browser that has intelligence built in. It's your research assistant, rubber duck, and journaler. Morph tends to your long term and short term web needs. It changes the way you view and interact with the world.

Adopted 2026-11-11. Build order is in `PRIORITIES.md`. Technical detail is in `ARCHITECTURE.md`.

# Overview

Morph is a web browser with local AI embedded, designed from the ground up, built on a solid foundation. It's more than just a web browser: it's standards driven, support W3 standards at it's core; it's safe, built in Rust from the ground up, with a Flutter shell; it's competent, with AI that builds context about you as you interact with it; it's the place where you projects start, with a built in projects management suite; it's decentralized, opening up the possibillities of peer to peer web for you, your friends, and all the connections that you find along the way, under `weirdnet-morph://`; it's more than the web, bringing in chat, projects, notes, and more; it's secure, built on top of a strict key and cert exchange that puts you in the control seat; it doesn't leak, all your data is private and yours; it looks like yours, with themes and swappable components to fit the way you work and live; it's scriptable, with a Rust language interface into the page through the web view's control layer, and Dart reaching the page only as compiled JavaScript or WasmGC.

# Features

- Standards based Web Browser
    - System web views. Linux: WPE DMA-BUF default, webkit2gtk-4.1 snapshot-copy fallback, Wayland first, control layer over gRPC (ARCHITECTURE.md section 2)
- Projects creation lab, code editor, and server
    - Rust first; Dart later
    - Private (default) or Public, Gated or Open; policy in `weirdnet-access`, Morph glue in `morph-access`
    - Public projects can be served over HTTPS at your own dynamic DNS name
- AI integration, chat, student, and researcher
    - Local models on by default, with one toggle to turn them off
- Decentralized network server, relay, and client
    - Same keys as Muninn: one identity across Weirdnet apps
    - Peer sites at `weirdnet-morph://`, through the `weirdnet-schemes` registry
    - Our own push service first; UnifiedPush later
- Dynamic dns client: tie a domain to your browser, whever you are
- Note taker and journaler
- Online chat and forums server
    - Forums stay Morph-only
- Themable components
- Swappable UI
- Signed app updates through Salty Gull, never forced

# Technologies

- Rust (2024 edition)
- Flutter (Windows, macOS, Linux, iOS, Android, and F-Droid)
- Weirdnet, including `weirdnet-access`, `weirdnet-schemes`, `weirdnet-tls`, and `weirdnet-model`
- Grindvakt (`grindvakt-core` 1.1.0, including CNAME)
- Salty Gull, for updates
- `weirdnetctl`, to hold and rotate the WeirdBrains signing key
