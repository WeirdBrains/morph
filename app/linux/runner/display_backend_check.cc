#include "display_backend.h"

#include <stdio.h>
#include <string.h>

static int failures = 0;

static void expect_null(const char* name, const char* got) {
  if (got != nullptr) {
    fprintf(stderr, "FAIL %s: expected NULL, got %s\n", name, got);
    failures++;
  }
}

static void expect_streq(const char* name, const char* got, const char* want) {
  if (got == nullptr || strcmp(got, want) != 0) {
    fprintf(stderr, "FAIL %s: expected %s, got %s\n", name, want,
            got != nullptr ? got : "NULL");
    failures++;
  }
}

int main() {
  expect_null("explicit x11",
              morph_preferred_gdk_backend("x11", "wayland-1", ":0", 1));
  expect_null("x11 first in list",
              morph_preferred_gdk_backend("x11,wayland", "wayland-1", ":0", 1));
  expect_null("no wayland session",
              morph_preferred_gdk_backend(nullptr, nullptr, ":0", 1));
  expect_null("empty wayland display",
              morph_preferred_gdk_backend(nullptr, "", ":0", 1));
  expect_null("wayland without XWayland",
              morph_preferred_gdk_backend(nullptr, "wayland-1", nullptr, 1));
  expect_null("empty X11 display",
              morph_preferred_gdk_backend(nullptr, "wayland-1", "", 1));
  expect_null("non-NVIDIA wayland",
              morph_preferred_gdk_backend(nullptr, "wayland-1", ":0", 0));
  expect_null("non-NVIDIA session list",
              morph_preferred_gdk_backend("wayland,x11,*", "wayland-1", ":0", 0));
  expect_streq("NVIDIA wayland with XWayland",
               morph_preferred_gdk_backend(nullptr, "wayland-1", ":0", 1),
               "x11");
  expect_streq("empty GDK_BACKEND is unset",
               morph_preferred_gdk_backend("", "wayland-1", ":0", 1), "x11");
  // Omarchy / Hyprland session default used to skip the fallback because
  // GDK_BACKEND was already set.
  expect_streq("Omarchy session list",
               morph_preferred_gdk_backend("wayland,x11,*", "wayland-1", ":0",
                                           1),
               "x11");
  expect_streq("explicit wayland still needs X11 on NVIDIA",
               morph_preferred_gdk_backend("wayland", "wayland-1", ":0", 1),
               "x11");
  expect_streq("spaces around first token",
               morph_preferred_gdk_backend(" wayland , x11 , *", "wayland-1",
                                           ":0", 1),
               "x11");

  if (failures != 0) {
    fprintf(stderr, "display_backend_check: %d failure(s)\n", failures);
    return 1;
  }
  printf("display_backend_check: ok\n");
  return 0;
}
