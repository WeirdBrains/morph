#include "frame.h"

#include <cstring>

guint morph_frame_interval_ms(void) {
  GdkDisplay* display = gdk_display_get_default();
  if (display != nullptr) {
    GdkMonitor* monitor = gdk_display_get_primary_monitor(display);
    if (monitor != nullptr) {
      gint refresh_milli_hz = gdk_monitor_get_refresh_rate(monitor);
      if (refresh_milli_hz >= 20000) {
        return static_cast<guint>((1000000 + refresh_milli_hz / 2) / refresh_milli_hz);
      }
    }
  }
  return 16;
}

void morph_copy_cairo_to_bgra(cairo_surface_t* surface,
                              uint8_t* dest,
                              uint32_t width,
                              uint32_t height) {
  cairo_surface_flush(surface);
  unsigned char* src = cairo_image_surface_get_data(surface);
  int stride = cairo_image_surface_get_stride(surface);
  const size_t row_bytes = static_cast<size_t>(width) * 4;
  if (stride == static_cast<int>(row_bytes)) {
    memcpy(dest, src, row_bytes * height);
    return;
  }
  for (uint32_t y = 0; y < height; y++) {
    memcpy(dest + static_cast<size_t>(y) * row_bytes,
           src + static_cast<size_t>(y) * static_cast<size_t>(stride), row_bytes);
  }
}

void morph_copy_cairo_to_rgba(cairo_surface_t* surface,
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
      out[0] = row[2];
      out[1] = row[1];
      out[2] = row[0];
      out[3] = row[3];
      row += 4;
      out += 4;
    }
  }
}
