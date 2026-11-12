#include "key_map.h"

#include <string.h>

static gboolean label_is(const gchar* key, const gchar* name) {
  return g_ascii_strcasecmp(key, name) == 0;
}

guint morph_keyval_from_label(const gchar* key) {
  if (key == nullptr || *key == '\0') {
    return GDK_KEY_VoidSymbol;
  }
  if (label_is(key, "Enter") || label_is(key, "Return") || label_is(key, "Numpad Enter")) {
    return GDK_KEY_Return;
  }
  if (label_is(key, "Backspace") || label_is(key, "BackSpace")) {
    return GDK_KEY_BackSpace;
  }
  if (label_is(key, "Tab")) {
    return GDK_KEY_Tab;
  }
  if (label_is(key, "Escape")) {
    return GDK_KEY_Escape;
  }
  if (label_is(key, "Arrow Left") || label_is(key, "ArrowLeft") || label_is(key, "Left")) {
    return GDK_KEY_Left;
  }
  if (label_is(key, "Arrow Right") || label_is(key, "ArrowRight") || label_is(key, "Right")) {
    return GDK_KEY_Right;
  }
  if (label_is(key, "Arrow Up") || label_is(key, "ArrowUp") || label_is(key, "Up")) {
    return GDK_KEY_Up;
  }
  if (label_is(key, "Arrow Down") || label_is(key, "ArrowDown") || label_is(key, "Down")) {
    return GDK_KEY_Down;
  }
  if (label_is(key, " ") || label_is(key, "Space") || label_is(key, "Numpad Space")) {
    return GDK_KEY_space;
  }
  if (label_is(key, "Delete") || label_is(key, "Del")) {
    return GDK_KEY_Delete;
  }
  if (label_is(key, "Home")) {
    return GDK_KEY_Home;
  }
  if (label_is(key, "End")) {
    return GDK_KEY_End;
  }
  if (label_is(key, "Page Up") || label_is(key, "PageUp")) {
    return GDK_KEY_Page_Up;
  }
  if (label_is(key, "Page Down") || label_is(key, "PageDown")) {
    return GDK_KEY_Page_Down;
  }
  if (label_is(key, "Insert")) {
    return GDK_KEY_Insert;
  }
  guint from_name = gdk_keyval_from_name(key);
  if (from_name != GDK_KEY_VoidSymbol) {
    return from_name;
  }
  if (g_utf8_strlen(key, -1) == 1) {
    gunichar c = g_utf8_get_char(key);
    if (c >= 0x20 && c != 0x7f) {
      return gdk_unicode_to_keyval(c);
    }
  }
  return GDK_KEY_VoidSymbol;
}

static guint keyval_from_control_char(gunichar c) {
  switch (c) {
    case 0x08:
      return GDK_KEY_BackSpace;
    case 0x09:
      return GDK_KEY_Tab;
    case 0x0a:
    case 0x0d:
      return GDK_KEY_Return;
    case 0x1b:
      return GDK_KEY_Escape;
    case 0x7f:
      return GDK_KEY_Delete;
    default:
      return GDK_KEY_VoidSymbol;
  }
}

guint morph_keyval_for_event(const gchar* key, const gchar* text) {
  guint from_label = morph_keyval_from_label(key);
  // Function keys (BackSpace, Return, arrows, etc.) live in 0xff00–0xffff.
  // Prefer the label so Flutter's control-character `text` cannot win.
  if (from_label != GDK_KEY_VoidSymbol && from_label >= 0xff00) {
    return from_label;
  }
  if (text != nullptr && g_utf8_strlen(text, -1) == 1) {
    gunichar c = g_utf8_get_char(text);
    guint from_control = keyval_from_control_char(c);
    if (from_control != GDK_KEY_VoidSymbol) {
      return from_control;
    }
    if (c >= 0x20 && c != 0x7f) {
      return gdk_unicode_to_keyval(c);
    }
  }
  return from_label;
}
