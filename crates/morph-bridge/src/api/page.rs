//! Page-bridge JSON contract, exposed to Dart.

/// Handle a page JSON message and return the JSON reply.
pub fn handle_page_message(json: String) -> Result<String, String> {
    morph_core::page_bridge::handle_page_message(&json)
}

#[cfg(test)]
mod tests {
    #[test]
    fn ping_from_the_bridge() {
        let raw = super::handle_page_message(r#"{"id":"1","type":"ping"}"#.into()).unwrap();
        assert!(raw.contains("pong"));
    }
}
