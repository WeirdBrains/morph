//! Settings > Agents > Connected servers: the MCP servers the companions can use. Separate from
//! Morph's own MCP server row. Add, edit and remove a server (a local command with arguments and
//! environment, or a URL with a bearer token and headers), switch it and each of its tools on and
//! off, and see why a server did not connect.
//!
//! Secret values are masked with an eye toggle, kept on this machine only ([`crate::mcp_host`]), and
//! a local command starts only after the user read its warning and allowed it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use gpui::{Context, Entity, SharedString, div, prelude::*, px, rgb};
use morph_ai::mcp::looks_secret;
use morph_ai::mcp_runtime::{ServerStatus, warning_for};
use morph_ai::{McpSecrets, McpServer, McpTransport, McpValue};

use super::SettingsView;
use crate::mcp_host;
use crate::text_input::TextInput;
use crate::widgets::{button, chip};

/// Splits an argument line: spaces separate, `"double"` or `'single'` quotes keep spaces.
pub fn split_args(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    for c in line.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if started || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The inverse of [`split_args`] for showing saved arguments in one field.
pub fn join_args(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            if a.is_empty() || a.contains(char::is_whitespace) || a.contains(['"', '\'']) {
                if a.contains('"') { format!("'{a}'") } else { format!("\"{a}\"") }
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// One environment variable or header being edited.
struct Row {
    name: Entity<TextInput>,
    value: Entity<TextInput>,
    /// Kept as a secret (masked, stored on this machine only).
    secret: bool,
    /// The eye is open: the value shows.
    shown: bool,
}

/// The add/edit form.
pub struct Form {
    /// The server being edited; `None` when adding.
    id: Option<String>,
    http: bool,
    name: Entity<TextInput>,
    command: Entity<TextInput>,
    args: Entity<TextInput>,
    url: Entity<TextInput>,
    bearer: Entity<TextInput>,
    bearer_shown: bool,
    rows: Vec<Row>,
}

/// What the page keeps between frames.
#[derive(Default)]
pub struct State {
    form: Option<Form>,
    /// The server whose start the user is being asked to allow.
    confirm: Option<String>,
    /// Servers whose tool list is open.
    tools_open: BTreeSet<String>,
    /// The last connect round the page drew.
    seen_generation: u64,
    pub note: Option<String>,
}

/// A text input with a hint, optionally masked.
fn input(text: &str, hint: &str, masked: bool, theme: morph_ui::theme::Theme, cx: &mut Context<SettingsView>) -> Entity<TextInput> {
    let hint = hint.to_string();
    let e = cx.new(|cx| {
        let t = TextInput::new(hint, theme, cx);
        if masked { t.masked() } else { t }
    });
    if !text.is_empty() {
        e.update(cx, |i, cx| i.set_text(text, cx));
    }
    e
}

fn set_masked(e: &Entity<TextInput>, hint: &str, masked: bool, cx: &mut Context<SettingsView>) {
    let hint = hint.to_string();
    e.update(cx, |i, cx| i.configure(hint, masked, cx));
}

/// A summary line for the list: the command line or the URL's host.
pub fn target_summary(s: &McpServer) -> String {
    match &s.transport {
        McpTransport::Stdio { command, args, .. } => format!("Command: {} {}", command, join_args(args)).trim_end().to_string(),
        McpTransport::Http { url, .. } => format!("URL: {url}"),
    }
}

impl SettingsView {
    fn servers_dir(&self) -> Option<std::path::PathBuf> {
        self.agents_dir()
    }

    fn servers_changed(&mut self, dir: &Path, cx: &mut Context<Self>) {
        mcp_host::get(dir).reload();
        cx.notify();
    }

    /// Run a change, reconnect, and show what went wrong.
    fn servers_apply(&mut self, change: impl FnOnce(&Path, &mcp_host::LocalSecrets) -> Result<(), String>, cx: &mut Context<Self>) -> bool {
        let Some(dir) = self.servers_dir() else { return false };
        let manager = mcp_host::get(&dir);
        let ok = match change(&dir, manager.secrets()) {
            Ok(()) => {
                self.servers.note = None;
                true
            }
            Err(e) => {
                self.servers.note = Some(e);
                false
            }
        };
        self.servers_changed(&dir, cx);
        ok
    }

    /// A tick: redraw when a connect round finished.
    pub(super) fn servers_poll(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = self.servers_dir() else { return };
        let Some(manager) = mcp_host::peek(&dir) else { return };
        let snap = manager.snapshot();
        if snap.generation != self.servers.seen_generation || snap.busy {
            self.servers.seen_generation = snap.generation;
            cx.notify();
        }
    }

    pub(super) fn servers_reload(&mut self) {
        self.servers = State::default();
    }

    fn servers_open_form(&mut self, existing: Option<&McpServer>, secrets: &dyn McpSecrets, cx: &mut Context<Self>) {
        let t = self.theme;
        let mut http = false;
        let (mut name, mut command, mut args, mut url, mut bearer) = (String::new(), String::new(), String::new(), String::new(), String::new());
        let mut rows = Vec::new();
        let row = |n: &str, v: &McpValue, id: &str, cx: &mut Context<Self>| {
            let (value, secret) = match v {
                McpValue::Plain { value } => (value.clone(), looks_secret(n)),
                McpValue::Secret { name } => (secrets.get(id, name).unwrap_or_default(), true),
            };
            Row { name: input(n, "NAME", false, t, cx), value: input(&value, "value", secret, t, cx), secret, shown: false }
        };
        if let Some(s) = existing {
            name = s.name.clone();
            match &s.transport {
                McpTransport::Stdio { command: c, args: a, env } => {
                    command = c.clone();
                    args = join_args(a);
                    for (k, v) in env {
                        rows.push(row(k, v, &s.id, cx));
                    }
                }
                McpTransport::Http { url: u, headers } => {
                    http = true;
                    url = u.clone();
                    for (k, v) in headers {
                        if k.eq_ignore_ascii_case("authorization") {
                            let full = match v {
                                McpValue::Plain { value } => value.clone(),
                                McpValue::Secret { name } => secrets.get(&s.id, name).unwrap_or_default(),
                            };
                            bearer = full.strip_prefix("Bearer ").unwrap_or(&full).to_string();
                        } else {
                            rows.push(row(k, v, &s.id, cx));
                        }
                    }
                }
            }
        }
        self.servers.form = Some(Form {
            id: existing.map(|s| s.id.clone()),
            http,
            name: input(&name, "Name, e.g. Docs", false, t, cx),
            command: input(&command, "Program, e.g. npx", false, t, cx),
            args: input(&args, "Arguments, e.g. -y some-mcp-server", false, t, cx),
            url: input(&url, "https://example.com/mcp", false, t, cx),
            bearer: input(&bearer, "Bearer token (optional)", true, t, cx),
            bearer_shown: false,
            rows,
        });
        cx.notify();
    }

    fn servers_add_row(&mut self, cx: &mut Context<Self>) {
        let t = self.theme;
        let row = Row { name: input("", "NAME", false, t, cx), value: input("", "value", false, t, cx), secret: false, shown: false };
        if let Some(f) = &mut self.servers.form {
            f.rows.push(row);
        }
        cx.notify();
    }

    fn servers_toggle_row_secret(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(r) = self.servers.form.as_mut().and_then(|f| f.rows.get_mut(i)) {
            r.secret = !r.secret;
            r.shown = false;
            set_masked(&r.value, "value", r.secret, cx);
        }
        cx.notify();
    }

    fn servers_toggle_row_eye(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(r) = self.servers.form.as_mut().and_then(|f| f.rows.get_mut(i)) {
            r.shown = !r.shown;
            set_masked(&r.value, "value", r.secret && !r.shown, cx);
        }
        cx.notify();
    }

    fn servers_toggle_bearer_eye(&mut self, cx: &mut Context<Self>) {
        if let Some(f) = &mut self.servers.form {
            f.bearer_shown = !f.bearer_shown;
            set_masked(&f.bearer, "Bearer token (optional)", !f.bearer_shown, cx);
        }
        cx.notify();
    }

    /// Save the form: build the server, store its secrets here and its config in `mcp.json`.
    fn servers_save(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = self.servers_dir() else { return };
        let Some(form) = &self.servers.form else { return };
        let existing = form.id.as_ref().and_then(|id| mcp_host::configs(&dir).servers.into_iter().find(|s| s.id == *id));
        let text = |e: &Entity<TextInput>| e.read(cx).text().trim().to_string();
        let mut values: BTreeMap<String, McpValue> = BTreeMap::new();
        let mut new_secrets: Vec<(String, String)> = Vec::new();
        for r in &form.rows {
            let (name, value) = (text(&r.name), r.value.read(cx).text().to_string());
            if name.is_empty() {
                continue;
            }
            if r.secret || looks_secret(&name) {
                if !value.is_empty() {
                    new_secrets.push((name.clone(), value));
                }
                values.insert(name.clone(), McpValue::Secret { name });
            } else {
                values.insert(name, mcp_host::plain(&value));
            }
        }
        let transport = if form.http {
            let token = text(&form.bearer);
            if !token.is_empty() {
                new_secrets.push(("Authorization".into(), format!("Bearer {token}")));
                values.insert("Authorization".into(), McpValue::Secret { name: "Authorization".into() });
            }
            McpTransport::Http { url: text(&form.url), headers: values }
        } else {
            McpTransport::Stdio { command: text(&form.command), args: split_args(form.args.read(cx).text()), env: values }
        };
        let mut server = match existing {
            Some(mut s) => {
                s.transport = transport;
                s
            }
            None => McpServer::new("", transport),
        };
        server.name = text(&form.name);
        let id = server.id.clone();
        let needs_ok = !mcp_host::is_approved(&dir, &server);
        let saved = server.clone();
        if self.servers_apply(move |d, secrets| mcp_host::save_server(d, secrets, server, &new_secrets), cx) {
            self.servers.form = None;
            self.servers.confirm = (needs_ok && saved.enabled).then_some(id);
            cx.notify();
        }
    }

    /// The first thing a local command shows the user: what would run, and the choice.
    fn confirm_card(&self, s: &McpServer, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = self.theme;
        let id = s.id.clone();
        let id2 = s.id.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .px(px(10.))
            .py(px(8.))
            .rounded(px(6.))
            .border_1()
            .border_color(rgb(t.error))
            .child(div().text_size(px(13.)).child(format!("Allow {} to start?", s.name)))
            .child(div().text_size(px(12.)).text_color(rgb(t.text_secondary)).child(warning_for(s).unwrap_or_default()))
            .child(div().text_size(px(12.)).text_color(rgb(t.text_secondary)).child(
                "Only allow commands you trust. Morph asks again if the command or its arguments change. Environment values are never shown here.",
            ))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(8.))
                    .child(button(t, ("srv-allow", 0usize), "Allow and start", true).on_click(cx.listener(move |this, _, _, cx| {
                        let id = id.clone();
                        this.servers.confirm = None;
                        this.servers_apply(move |d, _| mcp_host::approve(d, &id), cx);
                    })))
                    .child(button(t, ("srv-allow", 1usize), "Not now", false).on_click(cx.listener(move |this, _, _, cx| {
                        if this.servers.confirm.as_deref() == Some(id2.as_str()) {
                            this.servers.confirm = None;
                        }
                        cx.notify();
                    }))),
            )
            .into_any_element()
    }

    fn row_view(&self, i: usize, r: &Row, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = self.theme;
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.))
            .child(div().w(px(150.)).child(r.name.clone()))
            .child(div().flex_1().min_w_0().child(r.value.clone()))
            .child(
                chip(t, ("srv-row-secret", i), "Secret", r.secret).on_click(cx.listener(move |this, _, _, cx| this.servers_toggle_row_secret(i, cx))),
            )
            .children(r.secret.then(|| {
                chip(t, ("srv-row-eye", i), if r.shown { "👁 Hide" } else { "👁 Show" }, r.shown)
                    .on_click(cx.listener(move |this, _, _, cx| this.servers_toggle_row_eye(i, cx)))
            }))
            .child(chip(t, ("srv-row-del", i), "Remove", false).on_click(cx.listener(move |this, _, _, cx| {
                if let Some(f) = &mut this.servers.form
                    && i < f.rows.len()
                {
                    f.rows.remove(i);
                }
                cx.notify();
            })))
            .into_any_element()
    }

    fn form_view(&self, f: &Form, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = self.theme;
        let dim = |s: &str| div().text_size(px(12.)).text_color(rgb(t.text_secondary)).child(s.to_string());
        let title = if f.id.is_some() { "Edit server" } else { "Add a server" };
        let mut card = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .px(px(10.))
            .py(px(8.))
            .rounded(px(6.))
            .border_1()
            .border_color(rgb(t.accent))
            .child(div().text_size(px(13.)).child(title))
            .child(dim("Name"))
            .child(div().w(px(260.)).child(f.name.clone()))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(6.))
                    .child(chip(t, ("srv-kind", 0usize), "Local command", !f.http).on_click(cx.listener(|this, _, _, cx| {
                        if let Some(f) = &mut this.servers.form {
                            f.http = false;
                        }
                        cx.notify();
                    })))
                    .child(chip(t, ("srv-kind", 1usize), "URL (streamable HTTP)", f.http).on_click(cx.listener(|this, _, _, cx| {
                        if let Some(f) = &mut this.servers.form {
                            f.http = true;
                        }
                        cx.notify();
                    }))),
            );
        card = if f.http {
            card.child(dim("URL")).child(div().w(px(420.)).child(f.url.clone())).child(dim("Bearer token")).child(
                div().flex().flex_row().items_center().gap(px(6.)).child(div().w(px(320.)).child(f.bearer.clone())).child(
                    chip(t, "srv-bearer-eye", if f.bearer_shown { "👁 Hide" } else { "👁 Show" }, f.bearer_shown)
                        .on_click(cx.listener(|this, _, _, cx| this.servers_toggle_bearer_eye(cx))),
                ),
            )
        } else {
            card.child(dim("Command and arguments"))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(6.))
                        .child(div().w(px(140.)).child(f.command.clone()))
                        .child(div().flex_1().min_w_0().child(f.args.clone())),
                )
                .child(dim("A local command runs with your permissions. Morph shows its warning and asks before the first start."))
        };
        card = card.child(dim(if f.http { "Extra headers" } else { "Environment variables" }));
        for (i, r) in f.rows.iter().enumerate() {
            card = card.child(self.row_view(i, r, cx));
        }
        card.child(
            div()
                .flex()
                .flex_row()
                .gap(px(6.))
                .child(button(t, "srv-add-row", "+ Add", false).on_click(cx.listener(|this, _, _, cx| this.servers_add_row(cx)))),
        )
        .child(dim("Secret values stay on this device: they are never saved in the synced settings."))
        .child(
            div()
                .flex()
                .flex_row()
                .gap(px(8.))
                .child(button(t, "srv-save", "Save", true).on_click(cx.listener(|this, _, _, cx| this.servers_save(cx))))
                .child(button(t, "srv-cancel", "Cancel", false).on_click(cx.listener(|this, _, _, cx| {
                    this.servers.form = None;
                    cx.notify();
                }))),
        )
        .into_any_element()
    }

    fn server_view(&self, i: usize, s: &McpServer, status: Option<&ServerStatus>, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = self.theme;
        let dim = |s: String| div().text_size(px(12.)).text_color(rgb(t.text_secondary)).child(s);
        let bad = matches!(status, Some(ServerStatus::Failed(_) | ServerStatus::MissingSecrets(_)));
        let line = match status {
            Some(st) => st.summary(),
            None => "Connecting…".to_string(),
        };
        let id = s.id.clone();
        let (id_edit, id_remove, id_toggle, id_tools, id_review) = (id.clone(), id.clone(), id.clone(), id.clone(), id.clone());
        let enabled = s.enabled;
        let mut card = div().flex().flex_col().gap(px(4.)).px(px(10.)).py(px(8.)).rounded(px(6.)).bg(rgb(t.surface_variant));
        card = card.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .child(div().flex_1().min_w_0().flex().flex_col().child(div().text_size(px(13.)).child(s.name.clone())).child(dim(target_summary(s))))
                .child(chip(t, ("srv-on", i), if enabled { "On" } else { "Off" }, enabled).on_click(cx.listener(move |this, _, _, cx| {
                    let id = id_toggle.clone();
                    this.servers_apply(move |d, _| mcp_host::set_enabled(d, &id, !enabled), cx);
                })))
                .child(button(t, ("srv-edit", i), "Edit", false).on_click(cx.listener(move |this, _, _, cx| {
                    let Some(dir) = this.servers_dir() else { return };
                    let manager = mcp_host::get(&dir);
                    if let Some(s) = mcp_host::configs(&dir).servers.into_iter().find(|s| s.id == id_edit) {
                        this.servers_open_form(Some(&s), &**manager.secrets(), cx);
                    }
                })))
                .child(
                    div()
                        .id(("srv-remove", i))
                        .px(px(10.))
                        .h(px(26.))
                        .flex()
                        .items_center()
                        .rounded(px(6.))
                        .border_1()
                        .border_color(rgb(t.error))
                        .text_size(px(12.))
                        .text_color(rgb(t.error))
                        .cursor_pointer()
                        .child("Remove")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let id = id_remove.clone();
                            this.servers_apply(move |d, secrets| mcp_host::remove_server(d, secrets, &id), cx);
                        })),
                ),
        );
        let mut status_row = div().flex().flex_row().items_center().gap(px(8.)).child(
            div().flex_1().min_w_0().text_size(px(12.)).text_color(rgb(if bad { t.error } else { t.text_secondary })).child(SharedString::from(line)),
        );
        if matches!(status, Some(ServerStatus::NeedsApproval { .. })) {
            status_row = status_row.child(button(t, ("srv-review", i), "Review and allow", true).on_click(cx.listener(move |this, _, _, cx| {
                this.servers.confirm = Some(id_review.clone());
                cx.notify();
            })));
        }
        if let Some(ServerStatus::Connected { tools, .. }) = status {
            let open = self.servers.tools_open.contains(&s.id);
            status_row = status_row.child(chip(t, ("srv-tools", i), if open { "Hide tools" } else { "Tools" }, open).on_click(cx.listener(
                move |this, _, _, cx| {
                    if !this.servers.tools_open.remove(&id_tools) {
                        this.servers.tools_open.insert(id_tools.clone());
                    }
                    cx.notify();
                },
            )));
            card = card.child(status_row);
            if open {
                for (j, tool) in tools.iter().enumerate() {
                    let (id, name, on) = (s.id.clone(), tool.name.clone(), tool.enabled);
                    let desc: String = tool.description.chars().take(90).collect();
                    card = card.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .pl(px(8.))
                            .child(chip(t, ("srv-tool", i * 1000 + j), if on { "On" } else { "Off" }, on).on_click(cx.listener(
                                move |this, _, _, cx| {
                                    let (id, name) = (id.clone(), name.clone());
                                    this.servers_apply(move |d, _| mcp_host::set_tool(d, &id, &name, !on), cx);
                                },
                            )))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .child(div().text_size(px(12.)).child(format!("{} · {:?}", tool.name, tool.tier)))
                                    .child(dim(desc)),
                            ),
                    );
                }
            }
        } else {
            card = card.child(status_row);
        }
        card.into_any_element()
    }

    /// The Connected servers card for the Agents page.
    pub(super) fn servers_rows(&mut self, dir: &Path, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let t = self.theme;
        let manager = mcp_host::get(dir);
        let snap = manager.snapshot();
        self.servers.seen_generation = snap.generation;
        let servers = mcp_host::configs(dir).servers;
        let mut card = div().flex().flex_col().gap(px(8.)).px(px(12.)).py(px(10.)).rounded(px(8.)).bg(rgb(t.surface)).child(
            div().flex().flex_row().items_center().gap(px(8.)).child(div().flex_1().text_size(px(14.)).child("Connected servers")).child(
                button(t, "srv-add", "Add server", false).on_click(cx.listener(|this, _, _, cx| {
                    let Some(dir) = this.servers_dir() else { return };
                    let manager = mcp_host::get(&dir);
                    this.servers_open_form(None, &**manager.secrets(), cx);
                })),
            ),
        );
        card = card.child(
            div().text_size(px(12.)).text_color(rgb(t.text_secondary)).child(
                "MCP servers whose tools your companions can use, in addition to their own. A server's tools run at the tier its hints give, never below Research, so each use asks first unless you allowed it. Local commands run with your permissions. Not Morph's own MCP server (below).",
            ),
        );
        if servers.is_empty() && self.servers.form.is_none() {
            card = card.child(div().text_size(px(12.)).text_color(rgb(t.text_secondary)).child("None yet."));
        }
        for (i, s) in servers.iter().enumerate() {
            card = card.child(self.server_view(i, s, snap.statuses.get(&s.id), cx));
            if self.servers.confirm.as_deref() == Some(s.id.as_str()) {
                card = card.child(self.confirm_card(s, cx));
            }
        }
        if let Some(form) = self.servers.form.take() {
            card = card.child(self.form_view(&form, cx));
            self.servers.form = Some(form);
        }
        if let Some(note) = &self.servers.note {
            card = card.child(div().text_size(px(12.)).text_color(rgb(t.error)).child(SharedString::from(note.clone())));
        }
        vec![card.into_any_element()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_split_on_spaces_and_respect_quotes() {
        assert_eq!(split_args("-y some-server"), ["-y", "some-server"]);
        assert_eq!(split_args("  a   \"b c\" 'd e' f\"g h\"i  "), ["a", "b c", "d e", "fg hi"]);
        assert_eq!(split_args("\"\" x"), ["", "x"]);
        assert_eq!(split_args(""), Vec::<String>::new());
        assert_eq!(split_args("unclosed \"a b"), ["unclosed", "a b"]);
    }

    #[test]
    fn joining_arguments_round_trips() {
        for args in [vec!["-y", "srv"], vec!["--root", "/home/a b/c"], vec!["say", "it's x"], vec!["a\"b"], vec!["", "x"]] {
            let args: Vec<String> = args.into_iter().map(String::from).collect();
            assert_eq!(split_args(&join_args(&args)), args, "{args:?}");
        }
    }

    #[test]
    fn the_list_shows_the_command_line_or_url_never_environment_values() {
        let mut env = BTreeMap::new();
        env.insert("API_KEY".to_string(), McpValue::Plain { value: "sk-live-1".into() });
        let s = McpServer::new("S", McpTransport::Stdio { command: "npx".into(), args: vec!["-y".into(), "a b".into()], env });
        assert_eq!(target_summary(&s), "Command: npx -y \"a b\"");
        let h = McpServer::new("H", McpTransport::Http { url: "https://x.example/mcp".into(), headers: BTreeMap::new() });
        assert_eq!(target_summary(&h), "URL: https://x.example/mcp");
    }
}
