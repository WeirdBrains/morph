#ifndef MORPH_WEBVIEW_CONTROL_H_
#define MORPH_WEBVIEW_CONTROL_H_

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef void (*morph_control_navigate_fn)(int64_t view_id, const char* url, void* user_data);
typedef void (*morph_control_go_back_fn)(int64_t view_id, void* user_data);
typedef void (*morph_control_go_forward_fn)(int64_t view_id, void* user_data);
typedef void (*morph_control_reload_fn)(int64_t view_id, void* user_data);
typedef void (*morph_control_pointer_fn)(int64_t view_id,
                                         const char* phase,
                                         double x,
                                         double y,
                                         int32_t button,
                                         double scroll_delta_x,
                                         double scroll_delta_y,
                                         void* user_data);
typedef void (*morph_control_key_fn)(int64_t view_id,
                                     const char* phase,
                                     const char* key,
                                     uint32_t hardware_keycode,
                                     int ctrl,
                                     int alt,
                                     int shift,
                                     int meta,
                                     void* user_data);
typedef void (*morph_control_ime_fn)(int64_t view_id, const char* phase, const char* text, void* user_data);
typedef void (*morph_control_evaluate_js_fn)(int64_t view_id,
                                             uint64_t request_id,
                                             const char* script,
                                             void* user_data);

typedef struct MorphControlHandlers {
  morph_control_navigate_fn navigate;
  morph_control_go_back_fn go_back;
  morph_control_go_forward_fn go_forward;
  morph_control_reload_fn reload;
  morph_control_pointer_fn pointer;
  morph_control_key_fn key;
  morph_control_ime_fn ime;
  morph_control_evaluate_js_fn evaluate_js;
  void* user_data;
} MorphControlHandlers;

// Bind the in-process server. If socket_path is NULL a per-user path is chosen.
// On success, endpoint_out is filled with a URI (unix://... or tcp://127.0.0.1:port).
// Returns 0 on success.
int morph_control_start(const char* socket_path,
                        const MorphControlHandlers* handlers,
                        char* endpoint_out,
                        size_t endpoint_out_len);

void morph_control_stop(void);

int morph_control_emit_navigation(int64_t view_id,
                                  const char* url,
                                  const char* title,
                                  int can_go_back,
                                  int can_go_forward,
                                  int is_loading,
                                  double progress);

// Runs the Rust page-bridge hook, asks the backend to deliver the reply, and notifies clients.
int morph_control_emit_page_message(int64_t view_id, const char* json);

int morph_control_complete_evaluate_js(int64_t view_id,
                                       uint64_t request_id,
                                       const char* result,
                                       const char* error);

#ifdef __cplusplus
}
#endif

#endif  // MORPH_WEBVIEW_CONTROL_H_
