//! The window and the component host. The host knows components only
//! through [`Hosted`]; the browser is one implementation, and later
//! components (library, settings, chat, notes) are others.

use std::path::PathBuf;
use std::time::Duration;

use gpui::{AnyView, App, Bounds, Context, Entity, KeyBinding, Window, WindowBounds, WindowOptions, actions, div, prelude::*, px, rgb, size};
use morph_ui::actions::{Action, Keymap};
use morph_ui::component::{self, Catalog, KindId};
use morph_ui::layout::{self, Layout, Slot};
use morph_ui::theme::Theme;

use crate::browser_view::{self, BrowserView};
use crate::cli::Args;

actions!(
    morph,
    [
        NewTab,
        CloseTab,
        NextTab,
        PrevTab,
        SelectTab1,
        SelectTab2,
        SelectTab3,
        SelectTab4,
        SelectTab5,
        SelectTab6,
        SelectTab7,
        SelectTab8,
        SelectLastTab,
        FocusAddress,
        Find,
        Reload,
        Stop,
        Back,
        Forward,
        ToggleFullscreen,
        Quit
    ]
);

/// The GPUI action for a Morph action.
fn gpui_action(action: Action) -> Box<dyn gpui::Action> {
    match action {
        Action::NewTab => Box::new(NewTab),
        Action::CloseTab => Box::new(CloseTab),
        Action::NextTab => Box::new(NextTab),
        Action::PrevTab => Box::new(PrevTab),
        Action::SelectTab(0) => Box::new(SelectTab1),
        Action::SelectTab(1) => Box::new(SelectTab2),
        Action::SelectTab(2) => Box::new(SelectTab3),
        Action::SelectTab(3) => Box::new(SelectTab4),
        Action::SelectTab(4) => Box::new(SelectTab5),
        Action::SelectTab(5) => Box::new(SelectTab6),
        Action::SelectTab(6) => Box::new(SelectTab7),
        Action::SelectTab(_) => Box::new(SelectTab8),
        Action::SelectLastTab => Box::new(SelectLastTab),
        Action::FocusAddress => Box::new(FocusAddress),
        Action::Find => Box::new(Find),
        Action::Reload => Box::new(Reload),
        Action::Stop => Box::new(Stop),
        Action::Back => Box::new(Back),
        Action::Forward => Box::new(Forward),
        Action::ToggleFullscreen => Box::new(ToggleFullscreen),
        Action::Quit => Box::new(Quit),
    }
}

/// What the host needs from a component.
pub trait Hosted {
    fn kind(&self) -> KindId;
    fn view(&self) -> AnyView;
    /// Run a shell action; `false` when this component has no use for it.
    fn dispatch(&self, action: Action, window: &mut Window, cx: &mut App) -> bool;
    /// Periodic work (pumping web views). Returns keystrokes pressed inside
    /// the component that the shell should handle.
    fn tick(&self, window: &mut Window, cx: &mut App) -> Vec<String>;
    fn set_shown(&self, shown: bool, cx: &mut App);
    /// Pull keyboard focus back to the shell's own UI.
    fn focus_shell(&self, cx: &App);
    fn title(&self, cx: &App) -> String;
}

impl Hosted for Entity<BrowserView> {
    fn kind(&self) -> KindId {
        KindId::new(component::BROWSER)
    }
    fn view(&self) -> AnyView {
        self.clone().into()
    }
    fn dispatch(&self, action: Action, window: &mut Window, cx: &mut App) -> bool {
        self.update(cx, |view, cx| view.dispatch(action, window, cx))
    }
    fn tick(&self, window: &mut Window, cx: &mut App) -> Vec<String> {
        self.update(cx, |view, cx| view.tick(window, cx))
    }
    fn set_shown(&self, shown: bool, cx: &mut App) {
        self.update(cx, |view, _| view.set_shown(shown));
    }
    fn focus_shell(&self, cx: &App) {
        self.read(cx).browser().focus_shell();
    }
    fn title(&self, cx: &App) -> String {
        self.read(cx).title()
    }
}

pub struct Shell {
    theme: Theme,
    keymap: Keymap,
    catalog: Catalog,
    layout: Layout,
    components: Vec<Box<dyn Hosted>>,
    error: Option<String>,
    last_title: String,
}

impl Shell {
    fn component(&self, kind: &KindId) -> Option<&dyn Hosted> {
        self.components.iter().find(|c| &c.kind() == kind).map(|c| c.as_ref())
    }

    /// Route a shell action: the shell's own first, then the component in
    /// the main slot, then the others.
    fn run(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            Action::Quit => {
                cx.quit();
                return;
            }
            Action::ToggleFullscreen => {
                window.toggle_fullscreen();
                return;
            }
            _ => {}
        }
        let main = self.layout.shown(Slot::Main).cloned();
        let mut order: Vec<&dyn Hosted> = Vec::new();
        if let Some(c) = main.as_ref().and_then(|k| self.component(k)) {
            order.push(c);
        }
        order.extend(self.components.iter().map(|c| c.as_ref()).filter(|c| Some(c.kind()) != main));
        for component in order {
            if component.dispatch(action, window, cx) {
                break;
            }
        }
        cx.notify();
    }

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut shortcuts = Vec::new();
        for component in &self.components {
            shortcuts.extend(component.tick(window, cx));
        }
        for key in shortcuts {
            if let Some(action) = self.keymap.action_for(&key) {
                self.run(action, window, cx);
            }
        }
        let title = match self.layout.shown(Slot::Main).and_then(|k| self.component(k)) {
            Some(c) => c.title(cx),
            None => String::new(),
        };
        if title != self.last_title {
            let full = if title.is_empty() { "Morph".to_string() } else { format!("{title} — Morph") };
            window.set_window_title(&full);
            self.last_title = title;
        }
    }

    fn slot(&self, slot: Slot, cx: &App) -> Option<gpui::AnyElement> {
        let t = self.theme;
        let kind = self.layout.shown(slot)?;
        let content = match self.component(kind) {
            Some(component) => component.view().into_any_element(),
            None => {
                let title = self.catalog.get(kind).map(|k| k.title.clone()).unwrap_or_else(|| kind.as_str().to_string());
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(t.text_secondary))
                    .child(format!("{title} is not available yet"))
                    .into_any_element()
            }
        };
        let size = px(self.layout.state(slot).size as f32);
        let _ = cx;
        Some(match slot {
            Slot::Main => div().flex_1().flex().flex_col().min_w(px(0.)).child(content).into_any_element(),
            Slot::Sidebar => div().w(size).h_full().border_r_1().border_color(rgb(t.surface_variant)).child(content).into_any_element(),
            Slot::Bottom => div().h(size).w_full().border_t_1().border_color(rgb(t.surface_variant)).child(content).into_any_element(),
        })
    }
}

macro_rules! on_actions {
    ($el:expr, $cx:expr, [$($gpui:ident => $action:expr),* $(,)?]) => {
        $el$(.on_action($cx.listener(|this, _: &$gpui, window, cx| this.run($action, window, cx))))*
    };
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let root = div().flex().flex_col().size_full().bg(rgb(t.background)).text_color(rgb(t.text_primary)).font_family(".SystemUIFont");
        let root = on_actions!(root, cx, [
            NewTab => Action::NewTab,
            CloseTab => Action::CloseTab,
            NextTab => Action::NextTab,
            PrevTab => Action::PrevTab,
            SelectTab1 => Action::SelectTab(0),
            SelectTab2 => Action::SelectTab(1),
            SelectTab3 => Action::SelectTab(2),
            SelectTab4 => Action::SelectTab(3),
            SelectTab5 => Action::SelectTab(4),
            SelectTab6 => Action::SelectTab(5),
            SelectTab7 => Action::SelectTab(6),
            SelectTab8 => Action::SelectTab(7),
            SelectLastTab => Action::SelectLastTab,
            FocusAddress => Action::FocusAddress,
            Find => Action::Find,
            Reload => Action::Reload,
            Stop => Action::Stop,
            Back => Action::Back,
            Forward => Action::Forward,
            ToggleFullscreen => Action::ToggleFullscreen,
            Quit => Action::Quit,
        ]);
        let root = root.on_any_mouse_down(cx.listener(|this, _, _, cx| {
            for component in &this.components {
                component.focus_shell(cx);
            }
        }));
        if let Some(error) = &self.error {
            return root.items_center().justify_center().child(div().max_w(px(640.)).p(px(24.)).child(error.clone()));
        }
        let middle = div().flex().flex_row().flex_1().min_h(px(0.)).children(self.slot(Slot::Sidebar, cx)).children(self.slot(Slot::Main, cx));
        root.child(middle).children(self.slot(Slot::Bottom, cx))
    }
}

/// Keys for the address bar's editing actions, in its key context.
fn address_bindings() -> Vec<KeyBinding> {
    use browser_view::*;
    let c = Some(ADDRESS_CONTEXT);
    vec![
        KeyBinding::new("backspace", Backspace, c),
        KeyBinding::new("delete", Delete, c),
        KeyBinding::new("left", Left, c),
        KeyBinding::new("right", Right, c),
        KeyBinding::new("shift-left", SelectLeft, c),
        KeyBinding::new("shift-right", SelectRight, c),
        KeyBinding::new("ctrl-a", SelectAll, c),
        KeyBinding::new("home", Home, c),
        KeyBinding::new("end", End, c),
        KeyBinding::new("ctrl-v", Paste, c),
        KeyBinding::new("shift-insert", Paste, c),
        KeyBinding::new("ctrl-c", Copy, c),
        KeyBinding::new("ctrl-x", Cut, c),
        KeyBinding::new("enter", Submit, c),
        KeyBinding::new("escape", Cancel, c),
    ]
}

/// Keys the shell takes even while a page has focus: only those whose action
/// does something in this phase. Everything else (Ctrl+F until find in page
/// exists, Ctrl+C/V/X/A/Z, arrows, Page Up/Down, Space, Tab, keypad) goes
/// to the page.
fn page_shortcuts(keymap: &Keymap) -> Vec<String> {
    keymap.bindings().iter().filter(|b| b.action != Action::Find).map(|b| b.keystroke.clone()).collect()
}

/// Open the window and run until it closes.
pub fn run(home: PathBuf, args: Args) {
    gpui_platform::application().run(move |cx: &mut App| {
        let keymap = Keymap::default();
        let mut bindings = address_bindings();
        for binding in keymap.bindings() {
            match KeyBinding::load(&binding.keystroke, gpui_action(binding.action), None, false, None, cx) {
                Ok(b) => bindings.push(b),
                Err(e) => eprintln!("morph: ignoring key {}: {e:?}", binding.keystroke),
            }
        }
        cx.bind_keys(bindings);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let profile = match morph_core::profiles::load_catalog(&home) {
            Ok(catalog) => catalog.current().id.clone(),
            Err(e) => {
                eprintln!("morph: profiles: {e}; using the default profile without history");
                morph_core::profiles::DEFAULT_PROFILE_ID.to_string()
            }
        };
        let saved_layout = layout::load(&home, &profile);
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            app_id: Some("morph".into()),
            window_min_size: Some(size(px(480.), px(320.))),
            ..Default::default()
        };
        let opened = cx.open_window(options, |window, cx| {
            let theme = Theme::default();
            let keystrokes = page_shortcuts(&keymap);
            let start = args.open.clone().unwrap_or_default();
            let mut catalog = Catalog::new();
            for kind in component::builtin() {
                catalog.register(kind);
            }
            let (components, error): (Vec<Box<dyn Hosted>>, Option<String>) =
                match BrowserView::create(&home, &profile, &start, true, window, &keystrokes) {
                    Ok(browser) => {
                        let view = cx.new(|cx| BrowserView::new(browser, theme, cx));
                        (vec![Box::new(view)], None)
                    }
                    Err(e) => {
                        eprintln!("morph: {e}");
                        (Vec::new(), Some(e))
                    }
                };
            let shell =
                cx.new(|_| Shell { theme, keymap: keymap.clone(), catalog, layout: saved_layout, components, error, last_title: String::new() });
            // Components outside a visible slot keep their pages hidden.
            shell.update(cx, |shell, cx| {
                for c in &shell.components {
                    let shown = Slot::ALL.iter().any(|s| shell.layout.shown(*s) == Some(&c.kind()));
                    c.set_shown(shown, cx);
                }
            });
            shell
        });
        let window = match opened {
            Ok(window) => window,
            Err(e) => {
                eprintln!("morph: cannot open a window: {e:#}");
                cx.quit();
                return;
            }
        };
        let _ = window.update(cx, |shell, window, cx| {
            window.set_window_title("Morph");
            let _ = shell;
            cx.spawn_in(window, async move |this, cx| {
                loop {
                    cx.background_executor().timer(Duration::from_millis(8)).await;
                    if this.update_in(cx, |shell, window, cx| shell.tick(window, cx)).is_err() {
                        break;
                    }
                }
            })
            .detach();
            cx.activate(true);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_keep_keys_the_shell_does_not_use() {
        let keys = page_shortcuts(&Keymap::default());
        for taken in ["ctrl-t", "ctrl-w", "ctrl-l", "ctrl-r", "f5", "ctrl-tab", "ctrl-shift-tab", "f11", "ctrl-1", "ctrl-9"] {
            assert!(keys.iter().any(|k| k == taken), "{taken}");
        }
        for page in ["ctrl-f", "ctrl-c", "ctrl-v", "ctrl-x", "ctrl-a", "ctrl-z", "pageup", "space", "tab"] {
            assert!(!keys.iter().any(|k| k == page), "{page}");
        }
    }

    #[test]
    fn every_bound_action_has_a_gpui_action() {
        for binding in Keymap::default().bindings() {
            let action = gpui_action(binding.action);
            assert!(action.name().starts_with("morph::"), "{}", action.name());
        }
    }
}
