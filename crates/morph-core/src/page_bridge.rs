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
    fn bad_json_is_an_error() {
        assert!(handle_page_message("not-json").is_err());
    }
}
