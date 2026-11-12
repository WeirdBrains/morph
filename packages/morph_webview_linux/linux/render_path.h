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

// Disable WebKit's internal DMA-BUF renderer on NVIDIA (it is unreliable
// with the proprietary driver). Morph can still upload frames through a
// Flutter GL texture; true DMA-BUF import is the WPE path on Mesa.
gboolean morph_should_disable_webkit_dmabuf_renderer(void);

gboolean morph_has_nvidia_proprietary_driver(void);
gboolean morph_has_drm_render_node(void);
gboolean morph_has_egl_dma_buf_import(void);

G_END_DECLS

#endif  // MORPH_WEBVIEW_LINUX_RENDER_PATH_H_
