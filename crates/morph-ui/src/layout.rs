//! Where components sit. Swapping, hiding and moving a component is a change
//! to a [`Layout`] value, saved per profile next to its settings.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::component::{BROWSER, KindId};

/// A named area of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Slot {
    Main,
    Sidebar,
    Bottom,
}

impl Slot {
    pub const ALL: [Slot; 3] = [Slot::Main, Slot::Sidebar, Slot::Bottom];

    /// Default size in logical pixels (width for the sidebar, height for the
    /// bottom panel; the main slot takes what is left).
    fn default_size(self) -> u32 {
        match self {
            Slot::Main => 0,
            Slot::Sidebar => 280,
            Slot::Bottom => 220,
        }
    }

    fn size_range(self) -> (u32, u32) {
        match self {
            Slot::Main => (0, 0),
            Slot::Sidebar => (160, 640),
            Slot::Bottom => (120, 600),
        }
    }
}

/// One slot's content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotState {
    pub component: Option<KindId>,
    pub size: u32,
    pub visible: bool,
}

/// Which component is in which slot, and how big the slots are.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Layout {
    slots: BTreeMap<Slot, SlotState>,
}

impl Default for Layout {
    /// The browser in the main slot; the sidebar and bottom panel empty.
    fn default() -> Self {
        let mut slots = BTreeMap::new();
        for slot in Slot::ALL {
            let component = (slot == Slot::Main).then(|| KindId::new(BROWSER));
            slots.insert(slot, SlotState { component, size: slot.default_size(), visible: slot == Slot::Main });
        }
        Self { slots }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
}

impl Layout {
    pub fn state(&self, slot: Slot) -> &SlotState {
        self.slots.get(&slot).expect("every slot is present")
    }

    fn state_mut(&mut self, slot: Slot) -> &mut SlotState {
        self.slots.entry(slot).or_insert(SlotState { component: None, size: slot.default_size(), visible: false })
    }

    /// The component shown in `slot`: `None` when empty or hidden.
    pub fn shown(&self, slot: Slot) -> Option<&KindId> {
        let state = self.state(slot);
        if state.visible { state.component.as_ref() } else { None }
    }

    /// Put `kind` in `slot` and show it. A component lives in one slot at a
    /// time, so it leaves any other slot.
    pub fn place(&mut self, slot: Slot, kind: KindId) {
        for other in Slot::ALL {
            if other != slot && self.state(other).component.as_ref() == Some(&kind) {
                let state = self.state_mut(other);
                state.component = None;
                state.visible = false;
            }
        }
        let state = self.state_mut(slot);
        state.component = Some(kind);
        state.visible = true;
    }

    /// Empty `slot`.
    pub fn clear(&mut self, slot: Slot) {
        let state = self.state_mut(slot);
        state.component = None;
        state.visible = false;
    }

    /// Show or hide `slot`. An empty slot stays hidden. Returns whether it is
    /// now visible.
    pub fn toggle(&mut self, slot: Slot) -> bool {
        let state = self.state_mut(slot);
        state.visible = state.component.is_some() && !state.visible;
        state.visible
    }

    /// Show `kind` in `slot`, or hide the slot when it already shows it (a
    /// panel's shortcut opens and closes it). Returns whether it is shown.
    pub fn toggle_kind(&mut self, slot: Slot, kind: KindId) -> bool {
        if self.shown(slot) == Some(&kind) {
            self.state_mut(slot).visible = false;
            false
        } else {
            self.place(slot, kind);
            true
        }
    }

    /// Swap the contents of two slots, keeping each slot's size.
    pub fn swap(&mut self, a: Slot, b: Slot) {
        let (ca, va) = (self.state(a).component.clone(), self.state(a).visible);
        let (cb, vb) = (self.state(b).component.clone(), self.state(b).visible);
        let sa = self.state_mut(a);
        sa.component = cb;
        sa.visible = vb;
        let sb = self.state_mut(b);
        sb.component = ca;
        sb.visible = va;
    }

    /// Resize `slot`, clamped to its range. The main slot has no size.
    pub fn set_size(&mut self, slot: Slot, size: u32) {
        let (min, max) = slot.size_range();
        self.state_mut(slot).size = size.clamp(min, max);
    }
}

/// `<home>/profiles/<id>/layout.json`.
pub fn layout_path(home: impl AsRef<Path>, profile: &str) -> PathBuf {
    morph_core::profiles::profile_dir(home, profile).join("layout.json")
}

/// The profile's saved layout, or the default when none is saved or the file
/// is unreadable (a broken layout must never keep the shell from starting).
pub fn load(home: impl AsRef<Path>, profile: &str) -> Layout {
    let path = layout_path(home, profile);
    fs::read_to_string(path).ok().and_then(|text| serde_json::from_str::<Layout>(&text).ok()).map(fill_missing).unwrap_or_default()
}

fn fill_missing(mut layout: Layout) -> Layout {
    for slot in Slot::ALL {
        layout.state_mut(slot);
    }
    layout
}

/// Save the layout for `profile`, replacing the file atomically.
pub fn save(home: impl AsRef<Path>, profile: &str, layout: &Layout) -> Result<(), LayoutError> {
    let path = layout_path(home, profile);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(layout)?)?;
    fs::rename(tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_puts_the_browser_in_the_main_slot() {
        let layout = Layout::default();
        assert_eq!(layout.shown(Slot::Main).map(KindId::as_str), Some(BROWSER));
        assert_eq!(layout.shown(Slot::Sidebar), None);
        assert_eq!(layout.shown(Slot::Bottom), None);
    }

    #[test]
    fn a_component_lives_in_one_slot_and_swaps_keep_sizes() {
        let mut layout = Layout::default();
        layout.place(Slot::Sidebar, KindId::new("morph.notes"));
        layout.set_size(Slot::Sidebar, 300);
        layout.place(Slot::Bottom, KindId::new(BROWSER));
        assert_eq!(layout.shown(Slot::Main), None, "moved out of main");
        layout.swap(Slot::Main, Slot::Bottom);
        assert_eq!(layout.shown(Slot::Main).map(KindId::as_str), Some(BROWSER));
        assert_eq!(layout.state(Slot::Sidebar).size, 300);
        layout.set_size(Slot::Sidebar, 5);
        assert_eq!(layout.state(Slot::Sidebar).size, 160, "clamped");
    }

    #[test]
    fn toggling_hides_and_shows_but_an_empty_slot_stays_hidden() {
        let mut layout = Layout::default();
        assert!(!layout.toggle(Slot::Bottom));
        layout.place(Slot::Bottom, KindId::new("morph.chat"));
        assert!(!layout.toggle(Slot::Bottom));
        assert_eq!(layout.shown(Slot::Bottom), None);
        assert!(layout.toggle(Slot::Bottom));
        layout.clear(Slot::Bottom);
        assert_eq!(layout.shown(Slot::Bottom), None);
    }

    #[test]
    fn a_panel_shortcut_opens_closes_and_replaces_the_panel() {
        let mut layout = Layout::default();
        assert!(layout.toggle_kind(Slot::Sidebar, KindId::new("morph.library")));
        assert_eq!(layout.shown(Slot::Sidebar).map(KindId::as_str), Some("morph.library"));
        assert!(!layout.toggle_kind(Slot::Sidebar, KindId::new("morph.library")));
        assert_eq!(layout.shown(Slot::Sidebar), None);
        assert!(layout.toggle_kind(Slot::Sidebar, KindId::new("morph.library")));
        assert!(layout.toggle_kind(Slot::Sidebar, KindId::new("morph.settings")), "another panel takes the slot");
        assert_eq!(layout.shown(Slot::Sidebar).map(KindId::as_str), Some("morph.settings"));
        assert_eq!(layout.shown(Slot::Main).map(KindId::as_str), Some(BROWSER));
    }

    #[test]
    fn layouts_are_saved_per_profile_under_morph_home() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        assert_eq!(load(&home, "default"), Layout::default(), "nothing saved yet");
        let mut layout = Layout::default();
        layout.place(Slot::Sidebar, KindId::new("morph.notes"));
        save(&home, "p1", &layout).unwrap();
        assert_eq!(load(&home, "p1"), layout);
        assert_eq!(load(&home, "default"), Layout::default(), "other profiles untouched");
        assert!(layout_path(&home, "p1").starts_with(&home));
    }

    #[test]
    fn a_broken_layout_file_falls_back_to_the_default() {
        let tmp = tempfile::tempdir().unwrap();
        let path = layout_path(tmp.path(), "default");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(load(tmp.path(), "default"), Layout::default());
    }
}
