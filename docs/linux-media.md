# Video and audio on Linux

Morph's Linux build plays media through WebKitGTK, which decodes with the
system's GStreamer. Chrome and Firefox bundle or use their own decoders, so a
site playing there does not prove GStreamer can.

## Packages

| Distribution | Install |
|---|---|
| Arch | `gst-plugins-base gst-plugins-good gst-plugins-bad gst-libav` (optional: `gst-plugins-ugly`, `gst-plugin-va` for VA-API) |
| Debian, Ubuntu | `gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-libav` |
| Fedora | `gstreamer1-plugins-good gstreamer1-plugins-bad-free gstreamer1-libav` (RPM Fusion for H.264) |

What web video needs, and the elements that cover it (any one is enough):

| Format | Elements |
|---|---|
| H.264 | `avdec_h264`, `openh264dec`, `nvh264dec`, `vah264dec` |
| VP9 | `vp9dec`, `avdec_vp9`, `nvvp9dec`, `vavp9dec` |
| AV1 | `av1dec`, `dav1ddec`, `nvav1dec`, `vaav1dec` |
| Opus | `opusdec` |
| AAC | `avdec_aac`, `fdkaacdec`, `faad` |
| WebM / MP4 | `matroskademux` / `qtdemux` |

Check with `gst-inspect-1.0 <element>`. When a tab crashes, Morph checks the
registry and names any format with no element on the crash page.

## Known crash: WEBKIT_DISABLE_DMABUF_RENDERER

With `WEBKIT_DISABLE_DMABUF_RENDERER=1`, WebKitGTK 2.54 aborts the web process
on video pages (YouTube watch pages, Facebook stories): "This page crashed"
within seconds of the player starting. Morph used to set it on NVIDIA; it no
longer does. If your environment sets it, unset it. `MORPH_WEBKIT_DISABLE_DMABUF=1`
restores the old behaviour for debugging only.

## Debugging

- `MORPH_WEBVIEW_DEBUG=1` logs navigation decisions, load events and load
  failures with timings (scheme and host only).
- `MORPH_WEBVIEW_STATS=1` logs frame presents and input-to-present lag.
- `MORPH_KEEP_CORE_DUMPS=1` keeps web-process core dumps. Morph turns them
  off by default because a crashed page is only reported once its core (several
  GB for a video page) is written, which delayed the crash page by 3-9 s.
- `GST_DEBUG=3` for GStreamer; web-process output goes to the systemd journal
  (`journalctl --user -t WebKitWebProcess`).
