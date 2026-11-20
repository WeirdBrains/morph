//! Editing rules for a one-line text field (the address bar). They live in
//! weirdnet-text so every Weirdnet app edits text the same way; Morph re-exports
//! them here so callers keep using `morph_ui::text::TextField`.
pub use weirdnet_text::TextField;
