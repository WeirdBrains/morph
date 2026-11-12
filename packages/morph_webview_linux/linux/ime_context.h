#ifndef MORPH_WEBVIEW_LINUX_IME_CONTEXT_H_
#define MORPH_WEBVIEW_LINUX_IME_CONTEXT_H_

#include <webkit2/webkit2.h>

G_BEGIN_DECLS

#define MORPH_TYPE_IME_CONTEXT (morph_ime_context_get_type())
G_DECLARE_FINAL_TYPE(MorphImeContext, morph_ime_context, MORPH, IME_CONTEXT, WebKitInputMethodContext)

MorphImeContext* morph_ime_context_new(void);

void morph_ime_context_handle(MorphImeContext* self, const gchar* phase, const gchar* text);

G_END_DECLS

#endif  // MORPH_WEBVIEW_LINUX_IME_CONTEXT_H_
