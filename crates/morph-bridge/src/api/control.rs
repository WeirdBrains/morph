//! In-process webview control server, started from the Flutter app on macOS and Windows.

use crate::frb_generated::StreamSink;
use morph_webview_control::endpoint::ControlEndpoint;
use morph_webview_control::proto::{ImeEvent, KeyEvent, PointerEvent};
use morph_webview_control::server::ControlBackend;
use morph_webview_control::{start_global, stop_global, with_global};
use std::sync::{Arc, Mutex, OnceLock};

/// A command the Dart web-view backend must apply to the platform view.
pub struct ControlBackendEvent {
    pub kind: String,
    pub view_id: i64,
    pub url: String,
    pub request_id: u64,
    pub script: String,
    pub phase: String,
    pub x: f64,
    pub y: f64,
    pub button: i32,
    pub scroll_delta_x: f64,
    pub scroll_delta_y: f64,
    pub key: String,
    pub hardware_keycode: u32,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
    pub text: String,
}

impl ControlBackendEvent {
    fn base(kind: &str, view_id: i64) -> Self {
        Self {
            kind: kind.to_string(),
            view_id,
            url: String::new(),
            request_id: 0,
            script: String::new(),
            phase: String::new(),
            x: 0.0,
            y: 0.0,
            button: 0,
            scroll_delta_x: 0.0,
            scroll_delta_y: 0.0,
            key: String::new(),
            hardware_keycode: 0,
            ctrl: false,
            alt: false,
            shift: false,
            meta: false,
            text: String::new(),
        }
    }
}

static BACKEND_SINK: OnceLock<Mutex<Option<StreamSink<ControlBackendEvent>>>> = OnceLock::new();

fn sink_slot() -> &'static Mutex<Option<StreamSink<ControlBackendEvent>>> {
    BACKEND_SINK.get_or_init(|| Mutex::new(None))
}

fn emit_backend(event: ControlBackendEvent) {
    if let Ok(slot) = sink_slot().lock()
        && let Some(sink) = slot.as_ref()
    {
        let _ = sink.add(event);
    }
}

struct ForwardingBackend {}

impl ControlBackend for ForwardingBackend {
    fn navigate(&self, view_id: i64, url: String) {
        emit_backend(ControlBackendEvent { url, ..ControlBackendEvent::base("navigate", view_id) });
    }
    fn go_back(&self, view_id: i64) {
        emit_backend(ControlBackendEvent::base("go_back", view_id));
    }
    fn go_forward(&self, view_id: i64) {
        emit_backend(ControlBackendEvent::base("go_forward", view_id));
    }
    fn reload(&self, view_id: i64) {
        emit_backend(ControlBackendEvent::base("reload", view_id));
    }
    fn pointer(&self, view_id: i64, event: PointerEvent) {
        emit_backend(ControlBackendEvent {
            phase: event.phase,
            x: event.x,
            y: event.y,
            button: event.button,
            scroll_delta_x: event.scroll_delta_x,
            scroll_delta_y: event.scroll_delta_y,
            ..ControlBackendEvent::base("pointer", view_id)
        });
    }
    fn key(&self, view_id: i64, event: KeyEvent) {
        emit_backend(ControlBackendEvent {
            phase: event.phase,
            key: event.key,
            hardware_keycode: event.hardware_keycode,
            ctrl: event.ctrl,
            alt: event.alt,
            shift: event.shift,
            meta: event.meta,
            ..ControlBackendEvent::base("key", view_id)
        });
    }
    fn ime(&self, view_id: i64, event: ImeEvent) {
        emit_backend(ControlBackendEvent { phase: event.phase, text: event.text, ..ControlBackendEvent::base("ime", view_id) });
    }
    fn evaluate_js(&self, view_id: i64, request_id: u64, script: String) {
        emit_backend(ControlBackendEvent { request_id, script, ..ControlBackendEvent::base("evaluate_js", view_id) });
    }
}

/// Subscribe to backend commands the control server wants applied to a platform web view.
///
/// Call this before [start_webview_control] so no command is missed.
pub fn control_backend_events(sink: StreamSink<ControlBackendEvent>) {
    *sink_slot().lock().expect("control backend sink") = Some(sink);
}

/// Start the process-wide control server and return its URI (`unix://...` or `pipe://...`).
pub fn start_webview_control() -> Result<String, String> {
    let bound = start_global(None, Arc::new(ForwardingBackend {})).map_err(|e| e.to_string())?;
    Ok(bound.uri())
}

/// Start the server on loopback TCP (used if a client cannot open a named pipe).
pub fn start_webview_control_loopback() -> Result<String, String> {
    let bound = start_global(Some(ControlEndpoint::Loopback { port: 0 }), Arc::new(ForwardingBackend {})).map_err(|e| e.to_string())?;
    Ok(bound.uri())
}

pub fn stop_webview_control() {
    stop_global();
}

pub fn control_emit_navigation(
    view_id: i64,
    url: String,
    title: String,
    can_go_back: bool,
    can_go_forward: bool,
    is_loading: bool,
    progress: f64,
) -> Result<(), String> {
    with_global(|server| {
        server.emit_navigation(
            morph_webview_control::proto::NavigationEvent { url, title, can_go_back, can_go_forward, is_loading, progress },
            view_id,
        );
    })
    .ok_or_else(|| "control server is not running".to_string())
}

pub fn control_emit_page_message(view_id: i64, json: String) -> Result<(), String> {
    with_global(|server| server.emit_page_message(view_id, &json)).ok_or_else(|| "control server is not running".to_string())
}

pub fn control_complete_evaluate_js(view_id: i64, request_id: u64, result: String, error: String) -> Result<(), String> {
    with_global(|server| server.complete_evaluate_js(view_id, request_id, &result, &error)).ok_or_else(|| "control server is not running".to_string())
}
