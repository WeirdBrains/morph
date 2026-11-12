# morph_webview

Dart interface for Morph's web view. Linux uses `morph_webview_linux`
(WPE DMA-BUF when feature detection allows, otherwise a webkit2gtk-4.1
offscreen snapshot texture). Other platforms will use
`flutter_inappwebview` behind this same API.
