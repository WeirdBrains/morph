#include "include/morph_webview_linux/morph_webview_linux_plugin.h"

#include "morph_webview_control.h"
#include "ime_context.h"
#include "key_map.h"
#include "render_path.h"

#include <cairo/cairo.h>
#include <flutter_linux/flutter_linux.h>
#include <gtk/gtk.h>
#include <webkit2/webkit2.h>

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

struct MorphView;

G_DECLARE_FINAL_TYPE(MorphPixelBufferTexture,
                     morph_pixel_buffer_texture,
                     MORPH,
                     PIXEL_BUFFER_TEXTURE,
                     FlPixelBufferTexture)

struct _MorphPixelBufferTexture {
  FlPixelBufferTexture parent_instance;
  MorphView* view;
};

struct MorphView {
  int64_t view_id;
  MorphWebviewLinuxPlugin* plugin;
  FlTextureRegistrar* textures;
  MorphPixelBufferTexture* texture;
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

__attribute__((unused)) static void emit_event(MorphView* view, FlValue* event) {
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

static void copy_cairo_to_rgba(cairo_surface_t* surface,
                               uint8_t* dest,
                               uint32_t width,
                               uint32_t height) {
  cairo_surface_flush(surface);
  unsigned char* src = cairo_image_surface_get_data(surface);
  int stride = cairo_image_surface_get_stride(surface);
  for (uint32_t y = 0; y < height; y++) {
    unsigned char* row = src + static_cast<size_t>(y) * stride;
    uint8_t* out = dest + static_cast<size_t>(y) * width * 4;
    for (uint32_t x = 0; x < width; x++) {
      // CAIRO_FORMAT_ARGB32 is native-endian, BGRA on little endian.
      out[0] = row[2];
      out[1] = row[1];
      out[2] = row[0];
      out[3] = row[3];
      row += 4;
      out += 4;
    }
  }
}

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
  copy_cairo_to_rgba(surface, view->back, view->tex_width, view->tex_height);
  uint8_t* tmp = view->front;
  view->front = view->back;
  view->back = tmp;
  g_mutex_unlock(&view->lock);
  if (view->textures != nullptr && view->texture != nullptr) {
    fl_texture_registrar_mark_texture_frame_available(view->textures, FL_TEXTURE(view->texture));
  }
}

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
    apply_snapshot_surface(view, surface);
    cairo_surface_destroy(surface);
  }
  if (view->snapshot_dirty) {
    view->snapshot_dirty = FALSE;
    request_snapshot(view);
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

static gboolean snapshot_timer_cb(gpointer user_data) {
  MorphView* view = static_cast<MorphView*>(user_data);
  request_snapshot(view);
  gboolean is_loading = FALSE;
  if (view->web_view != nullptr) {
    g_object_get(view->web_view, "is-loading", &is_loading, nullptr);
  }
  if (!is_loading) {
    view->snapshot_timer = 0;
    return G_SOURCE_REMOVE;
  }
  return G_SOURCE_CONTINUE;
}

static void start_snapshot_timer(MorphView* view) {
  if (view->snapshot_timer == 0) {
    view->snapshot_timer = g_timeout_add(33, snapshot_timer_cb, view);
  }
}

static void on_load_changed(WebKitWebView* web_view, WebKitLoadEvent load_event, gpointer user_data) {
  MorphView* view = static_cast<MorphView*>(user_data);
  (void)web_view;
  emit_navigation(view);
  if (load_event == WEBKIT_LOAD_STARTED) {
    start_snapshot_timer(view);
  }
  if (load_event == WEBKIT_LOAD_COMMITTED || load_event == WEBKIT_LOAD_FINISHED) {
    request_snapshot(view);
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

static void add_user_script(WebKitUserContentManager* manager, const gchar* source) {
  if (source == nullptr || *source == '\0') {
    return;
  }
  WebKitUserScript* script =
      webkit_user_script_new(source, WEBKIT_USER_CONTENT_INJECT_TOP_FRAME,
                             WEBKIT_USER_SCRIPT_INJECT_AT_DOCUMENT_START, nullptr, nullptr);
  webkit_user_content_manager_add_script(manager, script);
  webkit_user_script_unref(script);
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
  request_snapshot(view);
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
                                    const gchar* user_script) {
  if (morph_should_disable_webkit_dmabuf_renderer() &&
      g_getenv("WEBKIT_DISABLE_DMABUF_RENDERER") == nullptr) {
    g_setenv("WEBKIT_DISABLE_DMABUF_RENDERER", "1", FALSE);
  }

  MorphView* view = g_new0(MorphView, 1);
  g_mutex_init(&view->lock);
  view->plugin = plugin;
  view->textures = plugin->textures;
  view->view_id = plugin->next_view_id++;
  gchar* reason = nullptr;
  MorphRenderPath path = morph_detect_render_path(&reason);
  view->render_path = g_strdup(morph_render_path_name(path));
  view->render_reason = reason != nullptr ? reason : g_strdup("unknown");

  view->texture = morph_pixel_buffer_texture_new(view);
  if (!fl_texture_registrar_register_texture(plugin->textures, FL_TEXTURE(view->texture))) {
    g_warning("morph_webview: failed to register texture");
  }

  WebKitUserContentManager* manager = webkit_user_content_manager_new();
  g_signal_connect(manager, "script-message-received::morphRust", G_CALLBACK(on_script_message),
                   view);
  webkit_user_content_manager_register_script_message_handler(manager, "morphRust");
  add_user_script(manager, kMorphRustShim);
  add_user_script(manager, user_script);

  view->offscreen = gtk_offscreen_window_new();
  view->web_view = WEBKIT_WEB_VIEW(webkit_web_view_new_with_user_content_manager(manager));
  g_object_unref(manager);
  gtk_container_add(GTK_CONTAINER(view->offscreen), GTK_WIDGET(view->web_view));
  gtk_widget_set_can_focus(GTK_WIDGET(view->web_view), TRUE);
  gtk_widget_add_events(GTK_WIDGET(view->web_view),
                        GDK_POINTER_MOTION_MASK | GDK_BUTTON_PRESS_MASK | GDK_BUTTON_RELEASE_MASK |
                            GDK_SCROLL_MASK | GDK_SMOOTH_SCROLL_MASK | GDK_ENTER_NOTIFY_MASK |
                            GDK_LEAVE_NOTIFY_MASK | GDK_KEY_PRESS_MASK | GDK_KEY_RELEASE_MASK |
                            GDK_FOCUS_CHANGE_MASK);
  gtk_widget_show_all(view->offscreen);

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
  if (view->textures != nullptr && view->texture != nullptr) {
    fl_texture_registrar_unregister_texture(view->textures, FL_TEXTURE(view->texture));
  }
  if (view->texture != nullptr) {
    view->texture->view = nullptr;
    g_object_unref(view->texture);
    view->texture = nullptr;
  }
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
      dispatch_pointer(view, job->s1 != nullptr ? job->s1 : "down", job->x, job->y, job->button,
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
    request_snapshot(view);
    start_snapshot_timer(view);
    return;
  }

  if (g_strcmp0(phase, "enter") == 0) {
    dispatch_crossing(view, TRUE, px, py, state);
    maybe_emit_cursor(view);
    request_snapshot(view);
    return;
  }
  if (g_strcmp0(phase, "leave") == 0) {
    dispatch_crossing(view, FALSE, px, py, state);
    if (g_strcmp0(view->cursor_shape, "default") != 0) {
      g_free(view->cursor_shape);
      view->cursor_shape = g_strdup("default");
      morph_control_emit_cursor(view->view_id, "default");
    }
    request_snapshot(view);
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
    request_snapshot(view);
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
  request_snapshot(view);
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
  request_snapshot(view);
}

static void dispatch_ime(MorphView* view, const gchar* phase, const gchar* text) {
  if (g_strcmp0(phase, "focus") == 0) {
    grab_view_focus(view);
  }
  if (view->ime != nullptr) {
    morph_ime_context_handle(view->ime, phase, text);
  }
  request_snapshot(view);
}

static FlMethodResponse* handle_create(MorphWebviewLinuxPlugin* plugin, FlValue* args) {
  const gchar* url = arg_string(args, "initialUrl", "https://example.com");
  int64_t width = arg_int(args, "width", 800);
  int64_t height = arg_int(args, "height", 600);
  double dpr = arg_float(args, "devicePixelRatio", 1);
  const gchar* script = arg_string(args, "userScript", "");
  MorphView* view = morph_view_create(plugin, url, static_cast<uint32_t>(width),
                                      static_cast<uint32_t>(height), dpr, script);
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
