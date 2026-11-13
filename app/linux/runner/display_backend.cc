#include "display_backend.h"

#include <glib.h>
#include <string.h>

// First comma-separated GDK_BACKEND token, ignoring surrounding spaces.
// Copies into |out| (size |out_len|) and returns out, or NULL if none.
static const char* first_gdk_backend_token(const char* gdk_backend,
                                           char* out,
                                           size_t out_len) {
  if (gdk_backend == nullptr || out == nullptr || out_len == 0) {
    return nullptr;
  }
  while (*gdk_backend == ' ' || *gdk_backend == '\t') {
    gdk_backend++;
  }
  if (*gdk_backend == '\0') {
    return nullptr;
  }
  size_t n = 0;
  while (gdk_backend[n] != '\0' && gdk_backend[n] != ',' &&
         gdk_backend[n] != ' ' && gdk_backend[n] != '\t') {
    n++;
  }
  if (n == 0 || n >= out_len) {
    return nullptr;
  }
  memcpy(out, gdk_backend, n);
  out[n] = '\0';
  return out;
}

int morph_runner_has_nvidia_proprietary_driver(void) {
  return g_file_test("/proc/driver/nvidia/version", G_FILE_TEST_EXISTS) ||
         g_file_test("/proc/driver/nvidia", G_FILE_TEST_IS_DIR);
}

const char* morph_preferred_gdk_backend(const char* gdk_backend,
                                        const char* wayland_display,
                                        const char* x11_display,
                                        int has_nvidia) {
  if (x11_display == nullptr || x11_display[0] == '\0') {
    return nullptr;
  }
  if (!has_nvidia) {
    return nullptr;
  }

  char token[32];
  const char* first = first_gdk_backend_token(gdk_backend, token, sizeof(token));
  if (first != nullptr && strcmp(first, "x11") == 0) {
    return nullptr;
  }

  const int wayland_session =
      wayland_display != nullptr && wayland_display[0] != '\0';
  const int asks_wayland = first != nullptr && strcmp(first, "wayland") == 0;
  if (asks_wayland || (first == nullptr && wayland_session)) {
    return "x11";
  }
  return nullptr;
}

void morph_prepare_gdk_backend(void) {
  const char* allow = g_getenv("MORPH_ALLOW_WAYLAND");
  if (allow != nullptr && strcmp(allow, "1") == 0) {
    return;
  }
  const char* forced = g_getenv("MORPH_GDK_BACKEND");
  if (forced != nullptr && forced[0] != '\0') {
    g_setenv("GDK_BACKEND", forced, TRUE);
    return;
  }
  const char* current = g_getenv("GDK_BACKEND");
  const char* chosen = morph_preferred_gdk_backend(
      current, g_getenv("WAYLAND_DISPLAY"), g_getenv("DISPLAY"),
      morph_runner_has_nvidia_proprietary_driver());
  if (chosen != nullptr) {
    // Overwrite session defaults such as Omarchy's "wayland,x11,*".
    g_setenv("GDK_BACKEND", chosen, TRUE);
    g_message("morph: NVIDIA Wayland: using GDK_BACKEND=%s (was %s) so "
              "Flutter can create a GL context",
              chosen, current != nullptr && current[0] != '\0' ? current : "unset");
  }
}
