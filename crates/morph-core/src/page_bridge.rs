//! Page ↔ Rust message contract used by `morph-bridge.js`.
//!
//! Rust never touches the DOM. The page sends JSON; this module answers JSON. The same
//! contract will be injected on every web-view backend.
use serde::{Deserialize, Serialize};

/// A message from the page (or a test stand-in for the injected script).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageMessage {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

/// A reply the page can post back into itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageReply {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The page side of the contract (`window.MorphBridge`), injected at document
/// start. One file for every backend: the Flutter app ships the same one.
pub const BRIDGE_SCRIPT: &str = include_str!("../../../packages/morph_webview_inapp/assets/morph-bridge.js");

/// The reply to hand back to the page for `json`: the answer, or an error
/// reply when the message has an id but cannot be handled. `None` when there
/// is no id to answer to.
pub fn answer(json: &str) -> Option<String> {
    match handle_page_message(json) {
        Ok(reply) => Some(reply),
        Err(error) => {
            let id = serde_json::from_str::<serde_json::Value>(json).ok()?.get("id")?.as_str()?.to_string();
            if id.is_empty() {
                return None;
            }
            serde_json::to_string(&PageReply { id, kind: "error".into(), payload: None, error: Some(error) }).ok()
        }
    }
}

/// Handle one page message. `ping` is answered with `pong`. Other types are echoed as
/// `ok` so a later hook can take over without changing the envelope.
pub fn handle_page_message(json: &str) -> Result<String, String> {
    let msg: PageMessage = serde_json::from_str(json).map_err(|e| format!("invalid page message: {e}"))?;
    if msg.id.is_empty() || msg.kind.is_empty() {
        return Err("page message needs id and type".into());
    }
    let reply = match msg.kind.as_str() {
        "ping" => PageReply { id: msg.id, kind: "pong".into(), payload: msg.payload, error: None },
        _ => PageReply {
            id: msg.id,
            kind: "ok".into(),
            payload: Some(serde_json::json!({
                "name": msg.name,
                "echo": msg.payload,
            })),
            error: None,
        },
    };
    serde_json::to_string(&reply).map_err(|e| format!("encode reply: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_round_trips() {
        let raw = handle_page_message(r#"{"id":"1","type":"ping"}"#).unwrap();
        let reply: PageReply = serde_json::from_str(&raw).unwrap();
        assert_eq!(reply.id, "1");
        assert_eq!(reply.kind, "pong");
    }

    #[test]
    fn unknown_type_is_echoed() {
        let raw = handle_page_message(r#"{"id":"7","type":"event","name":"click","payload":{"x":1}}"#).unwrap();
        let reply: PageReply = serde_json::from_str(&raw).unwrap();
        assert_eq!(reply.kind, "ok");
        assert_eq!(reply.payload.unwrap()["name"], "click");
    }

    #[test]
    fn answers_carry_errors_back_to_the_page() {
        let raw = answer(r#"{"id":"9","type":""}"#).unwrap();
        let reply: PageReply = serde_json::from_str(&raw).unwrap();
        assert_eq!(reply.id, "9");
        assert!(reply.error.unwrap().contains("needs id and type"));
        assert!(answer("not-json").is_none());
        assert!(answer(r#"{"type":"ping"}"#).is_none());
        assert_eq!(answer(r#"{"id":"1","type":"ping"}"#).unwrap(), handle_page_message(r#"{"id":"1","type":"ping"}"#).unwrap());
    }

    #[test]
    fn the_script_exposes_the_bridge() {
        assert!(BRIDGE_SCRIPT.contains("window.MorphBridge"));
        assert!(BRIDGE_SCRIPT.contains("morphRust.postMessage"));
    }

    #[test]
    fn bad_json_is_an_error() {
        assert!(handle_page_message("not-json").is_err());
    }
}
