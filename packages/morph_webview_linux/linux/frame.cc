#include "frame.h"

#include <cstdlib>
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

MorphGlUploadMode morph_choose_gl_upload(gboolean desktop_gl, gboolean has_bgra8888) {
  if (desktop_gl) {
    return MORPH_GL_UPLOAD_BGRA_DESKTOP;
  }
  if (has_bgra8888) {
    return MORPH_GL_UPLOAD_BGRA_EXT;
  }
  return MORPH_GL_UPLOAD_RGBA_SWIZZLE;
}

gboolean morph_next_gl_upload(MorphGlUploadMode mode, MorphGlUploadMode* next) {
  if (mode == MORPH_GL_UPLOAD_RGBA_SWIZZLE) {
    return FALSE;
  }
  *next = MORPH_GL_UPLOAD_RGBA_SWIZZLE;
  return TRUE;
}

const char* morph_gl_upload_name(MorphGlUploadMode mode) {
  switch (mode) {
    case MORPH_GL_UPLOAD_BGRA_DESKTOP:
      return "gl-rgba8-bgra";
    case MORPH_GL_UPLOAD_BGRA_EXT:
      return "gles-bgra-ext";
    case MORPH_GL_UPLOAD_RGBA_SWIZZLE:
    default:
      return "rgba-swizzle";
  }
}

gboolean morph_frame_is_blank(const uint8_t* data,
                              uint32_t width,
                              uint32_t height,
                              size_t stride) {
  if (data == nullptr || width == 0 || height == 0) {
    return TRUE;
  }
  const uint32_t steps = 32;
  for (uint32_t sy = 0; sy < steps; sy++) {
    uint32_t y = static_cast<uint32_t>((static_cast<uint64_t>(sy) * 2 + 1) * height / (steps * 2));
    const uint8_t* row = data + static_cast<size_t>(y) * stride;
    for (uint32_t sx = 0; sx < steps; sx++) {
      uint32_t x = static_cast<uint32_t>((static_cast<uint64_t>(sx) * 2 + 1) * width / (steps * 2));
      const uint8_t* p = row + static_cast<size_t>(x) * 4;
      if (p[0] != 0 || p[1] != 0 || p[2] != 0) {
        return FALSE;
      }
    }
  }
  return TRUE;
}

gboolean morph_rgba_matches_bgra(const uint8_t* rgba, const uint8_t* bgra, int tolerance) {
  return std::abs(rgba[0] - bgra[2]) <= tolerance && std::abs(rgba[1] - bgra[1]) <= tolerance &&
         std::abs(rgba[2] - bgra[0]) <= tolerance && std::abs(rgba[3] - bgra[3]) <= tolerance;
}

void morph_swizzle_bgra_to_rgba(const uint8_t* src, uint8_t* dest, size_t pixel_count) {
  for (size_t i = 0; i < pixel_count; i++) {
    dest[0] = src[2];
    dest[1] = src[1];
    dest[2] = src[0];
    dest[3] = src[3];
    src += 4;
    dest += 4;
  }
}
