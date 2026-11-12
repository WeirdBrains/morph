#include "include/morph_webview_linux/morph_webview_linux_plugin.h"

#include "morph_webview_control.h"
#include "ime_context.h"
#include "key_map.h"
#include "render_path.h"
#include "session.h"
#include "frame.h"

#include <cairo/cairo.h>
#include <flutter_linux/flutter_linux.h>
#include <gtk/gtk.h>
#include <webkit2/webkit2.h>

#include <epoxy/gl.h>

#include <cstring>
#include <map>
#include <string>

#define MORPH_WEBVIEW_LINUX_PLUGIN(obj)                                     \
  (G_TYPE_CHECK_INSTANCE_CAST((obj), morph_webview_linux_plugin_get_type(), \
                              MorphWebviewLinuxPlugin))

static const char kMethodChannel[] = "org.weirdbrains.morph_webview";
static const char kEventChannel[] = "org.weirdbrains.morph_webview/events";

static const char kMorphRustShim[] = R"JS(
(function () {
  if (!window.morphRust) {
    window.morphRust = {
      postMessage: function (msg) {
        if (window.webkit && window.webkit.messageHandlers &&
            window.webkit.messageHandlers.morphRust) {
          window.webkit.messageHandlers.morphRust.postMessage(msg);
        }
      }
    };
  }
})();
)JS";

// The offscreen web view cannot host a GTK drag-and-drop session: dragging a
// selection or an image left a stuck drag ghost. Cancelling dragstart keeps
// WebKit from starting one, so the gesture extends or restarts the text
// selection instead. Click-drag selection and double-click word selection are
// unaffected. Linux only; the Windows and macOS views are native and keep DnD.
static const char kNoDragShim[] = R"JS(
window.addEventListener('dragstart', function (event) {
  event.preventDefault();
}, true);
)JS";

struct MorphView;

G_DECLARE_FINAL_TYPE(MorphPixelBufferTexture,
                     morph_pixel_buffer_texture,
                     MORPH,
                     PIXEL_BUFFER_TEXTURE,
                     FlPixelBufferTexture)

G_DECLARE_FINAL_TYPE(MorphGlTexture, morph_gl_texture, MORPH, GL_TEXTURE, FlTextureGL)

struct _MorphPixelBufferTexture {
  FlPixelBufferTexture parent_instance;
  MorphView* view;
};

struct _MorphGlTexture {
  FlTextureGL parent_instance;
  MorphView* view;
  GLuint texture_id;
  GLuint verify_fbo;
  uint32_t uploaded_width;
  uint32_t uploaded_height;
  // Upload format for the GL context Flutter calls populate() with. Chosen on
  // first use, lowered if the self-check fails. Read by stats on the main
  // thread, so it is accessed with g_atomic_int_*.
  gint mode;
  gboolean mode_chosen;
  gboolean failed;
  // Frames whose read-back matched non-blank source pixels in this mode.
  guint verified;
  guint verify_attempts;
  uint8_t* scratch;
  size_t scratch_bytes;
};

struct MorphView {
  int64_t view_id;
  gchar* profile_id;
  MorphWebviewLinuxPlugin* plugin;
  FlTextureRegistrar* textures;
  FlTexture* texture;
  MorphPixelBufferTexture* pixel_texture;
  MorphGlTexture* gl_texture;
  gboolean use_gl;
  gboolean frame_dirty;
  gboolean live_present;
  gint64 last_present_us;
  gint64 last_input_us;
  guint tick_id;
  guint presents;
  guint present_us_accum;
  GtkWidget* offscreen;
  WebKitWebView* web_view;
  uint8_t* front;
  uint8_t* back;
  uint32_t tex_width;
  uint32_t tex_height;
  uint32_t logical_width;
  uint32_t logical_height;
  double dpr;
  GMutex lock;
  gboolean snapshot_in_flight;
  gboolean snapshot_dirty;
  // Frame source self-check: the offscreen surface came out blank while a
  // WebKit snapshot of the same page had content, so present snapshots.
  gboolean offscreen_blank_fallback;
  gboolean blank_probe_in_flight;
  gboolean loaded_once;
  guint offscreen_blank_frames;
  gint64 blank_probe_after_us;
  guint snapshot_timer;
  gchar* render_path;
  gchar* render_reason;
  gchar* cursor_shape;
  MorphImeContext* ime;
  guint pending_move_source;
  gchar* pending_phase;
  double pending_x;
  double pending_y;
  double pending_sx;
  double pending_sy;
  int32_t pending_button;
  int32_t pending_buttons;
  int pending_ctrl;
  int pending_alt;
  int pending_shift;
  int pending_meta;
};

struct _MorphWebviewLinuxPlugin {
  GObject parent_instance;
  FlPluginRegistrar* registrar;
  FlTextureRegistrar* textures;
  FlMethodChannel* channel;
  FlEventChannel* events;
  gboolean events_listening;
  int64_t next_view_id;
  std::map<int64_t, MorphView*>* views;
  gchar* control_endpoint;
};

G_DEFINE_TYPE(MorphWebviewLinuxPlugin, morph_webview_linux_plugin, G_TYPE_OBJECT)
G_DEFINE_TYPE(MorphPixelBufferTexture,
              morph_pixel_buffer_texture,
              fl_pixel_buffer_texture_get_type())
G_DEFINE_TYPE(MorphGlTexture, morph_gl_texture, fl_texture_gl_get_type())

static gboolean morph_pixel_buffer_texture_copy_pixels(FlPixelBufferTexture* texture,
                                                       const uint8_t** buffer,
                                                       uint32_t* width,
                                                       uint32_t* height,
                                                       GError** error) {
  MorphPixelBufferTexture* self = MORPH_PIXEL_BUFFER_TEXTURE(texture);
  MorphView* view = self->view;
  if (view == nullptr) {
    g_set_error(error, G_IO_ERROR, G_IO_ERROR_FAILED, "view disposed");
    return FALSE;
  }
  g_mutex_lock(&view->lock);
  if (view->front == nullptr || view->tex_width == 0 || view->tex_height == 0) {
    g_mutex_unlock(&view->lock);
    g_set_error(error, G_IO_ERROR, G_IO_ERROR_FAILED, "no frame yet");
    return FALSE;
  }
  *buffer = view->front;
  *width = view->tex_width;
  *height = view->tex_height;
  g_mutex_unlock(&view->lock);
  return TRUE;
}

static void morph_pixel_buffer_texture_class_init(MorphPixelBufferTextureClass* klass) {
  FL_PIXEL_BUFFER_TEXTURE_CLASS(klass)->copy_pixels = morph_pixel_buffer_texture_copy_pixels;
}

static void morph_pixel_buffer_texture_init(MorphPixelBufferTexture* self) {
  self->view = nullptr;
}

static MorphPixelBufferTexture* morph_pixel_buffer_texture_new(MorphView* view) {
  MorphPixelBufferTexture* texture = MORPH_PIXEL_BUFFER_TEXTURE(
      g_object_new(morph_pixel_buffer_texture_get_type(), nullptr));
  texture->view = view;
  return texture;
}

static gboolean morph_stats_enabled(void) {
  static gint enabled = -1;
  if (enabled < 0) {
    enabled = g_getenv("MORPH_WEBVIEW_STATS") != nullptr ? 1 : 0;
  }
  return enabled == 1;
}

// Saves and restores the GL state populate() touches, so Flutter's renderer
// (Skia or Impeller) finds its context as it left it.
struct GlStateGuard {
  GLint texture = 0;
  GLint framebuffer = 0;
  GLint unpack_alignment = 4;
  GLint pack_alignment = 4;
  GLint unpack_buffer = 0;
  GLint pack_buffer = 0;
  GLint unpack_row_length = 0;
  GLint unpack_skip_rows = 0;
  GLint unpack_skip_pixels = 0;
  GLint pack_row_length = 0;
  bool es3_or_desktop = false;

  GlStateGuard() {
    es3_or_desktop = epoxy_is_desktop_gl() || epoxy_gl_version() >= 30;
    glGetIntegerv(GL_TEXTURE_BINDING_2D, &texture);
    glGetIntegerv(GL_FRAMEBUFFER_BINDING, &framebuffer);
    glGetIntegerv(GL_UNPACK_ALIGNMENT, &unpack_alignment);
    glGetIntegerv(GL_PACK_ALIGNMENT, &pack_alignment);
    if (es3_or_desktop) {
      glGetIntegerv(GL_PIXEL_UNPACK_BUFFER_BINDING, &unpack_buffer);
      glGetIntegerv(GL_PIXEL_PACK_BUFFER_BINDING, &pack_buffer);
      glGetIntegerv(GL_UNPACK_ROW_LENGTH, &unpack_row_length);
      glGetIntegerv(GL_UNPACK_SKIP_ROWS, &unpack_skip_rows);
      glGetIntegerv(GL_UNPACK_SKIP_PIXELS, &unpack_skip_pixels);
      glGetIntegerv(GL_PACK_ROW_LENGTH, &pack_row_length);
      glBindBuffer(GL_PIXEL_UNPACK_BUFFER, 0);
      glBindBuffer(GL_PIXEL_PACK_BUFFER, 0);
      glPixelStorei(GL_UNPACK_ROW_LENGTH, 0);
      glPixelStorei(GL_UNPACK_SKIP_ROWS, 0);
      glPixelStorei(GL_UNPACK_SKIP_PIXELS, 0);
      glPixelStorei(GL_PACK_ROW_LENGTH, 0);
    }
    glPixelStorei(GL_UNPACK_ALIGNMENT, 4);
    glPixelStorei(GL_PACK_ALIGNMENT, 4);
  }

  ~GlStateGuard() {
    if (es3_or_desktop) {
      glBindBuffer(GL_PIXEL_UNPACK_BUFFER, unpack_buffer);
      glBindBuffer(GL_PIXEL_PACK_BUFFER, pack_buffer);
      glPixelStorei(GL_UNPACK_ROW_LENGTH, unpack_row_length);
      glPixelStorei(GL_UNPACK_SKIP_ROWS, unpack_skip_rows);
      glPixelStorei(GL_UNPACK_SKIP_PIXELS, unpack_skip_pixels);
      glPixelStorei(GL_PACK_ROW_LENGTH, pack_row_length);
    }
    glPixelStorei(GL_UNPACK_ALIGNMENT, unpack_alignment);
    glPixelStorei(GL_PACK_ALIGNMENT, pack_alignment);
    glBindFramebuffer(GL_FRAMEBUFFER, framebuffer);
    glBindTexture(GL_TEXTURE_2D, texture);
  }
};

static void gl_clear_errors(void) {
  for (int i = 0; i < 16 && glGetError() != GL_NO_ERROR; i++) {
  }
}

// Uploads view->front (BGRA) into the bound texture in the current mode.
// Returns the GL error, GL_NO_ERROR on success.
static GLenum gl_upload_front(MorphGlTexture* self, MorphView* view, gboolean allocate) {
  const uint8_t* pixels = view->front;
  GLenum internal_format = GL_RGBA8;
  GLenum format = GL_BGRA;
  switch (static_cast<MorphGlUploadMode>(g_atomic_int_get(&self->mode))) {
    case MORPH_GL_UPLOAD_BGRA_DESKTOP:
      internal_format = GL_RGBA8;
      format = GL_BGRA;
      break;
    case MORPH_GL_UPLOAD_BGRA_EXT:
      internal_format = GL_BGRA_EXT;
      format = GL_BGRA_EXT;
      break;
    case MORPH_GL_UPLOAD_RGBA_SWIZZLE:
    default: {
      const size_t count = static_cast<size_t>(view->tex_width) * view->tex_height;
      if (self->scratch_bytes < count * 4) {
        g_free(self->scratch);
        self->scratch = static_cast<uint8_t*>(g_malloc(count * 4));
        self->scratch_bytes = count * 4;
      }
      morph_swizzle_bgra_to_rgba(view->front, self->scratch, count);
      pixels = self->scratch;
      internal_format = GL_RGBA;
      format = GL_RGBA;
      break;
    }
  }
  gl_clear_errors();
  if (allocate) {
    glTexImage2D(GL_TEXTURE_2D, 0, internal_format, view->tex_width, view->tex_height, 0, format,
                 GL_UNSIGNED_BYTE, pixels);
  } else {
    glTexSubImage2D(GL_TEXTURE_2D, 0, 0, 0, view->tex_width, view->tex_height, format,
                    GL_UNSIGNED_BYTE, pixels);
  }
  return glGetError();
}

// Optional evidence dump of a read-back frame (MORPH_WEBVIEW_GL_DUMP=prefix).
static void gl_dump_readback(const uint8_t* rgba, uint32_t width, uint32_t height, guint index) {
  const gchar* prefix = g_getenv("MORPH_WEBVIEW_GL_DUMP");
  if (prefix == nullptr || *prefix == '\0') {
    return;
  }
  cairo_surface_t* surface =
      cairo_image_surface_create(CAIRO_FORMAT_ARGB32, static_cast<int>(width), static_cast<int>(height));
  unsigned char* data = cairo_image_surface_get_data(surface);
  const int stride = cairo_image_surface_get_stride(surface);
  for (uint32_t y = 0; y < height; y++) {
    morph_swizzle_bgra_to_rgba(rgba + static_cast<size_t>(y) * width * 4,
                               data + static_cast<size_t>(y) * stride, width);
  }
  cairo_surface_mark_dirty(surface);
  g_autofree gchar* path = g_strdup_printf("%s-%u.png", prefix, index);
  cairo_surface_write_to_png(surface, path);
  cairo_surface_destroy(surface);
}

// Reads the uploaded texture back through a framebuffer and compares it with
// the BGRA source. Returns 1 when it matches, 0 when it does not, and -1 when
// the read-back is not possible here (the GL error check still applies).
static int gl_verify_upload(MorphGlTexture* self, MorphView* view) {
  const uint32_t width = view->tex_width;
  const uint32_t height = view->tex_height;
  if (self->verify_fbo == 0) {
    glGenFramebuffers(1, &self->verify_fbo);
  }
  glBindFramebuffer(GL_FRAMEBUFFER, self->verify_fbo);
  glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, self->texture_id, 0);
  int result = -1;
  if (glCheckFramebufferStatus(GL_FRAMEBUFFER) == GL_FRAMEBUFFER_COMPLETE) {
    const size_t bytes = static_cast<size_t>(width) * height * 4;
    auto* readback = static_cast<uint8_t*>(g_malloc(bytes));
    gl_clear_errors();
    glReadPixels(0, 0, static_cast<GLsizei>(width), static_cast<GLsizei>(height), GL_RGBA,
                 GL_UNSIGNED_BYTE, readback);
    if (glGetError() == GL_NO_ERROR) {
      size_t sampled = 0;
      size_t mismatched = 0;
      double gl_sum[4] = {0, 0, 0, 0};
      double src_sum[4] = {0, 0, 0, 0};
      const uint32_t step_x = width > 64 ? width / 64 : 1;
      const uint32_t step_y = height > 64 ? height / 64 : 1;
      for (uint32_t y = step_y / 2; y < height; y += step_y) {
        for (uint32_t x = step_x / 2; x < width; x += step_x) {
          const size_t i = (static_cast<size_t>(y) * width + x) * 4;
          const uint8_t* gl_px = readback + i;
          const uint8_t* src_px = view->front + i;
          sampled++;
          if (!morph_rgba_matches_bgra(gl_px, src_px, 2)) {
            mismatched++;
          }
          gl_sum[0] += gl_px[0];
          gl_sum[1] += gl_px[1];
          gl_sum[2] += gl_px[2];
          gl_sum[3] += gl_px[3];
          src_sum[0] += src_px[2];
          src_sum[1] += src_px[1];
          src_sum[2] += src_px[0];
          src_sum[3] += src_px[3];
        }
      }
      result = mismatched * 100 <= sampled ? 1 : 0;
      if (morph_stats_enabled() && sampled > 0) {
        g_printerr(
            "morph_webview: gl-check mode=%s %ux%u sampled=%zu mismatched=%zu "
            "gl_mean_rgba=%.1f,%.1f,%.1f,%.1f source_mean_rgba=%.1f,%.1f,%.1f,%.1f result=%s\n",
            morph_gl_upload_name(static_cast<MorphGlUploadMode>(g_atomic_int_get(&self->mode))),
            width, height, sampled, mismatched, gl_sum[0] / sampled, gl_sum[1] / sampled,
            gl_sum[2] / sampled, gl_sum[3] / sampled, src_sum[0] / sampled, src_sum[1] / sampled,
            src_sum[2] / sampled, src_sum[3] / sampled, result == 1 ? "ok" : "mismatch");
      }
      gl_dump_readback(readback, width, height, self->verify_attempts);
    }
    g_free(readback);
  }
  glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, 0, 0);
  gl_clear_errors();
  return result;
}

// Lowers the upload mode after a failed self-check. FALSE when none is left.
static gboolean gl_fall_back(MorphGlTexture* self, const char* why, GLenum error) {
  const MorphGlUploadMode mode = static_cast<MorphGlUploadMode>(g_atomic_int_get(&self->mode));
  MorphGlUploadMode next = mode;
  if (!morph_next_gl_upload(mode, &next)) {
    self->failed = TRUE;
    g_warning("morph_webview: GL upload %s failed (%s, GL error 0x%x); no fallback left",
              morph_gl_upload_name(mode), why, error);
    return FALSE;
  }
  g_warning("morph_webview: GL upload %s failed (%s, GL error 0x%x); falling back to %s",
            morph_gl_upload_name(mode), why, error, morph_gl_upload_name(next));
  g_atomic_int_set(&self->mode, next);
  self->verified = 0;
  self->uploaded_width = 0;
  self->uploaded_height = 0;
  return TRUE;
}

static gboolean morph_gl_texture_populate(FlTextureGL* texture,
                                          uint32_t* target,
                                          uint32_t* name,
                                          uint32_t* width,
                                          uint32_t* height,
                                          GError** error) {
  MorphGlTexture* self = MORPH_GL_TEXTURE(texture);
  MorphView* view = self->view;
  if (view == nullptr) {
    g_set_error(error, G_IO_ERROR, G_IO_ERROR_FAILED, "view disposed");
    return FALSE;
  }
  if (self->failed) {
    g_set_error(error, G_IO_ERROR, G_IO_ERROR_FAILED, "GL upload unavailable");
    return FALSE;
  }
  g_mutex_lock(&view->lock);
  if (view->front == nullptr || view->tex_width == 0 || view->tex_height == 0) {
    g_mutex_unlock(&view->lock);
    g_set_error(error, G_IO_ERROR, G_IO_ERROR_FAILED, "no frame yet");
    return FALSE;
  }
  if (!self->mode_chosen) {
    // populate() runs with Flutter's raster context current, which is an
    // OpenGL ES context on the Linux embedder.
    const gboolean desktop = epoxy_is_desktop_gl();
    const gboolean bgra = epoxy_has_gl_extension("GL_EXT_texture_format_BGRA8888");
    MorphGlUploadMode mode = morph_choose_gl_upload(desktop, bgra);
    // Debug override to exercise the self-check: MORPH_WEBVIEW_GL_UPLOAD=
    // desktop|bgra-ext|rgba. "desktop" on GLES reproduces the black frame.
    const gchar* forced = g_getenv("MORPH_WEBVIEW_GL_UPLOAD");
    if (g_strcmp0(forced, "desktop") == 0) {
      mode = MORPH_GL_UPLOAD_BGRA_DESKTOP;
    } else if (g_strcmp0(forced, "bgra-ext") == 0) {
      mode = MORPH_GL_UPLOAD_BGRA_EXT;
    } else if (g_strcmp0(forced, "rgba") == 0) {
      mode = MORPH_GL_UPLOAD_RGBA_SWIZZLE;
    }
    g_atomic_int_set(&self->mode, mode);
    self->mode_chosen = TRUE;
    if (morph_stats_enabled()) {
      g_printerr("morph_webview: gl context %s (%s) es=%d bgra8888=%d upload=%s\n",
                 reinterpret_cast<const char*>(glGetString(GL_VERSION)),
                 reinterpret_cast<const char*>(glGetString(GL_RENDERER)), desktop ? 0 : 1,
                 bgra ? 1 : 0,
                 morph_gl_upload_name(static_cast<MorphGlUploadMode>(self->mode)));
    }
  }
  gboolean ok = FALSE;
  {
    GlStateGuard guard;
    if (self->texture_id == 0) {
      glGenTextures(1, &self->texture_id);
      glBindTexture(GL_TEXTURE_2D, self->texture_id);
      glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
      glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
      glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
      glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
    } else {
      glBindTexture(GL_TEXTURE_2D, self->texture_id);
    }
    for (;;) {
      const gboolean allocate =
          self->uploaded_width != view->tex_width || self->uploaded_height != view->tex_height;
      GLenum gl_error = gl_upload_front(self, view, allocate);
      if (gl_error != GL_NO_ERROR) {
        if (gl_fall_back(self, allocate ? "glTexImage2D" : "glTexSubImage2D", gl_error)) {
          continue;
        }
        break;
      }
      self->uploaded_width = view->tex_width;
      self->uploaded_height = view->tex_height;
      // Read the first real frames back once per mode: a texture can upload
      // without a GL error and still sample as black.
      if (self->verified < 2 && self->verify_attempts < 12 &&
          !morph_frame_is_blank(view->front, view->tex_width, view->tex_height,
                                static_cast<size_t>(view->tex_width) * 4)) {
        self->verify_attempts++;
        const int check = gl_verify_upload(self, view);
        if (check == 0) {
          if (gl_fall_back(self, "read-back mismatch", GL_NO_ERROR)) {
            continue;
          }
          break;
        }
        self->verified++;
      }
      ok = TRUE;
      break;
    }
  }
  if (!ok) {
    g_mutex_unlock(&view->lock);
    g_set_error(error, G_IO_ERROR, G_IO_ERROR_FAILED, "GL upload failed");
    return FALSE;
  }
  *target = GL_TEXTURE_2D;
  *name = self->texture_id;
  *width = view->tex_width;
  *height = view->tex_height;
  g_mutex_unlock(&view->lock);
  return TRUE;
}

static void morph_gl_texture_dispose(GObject* object) {
  MorphGlTexture* self = MORPH_GL_TEXTURE(object);
  if (self->texture_id != 0) {
    glDeleteTextures(1, &self->texture_id);
    self->texture_id = 0;
  }
  if (self->verify_fbo != 0) {
    glDeleteFramebuffers(1, &self->verify_fbo);
    self->verify_fbo = 0;
  }
  g_clear_pointer(&self->scratch, g_free);
  self->scratch_bytes = 0;
  G_OBJECT_CLASS(morph_gl_texture_parent_class)->dispose(object);
}

static void morph_gl_texture_class_init(MorphGlTextureClass* klass) {
  FL_TEXTURE_GL_CLASS(klass)->populate = morph_gl_texture_populate;
  G_OBJECT_CLASS(klass)->dispose = morph_gl_texture_dispose;
}

static void morph_gl_texture_init(MorphGlTexture* self) {
  self->view = nullptr;
  self->texture_id = 0;
  self->uploaded_width = 0;
  self->uploaded_height = 0;
  self->verify_fbo = 0;
  self->mode = MORPH_GL_UPLOAD_BGRA_DESKTOP;
  self->mode_chosen = FALSE;
  self->failed = FALSE;
  self->verified = 0;
  self->verify_attempts = 0;
  self->scratch = nullptr;
  self->scratch_bytes = 0;
}

static MorphGlTexture* morph_gl_texture_new(MorphView* view) {
  MorphGlTexture* texture =
      MORPH_GL_TEXTURE(g_object_new(morph_gl_texture_get_type(), nullptr));
  texture->view = view;
  return texture;
}

static void emit_event(MorphView* view, FlValue* event) {
  MorphWebviewLinuxPlugin* plugin = view->plugin;
  if (plugin == nullptr || !plugin->events_listening || plugin->events == nullptr) {
    fl_value_unref(event);
    return;
  }
  fl_value_set_string_take(event, "viewId", fl_value_new_int(view->view_id));
  g_autoptr(GError) error = nullptr;
  if (!fl_event_channel_send(plugin->events, event, nullptr, &error)) {
    g_warning("morph_webview: failed to send event: %s", error->message);
  }
  fl_value_unref(event);
}

static void emit_navigation(MorphView* view) {
  const gchar* uri = webkit_web_view_get_uri(view->web_view);
  const gchar* title = webkit_web_view_get_title(view->web_view);
  gboolean is_loading = FALSE;
  g_object_get(view->web_view, "is-loading", &is_loading, nullptr);
  gdouble progress = 0;
  g_object_get(view->web_view, "estimated-load-progress", &progress, nullptr);
  morph_control_emit_navigation(
      view->view_id, uri != nullptr ? uri : "", title != nullptr ? title : "",
      webkit_web_view_can_go_back(view->web_view) ? 1 : 0,
      webkit_web_view_can_go_forward(view->web_view) ? 1 : 0, is_loading ? 1 : 0,
      progress);
}

static void request_snapshot(MorphView* view);
static void schedule_present(MorphView* view);

static void ensure_buffers(MorphView* view, uint32_t width, uint32_t height) {
  if (width == 0) {
    width = 1;
  }
  if (height == 0) {
    height = 1;
  }
  if (view->front != nullptr && view->tex_width == width && view->tex_height == height) {
    return;
  }
  size_t bytes = static_cast<size_t>(width) * height * 4;
  auto* front = static_cast<uint8_t*>(g_malloc0(bytes));
  auto* back = static_cast<uint8_t*>(g_malloc0(bytes));
  g_free(view->front);
  g_free(view->back);
  view->front = front;
  view->back = back;
  view->tex_width = width;
  view->tex_height = height;
}

static void apply_snapshot_surface(MorphView* view, cairo_surface_t* surface) {
  if (cairo_surface_get_type(surface) != CAIRO_SURFACE_TYPE_IMAGE) {
    return;
  }
  int width = cairo_image_surface_get_width(surface);
  int height = cairo_image_surface_get_height(surface);
  if (width <= 0 || height <= 0) {
    return;
  }
  g_mutex_lock(&view->lock);
  ensure_buffers(view, static_cast<uint32_t>(width), static_cast<uint32_t>(height));
  if (view->use_gl) {
    morph_copy_cairo_to_bgra(surface, view->back, view->tex_width, view->tex_height);
  } else {
    morph_copy_cairo_to_rgba(surface, view->back, view->tex_width, view->tex_height);
  }
  uint8_t* tmp = view->front;
  view->front = view->back;
  view->back = tmp;
  g_mutex_unlock(&view->lock);
  if (view->textures != nullptr && view->texture != nullptr) {
    fl_texture_registrar_mark_texture_frame_available(view->textures, FL_TEXTURE(view->texture));
  }
}

static void record_present(MorphView* view, gint64 started_us) {
  gint64 now = g_get_monotonic_time();
  view->last_present_us = now;
  view->presents++;
  if (now > started_us) {
    view->present_us_accum += static_cast<guint>(now - started_us);
  }
  if (!morph_stats_enabled()) {
    return;
  }
  if (view->presents % 60 == 0 || now - view->last_input_us < 200000) {
    const double avg_ms =
        view->presents > 0 ? (static_cast<double>(view->present_us_accum) / view->presents) / 1000.0
                           : 0;
    const double lag_ms =
        view->last_input_us > 0 ? static_cast<double>(now - view->last_input_us) / 1000.0 : 0;
    const char* upload =
        view->gl_texture != nullptr
            ? morph_gl_upload_name(
                  static_cast<MorphGlUploadMode>(g_atomic_int_get(&view->gl_texture->mode)))
            : "pixel-buffer";
    g_printerr(
        "morph_webview: path=%s upload=%s source=%s presents=%u avg_present=%.2fms "
        "input_lag=%.2fms %ux%u\n",
        view->render_path != nullptr ? view->render_path : "?", upload,
        view->offscreen_blank_fallback ? "snapshot" : "offscreen", view->presents, avg_ms, lag_ms,
        view->tex_width, view->tex_height);
  }
}

static void on_blank_probe_ready(GObject* source, GAsyncResult* result, gpointer user_data) {
  MorphView* view = static_cast<MorphView*>(user_data);
  view->blank_probe_in_flight = FALSE;
  cairo_surface_t* surface =
      webkit_web_view_get_snapshot_finish(WEBKIT_WEB_VIEW(source), result, nullptr);
  if (surface == nullptr) {
    return;
  }
  cairo_surface_flush(surface);
  const gboolean snapshot_blank =
      cairo_surface_get_type(surface) != CAIRO_SURFACE_TYPE_IMAGE ||
      morph_frame_is_blank(cairo_image_surface_get_data(surface),
                           static_cast<uint32_t>(cairo_image_surface_get_width(surface)),
                           static_cast<uint32_t>(cairo_image_surface_get_height(surface)),
                           static_cast<size_t>(cairo_image_surface_get_stride(surface)));
  if (snapshot_blank) {
    // The page really is black; check again later in case that changes.
    view->offscreen_blank_frames = 0;
    view->blank_probe_after_us = g_get_monotonic_time() + 2 * G_USEC_PER_SEC;
  } else {
    view->offscreen_blank_fallback = TRUE;
    g_warning("morph_webview: offscreen surface is blank but the WebKit snapshot is not; "
              "presenting snapshots for view %" G_GINT64_FORMAT,
              view->view_id);
    if (morph_stats_enabled()) {
      g_printerr("morph_webview: frame-source fallback view=%" G_GINT64_FORMAT
                 " offscreen=blank snapshot=content -> source=snapshot\n",
                 view->view_id);
    }
    apply_snapshot_surface(view, surface);
  }
  cairo_surface_destroy(surface);
}

// TRUE when the offscreen surface was presented. A surface that stays blank
// after the page has loaded is checked against a WebKit snapshot once; if the
// snapshot has content the view switches to snapshot presents for good.
static gboolean try_present_offscreen(MorphView* view) {
  if (view->offscreen == nullptr || view->offscreen_blank_fallback) {
    return FALSE;
  }
  cairo_surface_t* surface = gtk_offscreen_window_get_surface(GTK_OFFSCREEN_WINDOW(view->offscreen));
  if (surface == nullptr || cairo_surface_get_type(surface) != CAIRO_SURFACE_TYPE_IMAGE) {
    return FALSE;
  }
  if (cairo_image_surface_get_width(surface) <= 0 || cairo_image_surface_get_height(surface) <= 0) {
    return FALSE;
  }
  if (view->loaded_once && !view->blank_probe_in_flight) {
    cairo_surface_flush(surface);
    const gboolean blank =
        morph_frame_is_blank(cairo_image_surface_get_data(surface),
                             static_cast<uint32_t>(cairo_image_surface_get_width(surface)),
                             static_cast<uint32_t>(cairo_image_surface_get_height(surface)),
                             static_cast<size_t>(cairo_image_surface_get_stride(surface)));
    if (!blank) {
      view->offscreen_blank_frames = 0;
    } else if (++view->offscreen_blank_frames >= 3 &&
               g_get_monotonic_time() >= view->blank_probe_after_us) {
      view->blank_probe_in_flight = TRUE;
      webkit_web_view_get_snapshot(view->web_view, WEBKIT_SNAPSHOT_REGION_VISIBLE,
                                   WEBKIT_SNAPSHOT_OPTIONS_NONE, nullptr, on_blank_probe_ready,
                                   view);
    }
  }
  gint64 started = g_get_monotonic_time();
  apply_snapshot_surface(view, surface);
  record_present(view, started);
  return TRUE;
}

static void request_snapshot(MorphView* view);

static void on_snapshot_ready(GObject* source, GAsyncResult* result, gpointer user_data) {
  MorphView* view = static_cast<MorphView*>(user_data);
  view->snapshot_in_flight = FALSE;
  g_autoptr(GError) error = nullptr;
  cairo_surface_t* surface =
      webkit_web_view_get_snapshot_finish(WEBKIT_WEB_VIEW(source), result, &error);
  if (surface == nullptr) {
    if (error != nullptr) {
      g_debug("morph_webview: snapshot failed: %s", error->message);
    }
  } else {
    gint64 started = g_get_monotonic_time();
    apply_snapshot_surface(view, surface);
    cairo_surface_destroy(surface);
    record_present(view, started);
  }
  if (view->snapshot_dirty) {
    view->snapshot_dirty = FALSE;
    schedule_present(view);
  }
}

static void request_snapshot(MorphView* view) {
  if (view->web_view == nullptr) {
    return;
  }
  if (view->snapshot_in_flight) {
    view->snapshot_dirty = TRUE;
    return;
  }
  view->snapshot_in_flight = TRUE;
  webkit_web_view_get_snapshot(view->web_view, WEBKIT_SNAPSHOT_REGION_VISIBLE,
                               WEBKIT_SNAPSHOT_OPTIONS_NONE, nullptr, on_snapshot_ready, view);
}

static void schedule_present(MorphView* view) {
  view->frame_dirty = TRUE;
}

static gboolean on_web_view_draw(GtkWidget*, cairo_t*, gpointer user_data) {
  schedule_present(static_cast<MorphView*>(user_data));
  return FALSE;
}

static gboolean on_frame_tick(GtkWidget*, GdkFrameClock*, gpointer user_data) {
  MorphView* view = static_cast<MorphView*>(user_data);
  if (view->web_view == nullptr) {
    return G_SOURCE_CONTINUE;
  }
  gint64 now = g_get_monotonic_time();
  gint64 interval = static_cast<gint64>(morph_frame_interval_ms()) * 1000;
  if (now - view->last_present_us < interval) {
    return G_SOURCE_CONTINUE;
  }
  if (!view->frame_dirty && !view->live_present) {
    return G_SOURCE_CONTINUE;
  }
  if (try_present_offscreen(view)) {
    view->frame_dirty = FALSE;
    view->live_present = TRUE;
    return G_SOURCE_CONTINUE;
  }
  if (view->frame_dirty) {
    view->frame_dirty = FALSE;
    request_snapshot(view);
  }
  return G_SOURCE_CONTINUE;
}

static void on_load_changed(WebKitWebView* web_view, WebKitLoadEvent load_event, gpointer user_data) {
  MorphView* view = static_cast<MorphView*>(user_data);
  (void)web_view;
  emit_navigation(view);
  if (load_event == WEBKIT_LOAD_FINISHED) {
    view->loaded_once = TRUE;
  }
  if (load_event == WEBKIT_LOAD_STARTED || load_event == WEBKIT_LOAD_COMMITTED ||
      load_event == WEBKIT_LOAD_FINISHED) {
    schedule_present(view);
  }
}

static void on_notify_nav(GObject* object, GParamSpec* pspec, gpointer user_data) {
  (void)object;
  (void)pspec;
  emit_navigation(static_cast<MorphView*>(user_data));
}

static void on_script_message(WebKitUserContentManager* manager,
                              WebKitJavascriptResult* result,
                              gpointer user_data) {
  (void)manager;
  MorphView* view = static_cast<MorphView*>(user_data);
  JSCValue* value = webkit_javascript_result_get_js_value(result);
  g_autofree gchar* json = jsc_value_to_string(value);
  morph_control_emit_page_message(view->view_id, json != nullptr ? json : "");
}

static void add_user_script_to(WebKitUserContentManager* manager,
                               const gchar* source,
                               WebKitUserContentInjectedFrames frames) {
  if (source == nullptr || *source == '\0') {
    return;
  }
  WebKitUserScript* script =
      webkit_user_script_new(source, frames,
                             WEBKIT_USER_SCRIPT_INJECT_AT_DOCUMENT_START, nullptr, nullptr);
  webkit_user_content_manager_add_script(manager, script);
  webkit_user_script_unref(script);
}

static void add_user_script(WebKitUserContentManager* manager, const gchar* source) {
  add_user_script_to(manager, source, WEBKIT_USER_CONTENT_INJECT_TOP_FRAME);
}

static void apply_size(MorphView* view, uint32_t logical_width, uint32_t logical_height, double dpr) {
  if (logical_width == 0) {
    logical_width = 1;
  }
  if (logical_height == 0) {
    logical_height = 1;
  }
  if (dpr <= 0) {
    dpr = 1;
  }
  view->logical_width = logical_width;
  view->logical_height = logical_height;
  view->dpr = dpr;
  int phys_w = static_cast<int>(logical_width * dpr);
  int phys_h = static_cast<int>(logical_height * dpr);
  if (phys_w < 1) {
    phys_w = 1;
  }
  if (phys_h < 1) {
    phys_h = 1;
  }
  gtk_widget_set_size_request(GTK_WIDGET(view->web_view), phys_w, phys_h);
  gtk_window_resize(GTK_WINDOW(view->offscreen), phys_w, phys_h);
  webkit_web_view_set_zoom_level(view->web_view, dpr);
  g_mutex_lock(&view->lock);
  ensure_buffers(view, static_cast<uint32_t>(phys_w), static_cast<uint32_t>(phys_h));
  g_mutex_unlock(&view->lock);
  schedule_present(view);
}

static void on_mouse_target_changed(WebKitWebView* web_view,
                                    WebKitHitTestResult* hit_test_result,
                                    guint modifiers,
                                    gpointer user_data);

static MorphView* morph_view_create(MorphWebviewLinuxPlugin* plugin,
                                    const gchar* initial_url,
                                    uint32_t width,
                                    uint32_t height,
                                    double dpr,
                                    const gchar* user_script,
                                    WebKitWebView* related,
                                    const gchar* profile_id);
static void morph_view_destroy(MorphView* view);

static gboolean destroy_view_idle(gpointer user_data) {
  morph_view_destroy(static_cast<MorphView*>(user_data));
  return G_SOURCE_REMOVE;
}

static gboolean on_web_view_close(WebKitWebView* web_view, gpointer user_data) {
  (void)web_view;
  MorphView* view = static_cast<MorphView*>(user_data);
  if (view->plugin != nullptr && view->plugin->views != nullptr) {
    view->plugin->views->erase(view->view_id);
  }
  g_idle_add(destroy_view_idle, view);
  return FALSE;
}

static GtkWidget* on_web_view_create(WebKitWebView* web_view,
                                     WebKitNavigationAction* navigation_action,
                                     gpointer user_data) {
  MorphView* parent = static_cast<MorphView*>(user_data);
  MorphView* popup = morph_view_create(parent->plugin, nullptr, parent->logical_width,
                                       parent->logical_height, parent->dpr, nullptr, web_view,
                                       parent->profile_id);
  g_autoptr(FlValue) event = fl_value_new_map();
  fl_value_set_string_take(event, "type", fl_value_new_string("new-window"));
  const gchar* uri = nullptr;
  if (navigation_action != nullptr) {
    WebKitURIRequest* request = webkit_navigation_action_get_request(navigation_action);
    if (request != nullptr) {
      uri = webkit_uri_request_get_uri(request);
    }
  }
  fl_value_set_string_take(event, "url", fl_value_new_string(uri != nullptr ? uri : ""));
  emit_event(popup, fl_value_ref(event));
  return GTK_WIDGET(popup->web_view);
}

static MorphView* morph_view_create(MorphWebviewLinuxPlugin* plugin,
                                    const gchar* initial_url,
                                    uint32_t width,
                                    uint32_t height,
                                    double dpr,
                                    const gchar* user_script,
                                    WebKitWebView* related,
                                    const gchar* profile_id) {
  if (morph_should_disable_webkit_dmabuf_renderer() &&
      g_getenv("WEBKIT_DISABLE_DMABUF_RENDERER") == nullptr) {
    g_setenv("WEBKIT_DISABLE_DMABUF_RENDERER", "1", FALSE);
  }

  MorphView* view = g_new0(MorphView, 1);
  g_mutex_init(&view->lock);
  view->plugin = plugin;
  view->textures = plugin->textures;
  view->view_id = plugin->next_view_id++;
  view->profile_id = g_strdup(profile_id != nullptr && profile_id[0] != '\0' ? profile_id : "default");
  gchar* reason = nullptr;
  MorphRenderPath path = morph_detect_render_path(&reason);
  view->render_path = g_strdup(morph_render_path_name(path));
  view->render_reason = reason != nullptr ? reason : g_strdup("unknown");

  view->use_gl = path == MORPH_RENDER_PATH_FLUTTER_GL || path == MORPH_RENDER_PATH_WPE_DMABUF;
  if (view->use_gl) {
    view->gl_texture = morph_gl_texture_new(view);
    view->texture = FL_TEXTURE(view->gl_texture);
  } else {
    view->pixel_texture = morph_pixel_buffer_texture_new(view);
    view->texture = FL_TEXTURE(view->pixel_texture);
  }
  if (!fl_texture_registrar_register_texture(plugin->textures, view->texture)) {
    g_warning("morph_webview: failed to register texture");
  }

  WebKitUserContentManager* manager = webkit_user_content_manager_new();
  g_signal_connect(manager, "script-message-received::morphRust", G_CALLBACK(on_script_message),
                   view);
  webkit_user_content_manager_register_script_message_handler(manager, "morphRust");
  add_user_script(manager, kMorphRustShim);
  add_user_script_to(manager, kNoDragShim, WEBKIT_USER_CONTENT_INJECT_ALL_FRAMES);
  add_user_script(manager, user_script);

  view->offscreen = gtk_offscreen_window_new();
  WebKitSettings* settings = morph_session_create_settings();
  if (related != nullptr) {
    view->web_view = WEBKIT_WEB_VIEW(g_object_new(WEBKIT_TYPE_WEB_VIEW, "related-view", related,
                                                 "user-content-manager", manager, "settings",
                                                 settings, nullptr));
  } else {
    view->web_view = WEBKIT_WEB_VIEW(g_object_new(
        WEBKIT_TYPE_WEB_VIEW, "web-context",
        morph_session_context_for_profile(view->profile_id), "user-content-manager", manager,
        "settings", settings, nullptr));
  }
  g_object_unref(settings);
  g_object_unref(manager);
  gtk_container_add(GTK_CONTAINER(view->offscreen), GTK_WIDGET(view->web_view));
  gtk_widget_set_can_focus(GTK_WIDGET(view->web_view), TRUE);
  gtk_widget_add_events(GTK_WIDGET(view->web_view),
                        GDK_POINTER_MOTION_MASK | GDK_BUTTON_PRESS_MASK | GDK_BUTTON_RELEASE_MASK |
                            GDK_SCROLL_MASK | GDK_SMOOTH_SCROLL_MASK | GDK_ENTER_NOTIFY_MASK |
                            GDK_LEAVE_NOTIFY_MASK | GDK_KEY_PRESS_MASK | GDK_KEY_RELEASE_MASK |
                            GDK_FOCUS_CHANGE_MASK);
  gtk_widget_show_all(view->offscreen);
  view->frame_dirty = TRUE;
  view->live_present = view->use_gl ? TRUE : FALSE;
  view->tick_id = gtk_widget_add_tick_callback(view->offscreen, on_frame_tick, view, nullptr);
  g_signal_connect(view->web_view, "draw", G_CALLBACK(on_web_view_draw), view);

  view->ime = morph_ime_context_new();
  webkit_web_view_set_input_method_context(view->web_view, WEBKIT_INPUT_METHOD_CONTEXT(view->ime));
  view->cursor_shape = g_strdup("default");

  g_signal_connect(view->web_view, "load-changed", G_CALLBACK(on_load_changed), view);
  g_signal_connect(view->web_view, "notify::title", G_CALLBACK(on_notify_nav), view);
  g_signal_connect(view->web_view, "notify::uri", G_CALLBACK(on_notify_nav), view);
  g_signal_connect(view->web_view, "notify::estimated-load-progress", G_CALLBACK(on_notify_nav),
                   view);
  g_signal_connect(view->web_view, "mouse-target-changed", G_CALLBACK(on_mouse_target_changed),
                   view);
  g_signal_connect(view->web_view, "create", G_CALLBACK(on_web_view_create), view);
  g_signal_connect(view->web_view, "close", G_CALLBACK(on_web_view_close), view);

  apply_size(view, width, height, dpr);
  if (initial_url != nullptr && *initial_url != '\0') {
    webkit_web_view_load_uri(view->web_view, initial_url);
  }
  (*plugin->views)[view->view_id] = view;
  return view;
}

static void morph_view_destroy(MorphView* view) {
  if (view == nullptr) {
    return;
  }
  if (view->snapshot_timer != 0) {
    g_source_remove(view->snapshot_timer);
    view->snapshot_timer = 0;
  }
  if (view->tick_id != 0 && view->offscreen != nullptr) {
    gtk_widget_remove_tick_callback(view->offscreen, view->tick_id);
    view->tick_id = 0;
  }
  if (view->textures != nullptr && view->texture != nullptr) {
    fl_texture_registrar_unregister_texture(view->textures, view->texture);
  }
  if (view->gl_texture != nullptr) {
    view->gl_texture->view = nullptr;
    g_object_unref(view->gl_texture);
    view->gl_texture = nullptr;
  }
  if (view->pixel_texture != nullptr) {
    view->pixel_texture->view = nullptr;
    g_object_unref(view->pixel_texture);
    view->pixel_texture = nullptr;
  }
  view->texture = nullptr;
  if (view->offscreen != nullptr) {
    gtk_widget_destroy(view->offscreen);
    view->offscreen = nullptr;
    view->web_view = nullptr;
  }
  g_mutex_lock(&view->lock);
  g_free(view->front);
  g_free(view->back);
  view->front = nullptr;
  view->back = nullptr;
  g_mutex_unlock(&view->lock);
  g_mutex_clear(&view->lock);
  g_free(view->render_path);
  g_free(view->render_reason);
  g_free(view->cursor_shape);
  g_free(view->profile_id);
  g_free(view->pending_phase);
  if (view->pending_move_source != 0) {
    g_source_remove(view->pending_move_source);
    view->pending_move_source = 0;
  }
  if (view->ime != nullptr) {
    g_object_unref(view->ime);
    view->ime = nullptr;
  }
  g_free(view);
}

static MorphView* lookup_view(MorphWebviewLinuxPlugin* plugin, FlValue* args) {
  FlValue* id_value = fl_value_lookup_string(args, "viewId");
  if (id_value == nullptr || fl_value_get_type(id_value) != FL_VALUE_TYPE_INT) {
    return nullptr;
  }
  int64_t id = fl_value_get_int(id_value);
  auto it = plugin->views->find(id);
  if (it == plugin->views->end()) {
    return nullptr;
  }
  return it->second;
}

static void dispatch_pointer(MorphView* view,
                             const gchar* phase,
                             double x,
                             double y,
                             int button,
                             int buttons,
                             double dx,
                             double dy,
                             gboolean ctrl,
                             gboolean alt,
                             gboolean shift,
                             gboolean meta);
static void queue_pointer(MorphView* view,
                          const gchar* phase,
                          double x,
                          double y,
                          int button,
                          int buttons,
                          double dx,
                          double dy,
                          gboolean ctrl,
                          gboolean alt,
                          gboolean shift,
                          gboolean meta);
static void dispatch_key(MorphView* view,
                         const gchar* phase,
                         const gchar* key,
                         const gchar* text,
                         guint hardware_keycode,
                         gboolean ctrl,
                         gboolean alt,
                         gboolean shift,
                         gboolean meta,
                         gboolean repeat);
static void dispatch_ime(MorphView* view, const gchar* phase, const gchar* text);
static void grab_view_focus(MorphView* view);

static MorphView* find_view(MorphWebviewLinuxPlugin* plugin, int64_t view_id) {
  if (plugin == nullptr || plugin->views == nullptr) {
    return nullptr;
  }
  auto it = plugin->views->find(view_id);
  if (it == plugin->views->end()) {
    return nullptr;
  }
  return it->second;
}

enum ControlJobKind {
  kControlNavigate = 0,
  kControlGoBack,
  kControlGoForward,
  kControlReload,
  kControlPointer,
  kControlKey,
  kControlIme,
  kControlEvaluateJs,
};

struct ControlJob {
  MorphWebviewLinuxPlugin* plugin;
  int64_t view_id;
  ControlJobKind kind;
  gchar* s1;
  gchar* s2;
  gchar* s3;
  double x;
  double y;
  double sx;
  double sy;
  int32_t button;
  int32_t buttons;
  uint32_t keycode;
  uint64_t request_id;
  int ctrl;
  int alt;
  int shift;
  int meta;
  int repeat;
};

static void free_control_job(ControlJob* job) {
  if (job->plugin != nullptr) {
    g_object_unref(job->plugin);
  }
  g_free(job->s1);
  g_free(job->s2);
  g_free(job->s3);
  delete job;
}

static void on_control_js_finished(GObject* source, GAsyncResult* result, gpointer user_data) {
  ControlJob* job = static_cast<ControlJob*>(user_data);
  g_autoptr(GError) error = nullptr;
  JSCValue* value =
      webkit_web_view_evaluate_javascript_finish(WEBKIT_WEB_VIEW(source), result, &error);
  if (value == nullptr) {
    morph_control_complete_evaluate_js(job->view_id, job->request_id, "",
                                       error != nullptr ? error->message : "evaluate failed");
  } else {
    g_autofree gchar* text = jsc_value_to_string(value);
    morph_control_complete_evaluate_js(job->view_id, job->request_id, text != nullptr ? text : "",
                                       "");
  }
  free_control_job(job);
}

static gboolean run_control_job(gpointer data) {
  ControlJob* job = static_cast<ControlJob*>(data);
  MorphView* view = find_view(job->plugin, job->view_id);
  if (view == nullptr) {
    if (job->kind == kControlEvaluateJs) {
      morph_control_complete_evaluate_js(job->view_id, job->request_id, "", "unknown view");
    }
    free_control_job(job);
    return G_SOURCE_REMOVE;
  }
  switch (job->kind) {
    case kControlNavigate:
      webkit_web_view_load_uri(view->web_view, job->s1 != nullptr ? job->s1 : "about:blank");
      break;
    case kControlGoBack:
      webkit_web_view_go_back(view->web_view);
      break;
    case kControlGoForward:
      webkit_web_view_go_forward(view->web_view);
      break;
    case kControlReload:
      webkit_web_view_reload(view->web_view);
      break;
    case kControlPointer:
      queue_pointer(view, job->s1 != nullptr ? job->s1 : "down", job->x, job->y, job->button,
                       job->buttons, job->sx, job->sy, job->ctrl, job->alt, job->shift, job->meta);
      break;
    case kControlKey:
      dispatch_key(view, job->s1 != nullptr ? job->s1 : "down", job->s2 != nullptr ? job->s2 : "",
                   job->s3 != nullptr ? job->s3 : "", job->keycode, job->ctrl, job->alt, job->shift,
                   job->meta, job->repeat);
      break;
    case kControlIme:
      dispatch_ime(view, job->s1 != nullptr ? job->s1 : "", job->s2 != nullptr ? job->s2 : "");
      break;
    case kControlEvaluateJs: {
      const gchar* script = job->s1 != nullptr ? job->s1 : "";
      webkit_web_view_evaluate_javascript(view->web_view, script, -1, nullptr, nullptr, nullptr,
                                          on_control_js_finished, job);
      return G_SOURCE_REMOVE;
    }
  }
  free_control_job(job);
  return G_SOURCE_REMOVE;
}

static ControlJob* new_control_job(void* user_data, int64_t view_id, ControlJobKind kind) {
  auto* job = new ControlJob();
  memset(job, 0, sizeof(*job));
  job->plugin = MORPH_WEBVIEW_LINUX_PLUGIN(user_data);
  if (job->plugin != nullptr) {
    g_object_ref(job->plugin);
  }
  job->view_id = view_id;
  job->kind = kind;
  return job;
}

static void queue_control_job(ControlJob* job) { g_idle_add(run_control_job, job); }

static void control_navigate(int64_t view_id, const char* url, void* user_data) {
  ControlJob* job = new_control_job(user_data, view_id, kControlNavigate);
  job->s1 = g_strdup(url);
  queue_control_job(job);
}
static void control_go_back(int64_t view_id, void* user_data) {
  queue_control_job(new_control_job(user_data, view_id, kControlGoBack));
}
static void control_go_forward(int64_t view_id, void* user_data) {
  queue_control_job(new_control_job(user_data, view_id, kControlGoForward));
}
static void control_reload(int64_t view_id, void* user_data) {
  queue_control_job(new_control_job(user_data, view_id, kControlReload));
}
static void control_pointer(int64_t view_id,
                            const char* phase,
                            double x,
                            double y,
                            int32_t button,
                            int32_t buttons,
                            double scroll_delta_x,
                            double scroll_delta_y,
                            int ctrl,
                            int alt,
                            int shift,
                            int meta,
                            void* user_data) {
  ControlJob* job = new_control_job(user_data, view_id, kControlPointer);
  job->s1 = g_strdup(phase);
  job->x = x;
  job->y = y;
  job->button = button;
  job->buttons = buttons;
  job->sx = scroll_delta_x;
  job->sy = scroll_delta_y;
  job->ctrl = ctrl;
  job->alt = alt;
  job->shift = shift;
  job->meta = meta;
  queue_control_job(job);
}
static void control_key(int64_t view_id,
                        const char* phase,
                        const char* key,
                        const char* text,
                        uint32_t hardware_keycode,
                        int ctrl,
                        int alt,
                        int shift,
                        int meta,
                        int repeat,
                        void* user_data) {
  ControlJob* job = new_control_job(user_data, view_id, kControlKey);
  job->s1 = g_strdup(phase);
  job->s2 = g_strdup(key);
  job->s3 = g_strdup(text);
  job->keycode = hardware_keycode;
  job->ctrl = ctrl;
  job->alt = alt;
  job->shift = shift;
  job->meta = meta;
  job->repeat = repeat;
  queue_control_job(job);
}
static void control_ime(int64_t view_id, const char* phase, const char* text, void* user_data) {
  ControlJob* job = new_control_job(user_data, view_id, kControlIme);
  job->s1 = g_strdup(phase);
  job->s2 = g_strdup(text);
  queue_control_job(job);
}
static void control_evaluate_js(int64_t view_id, uint64_t request_id, const char* script, void* user_data) {
  ControlJob* job = new_control_job(user_data, view_id, kControlEvaluateJs);
  job->s1 = g_strdup(script);
  job->request_id = request_id;
  queue_control_job(job);
}

static void start_control_server(MorphWebviewLinuxPlugin* plugin) {
  MorphControlHandlers handlers = {};
  handlers.navigate = control_navigate;
  handlers.go_back = control_go_back;
  handlers.go_forward = control_go_forward;
  handlers.reload = control_reload;
  handlers.pointer = control_pointer;
  handlers.key = control_key;
  handlers.ime = control_ime;
  handlers.evaluate_js = control_evaluate_js;
  handlers.user_data = plugin;
  char uri[1024];
  if (morph_control_start(nullptr, &handlers, uri, sizeof(uri)) != 0) {
    g_warning("morph_webview: failed to start in-process control server");
    return;
  }
  g_free(plugin->control_endpoint);
  plugin->control_endpoint = g_strdup(uri);
}

static const gchar* arg_string(FlValue* args, const char* key, const gchar* fallback) {
  FlValue* value = fl_value_lookup_string(args, key);
  if (value == nullptr || fl_value_get_type(value) != FL_VALUE_TYPE_STRING) {
    return fallback;
  }
  return fl_value_get_string(value);
}

static int64_t arg_int(FlValue* args, const char* key, int64_t fallback) {
  FlValue* value = fl_value_lookup_string(args, key);
  if (value == nullptr) {
    return fallback;
  }
  if (fl_value_get_type(value) == FL_VALUE_TYPE_INT) {
    return fl_value_get_int(value);
  }
  if (fl_value_get_type(value) == FL_VALUE_TYPE_FLOAT) {
    return static_cast<int64_t>(fl_value_get_float(value));
  }
  return fallback;
}

static double arg_float(FlValue* args, const char* key, double fallback) {
  FlValue* value = fl_value_lookup_string(args, key);
  if (value == nullptr) {
    return fallback;
  }
  if (fl_value_get_type(value) == FL_VALUE_TYPE_FLOAT) {
    return fl_value_get_float(value);
  }
  if (fl_value_get_type(value) == FL_VALUE_TYPE_INT) {
    return static_cast<double>(fl_value_get_int(value));
  }
  return fallback;
}

__attribute__((unused)) static gboolean arg_bool(FlValue* args, const char* key, gboolean fallback) {
  FlValue* value = fl_value_lookup_string(args, key);
  if (value == nullptr) {
    return fallback;
  }
  if (fl_value_get_type(value) == FL_VALUE_TYPE_BOOL) {
    return fl_value_get_bool(value);
  }
  if (fl_value_get_type(value) == FL_VALUE_TYPE_INT) {
    return fl_value_get_int(value) != 0;
  }
  return fallback;
}

static GdkDevice* default_pointer_device() {
  GdkDisplay* display = gdk_display_get_default();
  if (display == nullptr) {
    return nullptr;
  }
  GdkSeat* seat = gdk_display_get_default_seat(display);
  if (seat == nullptr) {
    return nullptr;
  }
  return gdk_seat_get_pointer(seat);
}

static GdkDevice* default_keyboard_device() {
  GdkDisplay* display = gdk_display_get_default();
  if (display == nullptr) {
    return nullptr;
  }
  GdkSeat* seat = gdk_display_get_default_seat(display);
  if (seat == nullptr) {
    return nullptr;
  }
  return gdk_seat_get_keyboard(seat);
}

static GdkModifierType pointer_state(int buttons, gboolean ctrl, gboolean alt, gboolean shift, gboolean meta) {
  GdkModifierType state = static_cast<GdkModifierType>(0);
  if (buttons & 1) {
    state = static_cast<GdkModifierType>(state | GDK_BUTTON1_MASK);
  }
  if (buttons & 2) {
    state = static_cast<GdkModifierType>(state | GDK_BUTTON2_MASK);
  }
  if (buttons & 4) {
    state = static_cast<GdkModifierType>(state | GDK_BUTTON3_MASK);
  }
  if (buttons & 8) {
    state = static_cast<GdkModifierType>(state | GDK_BUTTON4_MASK);
  }
  if (buttons & 16) {
    state = static_cast<GdkModifierType>(state | GDK_BUTTON5_MASK);
  }
  if (ctrl) {
    state = static_cast<GdkModifierType>(state | GDK_CONTROL_MASK);
  }
  if (alt) {
    state = static_cast<GdkModifierType>(state | GDK_MOD1_MASK);
  }
  if (shift) {
    state = static_cast<GdkModifierType>(state | GDK_SHIFT_MASK);
  }
  if (meta) {
    state = static_cast<GdkModifierType>(state | GDK_META_MASK | GDK_SUPER_MASK);
  }
  return state;
}

static const gchar* css_cursor_from_gdk(GdkCursor* cursor) {
  if (cursor == nullptr) {
    return "default";
  }
  // gdk_cursor_get_name() is GTK 4 only. Map GdkCursorType on GTK 3, and treat
  // GDK_CURSOR_IS_PIXMAP (named/custom cursors) as unknown so the caller can
  // fall back to the WebKit hit-test result.
  switch (gdk_cursor_get_cursor_type(cursor)) {
    case GDK_HAND1:
    case GDK_HAND2:
      return "pointer";
    case GDK_XTERM:
      return "text";
    case GDK_WATCH:
      return "wait";
    case GDK_CROSSHAIR:
    case GDK_CROSS:
    case GDK_TCROSS:
      return "crosshair";
    case GDK_FLEUR:
      return "move";
    case GDK_SB_H_DOUBLE_ARROW:
      return "ew-resize";
    case GDK_SB_V_DOUBLE_ARROW:
      return "ns-resize";
    case GDK_QUESTION_ARROW:
      return "help";
    case GDK_PIRATE:
    case GDK_X_CURSOR:
      return "not-allowed";
    case GDK_BLANK_CURSOR:
      return "none";
    case GDK_CURSOR_IS_PIXMAP:
      return nullptr;
    default:
      return "default";
  }
}

static const gchar* css_cursor_from_hit_test(WebKitHitTestResult* hit) {
  if (hit == nullptr) {
    return nullptr;
  }
  if (webkit_hit_test_result_context_is_editable(hit)) {
    return "text";
  }
  if (webkit_hit_test_result_context_is_link(hit)) {
    return "pointer";
  }
  return "default";
}

static void maybe_emit_cursor(MorphView* view, WebKitHitTestResult* hit = nullptr) {
  GdkWindow* window = gtk_widget_get_window(GTK_WIDGET(view->web_view));
  const gchar* shape = nullptr;
  if (window != nullptr) {
    shape = css_cursor_from_gdk(gdk_window_get_cursor(window));
  }
  if (shape == nullptr) {
    shape = css_cursor_from_hit_test(hit);
  }
  if (shape == nullptr) {
    return;
  }
  if (g_strcmp0(view->cursor_shape, shape) != 0) {
    g_free(view->cursor_shape);
    view->cursor_shape = g_strdup(shape);
    morph_control_emit_cursor(view->view_id, shape);
  }
}

static void on_mouse_target_changed(WebKitWebView* web_view,
                                    WebKitHitTestResult* hit_test_result,
                                    guint modifiers,
                                    gpointer user_data) {
  (void)web_view;
  (void)modifiers;
  maybe_emit_cursor(static_cast<MorphView*>(user_data), hit_test_result);
}

static void grab_view_focus(MorphView* view) {
  GtkWidget* widget = GTK_WIDGET(view->web_view);
  gtk_widget_set_can_focus(widget, TRUE);
  gtk_widget_grab_focus(widget);
  GdkWindow* window = gtk_widget_get_window(widget);
  if (window == nullptr) {
    return;
  }
  GdkEvent* event = gdk_event_new(GDK_FOCUS_CHANGE);
  event->focus_change.window = window;
  g_object_ref(window);
  event->focus_change.send_event = TRUE;
  event->focus_change.in = TRUE;
  gtk_widget_event(widget, event);
  gdk_event_free(event);
}

static void dispatch_crossing(MorphView* view, gboolean enter, double px, double py, GdkModifierType state) {
  GdkWindow* window = gtk_widget_get_window(GTK_WIDGET(view->web_view));
  if (window == nullptr) {
    return;
  }
  GdkEvent* event = gdk_event_new(enter ? GDK_ENTER_NOTIFY : GDK_LEAVE_NOTIFY);
  event->crossing.window = window;
  g_object_ref(window);
  event->crossing.send_event = TRUE;
  event->crossing.time = GDK_CURRENT_TIME;
  event->crossing.x = px;
  event->crossing.y = py;
  event->crossing.x_root = px;
  event->crossing.y_root = py;
  event->crossing.mode = GDK_CROSSING_NORMAL;
  event->crossing.detail = GDK_NOTIFY_ANCESTOR;
  event->crossing.focus = TRUE;
  event->crossing.state = state;
  gdk_event_set_device(event, default_pointer_device());
  gtk_widget_event(GTK_WIDGET(view->web_view), event);
  gdk_event_free(event);
}

static void dispatch_pointer(MorphView* view,
                             const gchar* phase,
                             double x,
                             double y,
                             int button,
                             int buttons,
                             double dx,
                             double dy,
                             gboolean ctrl,
                             gboolean alt,
                             gboolean shift,
                             gboolean meta) {
  GdkWindow* window = gtk_widget_get_window(GTK_WIDGET(view->web_view));
  if (window == nullptr) {
    return;
  }
  double px = x * view->dpr;
  double py = y * view->dpr;
  GdkModifierType state = pointer_state(buttons, ctrl, alt, shift, meta);

  if (g_strcmp0(phase, "scroll") == 0) {
    GdkEvent* event = gdk_event_new(GDK_SCROLL);
    event->scroll.window = window;
    g_object_ref(window);
    event->scroll.send_event = TRUE;
    event->scroll.time = GDK_CURRENT_TIME;
    event->scroll.x = px;
    event->scroll.y = py;
    event->scroll.x_root = px;
    event->scroll.y_root = py;
    event->scroll.direction = GDK_SCROLL_SMOOTH;
    // Flutter deltas are logical pixels; GDK_SCROLL_SMOOTH is typically in wheel ticks
    // (~40 px). Keep fractional values so trackpad motion stays smooth.
    event->scroll.delta_x = dx / 40.0;
    event->scroll.delta_y = dy / 40.0;
    event->scroll.state = state;
    gdk_event_set_device(event, default_pointer_device());
    gtk_widget_event(GTK_WIDGET(view->web_view), event);
    gdk_event_free(event);
    schedule_present(view);
    return;
  }

  if (g_strcmp0(phase, "enter") == 0) {
    dispatch_crossing(view, TRUE, px, py, state);
    maybe_emit_cursor(view);
    schedule_present(view);
    return;
  }
  if (g_strcmp0(phase, "leave") == 0) {
    dispatch_crossing(view, FALSE, px, py, state);
    if (g_strcmp0(view->cursor_shape, "default") != 0) {
      g_free(view->cursor_shape);
      view->cursor_shape = g_strdup("default");
      morph_control_emit_cursor(view->view_id, "default");
    }
    schedule_present(view);
    return;
  }

  if (g_strcmp0(phase, "move") == 0 || g_strcmp0(phase, "hover") == 0) {
    GdkEvent* event = gdk_event_new(GDK_MOTION_NOTIFY);
    event->motion.window = window;
    g_object_ref(window);
    event->motion.send_event = TRUE;
    event->motion.time = GDK_CURRENT_TIME;
    event->motion.x = px;
    event->motion.y = py;
    event->motion.x_root = px;
    event->motion.y_root = py;
    event->motion.state = state;
    event->motion.is_hint = FALSE;
    gdk_event_set_device(event, default_pointer_device());
    gtk_widget_event(GTK_WIDGET(view->web_view), event);
    gdk_event_free(event);
    maybe_emit_cursor(view);
    schedule_present(view);
    return;
  }

  if (g_strcmp0(phase, "down") == 0) {
    grab_view_focus(view);
  }
  GdkEventType type = GDK_BUTTON_PRESS;
  if (g_strcmp0(phase, "up") == 0 || g_strcmp0(phase, "cancel") == 0) {
    type = GDK_BUTTON_RELEASE;
  }
  GdkEvent* event = gdk_event_new(type);
  event->button.window = window;
  g_object_ref(window);
  event->button.send_event = TRUE;
  event->button.time = GDK_CURRENT_TIME;
  event->button.x = px;
  event->button.y = py;
  event->button.x_root = px;
  event->button.y_root = py;
  event->button.button = button == 0 ? 1 : static_cast<guint>(button);
  event->button.state = state;
  gdk_event_set_device(event, default_pointer_device());
  gtk_widget_event(GTK_WIDGET(view->web_view), event);
  gdk_event_free(event);
  maybe_emit_cursor(view);
  schedule_present(view);
}

static gboolean flush_pending_pointer(gpointer data) {
  MorphView* view = static_cast<MorphView*>(data);
  view->pending_move_source = 0;
  if (view->pending_phase == nullptr || view->web_view == nullptr) {
    return G_SOURCE_REMOVE;
  }
  dispatch_pointer(view, view->pending_phase, view->pending_x, view->pending_y, view->pending_button,
                   view->pending_buttons, view->pending_sx, view->pending_sy, view->pending_ctrl,
                   view->pending_alt, view->pending_shift, view->pending_meta);
  g_free(view->pending_phase);
  view->pending_phase = nullptr;
  view->pending_sx = 0;
  view->pending_sy = 0;
  return G_SOURCE_REMOVE;
}

static gboolean is_coalesced_phase(const gchar* phase) {
  return g_strcmp0(phase, "move") == 0 || g_strcmp0(phase, "hover") == 0 ||
         g_strcmp0(phase, "scroll") == 0;
}

static void queue_pointer(MorphView* view,
                          const gchar* phase,
                          double x,
                          double y,
                          int button,
                          int buttons,
                          double dx,
                          double dy,
                          gboolean ctrl,
                          gboolean alt,
                          gboolean shift,
                          gboolean meta) {
  if (view == nullptr || view->web_view == nullptr) {
    return;
  }
  view->last_input_us = g_get_monotonic_time();
  if (is_coalesced_phase(phase)) {
    if (g_strcmp0(phase, "scroll") == 0 && g_strcmp0(view->pending_phase, "scroll") == 0) {
      view->pending_x = x;
      view->pending_y = y;
      view->pending_sx += dx;
      view->pending_sy += dy;
      view->pending_buttons = buttons;
      view->pending_ctrl = ctrl;
      view->pending_alt = alt;
      view->pending_shift = shift;
      view->pending_meta = meta;
    } else {
      g_free(view->pending_phase);
      view->pending_phase = g_strdup(phase);
      view->pending_x = x;
      view->pending_y = y;
      view->pending_button = button;
      view->pending_buttons = buttons;
      view->pending_sx = dx;
      view->pending_sy = dy;
      view->pending_ctrl = ctrl;
      view->pending_alt = alt;
      view->pending_shift = shift;
      view->pending_meta = meta;
    }
    if (view->pending_move_source == 0) {
      view->pending_move_source = g_idle_add(flush_pending_pointer, view);
    }
    return;
  }
  if (view->pending_move_source != 0) {
    g_source_remove(view->pending_move_source);
    flush_pending_pointer(view);
  }
  dispatch_pointer(view, phase, x, y, button, buttons, dx, dy, ctrl, alt, shift, meta);
}

static guint16 hardware_keycode_for(guint keyval, guint provided, guint8* group) {
  if (group != nullptr) {
    *group = 0;
  }
  GdkDisplay* display = gdk_display_get_default();
  if (display != nullptr) {
    GdkKeymap* keymap = gdk_keymap_get_for_display(display);
    GdkKeymapKey* keys = nullptr;
    gint n_keys = 0;
    if (gdk_keymap_get_entries_for_keyval(keymap, keyval, &keys, &n_keys) && n_keys > 0) {
      guint16 code = static_cast<guint16>(keys[0].keycode);
      if (group != nullptr) {
        *group = static_cast<guint8>(keys[0].group);
      }
      g_free(keys);
      return code;
    }
    g_free(keys);
  }
  if (provided >= 8 && provided <= 255) {
    return static_cast<guint16>(provided);
  }
  return 0;
}

static void dispatch_key(MorphView* view,
                         const gchar* phase,
                         const gchar* key,
                         const gchar* text,
                         guint hardware_keycode,
                         gboolean ctrl,
                         gboolean alt,
                         gboolean shift,
                         gboolean meta,
                         gboolean repeat) {
  GdkWindow* window = gtk_widget_get_window(GTK_WIDGET(view->web_view));
  if (window == nullptr) {
    return;
  }
  view->last_input_us = g_get_monotonic_time();
  grab_view_focus(view);
  GdkEventType type = g_strcmp0(phase, "up") == 0 ? GDK_KEY_RELEASE : GDK_KEY_PRESS;
  guint keyval = morph_keyval_for_event(key, text);
  if (keyval == GDK_KEY_Tab && shift && !ctrl && !alt && !meta) {
    keyval = GDK_KEY_ISO_Left_Tab;
  }
  if (keyval == GDK_KEY_VoidSymbol) {
    return;
  }
  GdkEvent* event = gdk_event_new(type);
  event->key.window = window;
  g_object_ref(window);
  event->key.send_event = TRUE;
  event->key.time = GDK_CURRENT_TIME;
  event->key.keyval = keyval;
  event->key.state = (ctrl ? GDK_CONTROL_MASK : 0) | (alt ? GDK_MOD1_MASK : 0) |
                     (shift ? GDK_SHIFT_MASK : 0) | (meta ? GDK_META_MASK | GDK_SUPER_MASK : 0);
  guint8 group = 0;
  event->key.hardware_keycode = hardware_keycode_for(keyval, hardware_keycode, &group);
  event->key.group = group;
  event->key.is_modifier = (keyval == GDK_KEY_Shift_L || keyval == GDK_KEY_Shift_R || keyval == GDK_KEY_Control_L ||
                            keyval == GDK_KEY_Control_R || keyval == GDK_KEY_Alt_L || keyval == GDK_KEY_Alt_R ||
                            keyval == GDK_KEY_Meta_L || keyval == GDK_KEY_Meta_R || keyval == GDK_KEY_Super_L ||
                            keyval == GDK_KEY_Super_R)
                               ? TRUE
                               : FALSE;
  if (type == GDK_KEY_PRESS && text != nullptr && text[0] != '\0' && !ctrl && !alt && !meta) {
    gunichar c = g_utf8_get_char(text);
    if (c >= 0x20 && c != 0x7f) {
      event->key.string = g_strdup(text);
      event->key.length = static_cast<gint>(strlen(text));
    }
  }
  (void)repeat;
  gdk_event_set_device(event, default_keyboard_device());
  gtk_widget_event(GTK_WIDGET(view->web_view), event);
  gdk_event_free(event);
  schedule_present(view);
}

static void dispatch_ime(MorphView* view, const gchar* phase, const gchar* text) {
  if (g_strcmp0(phase, "focus") == 0) {
    grab_view_focus(view);
  }
  if (view->ime != nullptr) {
    morph_ime_context_handle(view->ime, phase, text);
  }
  schedule_present(view);
}

static FlMethodResponse* handle_create(MorphWebviewLinuxPlugin* plugin, FlValue* args) {
  const gchar* url = arg_string(args, "initialUrl", "https://example.com");
  int64_t width = arg_int(args, "width", 800);
  int64_t height = arg_int(args, "height", 600);
  double dpr = arg_float(args, "devicePixelRatio", 1);
  const gchar* script = arg_string(args, "userScript", "");
  const gchar* profile_id = arg_string(args, "profileId", "default");
  MorphView* view = morph_view_create(plugin, url, static_cast<uint32_t>(width),
                                      static_cast<uint32_t>(height), dpr, script, nullptr,
                                      profile_id);
  g_autoptr(FlValue) result = fl_value_new_map();
  fl_value_set_string_take(result, "viewId", fl_value_new_int(view->view_id));
  fl_value_set_string_take(result, "textureId",
                           fl_value_new_int(fl_texture_get_id(FL_TEXTURE(view->texture))));
  fl_value_set_string_take(result, "renderPath", fl_value_new_string(view->render_path));
  fl_value_set_string_take(result, "renderReason", fl_value_new_string(view->render_reason));
  fl_value_set_string_take(
      result, "controlEndpoint",
      fl_value_new_string(plugin->control_endpoint != nullptr ? plugin->control_endpoint : ""));
  return FL_METHOD_RESPONSE(fl_method_success_response_new(result));
}

static void morph_webview_linux_plugin_handle_method_call(MorphWebviewLinuxPlugin* self,
                                                          FlMethodCall* method_call) {
  const gchar* method = fl_method_call_get_name(method_call);
  FlValue* args = fl_method_call_get_args(method_call);
  g_autoptr(FlMethodResponse) response = nullptr;

  if (strcmp(method, "create") == 0) {
    response = handle_create(self, args);
  } else {
    MorphView* view = lookup_view(self, args);
    if (view == nullptr) {
      response = FL_METHOD_RESPONSE(fl_method_error_response_new("missing_view", "unknown viewId",
                                                                 nullptr));
    } else if (strcmp(method, "dispose") == 0) {
      self->views->erase(view->view_id);
      morph_view_destroy(view);
      response = FL_METHOD_RESPONSE(fl_method_success_response_new(fl_value_new_null()));
    } else if (strcmp(method, "resize") == 0) {
      apply_size(view, static_cast<uint32_t>(arg_int(args, "width", view->logical_width)),
                 static_cast<uint32_t>(arg_int(args, "height", view->logical_height)),
                 arg_float(args, "devicePixelRatio", view->dpr));
      response = FL_METHOD_RESPONSE(fl_method_success_response_new(fl_value_new_null()));
    } else {
      response = FL_METHOD_RESPONSE(fl_method_not_implemented_response_new());
    }
  }

  fl_method_call_respond(method_call, response, nullptr);
}

static void method_call_cb(FlMethodChannel* channel,
                           FlMethodCall* method_call,
                           gpointer user_data) {
  (void)channel;
  morph_webview_linux_plugin_handle_method_call(MORPH_WEBVIEW_LINUX_PLUGIN(user_data), method_call);
}

static FlMethodErrorResponse* events_listen_cb(FlEventChannel* channel,
                                               FlValue* args,
                                               gpointer user_data) {
  (void)channel;
  (void)args;
  MorphWebviewLinuxPlugin* plugin = MORPH_WEBVIEW_LINUX_PLUGIN(user_data);
  plugin->events_listening = TRUE;
  return nullptr;
}

static FlMethodErrorResponse* events_cancel_cb(FlEventChannel* channel,
                                               FlValue* args,
                                               gpointer user_data) {
  (void)channel;
  (void)args;
  MorphWebviewLinuxPlugin* plugin = MORPH_WEBVIEW_LINUX_PLUGIN(user_data);
  plugin->events_listening = FALSE;
  return nullptr;
}

static void morph_webview_linux_plugin_dispose(GObject* object) {
  MorphWebviewLinuxPlugin* self = MORPH_WEBVIEW_LINUX_PLUGIN(object);
  morph_control_stop();
  g_clear_pointer(&self->control_endpoint, g_free);
  if (self->views != nullptr) {
    for (auto& entry : *self->views) {
      morph_view_destroy(entry.second);
    }
    delete self->views;
    self->views = nullptr;
  }
  g_clear_object(&self->channel);
  g_clear_object(&self->events);
  self->textures = nullptr;
  self->registrar = nullptr;
  G_OBJECT_CLASS(morph_webview_linux_plugin_parent_class)->dispose(object);
}

static void morph_webview_linux_plugin_class_init(MorphWebviewLinuxPluginClass* klass) {
  G_OBJECT_CLASS(klass)->dispose = morph_webview_linux_plugin_dispose;
}

static void morph_webview_linux_plugin_init(MorphWebviewLinuxPlugin* self) {
  self->next_view_id = 1;
  self->views = new std::map<int64_t, MorphView*>();
  self->events_listening = FALSE;
  self->control_endpoint = nullptr;
}

void morph_webview_linux_plugin_register_with_registrar(FlPluginRegistrar* registrar) {
  MorphWebviewLinuxPlugin* plugin = MORPH_WEBVIEW_LINUX_PLUGIN(
      g_object_new(morph_webview_linux_plugin_get_type(), nullptr));
  plugin->registrar = registrar;
  plugin->textures = fl_plugin_registrar_get_texture_registrar(registrar);

  g_autoptr(FlStandardMethodCodec) codec = fl_standard_method_codec_new();
  plugin->channel = fl_method_channel_new(fl_plugin_registrar_get_messenger(registrar),
                                          kMethodChannel, FL_METHOD_CODEC(codec));
  fl_method_channel_set_method_call_handler(plugin->channel, method_call_cb, g_object_ref(plugin),
                                            g_object_unref);

  plugin->events = fl_event_channel_new(fl_plugin_registrar_get_messenger(registrar), kEventChannel,
                                        FL_METHOD_CODEC(codec));
  fl_event_channel_set_stream_handlers(plugin->events, events_listen_cb, events_cancel_cb,
                                       g_object_ref(plugin), g_object_unref);

  start_control_server(plugin);

  g_object_unref(plugin);
}
