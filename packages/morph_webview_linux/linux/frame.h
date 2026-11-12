#ifndef MORPH_WEBVIEW_LINUX_FRAME_H_
#define MORPH_WEBVIEW_LINUX_FRAME_H_

#include <cairo/cairo.h>
#include <gdk/gdk.h>
#include <glib.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// Delay between presented frames, based on the primary monitor refresh
// rate, with a 16 ms (60 Hz) fallback when the rate is unknown.
guint morph_frame_interval_ms(void);

// Copy a CAIRO_FORMAT_ARGB32 image into a tightly packed BGRA buffer.
// On little-endian this is a straight copy (no per-pixel swizzle).
void morph_copy_cairo_to_bgra(cairo_surface_t* surface,
                              uint8_t* dest,
                              uint32_t width,
                              uint32_t height);

// The slower path Morph used originally: swizzle ARGB32/BGRA into RGBA.
void morph_copy_cairo_to_rgba(cairo_surface_t* surface,
                              uint8_t* dest,
                              uint32_t width,
                              uint32_t height);

// How a BGRA frame is handed to the GL context Flutter gives populate().
// Flutter's Linux embedder creates OpenGL ES contexts. GLES rejects a sized
// GL_RGBA8 internal format paired with GL_BGRA data (GL_INVALID_OPERATION on
// NVIDIA; Mesa happens to accept it), which leaves the texture empty.
typedef enum {
  // Desktop GL: internal GL_RGBA8, data GL_BGRA.
  MORPH_GL_UPLOAD_BGRA_DESKTOP = 0,
  // GLES with GL_EXT_texture_format_BGRA8888: internal and data GL_BGRA_EXT.
  MORPH_GL_UPLOAD_BGRA_EXT = 1,
  // Anything else: swizzle to RGBA on the CPU, internal and data GL_RGBA.
  // This is the same upload FlPixelBufferTexture does.
  MORPH_GL_UPLOAD_RGBA_SWIZZLE = 2,
} MorphGlUploadMode;

MorphGlUploadMode morph_choose_gl_upload(gboolean desktop_gl, gboolean has_bgra8888);

// The next mode to try after `mode` failed its self-check. Returns FALSE when
// there is nothing left to try.
gboolean morph_next_gl_upload(MorphGlUploadMode mode, MorphGlUploadMode* next);

const char* morph_gl_upload_name(MorphGlUploadMode mode);

// Swizzle a tightly packed BGRA buffer into RGBA.
void morph_swizzle_bgra_to_rgba(const uint8_t* src, uint8_t* dest, size_t pixel_count);

// TRUE when every sampled pixel of a 4-byte-per-pixel buffer has R, G and B
// at zero (black or fully transparent). Samples a grid, not every pixel.
gboolean morph_frame_is_blank(const uint8_t* data,
                              uint32_t width,
                              uint32_t height,
                              size_t stride);

// Compare one read-back RGBA pixel with the BGRA source pixel it came from.
gboolean morph_rgba_matches_bgra(const uint8_t* rgba, const uint8_t* bgra, int tolerance);

#ifdef __cplusplus
}
#endif

#endif  // MORPH_WEBVIEW_LINUX_FRAME_H_
