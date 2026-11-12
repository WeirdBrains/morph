#ifndef MORPH_WEBVIEW_LINUX_RENDER_PATH_H_
#define MORPH_WEBVIEW_LINUX_RENDER_PATH_H_

#include <glib.h>

G_BEGIN_DECLS

typedef enum {
  MORPH_RENDER_PATH_WPE_DMABUF = 0,
  MORPH_RENDER_PATH_FLUTTER_GL = 1,
  MORPH_RENDER_PATH_SNAPSHOT = 2,
} MorphRenderPath;

// Chooses the texture path. Caller must g_free() *reason.
MorphRenderPath morph_detect_render_path(gchar** reason);

const gchar* morph_render_path_name(MorphRenderPath path);

// Whether to disable WebKit's internal DMA-BUF renderer. Only when
// MORPH_WEBKIT_DISABLE_DMABUF=1: with it disabled WebKitGTK 2.54 crashes on
// video, so the old NVIDIA default is gone. True DMA-BUF import into Flutter
// is the WPE path on Mesa.
gboolean morph_should_disable_webkit_dmabuf_renderer(void);

gboolean morph_has_nvidia_proprietary_driver(void);
gboolean morph_has_drm_render_node(void);
gboolean morph_has_egl_dma_buf_import(void);

G_END_DECLS

#endif  // MORPH_WEBVIEW_LINUX_RENDER_PATH_H_
