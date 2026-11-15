#![allow(warnings, clippy::all)] // Upstream code (third_party/README.md).
#![cfg(any(target_os = "linux", target_os = "freebsd"))]
mod linux;

pub use linux::{current_platform, linux_platform, raw_key};
