// Micro-benchmark and correctness check for Linux frame copies.
// Compares the legacy BGRA→RGBA swizzle against a packed BGRA memcpy, and
// times webkit_web_view_get_snapshot versus copying the already-painted
// offscreen window surface. Run under xvfb-run.
#include "frame.h"
#include "session.h"

#include <gtk/gtk.h>
#include <webkit2/webkit2.h>

#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

static int g_failures = 0;

static void expect_true(bool ok, const char* message) {
  if (!ok) {
    std::fprintf(stderr, "FAIL: %s\n", message);
    g_failures++;
  }
}

static cairo_surface_t* make_argb_surface(int width, int height) {
  cairo_surface_t* surface = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, width, height);
  unsigned char* data = cairo_image_surface_get_data(surface);
  int stride = cairo_image_surface_get_stride(surface);
  for (int y = 0; y < height; y++) {
    unsigned char* row = data + static_cast<size_t>(y) * stride;
    for (int x = 0; x < width; x++) {
      row[0] = static_cast<unsigned char>(x);      // B
      row[1] = static_cast<unsigned char>(y);      // G
      row[2] = static_cast<unsigned char>(x + y);  // R
      row[3] = 255;                                // A
      row += 4;
    }
  }
  cairo_surface_mark_dirty(surface);
  return surface;
}

static void check_copies() {
  const int width = 128;
  const int height = 64;
  cairo_surface_t* surface = make_argb_surface(width, height);
  std::vector<uint8_t> bgra(static_cast<size_t>(width) * height * 4);
  std::vector<uint8_t> rgba(static_cast<size_t>(width) * height * 4);
  morph_copy_cairo_to_bgra(surface, bgra.data(), width, height);
  morph_copy_cairo_to_rgba(surface, rgba.data(), width, height);
  bool bgra_ok = true;
  bool rgba_ok = true;
  for (int y = 0; y < height; y++) {
    for (int x = 0; x < width; x++) {
      const uint8_t b = static_cast<uint8_t>(x);
      const uint8_t g = static_cast<uint8_t>(y);
      const uint8_t r = static_cast<uint8_t>(x + y);
      const uint8_t a = 255;
      size_t i = (static_cast<size_t>(y) * width + x) * 4;
      if (bgra[i] != b || bgra[i + 1] != g || bgra[i + 2] != r || bgra[i + 3] != a) {
        bgra_ok = false;
      }
      if (rgba[i] != r || rgba[i + 1] != g || rgba[i + 2] != b || rgba[i + 3] != a) {
        rgba_ok = false;
      }
    }
  }
  expect_true(bgra_ok, "BGRA copy should match the cairo buffer");
  expect_true(rgba_ok, "RGBA swizzle should swap R and B");
  expect_true(morph_frame_interval_ms() >= 1 && morph_frame_interval_ms() <= 100,
              "frame interval should be a reasonable refresh period");
  cairo_surface_destroy(surface);
}

static double now_ms() {
  return static_cast<double>(g_get_monotonic_time()) / 1000.0;
}

static void bench_copies() {
  const int width = 1920;
  const int height = 1080;
  cairo_surface_t* surface = make_argb_surface(width, height);
  std::vector<uint8_t> dest(static_cast<size_t>(width) * height * 4);
  const int iters = 30;
  morph_copy_cairo_to_bgra(surface, dest.data(), width, height);
  morph_copy_cairo_to_rgba(surface, dest.data(), width, height);
  double t0 = now_ms();
  for (int i = 0; i < iters; i++) {
    morph_copy_cairo_to_bgra(surface, dest.data(), width, height);
  }
  double bgra_ms = (now_ms() - t0) / iters;
  t0 = now_ms();
  for (int i = 0; i < iters; i++) {
    morph_copy_cairo_to_rgba(surface, dest.data(), width, height);
  }
  double rgba_ms = (now_ms() - t0) / iters;
  std::printf("copy 1920x1080: bgra-memcpy %.2f ms  rgba-swizzle %.2f ms  (n=%d)\n", bgra_ms,
              rgba_ms, iters);
  cairo_surface_destroy(surface);
}

struct SnapState {
  cairo_surface_t* surface;
  GMainLoop* loop;
};

static void on_snapshot(GObject* source, GAsyncResult* result, gpointer user_data) {
  auto* state = static_cast<SnapState*>(user_data);
  state->surface = webkit_web_view_get_snapshot_finish(WEBKIT_WEB_VIEW(source), result, nullptr);
  g_main_loop_quit(state->loop);
}

static cairo_surface_t* take_snapshot(WebKitWebView* view) {
  SnapState state = {nullptr, g_main_loop_new(nullptr, FALSE)};
  webkit_web_view_get_snapshot(view, WEBKIT_SNAPSHOT_REGION_VISIBLE, WEBKIT_SNAPSHOT_OPTIONS_NONE,
                               nullptr, on_snapshot, &state);
  g_main_loop_run(state.loop);
  g_main_loop_unref(state.loop);
  return state.surface;
}

static void on_load_finished(WebKitWebView*, WebKitLoadEvent event, gpointer user_data) {
  if (event == WEBKIT_LOAD_FINISHED) {
    g_main_loop_quit(static_cast<GMainLoop*>(user_data));
  }
}

static void bench_snapshot_vs_offscreen() {
  GtkWidget* offscreen = gtk_offscreen_window_new();
  WebKitSettings* settings = morph_session_create_settings();
  WebKitWebView* view = WEBKIT_WEB_VIEW(g_object_new(
      WEBKIT_TYPE_WEB_VIEW, "web-context", morph_session_shared_context(), "settings", settings,
      nullptr));
  g_object_unref(settings);
  gtk_widget_set_size_request(GTK_WIDGET(view), 1280, 720);
  gtk_container_add(GTK_CONTAINER(offscreen), GTK_WIDGET(view));
  gtk_window_resize(GTK_WINDOW(offscreen), 1280, 720);
  gtk_widget_show_all(offscreen);
  while (gtk_events_pending()) {
    gtk_main_iteration();
  }

  const char* html =
      "data:text/html,<html><body style='margin:0;background:#123;color:#fff;font:64px "
      "sans-serif'><div id=t>0</div><script>let n=0;setInterval(()=>{n++;document.getElementById('t')"
      ".textContent=n},16);</script></body></html>";
  GMainLoop* loop = g_main_loop_new(nullptr, FALSE);
  gulong handler = g_signal_connect(view, "load-changed", G_CALLBACK(on_load_finished), loop);
  webkit_web_view_load_uri(view, html);
  g_main_loop_run(loop);
  g_signal_handler_disconnect(view, handler);
  g_main_loop_unref(loop);
  // Let the first paint land in the offscreen window.
  gint64 until = g_get_monotonic_time() + 200000;
  while (g_get_monotonic_time() < until) {
    gtk_main_iteration_do(FALSE);
    g_usleep(2000);
  }

  const int iters = 20;
  std::vector<uint8_t> dest(static_cast<size_t>(1280) * 720 * 4);
  double snap_ms = 0;
  int snap_ok = 0;
  for (int i = 0; i < iters; i++) {
    double t0 = now_ms();
    cairo_surface_t* surface = take_snapshot(view);
    if (surface != nullptr) {
      uint32_t w = static_cast<uint32_t>(cairo_image_surface_get_width(surface));
      uint32_t h = static_cast<uint32_t>(cairo_image_surface_get_height(surface));
      if (dest.size() < static_cast<size_t>(w) * h * 4) {
        dest.resize(static_cast<size_t>(w) * h * 4);
      }
      morph_copy_cairo_to_rgba(surface, dest.data(), w, h);
      cairo_surface_destroy(surface);
      snap_ok++;
    }
    snap_ms += now_ms() - t0;
    gtk_main_iteration_do(FALSE);
  }

  double off_ms = 0;
  int off_ok = 0;
  for (int i = 0; i < iters; i++) {
    gtk_widget_queue_draw(GTK_WIDGET(view));
    while (gtk_events_pending()) {
      gtk_main_iteration();
    }
    double t0 = now_ms();
    cairo_surface_t* surface = gtk_offscreen_window_get_surface(GTK_OFFSCREEN_WINDOW(offscreen));
    if (surface != nullptr && cairo_surface_get_type(surface) == CAIRO_SURFACE_TYPE_IMAGE) {
      uint32_t w = static_cast<uint32_t>(cairo_image_surface_get_width(surface));
      uint32_t h = static_cast<uint32_t>(cairo_image_surface_get_height(surface));
      if (w > 0 && h > 0) {
        if (dest.size() < static_cast<size_t>(w) * h * 4) {
          dest.resize(static_cast<size_t>(w) * h * 4);
        }
        morph_copy_cairo_to_bgra(surface, dest.data(), w, h);
        off_ok++;
      }
    }
    off_ms += now_ms() - t0;
  }

  std::printf("present 1280x720: get_snapshot+swizzle %.2f ms (ok=%d/%d)  "
              "offscreen+bgra-copy %.2f ms (ok=%d/%d)  n=%d\n",
              snap_ok ? snap_ms / snap_ok : -1.0, snap_ok, iters, off_ok ? off_ms / off_ok : -1.0,
              off_ok, iters, iters);
  expect_true(snap_ok > 0, "get_snapshot should produce at least one frame");

  gtk_widget_destroy(offscreen);
}

int main(int argc, char** argv) {
  g_setenv("WEBKIT_DISABLE_DMABUF_RENDERER", "1", FALSE);
  g_setenv("WEBKIT_DISABLE_COMPOSITING_MODE", "1", FALSE);
  g_setenv("LIBGL_ALWAYS_SOFTWARE", "1", FALSE);
  gtk_init(&argc, &argv);
  gchar* temp_home = g_dir_make_tmp("morph-frame-check-XXXXXX", nullptr);
  if (temp_home != nullptr) {
    g_setenv("MORPH_HOME", temp_home, TRUE);
  }
  check_copies();
  bench_copies();
  bench_snapshot_vs_offscreen();
  if (temp_home != nullptr) {
    gchar* rm = g_strdup_printf("rm -rf %s", temp_home);
    g_spawn_command_line_sync(rm, nullptr, nullptr, nullptr, nullptr);
    g_free(rm);
    g_free(temp_home);
    g_unsetenv("MORPH_HOME");
  }
  if (g_failures > 0) {
    std::fprintf(stderr, "%d frame check(s) failed\n", g_failures);
    return 1;
  }
  std::fprintf(stderr, "all frame checks passed\n");
  return 0;
}
