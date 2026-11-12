//! In-memory tab list for the Morph shell.
//!
//! The Flutter UI owns each tab's web-view instance and its navigation
//! history. This module is the business logic for open, close, switch, and
//! cycle, so it can be tested without Flutter.

/// Identifier of a tab in a [`TabSet`]. Stable for the life of the tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(pub u64);

/// Presentation state for one tab. History lives in the platform web view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    pub id: TabId,
    pub url: String,
    pub title: String,
}

impl Tab {
    fn blank(id: TabId) -> Self {
        Self { id, url: String::new(), title: String::new() }
    }
}

/// Ordered list of tabs with a single active tab.
///
/// The set always contains at least one tab. Closing the last tab replaces
/// it with a fresh empty tab rather than leaving the shell with nothing to
/// show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabSet {
    tabs: Vec<Tab>,
    active: TabId,
    next_id: u64,
}

impl Default for TabSet {
    fn default() -> Self {
        Self::new()
    }
}

impl TabSet {
    /// A set with a single empty tab, already selected.
    pub fn new() -> Self {
        let id = TabId(1);
        Self { tabs: vec![Tab::blank(id)], active: id, next_id: 2 }
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn active_id(&self) -> TabId {
        self.active
    }

    pub fn active(&self) -> &Tab {
        self.tab(self.active).expect("active tab missing")
    }

    pub fn tab(&self, id: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|tab| tab.id == id)
    }

    pub fn index_of(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id == id)
    }

    /// Open a new tab after the active tab, select it, and return its id.
    ///
    /// `url` may be empty for a start page.
    pub fn open(&mut self, url: impl Into<String>) -> TabId {
        let url = url.into();
        let id = TabId(self.next_id);
        self.next_id += 1;
        let mut tab = Tab::blank(id);
        tab.url = url;
        let insert_at = self.index_of(self.active).map(|i| i + 1).unwrap_or(self.tabs.len());
        self.tabs.insert(insert_at, tab);
        self.active = id;
        id
    }

    /// Select `id`. Returns an error if the tab is not in the set.
    pub fn switch(&mut self, id: TabId) -> Result<(), String> {
        if self.tab(id).is_none() {
            return Err(format!("no tab {}", id.0));
        }
        self.active = id;
        Ok(())
    }

    /// Close `id`. If it was active, the next tab (or the previous, when
    /// closing the last) becomes active. Closing the only tab leaves a
    /// fresh empty tab selected.
    pub fn close(&mut self, id: TabId) -> Result<TabId, String> {
        let index = self.index_of(id).ok_or_else(|| format!("no tab {}", id.0))?;
        let was_active = self.active == id;
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            let fresh = TabId(self.next_id);
            self.next_id += 1;
            self.tabs.push(Tab::blank(fresh));
            self.active = fresh;
            return Ok(self.active);
        }
        if was_active {
            let next = index.min(self.tabs.len() - 1);
            self.active = self.tabs[next].id;
        }
        Ok(self.active)
    }

    /// Close every tab and leave a single fresh empty tab selected.
    pub fn close_all(&mut self) -> TabId {
        let fresh = TabId(self.next_id);
        self.next_id += 1;
        self.tabs = vec![Tab::blank(fresh)];
        self.active = fresh;
        self.active
    }

    /// Select the next tab, wrapping around.
    pub fn cycle_next(&mut self) -> TabId {
        let index = self.index_of(self.active).unwrap_or(0);
        let next = (index + 1) % self.tabs.len();
        self.active = self.tabs[next].id;
        self.active
    }

    /// Select the previous tab, wrapping around.
    pub fn cycle_prev(&mut self) -> TabId {
        let index = self.index_of(self.active).unwrap_or(0);
        let prev = if index == 0 { self.tabs.len() - 1 } else { index - 1 };
        self.active = self.tabs[prev].id;
        self.active
    }

    /// Update the URL and title shown for `id` after a navigation event.
    pub fn update(&mut self, id: TabId, url: String, title: String) -> Result<(), String> {
        let tab = self.tabs.iter_mut().find(|tab| tab.id == id).ok_or_else(|| format!("no tab {}", id.0))?;
        tab.url = url;
        tab.title = title;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{TabId, TabSet};

    #[test]
    fn starts_with_one_empty_active_tab() {
        let set = TabSet::new();
        assert_eq!(set.len(), 1);
        assert_eq!(set.active_id(), TabId(1));
        assert!(set.active().url.is_empty());
        assert!(set.active().title.is_empty());
    }

    #[test]
    fn open_inserts_after_the_active_tab_and_selects_it() {
        let mut set = TabSet::new();
        set.update(TabId(1), "https://a.example/".into(), "A".into()).unwrap();
        let second = set.open("https://b.example/");
        let third = set.open("https://c.example/");
        assert_eq!(set.len(), 3);
        assert_eq!(set.active_id(), third);
        let ids: Vec<_> = set.tabs().iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![TabId(1), second, third]);
    }

    #[test]
    fn switch_changes_the_active_tab() {
        let mut set = TabSet::new();
        let second = set.open("https://b.example/");
        set.switch(TabId(1)).unwrap();
        assert_eq!(set.active_id(), TabId(1));
        set.switch(second).unwrap();
        assert_eq!(set.active_id(), second);
        assert!(set.switch(TabId(99)).is_err());
    }

    #[test]
    fn close_selects_the_following_tab() {
        let mut set = TabSet::new();
        let b = set.open("https://b.example/");
        let c = set.open("https://c.example/");
        set.switch(b).unwrap();
        let active = set.close(b).unwrap();
        assert_eq!(active, c);
        assert_eq!(set.len(), 2);
        assert!(set.tab(b).is_none());
    }

    #[test]
    fn close_of_the_last_tab_selects_the_previous() {
        let mut set = TabSet::new();
        let b = set.open("https://b.example/");
        let active = set.close(b).unwrap();
        assert_eq!(active, TabId(1));
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn close_of_the_only_tab_leaves_a_fresh_empty_tab() {
        let mut set = TabSet::new();
        let first = set.active_id();
        let active = set.close(first).unwrap();
        assert_ne!(active, first);
        assert_eq!(set.len(), 1);
        assert!(set.active().url.is_empty());
        assert_eq!(set.active_id(), active);
    }

    #[test]
    fn closing_an_inactive_tab_keeps_the_current_active_tab() {
        let mut set = TabSet::new();
        let b = set.open("https://b.example/");
        set.switch(TabId(1)).unwrap();
        set.close(b).unwrap();
        assert_eq!(set.active_id(), TabId(1));
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn close_all_leaves_a_single_fresh_tab() {
        let mut set = TabSet::new();
        set.open("https://b.example/");
        set.open("https://c.example/");
        let first = set.tabs()[0].id;
        let active = set.close_all();
        assert_eq!(set.len(), 1);
        assert_ne!(active, first);
        assert!(set.active().url.is_empty());
    }

    #[test]
    fn cycle_wraps_in_both_directions() {
        let mut set = TabSet::new();
        let b = set.open("b");
        let c = set.open("c");
        assert_eq!(set.active_id(), c);
        assert_eq!(set.cycle_next(), TabId(1));
        assert_eq!(set.cycle_next(), b);
        assert_eq!(set.cycle_next(), c);
        assert_eq!(set.cycle_prev(), b);
        assert_eq!(set.cycle_prev(), TabId(1));
        assert_eq!(set.cycle_prev(), c);
    }

    #[test]
    fn update_writes_url_and_title() {
        let mut set = TabSet::new();
        set.update(TabId(1), "https://example.com/".into(), "Example".into()).unwrap();
        assert_eq!(set.active().url, "https://example.com/");
        assert_eq!(set.active().title, "Example");
        assert!(set.update(TabId(9), "x".into(), "y".into()).is_err());
    }
}
