//! Record ids for things made on this device (chats, journal entries, MCP servers).

use std::sync::atomic::{AtomicU64, Ordering};

/// A new id: time in nanoseconds plus a process counter, in hex. Unique per device; records
/// from other devices keep the ids they were made with.
pub fn new_id(prefix: &str) -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
    format!("{prefix}-{nanos:x}-{:x}", SEQ.fetch_add(1, Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    #[test]
    fn ids_differ_and_carry_the_prefix() {
        let (a, b) = (super::new_id("chat"), super::new_id("chat"));
        assert_ne!(a, b);
        assert!(a.starts_with("chat-"));
    }
}
