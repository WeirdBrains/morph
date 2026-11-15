//! Morph's UI component model, shared by every shell and free of any UI
//! toolkit (ARCHITECTURE.md section 2a).
//!
//! The desktop shell is a host for components: the browser is one of them,
//! next to the library, settings and, later, chat, notes and projects. This
//! crate holds what that host decides without drawing anything: which
//! components exist ([`component`]), where they sit ([`layout`]), the named
//! actions menus and keys dispatch ([`actions`]), the editing rules of a
//! one-line text field ([`text`]) and the colour roles ([`theme`]).
#![forbid(unsafe_code)]

pub mod actions;
pub mod chrome;
pub mod component;
pub mod layout;
pub mod text;
pub mod theme;
