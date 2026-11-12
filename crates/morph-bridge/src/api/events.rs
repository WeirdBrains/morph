//! Rust→Dart event stream (M1 wiring check).

use crate::frb_generated::StreamSink;
use std::time::Duration;

/// A monotonic tick, once a second, until the Dart side cancels the stream.
pub fn event_ticks(sink: StreamSink<u32>) {
    std::thread::spawn(move || {
        let mut n: u32 = 0;
        loop {
            n = n.saturating_add(1);
            if sink.add(n).is_err() {
                break;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}
