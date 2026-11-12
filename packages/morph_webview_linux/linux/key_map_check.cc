// Mapping regression check for Flutter Linux control-character key text.
#include "key_map.h"

#include <stdio.h>

static int failures = 0;

static void expect_keyval(const char* key, const char* text, guint want, const char* label) {
  guint got = morph_keyval_for_event(key, text);
  if (got != want) {
    fprintf(stderr, "FAIL %s: got 0x%x want 0x%x\n", label, got, want);
    failures++;
  } else {
    fprintf(stdout, "ok  %s\n", label);
  }
}

int main(void) {
  // Flutter's Linux embedder sets character from gdk_keyval_to_unicode().
  expect_keyval("Backspace", "\x08", GDK_KEY_BackSpace, "Backspace+U+0008");
  expect_keyval("Enter", "\x0d", GDK_KEY_Return, "Enter+U+000D");
  expect_keyval("Enter", "\n", GDK_KEY_Return, "Enter+U+000A");
  expect_keyval("Tab", "\x09", GDK_KEY_Tab, "Tab+U+0009");
  expect_keyval("Escape", "\x1b", GDK_KEY_Escape, "Escape+U+001B");
  expect_keyval("Delete", "\x7f", GDK_KEY_Delete, "Delete+U+007F");
  expect_keyval("Arrow Left", "", GDK_KEY_Left, "Arrow Left");
  expect_keyval("Arrow Right", "", GDK_KEY_Right, "Arrow Right");
  expect_keyval("Arrow Up", "", GDK_KEY_Up, "Arrow Up");
  expect_keyval("Arrow Down", "", GDK_KEY_Down, "Arrow Down");
  expect_keyval("Home", "", GDK_KEY_Home, "Home");
  expect_keyval("End", "", GDK_KEY_End, "End");
  expect_keyval("a", "a", GDK_KEY_a, "printable a");
  expect_keyval("A", "A", GDK_KEY_A, "printable A");
  expect_keyval("Space", " ", GDK_KEY_space, "Space");

  // Confirm the naive unicode path (the previous bug) is wrong for Backspace.
  guint naive = gdk_unicode_to_keyval(0x08);
  if (naive == GDK_KEY_BackSpace) {
    fprintf(stderr, "FAIL assumption: gdk_unicode_to_keyval(U+0008) unexpectedly equals GDK_KEY_BackSpace\n");
    failures++;
  } else {
    fprintf(stdout, "ok  gdk_unicode_to_keyval(U+0008) is not GDK_KEY_BackSpace (0x%x)\n", naive);
  }

  if (failures != 0) {
    fprintf(stderr, "%d key map check(s) failed\n", failures);
    return 1;
  }
  fprintf(stdout, "all key map checks passed\n");
  return 0;
}
