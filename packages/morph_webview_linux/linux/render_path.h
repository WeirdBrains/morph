#ifndef MORPH_WEBVIEW_LINUX_RENDER_PATH_H_
#define MORPH_WEBVIEW_LINUX_RENDER_PATH_H_

#include <glib.h>

G_BEGIN_DECLS

typedef enum {
  MORPH_RENDER_PATH_WPE_DMABUF = 0,
  MORPH_RENDER_PATH_SNAPSHOT = 1,
} MorphRenderPath;

// Chooses the texture path. Caller must g_free() *reason.
MorphRenderPath morph_detect_render_path(gchar** reason);

const gchar* morph_render_path_name(MorphRenderPath path);

// The snapshot WebKitGTK view should disable WebKit's internal DMA-BUF
// renderer on NVIDIA and when Morph itself is using the copy fallback.
gboolean morph_should_disable_webkit_dmabuf_renderer(void);

gboolean morph_has_nvidia_proprietary_driver(void);
gboolean morph_has_drm_render_node(void);
gboolean morph_has_egl_dma_buf_import(void);

G_END_DECLS

#endif  // MORPH_WEBVIEW_LINUX_RENDER_PATH_H_
