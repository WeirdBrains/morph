#ifndef MORPH_WEBVIEW_LINUX_KEY_MAP_H_
#define MORPH_WEBVIEW_LINUX_KEY_MAP_H_

#include <gdk/gdk.h>

#ifdef __cplusplus
extern "C" {
#endif

// Map a Flutter key label (or a few aliases) to a GDK keyval.
// Returns GDK_KEY_VoidSymbol when the label is not recognized.
guint morph_keyval_from_label(const gchar* key);

// Resolve the GDK keyval for a control-layer key event.
// Named non-printable keys always win over `text`. Flutter's Linux embedder
// sets KeyEvent.character from gdk_keyval_to_unicode(), so Backspace arrives
// as U+0008 and Enter as U+000D; gdk_unicode_to_keyval() does not map those
// control characters back to GDK_KEY_BackSpace / GDK_KEY_Return.
guint morph_keyval_for_event(const gchar* key, const gchar* text);

#ifdef __cplusplus
}
#endif

#endif  // MORPH_WEBVIEW_LINUX_KEY_MAP_H_
