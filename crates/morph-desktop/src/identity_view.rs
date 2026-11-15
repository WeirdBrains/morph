//! The identity panel (`morph.identity`, Ctrl+Shift+L): a profile's Weirdnet
//! identity, its recovery words and backups, and moving it between devices
//! with a QR link. The steps live in [`crate::identity`]; this draws them.

use std::path::Path;

use gpui::{
    App, ClipboardItem, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyDownEvent, Subscription, Window, canvas, div, fill, point,
    prelude::*, px, rgb, size,
};
use morph_services::device_link::Links;
use morph_ui::theme::Theme;

use crate::identity::{Identity, Outcome, Step, source_text};
use crate::shell::ShellRequest;
use crate::text_input::{TextInput, TextInputEvent};
use crate::widgets::{button, heading, icon};

const INPUTS: usize = 3;

pub struct IdentityView {
    model: Identity,
    theme: Theme,
    inputs: Vec<Entity<TextInput>>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ShellRequest> for IdentityView {}

impl IdentityView {
    pub fn new(home: &Path, profile: &str, profile_name: &str, theme: Theme, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let inputs: Vec<Entity<TextInput>> = (0..INPUTS).map(|_| cx.new(|cx| TextInput::new("", theme, cx))).collect();
        let subs = inputs
            .iter()
            .enumerate()
            .map(|(i, input)| {
                cx.subscribe_in(input, window, move |this, _, event, window, cx| match event {
                    TextInputEvent::Submit { .. } => this.field_submitted(i, window, cx),
                    TextInputEvent::Cancel => this.escape(window, cx),
                    TextInputEvent::Changed => {}
                })
            })
            .collect();
        Self { model: Identity::new(home, profile, profile_name, Links::global()), theme, inputs, focus: cx.focus_handle(), _subscriptions: subs }
    }

    pub fn set_profile_name(&mut self, name: &str) {
        self.model.set_profile_name(name);
    }

    /// The panel opened: start from the profile's first step.
    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.model.cancel();
        self.model.reload();
        self.enter_step(window, cx);
        cx.emit(ShellRequest::FocusShell);
    }

    /// Follow a link session; called by the host's tick.
    pub fn poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.model.poll() {
            self.enter_step(window, cx);
        }
    }

    /// Set the inputs up for the step and put the keyboard in the first one
    /// (or on the panel, whose keys pick an action).
    fn enter_step(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let fields = self.model.fields();
        let defaults = self.model.defaults();
        for (i, input) in self.inputs.iter().enumerate() {
            let (label, masked) = fields.get(i).map(|f| (f.label, f.masked)).unwrap_or(("", false));
            let text = defaults.get(i).cloned().unwrap_or_default();
            input.update(cx, |input, cx| {
                input.configure(label, masked, cx);
                input.set_text(&text, cx);
            });
        }
        match self.inputs.first().filter(|_| !fields.is_empty()) {
            Some(first) => window.focus(&first.focus_handle(cx), cx),
            None => window.focus(&self.focus, cx),
        }
        cx.notify();
    }

    fn field_submitted(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        if i + 1 < self.model.fields().len() {
            window.focus(&self.inputs[i + 1].focus_handle(cx), cx);
        } else {
            self.submit(window, cx);
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let values: Vec<String> = self.inputs.iter().take(self.model.fields().len()).map(|i| i.read(cx).text().to_string()).collect();
        let before = self.model.step.clone();
        if self.model.submit(&values) == Outcome::IdentityChanged {
            cx.emit(ShellRequest::IdentityChanged);
        }
        if self.model.step != before {
            self.enter_step(window, cx);
        } else {
            cx.notify();
        }
    }

    fn go(&mut self, step: Step, window: &mut Window, cx: &mut Context<Self>) {
        self.model.start(step);
        self.enter_step(window, cx);
    }

    fn offer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.model.start_offer();
        self.enter_step(window, cx);
    }

    /// Escape: back to the first step, or close the panel from there.
    fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.model.step, Step::Choose | Step::Overview) {
            cx.emit(ShellRequest::ClosePanel);
        } else {
            self.model.cancel();
            self.enter_step(window, cx);
        }
    }

    /// Keys while no field has the keyboard: digits and letters pick the
    /// buttons' actions, Enter the main one.
    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        if k.modifiers.control || k.modifiers.alt || k.modifiers.platform {
            return;
        }
        let handled = match (k.key.as_str(), &self.model.step) {
            ("escape", _) => {
                self.escape(window, cx);
                true
            }
            ("enter", Step::Words(_) | Step::RevealWords(_) | Step::Paired { .. } | Step::Adding { .. }) => {
                self.submit(window, cx);
                true
            }
            ("1", Step::Choose) => self.then(Step::Create, window, cx),
            ("2", Step::Choose) => self.then(Step::Import, window, cx),
            ("3", Step::Choose) => self.then(Step::Restore, window, cx),
            ("4", Step::Choose) => {
                self.offer(window, cx);
                true
            }
            ("w", Step::Overview) => self.then(Step::Reveal, window, cx),
            ("b", Step::Overview) => self.then(Step::Export, window, cx),
            ("a", Step::Overview) => self.then(Step::AddDevice, window, cx),
            _ => false,
        };
        if handled {
            cx.stop_propagation();
        }
    }

    fn then(&mut self, step: Step, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.go(step, window, cx);
        true
    }

    fn text(&self, s: impl Into<gpui::SharedString>) -> gpui::Div {
        div().text_size(px(12.)).text_color(rgb(self.theme.text_secondary)).child(s.into())
    }

    fn step_body(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = self.theme;
        let col = || div().flex().flex_col().gap(px(8.));
        let fields = self.model.fields();
        let inputs = div().flex().flex_col().gap(px(6.)).children(
            self.inputs
                .iter()
                .take(fields.len())
                .zip(fields)
                .map(|(input, f)| div().flex().flex_col().gap(px(2.)).child(heading(t, f.label)).child(div().flex().child(input.clone()))),
        );
        let main = |id: &'static str, label: &'static str| button(t, id, label, true).on_click(cx.listener(|this, _, w, cx| this.submit(w, cx)));
        let back = button(t, "identity-back", "Back", false).on_click(cx.listener(|this, _, w, cx| this.escape(w, cx)));
        let row = |a: gpui::AnyElement, b: gpui::AnyElement| div().flex().flex_row().gap(px(6.)).child(a).child(b);
        match &self.model.step {
            Step::Choose => col()
                .child(self.text("This profile has no Weirdnet identity yet. It signs what you share and moves with you to your other devices; creating one also encrypts this profile's bookmarks and history."))
                .child(button(t, "identity-create", "1  Create a new identity", true).on_click(cx.listener(|this, _, w, cx| this.go(Step::Create, w, cx))))
                .child(button(t, "identity-import", "2  Restore from recovery words", false).on_click(cx.listener(|this, _, w, cx| this.go(Step::Import, w, cx))))
                .child(button(t, "identity-restore", "3  Restore from a backup file", false).on_click(cx.listener(|this, _, w, cx| this.go(Step::Restore, w, cx))))
                .child(button(t, "identity-link", "4  Link from another device", false).on_click(cx.listener(|this, _, w, cx| this.offer(w, cx))))
                .into_any_element(),
            Step::Create | Step::Import | Step::Restore | Step::Received { .. } | Step::Reveal | Step::Export | Step::AddDevice => {
                let (intro, label): (&str, &'static str) = match self.model.step {
                    Step::Create => ("The password unlocks this identity on this device.", "Create"),
                    Step::Import => ("Type the 24 words you wrote down, in order.", "Restore"),
                    Step::Restore => ("A backup Morph saved, or a text file with the recovery words.", "Restore"),
                    Step::Received { .. } => ("Your identity arrived from the other device. Choose its password on this device.", "Finish"),
                    Step::Reveal => ("Your password shows the recovery words.", "Show"),
                    Step::Export => ("The backup holds the recovery words, sealed with its own password.", "Save backup"),
                    _ => ("On the new device, choose Link from another device and copy the link it shows here.", "Pair"),
                };
                col().child(self.text(intro)).child(inputs).child(row(main("identity-submit", label).into_any_element(), back.into_any_element())).into_any_element()
            }
            Step::Words(words) | Step::RevealWords(words) => col()
                .child(self.text("Write these 24 words down, in order, and keep them offline. Anyone with them has your identity."))
                .child(words_grid(t, words))
                .child(main("identity-done", "Done (Enter)"))
                .into_any_element(),
            Step::Offer { link, .. } => {
                let copy = link.clone();
                col()
                    .child(self.text("On your other device, open Identity, choose Add a device and scan this code or paste the link."))
                    .child(qr(link, t))
                    .child(div().text_size(px(11.)).text_color(rgb(t.text_secondary)).overflow_hidden().child(link.clone()))
                    .child(row(
                        button(t, "identity-copy", "Copy link", false)
                            .on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copy.clone())))
                            .into_any_element(),
                        back.into_any_element(),
                    ))
                    .child(self.text("Waiting for the other device…"))
                    .into_any_element()
            }
            Step::Paired { sas, .. } | Step::Adding { sas, .. } => {
                let device = match &self.model.step {
                    Step::Adding { device, .. } => format!("Pairing with {device}."),
                    _ => "Paired with your other device.".into(),
                };
                let label = if matches!(self.model.step, Step::Adding { .. }) { "They match: send (Enter)" } else { "They match (Enter)" };
                col()
                    .child(self.text(device))
                    .child(self.text("Check that both screens show the same code:"))
                    .child(div().text_size(px(28.)).text_color(rgb(t.accent)).child(sas.clone()))
                    .child(row(main("identity-match", label).into_any_element(), button(t, "identity-differ", "They differ", false).on_click(cx.listener(|this, _, w, cx| this.escape(w, cx))).into_any_element()))
                    .into_any_element()
            }
            Step::Overview => {
                let (user, finger, source) = self
                    .model
                    .identity
                    .as_ref()
                    .map(|i| (i.user_id.clone(), i.fingerprint.clone(), source_text(&i.source)))
                    .unwrap_or_default();
                col()
                    .child(heading(t, "User id"))
                    .child(div().text_size(px(12.)).overflow_hidden().child(user))
                    .child(heading(t, "Fingerprint"))
                    .child(div().text_size(px(12.)).child(finger))
                    .child(self.text(format!("This identity was {source}.")))
                    .child(button(t, "identity-words", "w  Show recovery words", false).on_click(cx.listener(|this, _, w, cx| this.go(Step::Reveal, w, cx))))
                    .child(button(t, "identity-backup", "b  Save a backup", false).on_click(cx.listener(|this, _, w, cx| this.go(Step::Export, w, cx))))
                    .child(button(t, "identity-add", "a  Add a device", false).on_click(cx.listener(|this, _, w, cx| this.go(Step::AddDevice, w, cx))))
                    .into_any_element()
            }
        }
    }
}

/// The recovery words, numbered, in two columns.
fn words_grid(t: Theme, words: &str) -> gpui::Div {
    let list: Vec<String> = words.split_whitespace().enumerate().map(|(i, w)| format!("{:>2}. {w}", i + 1)).collect();
    let half = list.len().div_ceil(2);
    let column = |items: &[String]| div().flex().flex_col().gap(px(2.)).children(items.iter().map(|w| div().text_size(px(13.)).child(w.clone())));
    div()
        .flex()
        .flex_row()
        .gap(px(24.))
        .p(px(8.))
        .rounded(px(6.))
        .bg(rgb(t.background))
        .font_family("monospace")
        .child(column(&list[..half]))
        .child(column(&list[half..]))
}

/// `text` as a QR code, dark modules on white with a quiet zone.
fn qr(text: &str, t: Theme) -> gpui::AnyElement {
    let Ok(code) = qrcode::QrCode::new(text.as_bytes()) else {
        return div().text_color(rgb(t.error)).child("The link is too long for a QR code; copy it instead.").into_any_element();
    };
    let width = code.width();
    let dark: Vec<bool> = code.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect();
    let side = 220.;
    div()
        .w(px(side))
        .h(px(side))
        .bg(rgb(0xffffff))
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let quiet = 4;
                    let cells = (width + 2 * quiet) as f32;
                    let cell = f32::from(bounds.size.width) / cells;
                    for (i, on) in dark.iter().enumerate() {
                        if *on {
                            let (x, y) = ((i % width + quiet) as f32, (i / width + quiet) as f32);
                            let origin = point(bounds.origin.x + px(x * cell), bounds.origin.y + px(y * cell));
                            window.paint_quad(fill(gpui::Bounds::new(origin, size(px(cell.ceil()), px(cell.ceil()))), rgb(0x000000)));
                        }
                    }
                },
            )
            .size_full(),
        )
        .into_any_element()
}

impl Focusable for IdentityView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for IdentityView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .id("identity-panel")
            .size_full()
            .flex()
            .flex_col()
            .gap(px(10.))
            .p(px(10.))
            .bg(rgb(t.surface))
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(div().flex_1().text_size(px(13.)).child("Identity and devices"))
                    .child(icon(t, "identity-close", "×").on_click(cx.listener(|_, _, _, cx| cx.emit(ShellRequest::ClosePanel)))),
            )
            .child(self.step_body(cx))
            .children(self.model.notice.clone().map(|n| div().text_size(px(12.)).text_color(rgb(t.accent)).child(n)))
            .children(self.model.error.clone().map(|e| div().text_size(px(12.)).text_color(rgb(t.error)).child(e)))
    }
}
