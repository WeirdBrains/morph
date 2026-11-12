//! C ABI used by `morph_webview_linux` (and later other native backends).
use crate::endpoint::ControlEndpoint;
use crate::server::ControlBackend;
use crate::{start_global, stop_global, with_global};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_uint};
use std::sync::Arc;

#[repr(C)]
pub struct MorphControlHandlers {
    pub navigate: Option<extern "C" fn(i64, *const c_char, *mut std::ffi::c_void)>,
    pub go_back: Option<extern "C" fn(i64, *mut std::ffi::c_void)>,
    pub go_forward: Option<extern "C" fn(i64, *mut std::ffi::c_void)>,
    pub reload: Option<extern "C" fn(i64, *mut std::ffi::c_void)>,
    pub pointer: Option<extern "C" fn(i64, *const c_char, f64, f64, i32, f64, f64, *mut std::ffi::c_void)>,
    pub key: Option<extern "C" fn(i64, *const c_char, *const c_char, c_uint, c_int, c_int, c_int, c_int, *mut std::ffi::c_void)>,
    pub ime: Option<extern "C" fn(i64, *const c_char, *const c_char, *mut std::ffi::c_void)>,
    pub evaluate_js: Option<extern "C" fn(i64, u64, *const c_char, *mut std::ffi::c_void)>,
    pub user_data: *mut std::ffi::c_void,
}

// SAFETY: the native plugin guarantees handlers stay valid for the life of the server
// and only marshal work onto the UI thread.
unsafe impl Send for MorphControlHandlers {}
unsafe impl Sync for MorphControlHandlers {}

struct FfiBackend {
    handlers: MorphControlHandlers,
}

fn c_string(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap_or_else(|_| CString::new("").unwrap())
}

impl ControlBackend for FfiBackend {
    fn navigate(&self, view_id: i64, url: String) {
        if let Some(f) = self.handlers.navigate {
            let url = c_string(&url);
            f(view_id, url.as_ptr(), self.handlers.user_data);
        }
    }
    fn go_back(&self, view_id: i64) {
        if let Some(f) = self.handlers.go_back {
            f(view_id, self.handlers.user_data);
        }
    }
    fn go_forward(&self, view_id: i64) {
        if let Some(f) = self.handlers.go_forward {
            f(view_id, self.handlers.user_data);
        }
    }
    fn reload(&self, view_id: i64) {
        if let Some(f) = self.handlers.reload {
            f(view_id, self.handlers.user_data);
        }
    }
    fn pointer(&self, view_id: i64, event: crate::proto::PointerEvent) {
        if let Some(f) = self.handlers.pointer {
            let phase = c_string(&event.phase);
            f(view_id, phase.as_ptr(), event.x, event.y, event.button, event.scroll_delta_x, event.scroll_delta_y, self.handlers.user_data);
        }
    }
    fn key(&self, view_id: i64, event: crate::proto::KeyEvent) {
        if let Some(f) = self.handlers.key {
            let phase = c_string(&event.phase);
            let key = c_string(&event.key);
            f(
                view_id,
                phase.as_ptr(),
                key.as_ptr(),
                event.hardware_keycode as c_uint,
                event.ctrl as c_int,
                event.alt as c_int,
                event.shift as c_int,
                event.meta as c_int,
                self.handlers.user_data,
            );
        }
    }
    fn ime(&self, view_id: i64, event: crate::proto::ImeEvent) {
        if let Some(f) = self.handlers.ime {
            let phase = c_string(&event.phase);
            let text = c_string(&event.text);
            f(view_id, phase.as_ptr(), text.as_ptr(), self.handlers.user_data);
        }
    }
    fn evaluate_js(&self, view_id: i64, request_id: u64, script: String) {
        if let Some(f) = self.handlers.evaluate_js {
            let script = c_string(&script);
            f(view_id, request_id, script.as_ptr(), self.handlers.user_data);
        }
    }
}

fn ptr_to_str<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(p) }.to_str().ok()
}

fn write_endpoint(out: *mut c_char, len: usize, uri: &str) -> c_int {
    if out.is_null() || len == 0 {
        return -1;
    }
    let c = match CString::new(uri) {
        Ok(c) => c,
        Err(_) => return -1,
    };
    let bytes = c.as_bytes_with_nul();
    if bytes.len() > len {
        return -1;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out as *mut u8, bytes.len());
    }
    0
}

/// Starts the process-wide control server and writes the bound URI to `endpoint_out`.
///
/// # Safety
///
/// `handlers` must be non-null and point to a valid `MorphControlHandlers` whose callbacks stay
/// valid until [`morph_control_stop`] returns. `socket_path` may be null or a valid C string.
/// `endpoint_out` must be a writable buffer of `endpoint_out_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morph_control_start(
    socket_path: *const c_char,
    handlers: *const MorphControlHandlers,
    endpoint_out: *mut c_char,
    endpoint_out_len: usize,
) -> c_int {
    if handlers.is_null() {
        return -1;
    }
    let handlers = unsafe { std::ptr::read(handlers) };
    let endpoint = ptr_to_str(socket_path).map(parse_endpoint);
    match start_global(endpoint, Arc::new(FfiBackend { handlers })) {
        Ok(bound) => write_endpoint(endpoint_out, endpoint_out_len, &bound.uri()),
        Err(_) => -1,
    }
}

fn parse_endpoint(s: &str) -> ControlEndpoint {
    if let Some(path) = s.strip_prefix("unix://") {
        return ControlEndpoint::Unix(path.into());
    }
    if let Some(addr) = s.strip_prefix("tcp://")
        && let Some(port) = addr.rsplit(':').next().and_then(|p| p.parse().ok())
    {
        return ControlEndpoint::Loopback { port };
    }
    if s.starts_with(r"\\.\pipe") || s.starts_with("pipe://") {
        let name = s.strip_prefix("pipe://").unwrap_or(s);
        return ControlEndpoint::NamedPipe(name.to_string());
    }
    ControlEndpoint::from_path(s)
}

#[unsafe(no_mangle)]
pub extern "C" fn morph_control_stop() {
    stop_global();
}

/// Emits a navigation event on the process-wide control server.
///
/// # Safety
///
/// `url` and `title` must be null or valid C strings. The server must still be running, or the
/// call returns an error code.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morph_control_emit_navigation(
    view_id: i64,
    url: *const c_char,
    title: *const c_char,
    can_go_back: c_int,
    can_go_forward: c_int,
    is_loading: c_int,
    progress: f64,
) -> c_int {
    let event = crate::proto::NavigationEvent {
        url: ptr_to_str(url).unwrap_or("").to_string(),
        title: ptr_to_str(title).unwrap_or("").to_string(),
        can_go_back: can_go_back != 0,
        can_go_forward: can_go_forward != 0,
        is_loading: is_loading != 0,
        progress,
    };
    match with_global(|server| server.emit_navigation(event, view_id)) {
        Some(()) => 0,
        None => -1,
    }
}

/// Emits a page-bridge message so Rust can handle it and deliver a reply.
///
/// # Safety
///
/// `json` must be null or a valid C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morph_control_emit_page_message(view_id: i64, json: *const c_char) -> c_int {
    let json = ptr_to_str(json).unwrap_or("");
    match with_global(|server| server.emit_page_message(view_id, json)) {
        Some(()) => 0,
        None => -1,
    }
}

/// Completes a pending `evaluateJavaScript` request.
///
/// # Safety
///
/// `result` and `error` must be null or valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morph_control_complete_evaluate_js(view_id: i64, request_id: u64, result: *const c_char, error: *const c_char) -> c_int {
    let result = ptr_to_str(result).unwrap_or("");
    let error = ptr_to_str(error).unwrap_or("");
    match with_global(|server| server.complete_evaluate_js(view_id, request_id, result, error)) {
        Some(()) => 0,
        None => -1,
    }
}
