#ifndef MORPH_WEBVIEW_LINUX_SESSION_H_
#define MORPH_WEBVIEW_LINUX_SESSION_H_

#include <webkit2/webkit2.h>

#ifdef __cplusplus
extern "C" {
#endif

// Morph data directory: MORPH_HOME if set and non-empty, else $HOME/.morph.
// The caller owns the returned string.
gchar* morph_session_home_dir(void);

// `<home>/profiles/<id>/webkit`. `id` falls back to "default" when null or
// invalid. The caller owns the returned string.
gchar* morph_session_webkit_dir(const gchar* profile_id);

// Cookie database: `<webkit>/cookies.sqlite`. The caller owns the returned string.
gchar* morph_session_cookie_path(const gchar* profile_id);

#ifndef MORPH_SESSION_NO_DEPRECATED
// WebKit directory for the default profile. The caller owns the returned string.
gchar* morph_session_profile_dir(void);
#endif

// WebKit context for [profile_id], created on first use. Tabs in the same
// profile share this context; different profiles never do. Do not unref it.
WebKitWebContext* morph_session_context_for_profile(const gchar* profile_id);

// The default-profile context. Same as context_for_profile("default").
WebKitWebContext* morph_session_shared_context(void);

// WebKit settings with a current Safari-like user agent and browser storage
// flags enabled. The caller owns the returned object.
WebKitSettings* morph_session_create_settings(void);

// User-agent string applied to Linux web views.
const gchar* morph_session_user_agent(void);

// Drop every cached context so the next context_for_profile() rebuilds it.
// For tests only; not used by the plugin.
void morph_session_reset_for_tests(void);

#ifdef __cplusplus
}
#endif

#endif  // MORPH_WEBVIEW_LINUX_SESSION_H_
