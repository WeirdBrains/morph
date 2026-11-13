#include "display_backend.h"

#include <glib.h>

int morph_runner_has_nvidia_proprietary_driver(void) {
  return g_file_test("/proc/driver/nvidia/version", G_FILE_TEST_EXISTS) ||
         g_file_test("/proc/driver/nvidia", G_FILE_TEST_IS_DIR);
}

const char* morph_preferred_gdk_backend(const char* gdk_backend,
                                        const char* wayland_display,
                                        const char* x11_display,
                                        int has_nvidia) {
  if (gdk_backend != nullptr && gdk_backend[0] != '\0') {
    return nullptr;
  }
  if (wayland_display == nullptr || wayland_display[0] == '\0') {
    return nullptr;
  }
  if (x11_display == nullptr || x11_display[0] == '\0') {
    return nullptr;
  }
  if (!has_nvidia) {
    return nullptr;
  }
  return "x11";
}

void morph_prepare_gdk_backend(void) {
  const char* chosen = morph_preferred_gdk_backend(
      g_getenv("GDK_BACKEND"), g_getenv("WAYLAND_DISPLAY"),
      g_getenv("DISPLAY"), morph_runner_has_nvidia_proprietary_driver());
  if (chosen != nullptr) {
    g_setenv("GDK_BACKEND", chosen, FALSE);
  }
}
