#include "session.h"

#include <errno.h>

// WebKitGTK 2.54 reports Version/60.5, which is not a real Safari release and
// causes some sites (including Meta login) to treat the browser as unknown.
// Use a current Safari-compatible UA; Morph is a WebKitGTK browser, so this
// matches what Epiphany-style embedders ship rather than inventing a product
// token that Facebook or Instagram reject.
static const gchar kSafariCompatibleUserAgent[] =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) "
    "Version/26.0 Safari/605.1.15";

static WebKitWebContext* g_shared_context = nullptr;

gchar* morph_session_home_dir(void) {
  const gchar* env = g_getenv("MORPH_HOME");
  if (env != nullptr && env[0] != '\0') {
    return g_strdup(env);
  }
  return g_build_filename(g_get_home_dir(), ".morph", nullptr);
}

gchar* morph_session_profile_dir(void) {
  gchar* home = morph_session_home_dir();
  gchar* dir = g_build_filename(home, "profile", "default", "webkit", nullptr);
  g_free(home);
  return dir;
}

gchar* morph_session_cookie_path(void) {
  gchar* profile = morph_session_profile_dir();
  gchar* path = g_build_filename(profile, "cookies.sqlite", nullptr);
  g_free(profile);
  return path;
}

static gboolean ensure_directory(const gchar* path) {
  if (g_mkdir_with_parents(path, 0700) != 0) {
    g_warning("morph_webview: failed to create %s: %s", path, g_strerror(errno));
    return FALSE;
  }
  return TRUE;
}

const gchar* morph_session_user_agent(void) {
  return kSafariCompatibleUserAgent;
}

WebKitSettings* morph_session_create_settings(void) {
  WebKitSettings* settings = webkit_settings_new();
  webkit_settings_set_user_agent(settings, kSafariCompatibleUserAgent);
  webkit_settings_set_javascript_can_open_windows_automatically(settings, TRUE);
  return settings;
}

WebKitWebContext* morph_session_shared_context(void) {
  if (g_shared_context != nullptr) {
    return g_shared_context;
  }

  gchar* profile = morph_session_profile_dir();
  gchar* data_dir = g_build_filename(profile, "data", nullptr);
  gchar* cache_dir = g_build_filename(profile, "cache", nullptr);
  gchar* cookie_path = g_build_filename(profile, "cookies.sqlite", nullptr);
  ensure_directory(data_dir);
  ensure_directory(cache_dir);
  ensure_directory(profile);

  // Without base-data-directory WebKit still writes localStorage under
  // $XDG_DATA_HOME/<prgname>, but cookies stay in memory unless persistent
  // storage is configured. Logins then vanish after a restart, and some
  // OAuth flows (Instagram / Facebook) bounce back to the login page.
  WebKitWebsiteDataManager* manager = webkit_website_data_manager_new(
      "base-data-directory", data_dir, "base-cache-directory", cache_dir, nullptr);
  // Leave ITP at the WebKitGTK default (disabled). Enabling it would go
  // beyond Safari/Firefox cookie defaults and can interfere with OAuth.
  WebKitCookieManager* cookies = webkit_website_data_manager_get_cookie_manager(manager);
  // NO_THIRD_PARTY matches Safari and Firefox: first-party cookies set
  // on a top-level OAuth redirect are accepted; cross-site trackers are not.
  webkit_cookie_manager_set_accept_policy(cookies, WEBKIT_COOKIE_POLICY_ACCEPT_NO_THIRD_PARTY);
  webkit_cookie_manager_set_persistent_storage(cookies, cookie_path,
                                               WEBKIT_COOKIE_PERSISTENT_STORAGE_SQLITE);

  g_shared_context = webkit_web_context_new_with_website_data_manager(manager);
  g_object_unref(manager);

  g_free(profile);
  g_free(data_dir);
  g_free(cache_dir);
  g_free(cookie_path);
  return g_shared_context;
}

void morph_session_reset_for_tests(void) {
  if (g_shared_context != nullptr) {
    g_object_unref(g_shared_context);
    g_shared_context = nullptr;
  }
}
