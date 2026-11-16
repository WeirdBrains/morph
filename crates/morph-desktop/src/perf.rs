//! Counts for `MORPH_FRAME_STATS=1`: how often the shell ticks, renders and
//! hands the page pointer moves and new bounds, printed every 2 s as a
//! `morph-render` line (the Xvfb checks read it).

use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

pub static TICKS: AtomicU32 = AtomicU32::new(0);
pub static SHELL_RENDERS: AtomicU32 = AtomicU32::new(0);
pub static BROWSER_RENDERS: AtomicU32 = AtomicU32::new(0);
pub static PAGE_MOVES: AtomicU32 = AtomicU32::new(0);
pub static PAGE_BOUNDS: AtomicU32 = AtomicU32::new(0);
/// Ticks that asked for a new frame, and why.
pub static NOTIFY_EVENTS: AtomicU32 = AtomicU32::new(0);
pub static NOTIFY_FRAMES: AtomicU32 = AtomicU32::new(0);

pub fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("MORPH_FRAME_STATS").is_some_and(|v| v == "1"))
}

pub fn count(c: &AtomicU32) {
    if on() {
        c.fetch_add(1, Relaxed);
    }
}

/// Print the rates and start over, every 2 s.
pub fn report() {
    if !on() {
        return;
    }
    static SINCE: Mutex<Option<Instant>> = Mutex::new(None);
    let mut since = SINCE.lock().unwrap_or_else(|p| p.into_inner());
    let at = *since.get_or_insert_with(Instant::now);
    let secs = at.elapsed().as_secs_f64();
    if secs < 2.0 {
        return;
    }
    *since = Some(Instant::now());
    let rate = |c: &AtomicU32| f64::from(c.swap(0, Relaxed)) / secs;
    println!(
        "morph-render ticks={:.0} shell={:.1} browser={:.1} moves={:.1} bounds={:.1} notify_events={:.1} notify_frames={:.1}",
        rate(&TICKS),
        rate(&SHELL_RENDERS),
        rate(&BROWSER_RENDERS),
        rate(&PAGE_MOVES),
        rate(&PAGE_BOUNDS),
        rate(&NOTIFY_EVENTS),
        rate(&NOTIFY_FRAMES)
    );
}
