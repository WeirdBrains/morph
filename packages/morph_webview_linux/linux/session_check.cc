// Cookie and session persistence check for the Linux WebKit web view.
// Uses a local HTTP server that sets a cookie and redirects, then verifies
// the cookie survives view recreation and a simulated app restart.
#include "session.h"

#include <gtk/gtk.h>
#include <libsoup/soup.h>
#include <webkit2/webkit2.h>

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>

static int g_failures = 0;

static void fail(const char* message) {
  std::fprintf(stderr, "FAIL: %s\n", message);
  g_failures++;
}

static void expect_true(bool condition, const char* message) {
  if (!condition) {
    fail(message);
  }
}

struct EvalState {
  gchar* value;
  GMainLoop* loop;
};

static void on_eval(GObject* source, GAsyncResult* result, gpointer user_data) {
  EvalState* state = static_cast<EvalState*>(user_data);
  GError* error = nullptr;
  JSCValue* value =
      webkit_web_view_evaluate_javascript_finish(WEBKIT_WEB_VIEW(source), result, &error);
  if (error != nullptr) {
    state->value = g_strdup_printf("ERR:%s", error->message);
    g_error_free(error);
  } else if (value != nullptr) {
    if (jsc_value_is_string(value)) {
      state->value = jsc_value_to_string(value);
    } else {
      state->value = g_strdup(jsc_value_to_json(value, 0));
    }
    g_object_unref(value);
  } else {
    state->value = g_strdup("null");
  }
  g_main_loop_quit(state->loop);
}

static gchar* eval_js(WebKitWebView* view, const char* script) {
  EvalState state = {nullptr, g_main_loop_new(nullptr, FALSE)};
  webkit_web_view_evaluate_javascript(view, script, -1, nullptr, nullptr, nullptr, on_eval,
                                      &state);
  g_main_loop_run(state.loop);
  g_main_loop_unref(state.loop);
  return state.value;
}

static void on_load_finished(WebKitWebView*, WebKitLoadEvent event, gpointer user_data) {
  if (event == WEBKIT_LOAD_FINISHED) {
    g_main_loop_quit(static_cast<GMainLoop*>(user_data));
  }
}

static void load_and_wait(WebKitWebView* view, const char* uri) {
  GMainLoop* loop = g_main_loop_new(nullptr, FALSE);
  gulong handler = g_signal_connect(view, "load-changed", G_CALLBACK(on_load_finished), loop);
  webkit_web_view_load_uri(view, uri);
  g_main_loop_run(loop);
  g_signal_handler_disconnect(view, handler);
  g_main_loop_unref(loop);
}

static void drain_main_loop_ms(guint milliseconds) {
  gint64 end = g_get_monotonic_time() + static_cast<gint64>(milliseconds) * 1000;
  while (g_get_monotonic_time() < end) {
    g_main_context_iteration(nullptr, FALSE);
    g_usleep(10000);
  }
}

static SoupServer* start_server(guint* port) {
  SoupServer* server = soup_server_new("server-header", "morph-session-check", nullptr);
  soup_server_add_handler(
      server, "/set",
      [](SoupServer*, SoupServerMessage* msg, const char*, GHashTable*, gpointer) {
        SoupMessageHeaders* headers = soup_server_message_get_response_headers(msg);
        soup_message_headers_append(headers, "Set-Cookie",
                                    "morph_session=ok; Path=/; Max-Age=86400; SameSite=Lax");
        soup_message_headers_append(headers, "Location", "/check");
        soup_server_message_set_status(msg, 302, nullptr);
        soup_server_message_set_response(msg, "text/plain", SOUP_MEMORY_STATIC, "redirect", 8);
      },
      nullptr, nullptr);
  soup_server_add_handler(
      server, "/check",
      [](SoupServer*, SoupServerMessage* msg, const char*, GHashTable*, gpointer) {
        SoupMessageHeaders* request_headers = soup_server_message_get_request_headers(msg);
        const char* cookie = soup_message_headers_get_one(request_headers, "Cookie");
        gchar* body = g_strdup_printf(
            "<!doctype html><title>check</title><body>cookie=%s</body>",
            cookie != nullptr ? cookie : "(none)");
        soup_server_message_set_status(msg, 200, nullptr);
        soup_server_message_set_response(msg, "text/html", SOUP_MEMORY_TAKE, body, strlen(body));
      },
      nullptr, nullptr);

  GError* error = nullptr;
  if (!soup_server_listen_local(server, 0, static_cast<SoupServerListenOptions>(0), &error)) {
    std::fprintf(stderr, "failed to listen: %s\n", error->message);
    g_error_free(error);
    std::exit(2);
  }
  GSList* uris = soup_server_get_uris(server);
  *port = static_cast<guint>(g_uri_get_port(static_cast<GUri*>(uris->data)));
  g_slist_free_full(uris, reinterpret_cast<GDestroyNotify>(g_uri_unref));
  return server;
}

static WebKitWebView* make_view_for_profile(const char* profile_id) {
  WebKitWebContext* context = morph_session_context_for_profile(profile_id);
  WebKitSettings* settings = morph_session_create_settings();
  WebKitWebView* view = WEBKIT_WEB_VIEW(g_object_new(WEBKIT_TYPE_WEB_VIEW, "web-context", context,
                                                     "settings", settings, nullptr));
  g_object_unref(settings);
  GtkWidget* window = gtk_offscreen_window_new();
  gtk_container_add(GTK_CONTAINER(window), GTK_WIDGET(view));
  gtk_widget_show_all(window);
  g_object_set_data_full(G_OBJECT(view), "morph-offscreen", window,
                         reinterpret_cast<GDestroyNotify>(gtk_widget_destroy));
  return view;
}

static void destroy_view(WebKitWebView* view) {
  GtkWidget* window = static_cast<GtkWidget*>(g_object_get_data(G_OBJECT(view), "morph-offscreen"));
  if (window != nullptr) {
    g_object_steal_data(G_OBJECT(view), "morph-offscreen");
    gtk_widget_destroy(window);
  }
}

static void check_paths() {
  gchar* home = morph_session_home_dir();
  gchar* webkit = morph_session_webkit_dir("default");
  gchar* cookies = morph_session_cookie_path("default");
  const gchar* expected_home = g_getenv("MORPH_HOME");
  expect_true(expected_home != nullptr && std::strcmp(home, expected_home) == 0,
              "home dir should follow MORPH_HOME");
  std::string expected_webkit = std::string(expected_home) + "/profiles/default/webkit";
  expect_true(expected_webkit == webkit, "webkit dir should be under profiles/<id>/webkit");
  std::string expected_cookies = expected_webkit + "/cookies.sqlite";
  expect_true(expected_cookies == cookies, "cookie path should be cookies.sqlite in the profile");
  gchar* other = morph_session_webkit_dir("p1");
  expect_true(std::string(other) == std::string(expected_home) + "/profiles/p1/webkit",
              "each profile id gets its own webkit directory");
  expect_true(std::strcmp(webkit, other) != 0, "default and p1 directories must differ");
  g_free(home);
  g_free(webkit);
  g_free(cookies);
  g_free(other);
}

static void check_context_setup() {
  WebKitWebContext* context = morph_session_shared_context();
  expect_true(!webkit_web_context_is_ephemeral(context), "shared context must not be ephemeral");
  WebKitWebsiteDataManager* manager = webkit_web_context_get_website_data_manager(context);
  expect_true(!webkit_website_data_manager_is_ephemeral(manager),
              "website data manager must not be ephemeral");
  const gchar* data = webkit_website_data_manager_get_base_data_directory(manager);
  const gchar* cache = webkit_website_data_manager_get_base_cache_directory(manager);
  const gchar* home = g_getenv("MORPH_HOME");
  expect_true(data != nullptr && std::strstr(data, home) != nullptr,
              "data directory should be under MORPH_HOME");
  expect_true(cache != nullptr && std::strstr(cache, home) != nullptr,
              "cache directory should be under MORPH_HOME");
  expect_true(!webkit_website_data_manager_get_itp_enabled(manager),
              "ITP should stay at the WebKitGTK default (disabled)");
  WebKitWebView* view = make_view_for_profile("default");
  expect_true(!webkit_web_view_is_ephemeral(view), "views must not be ephemeral");
  expect_true(webkit_web_view_get_context(view) == context, "view should use the shared context");
  WebKitSettings* settings = webkit_web_view_get_settings(view);
  expect_true(std::strcmp(webkit_settings_get_user_agent(settings), morph_session_user_agent()) ==
                  0,
              "user agent should be the Safari-compatible Morph UA");
  expect_true(std::strstr(morph_session_user_agent(), "Version/26.0") != nullptr &&
                  std::strstr(morph_session_user_agent(), "Safari/605.1.15") != nullptr,
              "user agent should look like a current Safari release");
  expect_true(webkit_settings_get_enable_media(settings), "media should be enabled");
  expect_true(webkit_settings_get_enable_mediasource(settings), "MediaSource should be enabled");
  expect_true(webkit_settings_get_enable_encrypted_media(settings),
              "encrypted media should be enabled");
  expect_true(webkit_settings_get_enable_webgl(settings), "WebGL should be enabled");
  expect_true(!webkit_settings_get_media_playback_requires_user_gesture(settings),
              "media should not require an extra user gesture");
  expect_true(webkit_settings_get_media_playback_allows_inline(settings),
              "inline media playback should be allowed");
  destroy_view(view);

  // Inspect the settings object directly. Virtual-framebuffer runs disable
  // compositing so the live view may report NEVER even though Morph asked
  // for ALWAYS.
  WebKitSettings* configured = morph_session_create_settings();
  WebKitHardwareAccelerationPolicy policy =
      webkit_settings_get_hardware_acceleration_policy(configured);
  if (g_strcmp0(g_getenv("WEBKIT_DISABLE_COMPOSITING_MODE"), "1") == 0) {
    expect_true(policy == WEBKIT_HARDWARE_ACCELERATION_POLICY_NEVER,
                "disabled compositing clamps hardware acceleration to NEVER");
  } else {
    expect_true(policy == WEBKIT_HARDWARE_ACCELERATION_POLICY_ALWAYS,
                "hardware acceleration should be always on");
  }
  expect_true(webkit_settings_get_enable_html5_local_storage(configured),
              "html5 localStorage should be enabled");
  expect_true(webkit_settings_get_enable_html5_database(configured),
              "html5 database / WebSQL should be enabled");
  expect_true(webkit_settings_get_enable_page_cache(configured), "page cache should be enabled");
  g_object_unref(configured);
}

static bool body_has_session_cookie(WebKitWebView* view) {
  gchar* body = eval_js(view, "document.body ? document.body.innerText : ''");
  const bool ok = body != nullptr && std::strstr(body, "morph_session=ok") != nullptr;
  if (!ok) {
    std::fprintf(stderr, "  body was: %s\n", body != nullptr ? body : "(null)");
  }
  g_free(body);
  return ok;
}


static void write_profile_state(WebKitWebView* view, const char* token) {
  gchar* script = g_strdup_printf(
      "document.cookie = 'morph_session=%s; path=/; max-age=86400';"
      "try { localStorage.setItem('k', '%s'); } catch (e) {}"
      "document.cookie + '|' + (function(){ try { return localStorage.getItem('k'); } catch(e) { return 'ERR'; } })();",
      token, token);
  gchar* result = eval_js(view, script);
  g_free(script);
  g_free(result);
}

static bool profile_state_is(WebKitWebView* view, const char* token, bool log_mismatch = true) {
  gchar* cookie = eval_js(view, "document.cookie");
  gchar* ls = eval_js(view,
                      "try { localStorage.getItem('k') || ''; } catch (e) { 'ERR'; }");
  const bool cookie_ok = cookie != nullptr && std::strstr(cookie, token) != nullptr;
  const bool ls_ok = ls != nullptr && std::strcmp(ls, token) == 0;
  if (log_mismatch && (!cookie_ok || !ls_ok)) {
    std::fprintf(stderr, "  expected token %s, cookie=%s localStorage=%s\n", token,
                 cookie != nullptr ? cookie : "(null)", ls != nullptr ? ls : "(null)");
  }
  g_free(cookie);
  g_free(ls);
  return cookie_ok && ls_ok;
}

static void check_profile_isolation(const char* check_url) {
  WebKitWebView* a = make_view_for_profile("default");
  WebKitWebView* b = make_view_for_profile("p1");
  WebKitWebView* c = make_view_for_profile("p2");
  expect_true(webkit_web_view_get_context(a) != webkit_web_view_get_context(b) &&
                  webkit_web_view_get_context(b) != webkit_web_view_get_context(c) &&
                  webkit_web_view_get_context(a) != webkit_web_view_get_context(c),
              "each profile must have its own WebKit context");
  load_and_wait(a, check_url);
  load_and_wait(b, check_url);
  load_and_wait(c, check_url);
  write_profile_state(a, "alpha");
  write_profile_state(b, "bravo");
  write_profile_state(c, "charlie");
  expect_true(profile_state_is(a, "alpha"), "profile default should keep its own cookie and localStorage");
  expect_true(profile_state_is(b, "bravo"), "profile p1 should keep its own cookie and localStorage");
  expect_true(profile_state_is(c, "charlie"), "profile p2 should keep its own cookie and localStorage");
  expect_true(!profile_state_is(a, "bravo", false) && !profile_state_is(a, "charlie", false),
              "default must not see p1 or p2 data");
  expect_true(!profile_state_is(b, "alpha", false) && !profile_state_is(b, "charlie", false),
              "p1 must not see default or p2 data");
  expect_true(!profile_state_is(c, "alpha", false) && !profile_state_is(c, "bravo", false),
              "p2 must not see default or p1 data");

  // sessionStorage survives in-tab navigation and stays per view.
  gchar* set_ss = eval_js(a, "sessionStorage.setItem('s','keep'); sessionStorage.getItem('s');");
  expect_true(set_ss != nullptr && std::strcmp(set_ss, "keep") == 0, "sessionStorage should write");
  g_free(set_ss);
  load_and_wait(a, check_url);
  gchar* get_ss = eval_js(a, "sessionStorage.getItem('s');");
  expect_true(get_ss != nullptr && std::strcmp(get_ss, "keep") == 0,
              "sessionStorage should survive in-tab navigation");
  g_free(get_ss);
  gchar* other_ss = eval_js(b, "sessionStorage.getItem('s');");
  expect_true(other_ss != nullptr && (other_ss[0] == '\0' || std::strcmp(other_ss, "null") == 0 ||
                                      std::strcmp(other_ss, "undefined") == 0),
              "sessionStorage must not be shared with another profile");
  g_free(other_ss);

  destroy_view(a);
  destroy_view(b);
  destroy_view(c);
  drain_main_loop_ms(400);
  morph_session_reset_for_tests();

  WebKitWebView* a2 = make_view_for_profile("default");
  WebKitWebView* b2 = make_view_for_profile("p1");
  WebKitWebView* c2 = make_view_for_profile("p2");
  load_and_wait(a2, check_url);
  load_and_wait(b2, check_url);
  load_and_wait(c2, check_url);
  expect_true(profile_state_is(a2, "alpha"), "default cookie and localStorage should survive a restart");
  expect_true(profile_state_is(b2, "bravo"), "p1 cookie and localStorage should survive a restart");
  expect_true(profile_state_is(c2, "charlie"), "p2 cookie and localStorage should survive a restart");
  destroy_view(a2);
  destroy_view(b2);
  destroy_view(c2);
  drain_main_loop_ms(200);
  morph_session_reset_for_tests();
}

int main(int argc, char** argv) {
  // Offscreen / virtual-framebuffer environments often have no hardware GL.
  g_setenv("WEBKIT_DISABLE_DMABUF_RENDERER", "1", FALSE);
  g_setenv("WEBKIT_DISABLE_COMPOSITING_MODE", "1", FALSE);
  g_setenv("LIBGL_ALWAYS_SOFTWARE", "1", FALSE);
  gtk_init(&argc, &argv);

  gchar* temp_home = g_dir_make_tmp("morph-session-check-XXXXXX", nullptr);
  if (temp_home == nullptr) {
    std::fprintf(stderr, "failed to create temp MORPH_HOME\n");
    return 2;
  }
  g_setenv("MORPH_HOME", temp_home, TRUE);

  check_paths();

  guint port = 0;
  SoupServer* server = start_server(&port);
  gchar* set_url = g_strdup_printf("http://127.0.0.1:%u/set", port);
  gchar* check_url = g_strdup_printf("http://127.0.0.1:%u/check", port);

  check_context_setup();

  WebKitWebView* first = make_view_for_profile("default");
  load_and_wait(first, set_url);
  const char* after_redirect = webkit_web_view_get_uri(first);
  expect_true(after_redirect != nullptr && std::strstr(after_redirect, "/check") != nullptr,
              "Set-Cookie response should redirect to /check");
  expect_true(body_has_session_cookie(first), "cookie should be sent after the redirect");
  gchar* document_cookie = eval_js(first, "document.cookie");
  expect_true(document_cookie != nullptr && std::strstr(document_cookie, "morph_session=ok") != nullptr,
              "document.cookie should expose the session cookie");
  g_free(document_cookie);

  WebKitWebView* second = make_view_for_profile("default");
  expect_true(webkit_web_view_get_context(second) == webkit_web_view_get_context(first),
              "tabs should share one persistent network session");
  load_and_wait(second, check_url);
  expect_true(body_has_session_cookie(second), "cookie set in one view should be visible in another");

  destroy_view(first);
  destroy_view(second);

  WebKitWebView* recreated = make_view_for_profile("default");
  load_and_wait(recreated, check_url);
  expect_true(body_has_session_cookie(recreated), "cookie should survive view recreation");
  destroy_view(recreated);

  // Simulate an app restart: drop the process-wide context, then reopen the
  // same Morph home and confirm the sqlite cookie jar is still used.
  drain_main_loop_ms(400);
  morph_session_reset_for_tests();
  WebKitWebView* restarted = make_view_for_profile("default");
  load_and_wait(restarted, check_url);
  expect_true(body_has_session_cookie(restarted), "cookie should survive an app restart");
  destroy_view(restarted);
  drain_main_loop_ms(200);
  morph_session_reset_for_tests();

  gchar* cookie_path = morph_session_cookie_path("default");
  expect_true(g_file_test(cookie_path, G_FILE_TEST_EXISTS),
              "cookies.sqlite should be created in the Morph profile");
  g_free(cookie_path);

  check_profile_isolation(check_url);

  g_object_unref(server);
  g_free(set_url);
  g_free(check_url);
  g_unsetenv("MORPH_HOME");
  gchar* rm = g_strdup_printf("rm -rf %s", temp_home);
  if (g_spawn_command_line_sync(rm, nullptr, nullptr, nullptr, nullptr) != TRUE) {
    std::fprintf(stderr, "warning: failed to remove %s\n", temp_home);
  }
  g_free(rm);
  g_free(temp_home);

  if (g_failures > 0) {
    std::fprintf(stderr, "%d session check(s) failed\n", g_failures);
    return 1;
  }
  std::fprintf(stdout, "all session checks passed\n");
  return 0;
}
