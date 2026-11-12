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
  `Session` stream over a Unix domain socket (loopback TCP on Windows). The
  Linux plugin keeps the method channel for create/dispose/resize/texture and
  routes navigation, input, and page-bridge traffic through the control
  server, which handles page messages in Rust.
