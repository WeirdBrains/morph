//! The identity panel (`morph.identity`, Ctrl+Shift+L): a profile's Weirdnet
//! identity, its recovery words and backups, and moving it between devices
//! with a QR link. The steps live in [`crate::identity`]; this draws them.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::{
    App, ClipboardItem, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyDownEvent, Subscription, Task, Window, canvas, div, fill, point,
    prelude::*, px, rgb, size,
};
use morph_services::device_link::Links;
use morph_ui::theme::Theme;

use crate::dialogs::{self, Ask};
use crate::identity::{Helper, Identity, Outcome, Secret, Step, join_paths, source_text};
use crate::shell::ShellRequest;
use crate::text_input::{TextInput, TextInputEvent};
use crate::widgets::{button, heading, icon};
use morph_identity::profile_identity::Zeroizing;

const INPUTS: usize = 3;

/// How long a copied secret stays on the clipboard.
pub const CLIPBOARD_TIME: Duration = Duration::from_secs(60);

pub struct IdentityView {
    model: Identity,
    theme: Theme,
    inputs: Vec<Entity<TextInput>>,
    focus: FocusHandle,
    /// A secret put on the clipboard, cleared after [`CLIPBOARD_TIME`] if
    /// the clipboard still holds it.
    copied: Option<(Zeroizing<String>, Instant)>,
    /// The shard shown as a QR code.
    qr_shard: Option<usize>,
    /// An open file dialog.
    dialog: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ShellRequest> for IdentityView {}

impl IdentityView {
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        for input in &self.inputs {
            input.update(cx, |i, cx| i.set_theme(theme, cx));
        }
        cx.notify();
    }

    pub fn new(home: &Path, profile: &str, profile_name: &str, theme: Theme, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let inputs: Vec<Entity<TextInput>> = (0..INPUTS).map(|_| cx.new(|cx| TextInput::new("", theme, cx))).collect();
        let subs = inputs
            .iter()
            .enumerate()
            .map(|(i, input)| {
                cx.subscribe_in(input, window, move |this, _, event, window, cx| match event {
                    TextInputEvent::Submit { .. } => this.field_submitted(i, window, cx),
                    TextInputEvent::Cancel => this.escape(window, cx),
                    TextInputEvent::Tab { back } => this.tab(i, *back, window, cx),
                    TextInputEvent::Changed => {}
                })
            })
            .collect();
        Self {
            model: Identity::new(home, profile, profile_name, Links::global()),
            theme,
            inputs,
            focus: cx.focus_handle(),
            copied: None,
            qr_shard: None,
            dialog: None,
            _subscriptions: subs,
        }
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
        } else if self.model.take_dirty() {
            cx.notify();
        }
        if self.copied.as_ref().is_some_and(|(_, at)| at.elapsed() >= clipboard_time()) {
            self.clear_clipboard(cx);
        }
    }

    /// Take a copied secret off the clipboard, unless something else
    /// replaced it since.
    fn clear_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some((secret, _)) = self.copied.take() else {
            return;
        };
        let current = cx.read_from_clipboard().and_then(|item| item.text()).map(Zeroizing::new);
        if current.as_deref().is_some_and(|c| c.as_str() == secret.as_str()) {
            cx.write_to_clipboard(ClipboardItem::new_string(String::new()));
            log_event("clipboard cleared");
        }
    }

    /// Put `secret` on the clipboard for a minute.
    fn copy_secret(&mut self, secret: Secret, cx: &mut Context<Self>) {
        let Some(text) = self.model.secret_text(secret) else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
        self.copied = Some((text, Instant::now()));
        self.model.error = None;
        self.model.notice = Some(format!("Copied. Morph clears the clipboard in {} seconds.", clipboard_time().as_secs()));
        log_event(match secret {
            Secret::Words => "copied words".to_string(),
            Secret::Shard(n) => format!("copied shard {}", n + 1),
        });
        cx.notify();
    }

    /// Save `secret` where a Save dialog says.
    fn download(&mut self, secret: Secret, cx: &mut Context<Self>) {
        let answer = dialogs::ask(cx, Ask::Save { name: self.model.file_name(secret) });
        self.dialog = Some(cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(paths) => {
                        if let Some(path) = paths.first() {
                            this.model.save_secret(secret, path);
                            log_event(format!("saved {}", path.display()));
                        }
                    }
                    Err(e) => this.model.error = Some(e),
                }
                cx.notify();
            });
        }));
    }

    /// Save every shard into a folder a dialog picks.
    fn download_all(&mut self, cx: &mut Context<Self>) {
        let answer = dialogs::ask(cx, Ask::Folder);
        self.dialog = Some(cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(paths) => {
                        if let Some(dir) = paths.first() {
                            this.model.save_all_shards(dir);
                            log_event(format!("saved shards in {}", dir.display()));
                        }
                    }
                    Err(e) => this.model.error = Some(e),
                }
                cx.notify();
            });
        }));
    }

    /// Fill field `i` from its helper: the clipboard, or a native dialog.
    fn helper(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(field) = self.model.fields().get(i).copied() else {
            return;
        };
        let input = self.inputs[i].clone();
        window.focus(&input.focus_handle(cx), cx);
        let ask = match field.helper {
            Helper::None => return,
            Helper::Paste => {
                let text = cx.read_from_clipboard().and_then(|item| item.text()).map(Zeroizing::new).unwrap_or_default();
                input.update(cx, |input, cx| input.set_text(text.trim(), cx));
                return;
            }
            Helper::Open => Ask::Open { multiple: true },
            Helper::Save => {
                let typed = input.read(cx).text().to_string();
                let name = PathBuf::from(typed.trim())
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "morph-identity-backup.txt".into());
                Ask::Save { name }
            }
        };
        let answer = dialogs::ask(cx, ask);
        self.dialog = Some(cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(paths) if !paths.is_empty() => input.update(cx, |input, cx| input.set_text(&join_paths(&paths), cx)),
                    Ok(_) => {}
                    Err(e) => this.model.error = Some(e),
                }
                cx.notify();
            });
        }));
    }

    /// Tab and Shift+Tab move between the step's fields, wrapping.
    fn tab(&mut self, i: usize, back: bool, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.model.fields().len();
        if n == 0 {
            return;
        }
        let next = if back { (i + n - 1) % n } else { (i + 1) % n };
        window.focus(&self.inputs[next].focus_handle(cx), cx);
        cx.notify();
    }

    /// Set the inputs up for the step and put the keyboard in the first one
    /// (or on the panel, whose keys pick an action).
    fn enter_step(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.qr_shard = None;
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
            ("enter", Step::Words(_) | Step::RevealWords(_) | Step::ShardList(_) | Step::Paired { .. } | Step::Adding { .. }) => {
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
            ("s", Step::Overview) => self.then(Step::Shards, window, cx),
            ("c", Step::Words(_) | Step::RevealWords(_)) => {
                self.copy_secret(Secret::Words, cx);
                true
            }
            ("d", Step::Words(_) | Step::RevealWords(_)) => {
                self.download(Secret::Words, cx);
                true
            }
            ("a", Step::ShardList(_)) => {
                self.download_all(cx);
                true
            }
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
        let inputs =
            div().flex().flex_col().gap(px(6.)).children(self.inputs.iter().take(fields.len()).zip(fields).enumerate().map(|(i, (input, f))| {
                let label = match f.helper {
                    Helper::None => None,
                    Helper::Paste => Some("Paste from clipboard"),
                    Helper::Open => Some("Choose files…"),
                    Helper::Save => Some("Choose…"),
                };
                let helper = label.map(|label| {
                    button(t, ("identity-helper", i), label, false).flex_none().on_click(cx.listener(move |this, _, w, cx| this.helper(i, w, cx)))
                });
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(heading(t, f.label))
                    .child(div().flex().flex_row().gap(px(6.)).child(div().flex_1().flex().child(input.clone())).children(helper))
            }));
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
            Step::AddDevice => {
                let mut body = col();
                if let Some(link) = self.model.invite_link() {
                    let copy = link.to_string();
                    body = body
                        .child(self.text("On the new device, open Identity, choose Link from another device and scan this code, or paste this link there."))
                        .child(qr(link, t))
                        .child(div().text_size(px(11.)).text_color(rgb(t.text_secondary)).overflow_hidden().child(link.to_string()))
                        .child(
                            button(t, "identity-copy", "Copy link", false)
                                .on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))),
                        );
                }
                let intro = if self.model.answered() {
                    "The new device answered. Type this profile's password and press Pair; both screens then show the same code."
                } else {
                    "Or, when the new device shows a code of its own, paste its link below. Your password unlocks the identity to send."
                };
                body.child(self.text(intro)).child(inputs).child(row(main("identity-submit", "Pair").into_any_element(), back.into_any_element())).into_any_element()
            }
            Step::Create | Step::Import | Step::Restore | Step::Received { .. } | Step::Reveal | Step::Shards | Step::Export => {
                let (intro, label): (&str, &'static str) = match self.model.step {
                    Step::Create => ("The password unlocks this identity on this device.", "Create"),
                    Step::Import => ("Type the 24 words you wrote down, in order.", "Restore"),
                    Step::Restore => ("A backup Morph saved, or a text file with the recovery words.", "Restore"),
                    Step::Received { .. } => ("Your identity arrived from the other device. Choose its password on this device.", "Finish"),
                    Step::Reveal => ("Your password shows the recovery words.", "Show"),
                    Step::Shards => (
                        "Split the identity into 5 shards, any 3 of which bring it back; one alone tells nothing. Give them to people or places you trust. Your password makes them.",
                        "Make shards",
                    ),
                    Step::Export => ("The backup holds the recovery words, sealed with its own password.", "Save backup"),
                    _ => ("", ""),
                };
                col().child(self.text(intro)).child(inputs).child(row(main("identity-submit", label).into_any_element(), back.into_any_element())).into_any_element()
            }
            Step::Words(words) | Step::RevealWords(words) => col()
                .child(self.text("Write these 24 words down, in order, and keep them offline. Anyone with them has your identity."))
                .child(words_grid(t, words))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(6.))
                        .child(button(t, "identity-words-copy", "c  Copy", false).on_click(cx.listener(|this, _, _, cx| this.copy_secret(Secret::Words, cx))))
                        .child(button(t, "identity-words-download", "d  Download…", false).on_click(cx.listener(|this, _, _, cx| this.download(Secret::Words, cx))))
                        .child(main("identity-done", "Done (Enter)")),
                )
                .child(self.text("A copy leaves the clipboard after a minute. A saved file belongs offline: print it, then delete it."))
                .into_any_element(),
            Step::ShardList(shards) => {
                let total = shards.len();
                let need = crate::identity::SHARD_THRESHOLD;
                col()
                    .child(self.text(format!("Any {need} of these {total} shards bring the identity back; one alone tells nothing. Give each to a different person or place, and keep them offline.")))
                    .children(shards.iter().enumerate().map(|(n, shard)| {
                        let qr_open = self.qr_shard == Some(n);
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .p(px(8.))
                            .rounded(px(6.))
                            .bg(rgb(t.background))
                            .child(heading(t, format!("Shard {} of {total} (need {need})", n + 1)))
                            .child(div().text_size(px(11.)).font_family("monospace").overflow_hidden().child(shard.clone()))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap(px(6.))
                                    .child(button(t, ("shard-copy", n), "Copy", false).on_click(cx.listener(move |this, _, _, cx| this.copy_secret(Secret::Shard(n), cx))))
                                    .child(button(t, ("shard-download", n), "Download…", false).on_click(cx.listener(move |this, _, _, cx| this.download(Secret::Shard(n), cx))))
                                    .child(button(t, ("shard-qr", n), if qr_open { "Hide QR" } else { "QR" }, false).on_click(cx.listener(move |this, _, _, cx| {
                                        this.qr_shard = if this.qr_shard == Some(n) { None } else { Some(n) };
                                        cx.notify();
                                    }))),
                            )
                            .children(qr_open.then(|| qr(shard, t)))
                    }))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(6.))
                            .child(button(t, "shards-download-all", "a  Download all shards…", false).on_click(cx.listener(|this, _, _, cx| this.download_all(cx))))
                            .child(main("identity-done", "Done (Enter)")),
                    )
                    .into_any_element()
            }
            Step::Offer { link, .. } => {
                let copy = link.clone();
                col()
                    .child(self.text("On your other device, open Identity, choose Add a device and scan this code or paste the link."))
                    .child(qr(link, t))
                    .child(div().text_size(px(11.)).text_color(rgb(t.text_secondary)).overflow_hidden().child(link.clone()))
                    .child(
                        button(t, "identity-copy", "Copy link", false).on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))),
                    )
                    .child(self.text("Or, when your other device shows a code under Add a device, paste its link here:"))
                    .child(inputs)
                    .child(row(main("identity-submit", "Send").into_any_element(), back.into_any_element()))
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
                    .child(button(t, "identity-shards", "s  Split into shards", false).on_click(cx.listener(|this, _, w, cx| this.go(Step::Shards, w, cx))))
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
    let Some((width, dark)) = qr_modules(text) else {
        return div().text_color(rgb(t.error)).child("Too long for a QR code; copy it instead.").into_any_element();
    };
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

/// [`CLIPBOARD_TIME`], or `MORPH_TEST_CLIPBOARD_SECS` under the checks.
fn clipboard_time() -> Duration {
    std::env::var("MORPH_TEST_CLIPBOARD_SECS").ok().and_then(|s| s.parse().ok()).map(Duration::from_secs).unwrap_or(CLIPBOARD_TIME)
}

/// `text` as QR modules: the side and each module, dark or not, row by row.
pub fn qr_modules(text: &str) -> Option<(usize, Vec<bool>)> {
    let code = qrcode::QrCode::new(text.as_bytes()).ok()?;
    Some((code.width(), code.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect()))
}

/// What happened, for the checks (`MORPH_LOG_EVENTS=1`). Never a secret.
fn log_event(what: impl AsRef<str>) {
    if std::env::var_os("MORPH_LOG_EVENTS").is_some_and(|v| v == "1") {
        println!("morph-event identity {}", what.as_ref());
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Draw the modules as a scanner would see them and read them back.
    fn decode(text: &str) -> String {
        let (width, dark) = qr_modules(text).unwrap();
        let (quiet, scale) = (4, 4);
        let side = (width + 2 * quiet) * scale;
        let mut img = rqrr::PreparedImage::prepare_from_greyscale(side, side, |x, y| {
            let (mx, my) = (x / scale, y / scale);
            let on = mx >= quiet && my >= quiet && mx < width + quiet && my < width + quiet && dark[(my - quiet) * width + (mx - quiet)];
            if on { 0 } else { 255 }
        });
        let grids = img.detect_grids();
        assert_eq!(grids.len(), 1);
        grids[0].decode().unwrap().1
    }

    #[test]
    fn the_link_and_shard_codes_decode_to_their_text() {
        let link = "weirdnet-link:v1:12D3KooWExamplePeerId0000000000000000000000000000:/ip4/192.168.1.20/tcp/41234:abcdef0123456789abcdef0123456789";
        assert_eq!(decode(link), link);
        let shard = "weirdnet-shard-v1:0a1b2c3d:3:2:02aabbccddeeff00112233445566778899aabbccddeeff0011223344556677889900:1234abcd";
        assert_eq!(decode(shard), shard);
    }
}
