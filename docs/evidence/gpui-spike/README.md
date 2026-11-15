# GPUI + wry spike (2026-11-14)

Evidence for ARCHITECTURE.md section 2.0. Throwaway code, not part of the
workspace and not the phase 1 design: one GPUI window (Zed `703b5ac`) with a
webkit2gtk child web view from wry 0.57.2, an X11 handle shim (GPUI gives an
XCB handle, wry wants Xlib), GTK pumped from a GPUI timer, and a `window.open`
handler that makes a native GTK popup related to the opener.

Run it only in a virtual display:

```sh
export CARGO_TARGET_DIR=<scratch-dir>/cargo-target/morph-spike
cargo build --manifest-path docs/evidence/gpui-spike/Cargo.toml
xvfb-run -a -s '-screen 0 1280x800x24' env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET \
  "$CARGO_TARGET_DIR/debug/morph-gpui-spike" "file://$PWD/docs/evidence/gpui-spike/page.html"
```

Title changes print to stderr (`TITLE`, `PTITLE` for the popup, `POPUP` for
`window.open`). What was checked with `xdotool` is in ARCHITECTURE.md
section 2.0.
