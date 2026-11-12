#ifndef MORPH_WEBVIEW_LINUX_MEDIA_CHECK_H_
#define MORPH_WEBVIEW_LINUX_MEDIA_CHECK_H_

#include <glib.h>

#ifdef __cplusplus
extern "C" {
#endif

// Whether a GStreamer element factory named [name] is installed.
typedef gboolean (*MorphHasElementFunc)(const gchar* name, gpointer user_data);

// Comma-separated web media formats with no installed GStreamer element
// ("VP9, Opus"), or an empty string when all are covered. The caller owns
// the result.
gchar* morph_media_missing_formats(MorphHasElementFunc has_element, gpointer user_data);

// The same check against this machine's GStreamer registry, loaded at run
// time (no build dependency). Null when GStreamer cannot be loaded. Cached
// after the first call; the caller must not free it.
const gchar* morph_media_missing_formats_here(void);

// A one-paragraph hint for a tab whose renderer crashed: missing decoders,
// or a WebKit setting known to crash video. Empty when there is nothing to
// add. The caller owns the result.
gchar* morph_media_crash_hint(const gchar* missing_formats, const gchar* disable_dmabuf_env);

#ifdef __cplusplus
}
#endif

#endif  // MORPH_WEBVIEW_LINUX_MEDIA_CHECK_H_
