#include "session.h"

#include <errno.h>
#include <string.h>
#include <glib/gstdio.h>

#include <map>
#include <string>

// WebKitGTK 2.54 reports Version/60.5, which is not a real Safari release and
// causes some sites (including Meta login) to treat the browser as unknown.
// Use a current Safari-compatible UA; Morph is a WebKitGTK browser, so this
// matches what Epiphany-style embedders ship rather than inventing a product
// token that Facebook or Instagram reject.
static const gchar kSafariCompatibleUserAgent[] =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) "
    "Version/26.0 Safari/605.1.15";

static const gchar kDefaultProfileId[] = "default";

static std::map<std::string, WebKitWebContext*> g_contexts;

gchar* morph_session_home_dir(void) {
  const gchar* env = g_getenv("MORPH_HOME");
  if (env != nullptr && env[0] != '\0') {
    return g_strdup(env);
  }
  return g_build_filename(g_get_home_dir(), ".morph", nullptr);
}

static gboolean morph_session_is_valid_id(const gchar* id) {
  if (id == nullptr || id[0] == '\0' || strlen(id) > 64) {
    return FALSE;
  }
  if (!g_ascii_isalnum(id[0])) {
    return FALSE;
  }
  for (const gchar* p = id + 1; *p != '\0'; p++) {
    if (!g_ascii_isalnum(*p) && *p != '_' && *p != '-') {
      return FALSE;
    }
  }
  return TRUE;
}

static const gchar* morph_session_normalize_id(const gchar* profile_id) {
  return morph_session_is_valid_id(profile_id) ? profile_id : kDefaultProfileId;
}

gchar* morph_session_webkit_dir(const gchar* profile_id) {
  const gchar* id = morph_session_normalize_id(profile_id);
  gchar* home = morph_session_home_dir();
  gchar* dir = g_build_filename(home, "profiles", id, "webkit", nullptr);
  g_free(home);
  return dir;
}

gchar* morph_session_cookie_path(const gchar* profile_id) {
  gchar* webkit = morph_session_webkit_dir(profile_id);
  gchar* path = g_build_filename(webkit, "cookies.sqlite", nullptr);
  g_free(webkit);
  return path;
}

gchar* morph_session_profile_dir(void) {
  return morph_session_webkit_dir(kDefaultProfileId);
}

static gboolean ensure_directory(const gchar* path) {
  if (g_mkdir_with_parents(path, 0700) != 0) {
    g_warning("morph_webview: failed to create %s: %s", path, g_strerror(errno));
    return FALSE;
  }
  return TRUE;
}

static gboolean dir_is_empty(const gchar* path) {
  GDir* dir = g_dir_open(path, 0, nullptr);
  if (dir == nullptr) {
    return TRUE;
  }
  const gboolean empty = g_dir_read_name(dir) == nullptr;
  g_dir_close(dir);
  return empty;
}

// Move leftover `<home>/profile/default[/webkit]` into `<home>/profiles/default/webkit`.
static void migrate_legacy_default_webkit(void) {
  gchar* home = morph_session_home_dir();
  gchar* dest = g_build_filename(home, "profiles", kDefaultProfileId, "webkit", nullptr);
  gchar* old_root = g_build_filename(home, "profile", kDefaultProfileId, nullptr);
  gchar* old_nested = g_build_filename(old_root, "webkit", nullptr);
  const gchar* source = nullptr;
  if (g_file_test(old_nested, G_FILE_TEST_IS_DIR)) {
    source = old_nested;
  } else if (g_file_test(old_root, G_FILE_TEST_IS_DIR)) {
    source = old_root;
  }
  if (source != nullptr) {
    const gboolean dest_missing = !g_file_test(dest, G_FILE_TEST_EXISTS);
    const gboolean dest_empty = g_file_test(dest, G_FILE_TEST_IS_DIR) && dir_is_empty(dest);
    if (dest_missing || dest_empty) {
      gchar* parent = g_path_get_dirname(dest);
      ensure_directory(parent);
      g_free(parent);
      if (dest_empty) {
        g_rmdir(dest);
      }
      if (g_rename(source, dest) != 0) {
        g_warning("morph_webview: failed to migrate %s to %s: %s", source, dest,
                  g_strerror(errno));
      }
    }
    gchar* legacy_parent = g_build_filename(home, "profile", nullptr);
    if (dir_is_empty(old_root)) {
      g_rmdir(old_root);
    }
    if (dir_is_empty(legacy_parent)) {
      g_rmdir(legacy_parent);
    }
    g_free(legacy_parent);
  }
  g_free(home);
  g_free(dest);
  g_free(old_root);
  g_free(old_nested);
}

const gchar* morph_session_user_agent(void) {
  return kSafariCompatibleUserAgent;
}

WebKitSettings* morph_session_create_settings(void) {
  WebKitSettings* settings = webkit_settings_new();
  webkit_settings_set_user_agent(settings, kSafariCompatibleUserAgent);
  webkit_settings_set_javascript_can_open_windows_automatically(settings, TRUE);
  // Explicit browser-like media flags. Encrypted media is off by default in
  // WebKitGTK, which makes some YouTube and Instagram streams report that
  // the browser cannot play the video. Hardware acceleration stays on so
  // video can be composited; software GStreamer decoders still work when
  // no type is forced to require hardware support.
  webkit_settings_set_enable_media(settings, TRUE);
  webkit_settings_set_enable_mediasource(settings, TRUE);
  webkit_settings_set_enable_encrypted_media(settings, TRUE);
  webkit_settings_set_enable_media_capabilities(settings, TRUE);
  webkit_settings_set_enable_media_stream(settings, TRUE);
  webkit_settings_set_enable_webgl(settings, TRUE);
  webkit_settings_set_media_playback_requires_user_gesture(settings, FALSE);
  webkit_settings_set_media_playback_allows_inline(settings, TRUE);
  webkit_settings_set_hardware_acceleration_policy(settings,
                                                   WEBKIT_HARDWARE_ACCELERATION_POLICY_ALWAYS);
  webkit_settings_set_media_content_types_requiring_hardware_support(settings, "");
  // Website storage must stay on so logins and site data persist in the
  // profile's WebsiteDataManager. Private browsing would isolate and drop it.
  webkit_settings_set_enable_html5_local_storage(settings, TRUE);
  webkit_settings_set_enable_html5_database(settings, TRUE);
  webkit_settings_set_enable_page_cache(settings, TRUE);
  // Offline AppCache and private-browsing setters are deprecated no-ops in
  // WebKitGTK 2.54. Persistence comes from a non-ephemeral WebsiteDataManager
  // with base-data/base-cache directories under the profile.
  return settings;
}

WebKitWebContext* morph_session_context_for_profile(const gchar* profile_id) {
  const gchar* id = morph_session_normalize_id(profile_id);
  auto it = g_contexts.find(id);
  if (it != g_contexts.end()) {
    return it->second;
  }

  if (g_strcmp0(id, kDefaultProfileId) == 0) {
    migrate_legacy_default_webkit();
  }

  gchar* webkit = morph_session_webkit_dir(id);
  gchar* data_dir = g_build_filename(webkit, "data", nullptr);
  gchar* cache_dir = g_build_filename(webkit, "cache", nullptr);
  gchar* cookie_path = g_build_filename(webkit, "cookies.sqlite", nullptr);
  ensure_directory(data_dir);
  ensure_directory(cache_dir);
  ensure_directory(webkit);

  // One WebsiteDataManager per profile, shared by that profile's tabs.
  // Cookies, localStorage, IndexedDB, service workers, Cache Storage, and
  // WebSQL all live under these directories and must never leak across ids.
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

  WebKitWebContext* context = webkit_web_context_new_with_website_data_manager(manager);
  g_object_unref(manager);
  g_contexts[id] = context;

  g_free(webkit);
  g_free(data_dir);
  g_free(cache_dir);
  g_free(cookie_path);
  return context;
}

WebKitWebContext* morph_session_shared_context(void) {
  return morph_session_context_for_profile(kDefaultProfileId);
}

void morph_session_reset_for_tests(void) {
  for (auto& entry : g_contexts) {
    if (entry.second != nullptr) {
      g_object_unref(entry.second);
    }
  }
  g_contexts.clear();
}

gboolean morph_session_load_error_shown(const gchar* domain, int code) {
  if (g_strcmp0(domain, "WebKitNetworkError") == 0 && code == WEBKIT_NETWORK_ERROR_CANCELLED) {
    return FALSE;
  }
  if (g_strcmp0(domain, "g-io-error-quark") == 0 && code == G_IO_ERROR_CANCELLED) {
    return FALSE;
  }
  if (g_strcmp0(domain, "WebKitPolicyError") == 0 &&
      code == WEBKIT_POLICY_ERROR_FRAME_LOAD_INTERRUPTED_BY_POLICY_CHANGE) {
    return FALSE;
  }
  if (g_strcmp0(domain, "WebKitPluginError") == 0 &&
      code == WEBKIT_PLUGIN_ERROR_WILL_HANDLE_LOAD) {
    return FALSE;
  }
  return TRUE;
}

const gchar* morph_session_tls_reason(GTlsCertificateFlags errors) {
  if (errors & G_TLS_CERTIFICATE_BAD_IDENTITY) {
    return "The certificate is for a different site.";
  }
  if (errors & G_TLS_CERTIFICATE_EXPIRED) {
    return "The certificate has expired.";
  }
  if (errors & G_TLS_CERTIFICATE_NOT_ACTIVATED) {
    return "The certificate is not valid yet.";
  }
  if (errors & G_TLS_CERTIFICATE_REVOKED) {
    return "The certificate was revoked.";
  }
  if (errors & G_TLS_CERTIFICATE_UNKNOWN_CA) {
    return "The certificate is not signed by a trusted authority.";
  }
  if (errors & G_TLS_CERTIFICATE_INSECURE) {
    return "The certificate uses insecure cryptography.";
  }
  return "The certificate is not valid.";
}

gboolean morph_session_popup_allowed(int open_popups) {
  return open_popups >= 0 && open_popups < MORPH_SESSION_MAX_POPUPS;
}

const gchar* morph_session_fault_for_termination(WebKitWebProcessTerminationReason reason) {
  switch (reason) {
    case WEBKIT_WEB_PROCESS_EXCEEDED_MEMORY_LIMIT:
      return "outOfMemory";
    case WEBKIT_WEB_PROCESS_TERMINATED_BY_API:
      return "killed";
    case WEBKIT_WEB_PROCESS_CRASHED:
    default:
      return "crashed";
  }
}
