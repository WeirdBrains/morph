#ifndef MORPH_WEBVIEW_LINUX_SESSION_H_
#define MORPH_WEBVIEW_LINUX_SESSION_H_

#include <webkit2/webkit2.h>

#ifdef __cplusplus
extern "C" {
#endif

// Morph data directory: MORPH_HOME if set and non-empty, else $HOME/.morph.
// The caller owns the returned string.
gchar* morph_session_home_dir(void);

// Per-profile WebKit directory: <home>/profile/default/webkit.
// The caller owns the returned string.
gchar* morph_session_profile_dir(void);

// Cookie database path under the profile directory.
// The caller owns the returned string.
gchar* morph_session_cookie_path(void);

// Process-wide WebKit context shared by every Linux tab. Website data,
// localStorage, and cookies persist under the Morph profile directory.
// The context is owned by the session module; do not unref it.
WebKitWebContext* morph_session_shared_context(void);

// WebKit settings with a current Safari-like user agent. The caller owns
// the returned object.
WebKitSettings* morph_session_create_settings(void);

// User-agent string applied to Linux web views.
const gchar* morph_session_user_agent(void);

// Drop the shared context so the next shared_context() call rebuilds it.
// For tests only; not used by the plugin.
void morph_session_reset_for_tests(void);

#ifdef __cplusplus
}
#endif

#endif  // MORPH_WEBVIEW_LINUX_SESSION_H_
