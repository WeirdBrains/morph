#include "render_path.h"

#include <EGL/egl.h>

#include <cstring>

gboolean morph_has_nvidia_proprietary_driver(void) {
  return g_file_test("/proc/driver/nvidia/version", G_FILE_TEST_EXISTS) ||
         g_file_test("/proc/driver/nvidia", G_FILE_TEST_IS_DIR);
}

gboolean morph_has_drm_render_node(void) {
  GDir* dir = g_dir_open("/dev/dri", 0, nullptr);
  if (dir == nullptr) {
    return FALSE;
  }
  gboolean found = FALSE;
  const gchar* name = nullptr;
  while ((name = g_dir_read_name(dir)) != nullptr) {
    if (g_str_has_prefix(name, "renderD")) {
      found = TRUE;
      break;
    }
  }
  g_dir_close(dir);
  return found;
}

gboolean morph_has_egl_dma_buf_import(void) {
  EGLDisplay display = eglGetDisplay(EGL_DEFAULT_DISPLAY);
  if (display == EGL_NO_DISPLAY) {
    return FALSE;
  }
  EGLint major = 0;
  EGLint minor = 0;
  if (!eglInitialize(display, &major, &minor)) {
    return FALSE;
  }
  const char* extensions = eglQueryString(display, EGL_EXTENSIONS);
  gboolean ok =
      extensions != nullptr && strstr(extensions, "EGL_EXT_image_dma_buf_import") != nullptr;
  eglTerminate(display);
  return ok;
}

const gchar* morph_render_path_name(MorphRenderPath path) {
  switch (path) {
    case MORPH_RENDER_PATH_WPE_DMABUF:
      return "wpe-dmabuf";
    case MORPH_RENDER_PATH_FLUTTER_GL:
      return "flutter-gl";
    case MORPH_RENDER_PATH_SNAPSHOT:
    default:
      return "webkitgtk-snapshot";
  }
}

gboolean morph_should_disable_webkit_dmabuf_renderer(void) {
  // Opt-in only. Forcing it on NVIDIA made WebKitGTK 2.54 abort the web
  // process on every video page (YouTube, Facebook stories); WebKit picks
  // its own NVIDIA fallbacks.
  return g_strcmp0(g_getenv("MORPH_WEBKIT_DISABLE_DMABUF"), "1") == 0;
}

MorphRenderPath morph_detect_render_path(gchar** reason) {
  const gchar* forced = g_getenv("MORPH_WEBVIEW_RENDER");
  if (forced != nullptr && *forced != '\0') {
    if (g_ascii_strcasecmp(forced, "snapshot") == 0 ||
        g_ascii_strcasecmp(forced, "webkitgtk-snapshot") == 0) {
      if (reason != nullptr) {
        *reason = g_strdup("MORPH_WEBVIEW_RENDER=snapshot");
      }
      return MORPH_RENDER_PATH_SNAPSHOT;
    }
    if (g_ascii_strcasecmp(forced, "gl") == 0 || g_ascii_strcasecmp(forced, "flutter-gl") == 0) {
      if (reason != nullptr) {
        *reason = g_strdup("MORPH_WEBVIEW_RENDER=gl");
      }
      return MORPH_RENDER_PATH_FLUTTER_GL;
    }
    if (g_ascii_strcasecmp(forced, "dmabuf") == 0 || g_ascii_strcasecmp(forced, "wpe-dmabuf") == 0) {
#ifdef MORPH_HAS_WPE
      if (reason != nullptr) {
        *reason = g_strdup("MORPH_WEBVIEW_RENDER=dmabuf");
      }
      return MORPH_RENDER_PATH_WPE_DMABUF;
#else
      if (reason != nullptr) {
        *reason = g_strdup("MORPH_WEBVIEW_RENDER=dmabuf but WPE was not built; using flutter-gl");
      }
      return MORPH_RENDER_PATH_FLUTTER_GL;
#endif
    }
  }

  if (reason != nullptr) {
    *reason = nullptr;
  }
#ifdef MORPH_HAS_WPE
  const gboolean allow_wpe =
      !morph_has_nvidia_proprietary_driver() || g_getenv("MORPH_FORCE_DMABUF") != nullptr;
  if (allow_wpe && morph_has_drm_render_node() && morph_has_egl_dma_buf_import()) {
    if (reason != nullptr) {
      *reason = g_strdup("feature-detect-ok");
    }
    return MORPH_RENDER_PATH_WPE_DMABUF;
  }
#endif
  if (reason != nullptr) {
    if (*reason == nullptr) {
#ifdef MORPH_HAS_WPE
      if (morph_has_nvidia_proprietary_driver() && g_getenv("MORPH_FORCE_DMABUF") == nullptr) {
        *reason = g_strdup("nvidia-proprietary; using flutter-gl");
      } else if (!morph_has_drm_render_node()) {
        *reason = g_strdup("no-drm-render-node; using flutter-gl");
      } else if (!morph_has_egl_dma_buf_import()) {
        *reason = g_strdup("no-egl-dma-buf-import; using flutter-gl");
      } else {
        *reason = g_strdup("flutter-gl");
      }
#else
      *reason = g_strdup("wpe-not-built; using flutter-gl");
#endif
    }
  }
  return MORPH_RENDER_PATH_FLUTTER_GL;
}
