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

#ifdef __cplusplus
}
#endif

#endif  // MORPH_WEBVIEW_LINUX_FRAME_H_
