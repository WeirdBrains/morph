// Automated check: GDK key mapping plus Backspace/Enter/etc. in an offscreen
// WebKit view loaded with the local pointer-keyboard test page.
#include "key_map.h"

#include <gtk/gtk.h>
#include <webkit2/webkit2.h>

#include <stdio.h>
#include <string.h>

static int g_failures = 0;

static void expect_keyval(const char* key, const char* text, guint want, const char* label) {
  guint got = morph_keyval_for_event(key, text);
  if (got != want) {
    fprintf(stderr, "FAIL %s: morph_keyval_for_event(\"%s\", ", label, key ? key : "");
    fprintf(stderr, "\"\\x%02x\") = 0x%x, want 0x%x\n", text && text[0] ? (unsigned char)text[0] : 0, got, want);
    g_failures++;
  } else {
    fprintf(stdout, "ok  %s\n", label);
  }
}

static void pump(void) {
  for (int i = 0; i < 32; i++) {
    if (!gtk_events_pending()) {
      break;
    }
    gtk_main_iteration();
  }
}

static GdkDevice* default_keyboard_device() {
  GdkDisplay* display = gdk_display_get_default();
  if (display == nullptr) {
    return nullptr;
  }
  GdkSeat* seat = gdk_display_get_default_seat(display);
  if (seat == nullptr) {
    return nullptr;
  }
  return gdk_seat_get_keyboard(seat);
}

static void grab_focus(GtkWidget* widget) {
  gtk_widget_set_can_focus(widget, TRUE);
  gtk_widget_grab_focus(widget);
  GdkWindow* window = gtk_widget_get_window(widget);
  if (window == nullptr) {
    return;
  }
  GdkEvent* event = gdk_event_new(GDK_FOCUS_CHANGE);
  event->focus_change.window = window;
  g_object_ref(window);
  event->focus_change.send_event = TRUE;
  event->focus_change.in = TRUE;
  gdk_event_set_device(event, default_keyboard_device());
  gtk_widget_event(widget, event);
  gdk_event_free(event);
  pump();
}

struct JsResult {
  gboolean done;
  gchar* value;
  gchar* error;
};

static void on_js(GObject* source, GAsyncResult* result, gpointer user_data) {
  JsResult* state = static_cast<JsResult*>(user_data);
  GError* error = nullptr;
  WebKitJavascriptResult* js = webkit_web_view_run_javascript_finish(WEBKIT_WEB_VIEW(source), result, &error);
  if (error != nullptr) {
    state->error = g_strdup(error->message);
    g_error_free(error);
    state->done = TRUE;
    return;
  }
  if (js != nullptr) {
    JSCValue* value = webkit_javascript_result_get_js_value(js);
    if (jsc_value_is_string(value)) {
      state->value = jsc_value_to_string(value);
    } else {
      state->value = g_strdup("");
    }
    webkit_javascript_result_unref(js);
  }
  state->done = TRUE;
}

static gchar* eval_js(WebKitWebView* view, const char* script) {
  JsResult state = {FALSE, nullptr, nullptr};
  webkit_web_view_run_javascript(view, script, nullptr, on_js, &state);
  while (!state.done) {
    gtk_main_iteration();
  }
  if (state.error != nullptr) {
    fprintf(stderr, "FAIL js: %s (%s)\n", state.error, script);
    g_failures++;
    g_free(state.error);
    return g_strdup("");
  }
  return state.value != nullptr ? state.value : g_strdup("");
}

static void send_key(GtkWidget* widget,
                     const char* phase,
                     const char* key,
                     const char* text,
                     gboolean ctrl,
                     gboolean alt,
                     gboolean shift,
                     gboolean meta) {
  GdkWindow* window = gtk_widget_get_window(widget);
  if (window == nullptr) {
    fprintf(stderr, "FAIL no gdk window for key %s\n", key);
    g_failures++;
    return;
  }
  guint keyval = morph_keyval_for_event(key, text);
  if (keyval == GDK_KEY_Tab && shift && !ctrl && !alt && !meta) {
    keyval = GDK_KEY_ISO_Left_Tab;
  }
  GdkEventType type = g_strcmp0(phase, "up") == 0 ? GDK_KEY_RELEASE : GDK_KEY_PRESS;
  GdkEvent* event = gdk_event_new(type);
  event->key.window = window;
  g_object_ref(window);
  event->key.send_event = TRUE;
  event->key.time = GDK_CURRENT_TIME;
  event->key.keyval = keyval;
  event->key.state = (ctrl ? GDK_CONTROL_MASK : 0) | (alt ? GDK_MOD1_MASK : 0) |
                     (shift ? GDK_SHIFT_MASK : 0) | (meta ? GDK_META_MASK | GDK_SUPER_MASK : 0);
  GdkDisplay* display = gdk_window_get_display(window);
  GdkKeymap* keymap = gdk_keymap_get_for_display(display);
  GdkKeymapKey* keys = nullptr;
  gint n_keys = 0;
  if (gdk_keymap_get_entries_for_keyval(keymap, keyval, &keys, &n_keys) && n_keys > 0) {
    event->key.hardware_keycode = static_cast<guint16>(keys[0].keycode);
    event->key.group = static_cast<guint8>(keys[0].group);
  }
  g_free(keys);
  if (type == GDK_KEY_PRESS && text != nullptr && text[0] != '\0' && !ctrl && !alt && !meta) {
    gunichar c = g_utf8_get_char(text);
    if (c >= 0x20 && c != 0x7f) {
      event->key.string = g_strdup(text);
      event->key.length = static_cast<gint>(strlen(text));
    }
  }
  gdk_event_set_device(event, default_keyboard_device());
  grab_focus(widget);
  gtk_widget_event(widget, event);
  gdk_event_free(event);
  pump();
}

static void tap(GtkWidget* widget, const char* key, const char* text = nullptr, gboolean shift = FALSE, gboolean ctrl = FALSE) {
  send_key(widget, "down", key, text, ctrl, FALSE, shift, FALSE);
  send_key(widget, "up", key, "", ctrl, FALSE, shift, FALSE);
}

static void expect_contains(const char* hay, const char* needle, const char* label) {
  if (hay == nullptr || strstr(hay, needle) == nullptr) {
    fprintf(stderr, "FAIL %s: expected to find %s in %s\n", label, needle, hay ? hay : "(null)");
    g_failures++;
  } else {
    fprintf(stdout, "ok  %s\n", label);
  }
}

static gchar* snapshot(WebKitWebView* view) {
  pump();
  return eval_js(view, "window.__morphSnapshot()");
}

static gchar* wait_snapshot(WebKitWebView* view, const char* needle, const char* label) {
  gchar* snap = nullptr;
  for (int i = 0; i < 80; i++) {
    g_free(snap);
    snap = snapshot(view);
    if (snap != nullptr && strstr(snap, needle) != nullptr) {
      fprintf(stdout, "ok  %s\n", label);
      return snap;
    }
    gtk_main_iteration_do(FALSE);
  }
  fprintf(stderr, "FAIL %s: expected to find %s in %s\n", label, needle, snap ? snap : "(null)");
  g_failures++;
  return snap;
}

int main(int argc, char** argv) {
  g_setenv("WEBKIT_DISABLE_COMPOSITING_MODE", "1", FALSE);
  gtk_init(&argc, &argv);

  // Mapping unit checks, including Flutter's control-character text.
  expect_keyval("Backspace", "\x08", GDK_KEY_BackSpace, "Backspace+U+0008");
  expect_keyval("Enter", "\x0d", GDK_KEY_Return, "Enter+U+000D");
  expect_keyval("Enter", "\n", GDK_KEY_Return, "Enter+U+000A");
  expect_keyval("Tab", "\x09", GDK_KEY_Tab, "Tab+U+0009");
  expect_keyval("Escape", "\x1b", GDK_KEY_Escape, "Escape+U+001B");
  expect_keyval("Delete", "\x7f", GDK_KEY_Delete, "Delete+U+007F");
  expect_keyval("Arrow Left", "", GDK_KEY_Left, "Arrow Left");
  expect_keyval("Arrow Right", "", GDK_KEY_Right, "Arrow Right");
  expect_keyval("Home", "", GDK_KEY_Home, "Home");
  expect_keyval("End", "", GDK_KEY_End, "End");
  expect_keyval("a", "a", GDK_KEY_a, "printable a");

  const char* html_path = argc > 1 ? argv[1] : nullptr;
  gchar* html = nullptr;
  if (html_path != nullptr) {
    GError* err = nullptr;
    if (!g_file_get_contents(html_path, &html, nullptr, &err)) {
      fprintf(stderr, "FAIL reading %s: %s\n", html_path, err->message);
      return 1;
    }
  } else {
    fprintf(stderr, "usage: key_input_check <pointer-keyboard-test.html>\n");
    return g_failures == 0 ? 2 : 1;
  }

  GtkWidget* offscreen = gtk_offscreen_window_new();
  WebKitWebView* view = WEBKIT_WEB_VIEW(webkit_web_view_new());
  gtk_container_add(GTK_CONTAINER(offscreen), GTK_WIDGET(view));
  gtk_widget_set_size_request(GTK_WIDGET(view), 800, 600);
  gtk_widget_set_can_focus(GTK_WIDGET(view), TRUE);
  gtk_widget_show_all(offscreen);
  gtk_widget_grab_focus(GTK_WIDGET(view));
  pump();

  gboolean loaded = FALSE;
  g_signal_connect(view, "load-changed", G_CALLBACK(+[](WebKitWebView*, WebKitLoadEvent ev, gpointer data) {
                     if (ev == WEBKIT_LOAD_FINISHED) {
                       *static_cast<gboolean*>(data) = TRUE;
                     }
                   }),
                   &loaded);
  webkit_web_view_load_html(view, html, "file:///morph-key-check/");
  g_free(html);
  for (int i = 0; i < 200 && !loaded; i++) {
    gtk_main_iteration_do(TRUE);
  }
  if (!loaded) {
    fprintf(stderr, "FAIL page did not finish loading\n");
    return 1;
  }
  pump();

  eval_js(view, "document.getElementById('field').focus()");
  pump();
  grab_focus(GTK_WIDGET(view));
  tap(GTK_WIDGET(view), "a", "a");
  tap(GTK_WIDGET(view), "b", "b");
  tap(GTK_WIDGET(view), "c", "c");
  gchar* snap = wait_snapshot(view, "\"field\":\"abc\"", "typed abc into input");
  g_free(snap);

  tap(GTK_WIDGET(view), "Backspace", "\x08");
  snap = wait_snapshot(view, "\"field\":\"ab\"", "Backspace deletes in input");
  g_free(snap);

  tap(GTK_WIDGET(view), "Enter", "\x0d");
  snap = wait_snapshot(view, "\"submitted\":true", "Enter submits the form");
  g_free(snap);

  eval_js(view, "document.getElementById('area').focus(); document.getElementById('area').value='';");
  grab_focus(GTK_WIDGET(view));
  pump();
  tap(GTK_WIDGET(view), "h", "h");
  tap(GTK_WIDGET(view), "i", "i");
  tap(GTK_WIDGET(view), "Enter", "\x0d");
  snap = wait_snapshot(view, "\"area\":\"hi\\n\"", "Enter inserts a newline in textarea");
  g_free(snap);

  eval_js(view, "(function(){ var f=document.getElementById('field'); f.focus(); f.value='wxyz'; f.setSelectionRange(4,4); })()");
  grab_focus(GTK_WIDGET(view));
  pump();
  tap(GTK_WIDGET(view), "Home", "");
  eval_js(view, "window.__morph.fieldSel = String(document.getElementById('field').selectionStart); window.__morphSnapshot()");
  snap = snapshot(view);
  // Home should land at 0; snapshot itself doesn't include fieldSel, eval last script already ran.
  g_free(snap);
  gchar* sel = eval_js(view, "String(document.getElementById('field').selectionStart)");
  expect_contains(sel, "0", "Home moves caret to start");
  g_free(sel);

  tap(GTK_WIDGET(view), "End", "");
  sel = eval_js(view, "String(document.getElementById('field').selectionStart)");
  expect_contains(sel, "4", "End moves caret to end");
  g_free(sel);

  tap(GTK_WIDGET(view), "Arrow Left", "");
  sel = eval_js(view, "String(document.getElementById('field').selectionStart)");
  expect_contains(sel, "3", "Arrow Left moves caret");
  g_free(sel);

  tap(GTK_WIDGET(view), "Delete", "\x7f");
  snap = wait_snapshot(view, "\"field\":\"wxy\"", "Delete removes the character after the caret");
  g_free(snap);

  tap(GTK_WIDGET(view), "Arrow Left", "", TRUE, FALSE);
  gchar* selected = eval_js(view,
      "(function(){ var f=document.getElementById('field'); return String(f.selectionStart)+':'+String(f.selectionEnd)+':'+f.value.substring(f.selectionStart,f.selectionEnd); })()");
  expect_contains(selected, "2:3:x", "Shift+Arrow Left selects a character");
  g_free(selected);

  tap(GTK_WIDGET(view), "a", "a", FALSE, TRUE);
  gchar* last = eval_js(view, "JSON.stringify(window.__morph.keys.filter(k => k.ctrl && k.key.toLowerCase()==='a').pop() || {})");
  expect_contains(last, "\"ctrl\":true", "Ctrl+A is dispatched");
  g_free(last);

  eval_js(view, "document.getElementById('field').focus()");
  pump();
  grab_focus(GTK_WIDGET(view));
  tap(GTK_WIDGET(view), "Tab", "\x09");
  gchar* active = eval_js(view, "document.activeElement ? document.activeElement.id : ''");
  expect_contains(active, "area", "Tab moves focus to the textarea");
  g_free(active);

  tap(GTK_WIDGET(view), "Escape", "\x1b");
  last = eval_js(view, "JSON.stringify(window.__morph.keys.filter(k => k.key==='Escape').pop() || {})");
  expect_contains(last, "\"key\":\"Escape\"", "Escape is dispatched");
  g_free(last);

  eval_js(view, "(function(){ var a=document.getElementById('area'); a.focus(); a.value='xxxx'; a.setSelectionRange(4,4); })()");
  grab_focus(GTK_WIDGET(view));
  pump();
  // Held Backspace is a series of key-press events (repeat).
  send_key(GTK_WIDGET(view), "down", "Backspace", "\x08", FALSE, FALSE, FALSE, FALSE);
  send_key(GTK_WIDGET(view), "down", "Backspace", "\x08", FALSE, FALSE, FALSE, FALSE);
  send_key(GTK_WIDGET(view), "down", "Backspace", "\x08", FALSE, FALSE, FALSE, FALSE);
  send_key(GTK_WIDGET(view), "up", "Backspace", "", FALSE, FALSE, FALSE, FALSE);
  snap = wait_snapshot(view, "\"area\":\"x\"", "repeated Backspace deletes multiple characters");
  g_free(snap);

  // Ctrl+C / V / Z at least reach the page.
  tap(GTK_WIDGET(view), "c", "c", FALSE, TRUE);
  tap(GTK_WIDGET(view), "v", "v", FALSE, TRUE);
  tap(GTK_WIDGET(view), "z", "z", FALSE, TRUE);
  last = eval_js(view, "JSON.stringify(window.__morph.keys.filter(k => k.ctrl).map(k => k.key.toLowerCase()))");
  expect_contains(last, "\"c\"", "Ctrl+C dispatched");
  expect_contains(last, "\"v\"", "Ctrl+V dispatched");
  expect_contains(last, "\"z\"", "Ctrl+Z dispatched");
  g_free(last);

  if (g_failures != 0) {
    fprintf(stderr, "%d check(s) failed\n", g_failures);
    return 1;
  }
  fprintf(stdout, "all key input checks passed\n");
  return 0;
}
