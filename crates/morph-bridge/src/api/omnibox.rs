//! The address-bar rule for the Flutter shell: the same `morph-core` rule the
//! desktop shell uses, so both open the same thing for the same text.

/// What typing `input` into the address bar opens: a web address when the
/// text looks like one, else a search. `None` for blank input.
#[flutter_rust_bridge::frb(sync)]
pub fn omnibox_target(input: String) -> Option<String> {
    morph_core::omnibox::target(&input)
}

#[cfg(test)]
mod tests {
    use super::omnibox_target;

    #[test]
    fn follows_the_core_rule() {
        assert_eq!(omnibox_target("example.com".into()).as_deref(), Some("https://example.com"));
        assert_eq!(omnibox_target("how to fix example.com".into()), Some(morph_core::omnibox::search_url("how to fix example.com")));
        assert_eq!(omnibox_target("javascript:alert(1)".into()), Some(morph_core::omnibox::search_url("javascript:alert(1)")));
        assert_eq!(omnibox_target("  ".into()), None);
    }
}
