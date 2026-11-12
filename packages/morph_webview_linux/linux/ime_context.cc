#include "ime_context.h"

#include <string.h>

struct _MorphImeContext {
  WebKitInputMethodContext parent;
  gchar* preedit;
  guint cursor;
  gboolean composing;
};

G_DEFINE_TYPE(MorphImeContext, morph_ime_context, WEBKIT_TYPE_INPUT_METHOD_CONTEXT)

static void morph_ime_context_set_preedit(MorphImeContext* self, const gchar* text) {
  g_free(self->preedit);
  self->preedit = g_strdup(text != nullptr ? text : "");
  self->cursor = static_cast<guint>(g_utf8_strlen(self->preedit, -1));
}

static void morph_ime_context_get_preedit(WebKitInputMethodContext* context,
                                          gchar** text,
                                          GList** underlines,
                                          guint* cursor_offset) {
  MorphImeContext* self = MORPH_IME_CONTEXT(context);
  if (text != nullptr) {
    *text = g_strdup(self->preedit != nullptr ? self->preedit : "");
  }
  if (underlines != nullptr) {
    *underlines = nullptr;
    if (self->preedit != nullptr && self->preedit[0] != '\0') {
      *underlines = g_list_append(*underlines, webkit_input_method_underline_new(0, self->cursor));
    }
  }
  if (cursor_offset != nullptr) {
    *cursor_offset = self->cursor;
  }
}

static gboolean morph_ime_context_filter_key_event(WebKitInputMethodContext* context, GdkEventKey* key_event) {
  MorphImeContext* self = MORPH_IME_CONTEXT(context);
  (void)key_event;
  // Flutter already ran the system IME. Swallow raw keys only while a preedit is active.
  return self->composing ? TRUE : FALSE;
}

static void morph_ime_context_reset(WebKitInputMethodContext* context) {
  MorphImeContext* self = MORPH_IME_CONTEXT(context);
  if (self->composing) {
    self->composing = FALSE;
    morph_ime_context_set_preedit(self, "");
    g_signal_emit_by_name(self, "preedit-finished");
  }
}

static void morph_ime_context_finalize(GObject* object) {
  MorphImeContext* self = MORPH_IME_CONTEXT(object);
  g_free(self->preedit);
  self->preedit = nullptr;
  G_OBJECT_CLASS(morph_ime_context_parent_class)->finalize(object);
}

static void morph_ime_context_class_init(MorphImeContextClass* klass) {
  GObjectClass* object_class = G_OBJECT_CLASS(klass);
  object_class->finalize = morph_ime_context_finalize;
  WebKitInputMethodContextClass* im_class = WEBKIT_INPUT_METHOD_CONTEXT_CLASS(klass);
  im_class->get_preedit = morph_ime_context_get_preedit;
  im_class->filter_key_event = morph_ime_context_filter_key_event;
  im_class->reset = morph_ime_context_reset;
}

static void morph_ime_context_init(MorphImeContext* self) {
  self->preedit = g_strdup("");
  self->cursor = 0;
  self->composing = FALSE;
}

MorphImeContext* morph_ime_context_new(void) {
  return MORPH_IME_CONTEXT(g_object_new(MORPH_TYPE_IME_CONTEXT, nullptr));
}

void morph_ime_context_handle(MorphImeContext* self, const gchar* phase, const gchar* text) {
  if (self == nullptr) {
    return;
  }
  const gchar* value = text != nullptr ? text : "";
  if (g_strcmp0(phase, "focus") == 0) {
    webkit_input_method_context_notify_focus_in(WEBKIT_INPUT_METHOD_CONTEXT(self));
    return;
  }
  if (g_strcmp0(phase, "blur") == 0) {
    if (self->composing) {
      morph_ime_context_reset(WEBKIT_INPUT_METHOD_CONTEXT(self));
    }
    webkit_input_method_context_notify_focus_out(WEBKIT_INPUT_METHOD_CONTEXT(self));
    return;
  }
  if (g_strcmp0(phase, "compose_start") == 0) {
    if (!self->composing) {
      self->composing = TRUE;
      g_signal_emit_by_name(self, "preedit-started");
    }
    morph_ime_context_set_preedit(self, value);
    g_signal_emit_by_name(self, "preedit-changed");
    return;
  }
  if (g_strcmp0(phase, "compose_update") == 0) {
    if (!self->composing) {
      self->composing = TRUE;
      g_signal_emit_by_name(self, "preedit-started");
    }
    morph_ime_context_set_preedit(self, value);
    g_signal_emit_by_name(self, "preedit-changed");
    return;
  }
  if (g_strcmp0(phase, "compose_end") == 0) {
    if (self->composing) {
      self->composing = FALSE;
      morph_ime_context_set_preedit(self, "");
      g_signal_emit_by_name(self, "preedit-finished");
    }
    return;
  }
  if (g_strcmp0(phase, "commit") == 0) {
    if (self->composing) {
      self->composing = FALSE;
      morph_ime_context_set_preedit(self, "");
      g_signal_emit_by_name(self, "preedit-finished");
    }
    if (value[0] != '\0') {
      g_signal_emit_by_name(self, "committed", value);
    }
  }
}
