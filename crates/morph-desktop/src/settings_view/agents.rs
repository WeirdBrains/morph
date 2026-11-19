//! Settings > Agents: the companions (rename, default, model and its download state), per-site
//! grants, Always allow entries, the research search provider and the MCP placeholder.
//!
//! Every change reads the profile's `companions/` files fresh, applies, saves and tells the shell
//! ([`ShellRequest::AgentsChanged`]) so the Companion panel reloads; the functions at the top do
//! that without any window so they test on their own. This page has no keyboard items beyond
//! [`Item::DefaultCompanion`]; everything else is clicked.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{Context, Entity, SharedString, Subscription, Window, div, prelude::*, px, rgb};
use morph_ai::{AgentSettings, AiStore, CpuLimit, FileStore, Grants, ModelChoice, OffloadMode, Platform, Roster, SearchProvider};
use weirdnet_agent::{AgentId, AllowRule, CompanionScope, SiteLevel, StandingAllow};
use weirdnet_model::{ModelStore, catalog, human};

use super::{Item, SettingsView};
use crate::shell::ShellRequest;
use crate::text_input::{TextInput, TextInputEvent};
use crate::widgets::{button, chip};

/// `<profile>/companions`, where the panel keeps its roster, policy and settings.
pub fn dir(home: &Path, profile: &str) -> PathBuf {
    morph_core::profiles::profile_dir(home, profile).join("companions")
}

fn store(dir: &Path) -> FileStore {
    morph_services::profile_io::file_store(dir)
}

/// The roster as the panel has it (a first-run roster when nothing is saved).
pub fn load_roster(dir: &Path) -> Roster {
    match store(dir).load_roster() {
        Ok(Some(r)) if r.default_companion().is_some() => r,
        _ => Roster::first_run(ModelChoice::default_for(Platform::Desktop)),
    }
}

/// The model this device serves to its other devices: the default local companion's.
pub fn roster_model(dir: &Path) -> ModelChoice {
    morph_ai::serving_choice(&load_roster(dir))
}

fn edit_roster(dir: &Path, f: impl FnOnce(&mut Roster) -> Result<(), morph_ai::RosterError>) -> Result<(), String> {
    let mut roster = load_roster(dir);
    f(&mut roster).map_err(|e| e.to_string())?;
    store(dir).save_roster(&roster).map_err(|e| e.to_string())
}

pub fn rename(dir: &Path, id: &AgentId, name: &str) -> Result<(), String> {
    edit_roster(dir, |r| r.rename(id, name))
}

pub fn set_default(dir: &Path, id: &AgentId) -> Result<(), String> {
    edit_roster(dir, |r| r.set_default(id))
}

pub fn set_model(dir: &Path, id: &AgentId, model: ModelChoice) -> Result<(), String> {
    edit_roster(dir, |r| r.set_model(id, model))
}

fn grants(dir: &Path) -> Result<Grants, String> {
    Grants::load(Arc::new(store(dir))).map_err(|e| e.to_string())
}

/// What a site level applies to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Who {
    One(AgentId),
    /// Every companion in the roster now.
    All,
}

/// The host in what the user typed (`https://www.Example.com/a` or `example.com/a`), or `None`.
pub fn site_from_input(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() || text.contains(char::is_whitespace) {
        return None;
    }
    let site = if text.contains("://") {
        weirdnet_agent::site_of(text)?
    } else {
        let host = text.split(['/', '?', '#']).next().unwrap_or("").split(':').next().unwrap_or("").to_lowercase();
        host.strip_prefix("www.").unwrap_or(&host).to_string()
    };
    (!site.is_empty() && site.contains('.') || site == "localhost").then_some(site)
}

pub fn set_level(dir: &Path, who: &Who, site: &str, level: SiteLevel) -> Result<(), String> {
    let site = site_from_input(site).ok_or_else(|| "Enter a site such as example.com.".to_string())?;
    let g = grants(dir)?;
    let agents: Vec<AgentId> = match who {
        Who::One(id) => vec![id.clone()],
        Who::All => load_roster(dir).companions().iter().map(|c| c.id().clone()).collect(),
    };
    g.set_site_level_for(&agents, &site, level).map_err(|e| e.to_string())
}

/// Adds an Always allow by hand (Agents › Permissions › Add): navigate on `site` or everywhere,
/// or research with any provider, for one companion or all of them.
pub fn add_allow(dir: &Path, who: &Who, rule: AllowRule) -> Result<(), String> {
    let companion = match who {
        Who::One(id) => CompanionScope::One(id.clone()),
        Who::All => CompanionScope::All,
    };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or_default();
    grants(dir)?.add_allow(companion, rule, now).map(|_| ()).map_err(|e| e.to_string())
}

pub fn revoke(dir: &Path, allow_id: u64) -> Result<bool, String> {
    grants(dir)?.revoke(allow_id).map_err(|e| e.to_string())
}

pub fn revoke_all(dir: &Path) -> Result<(), String> {
    grants(dir)?.revoke_all().map_err(|e| e.to_string())
}

/// Forget a companion's per-site levels and its standing allows.
pub fn forget(dir: &Path, id: &AgentId) -> Result<(), String> {
    grants(dir)?.forget(id).map_err(|e| e.to_string())
}

pub fn load_settings(dir: &Path) -> AgentSettings {
    store(dir).load_settings().ok().flatten().unwrap_or_default()
}

pub fn set_provider(dir: &Path, provider: SearchProvider) -> Result<(), String> {
    let mut s = load_settings(dir);
    s.search_provider = provider;
    store(dir).save_settings(&s).map_err(|e| e.to_string())
}

/// The most requests a second to any one site, for every companion and research project.
pub fn set_site_rate(dir: &Path, rate: f64) -> Result<(), String> {
    let mut s = load_settings(dir);
    s.site_rate = rate;
    store(dir).save_settings(&s).map_err(|e| e.to_string())
}

/// "0.5 requests a second (one every 2 s)".
pub fn site_rate_label(rate: f64) -> String {
    let every = 1.0 / rate;
    if rate >= 1.0 { format!("{rate}/s") } else { format!("{rate}/s (one every {} s)", (every * 10.0).round() / 10.0) }
}

/// Where turns may run when this device cannot run local models.
pub fn set_offload_mode(dir: &Path, mode: OffloadMode) -> Result<(), String> {
    let mut s = load_settings(dir);
    s.offload.mode = mode;
    store(dir).save_settings(&s).map_err(|e| e.to_string())
}

/// "Serve my models to my other devices": this device answers model turns from the user's
/// linked devices with its local model. Synced with the other agent settings.
pub fn set_serve_models(dir: &Path, on: bool) -> Result<(), String> {
    let mut s = load_settings(dir);
    s.offload.serve = on;
    store(dir).save_settings(&s).map_err(|e| e.to_string())
}

/// What the serving row says besides the switch: what is served, or why nothing is.
pub fn serving_note(on: bool, cpu: Option<&CpuLimit>, model: &ModelChoice, state: &ModelState) -> String {
    if !on {
        return "Off: your other devices run their own model turns.".into();
    }
    if let Some(c) = cpu {
        return format!("On, but nothing is served: {}.", c.reason());
    }
    match state {
        ModelState::Installed => {
            format!("On: {} is served to your linked devices, one turn at a time, and your own questions here go first.", model.label())
        }
        ModelState::NotDownloaded { .. } => {
            format!("On, but {} is not downloaded yet, so other devices run their own turns until it is.", model.label())
        }
        ModelState::NotInCatalog => format!("On, but {} is not a model Morph can run locally, so nothing is served.", model.label()),
    }
}

/// Whether the model picker and companion setup offer local models: not on a CPU that lacks what
/// the build's llama.cpp needs.
pub fn local_models_offered(cpu: Option<&CpuLimit>) -> bool {
    cpu.is_none()
}

/// The two lines Settings > Agents shows when local models cannot run: the reason, then the way out.
pub fn cpu_notice(cpu: Option<&CpuLimit>) -> Option<(String, String)> {
    cpu.map(|c| (format!("{}.", c.reason()), CpuLimit::INSTEAD.to_string()))
}

/// Where a companion's model stands on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelState {
    Installed,
    NotDownloaded {
        size: u64,
        partial: u64,
    },
    /// Not one of Morph's catalog models, so it cannot run locally yet.
    NotInCatalog,
}

impl ModelState {
    pub fn text(&self) -> String {
        match self {
            ModelState::Installed => "On this device.".into(),
            ModelState::NotDownloaded { size, partial: 0 } => {
                format!("Not downloaded yet ({}). The Companion panel asks before it downloads.", human(*size))
            }
            ModelState::NotDownloaded { size, partial } => {
                format!("Partly downloaded ({} of {}). The Companion panel resumes it after you agree.", human(*partial), human(*size))
            }
            ModelState::NotInCatalog => "Not a model Morph can download and run locally yet.".into(),
        }
    }
}

pub fn model_state(choice: &ModelChoice, models: &ModelStore) -> ModelState {
    let spec = match choice {
        ModelChoice::Gemma4E4B => catalog::find(catalog::GEMMA_4_E4B),
        ModelChoice::Gemma4E2B => catalog::find(catalog::GEMMA_4_E2B),
        ModelChoice::Custom(id) => catalog::find(id),
    };
    match spec {
        None => ModelState::NotInCatalog,
        Some(s) if models.installed(&s) => ModelState::Installed,
        Some(s) => ModelState::NotDownloaded { size: s.size, partial: models.partial_bytes(&s) },
    }
}

/// `2026-11-18` for a Unix time in milliseconds (UTC).
pub fn date_of(unix_ms: u64) -> String {
    let days = (unix_ms / 86_400_000) as i64;
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// One standing allow as the list shows it: what it skips, and for whom and since when.
pub fn describe_allow(a: &StandingAllow, roster: &Roster) -> (String, String) {
    let what = match &a.rule {
        AllowRule::Navigate { site: Some(s) } => format!("Navigate on {s}"),
        AllowRule::Navigate { site: None } => "Navigate everywhere".to_string(),
        AllowRule::Research { provider: None } => "Research with any provider".to_string(),
        AllowRule::Research { provider: Some(p) } => format!("Research with {p}"),
        _ => "A rule this version does not know".to_string(),
    };
    let who = match &a.companion {
        CompanionScope::All => "all companions".to_string(),
        CompanionScope::One(id) => roster.get(id).map(|c| c.name().to_string()).unwrap_or_else(|| format!("{id} (removed)")),
        _ => "some companions".to_string(),
    };
    (what, format!("for {who} · since {}", date_of(a.created_unix_ms)))
}

pub fn level_label(l: SiteLevel) -> &'static str {
    match l {
        SiteLevel::Off => "Off",
        SiteLevel::Ask => "Ask",
        SiteLevel::Act => "Act",
        _ => "Other",
    }
}

const LEVELS: [SiteLevel; 3] = [SiteLevel::Off, SiteLevel::Ask, SiteLevel::Act];

/// What the page keeps between frames.
pub struct State {
    pub rename: Entity<TextInput>,
    pub custom_model: Entity<TextInput>,
    pub site: Entity<TextInput>,
    /// The companion being edited.
    pub selected: Option<AgentId>,
    /// The level the add-site row will set, and whether it applies to every companion.
    pub new_level: SiteLevel,
    pub new_all: bool,
    pub custom_open: bool,
    /// Set when this CPU cannot run local models (checked once).
    pub cpu: Option<CpuLimit>,
    pub note: Option<String>,
}

impl State {
    pub fn new(theme: morph_ui::theme::Theme, cx: &mut Context<SettingsView>) -> Self {
        Self {
            rename: cx.new(|cx| TextInput::new("New name", theme, cx)),
            custom_model: cx.new(|cx| TextInput::new("Model id from the catalog", theme, cx)),
            site: cx.new(|cx| TextInput::new("example.com", theme, cx)),
            selected: None,
            new_level: SiteLevel::Off,
            new_all: false,
            custom_open: false,
            cpu: morph_ai::cpu_limit(),
            note: None,
        }
    }
}

pub fn subscribe(state: &State, window: &mut Window, cx: &mut Context<SettingsView>) -> Vec<Subscription> {
    vec![
        cx.subscribe_in(&state.rename, window, |this, input, event, window, cx| {
            if let TextInputEvent::Submit { .. } = event {
                let name = input.read(cx).text().to_string();
                this.agents_rename(&name, cx);
                window.focus(&this.focus, cx);
            }
        }),
        cx.subscribe_in(&state.custom_model, window, |this, input, event, window, cx| {
            if let TextInputEvent::Submit { .. } = event {
                let id = input.read(cx).text().trim().to_string();
                if !id.is_empty() {
                    this.agents_set_model(ModelChoice::Custom(id), cx);
                }
                window.focus(&this.focus, cx);
            }
        }),
        cx.subscribe_in(&state.site, window, |this, _, event, window, cx| {
            if let TextInputEvent::Submit { .. } = event {
                this.agents_add_site(cx);
                window.focus(&this.focus, cx);
            }
        }),
    ]
}

impl SettingsView {
    pub(super) fn agents_dir(&self) -> Option<PathBuf> {
        self.profile_id().map(|id| dir(&self.home, &id))
    }

    /// The companion being edited: the chosen one, else the default.
    fn agents_selected(&self, roster: &Roster) -> Option<AgentId> {
        self.agents.selected.clone().filter(|id| roster.get(id).is_some()).or_else(|| roster.default_companion().map(|c| c.id().clone()))
    }

    /// Apply a change, tell the panel, and show what went wrong.
    fn agents_apply(&mut self, result: impl FnOnce(&Path) -> Result<(), String>, cx: &mut Context<Self>) -> bool {
        let Some(dir) = self.agents_dir() else {
            self.agents.note = Some("No profile is open.".into());
            cx.notify();
            return false;
        };
        let ok = match result(&dir) {
            Ok(()) => {
                self.agents.note = None;
                cx.emit(ShellRequest::AgentsChanged);
                true
            }
            Err(e) => {
                self.agents.note = Some(e);
                false
            }
        };
        cx.notify();
        ok
    }

    fn agents_rename(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(id) = self.agents_dir().map(|d| load_roster(&d)).and_then(|r| self.agents_selected(&r)) else { return };
        let name = name.to_string();
        if self.agents_apply(|d| rename(d, &id, &name), cx) {
            self.agents.rename.update(cx, |i, cx| i.set_text("", cx));
        }
    }

    fn agents_set_model(&mut self, model: ModelChoice, cx: &mut Context<Self>) {
        let Some(id) = self.agents_dir().map(|d| load_roster(&d)).and_then(|r| self.agents_selected(&r)) else { return };
        self.agents.custom_open = matches!(model, ModelChoice::Custom(_));
        self.agents_apply(|d| set_model(d, &id, model), cx);
    }

    fn agents_set_default(&mut self, id: AgentId, cx: &mut Context<Self>) {
        self.agents_apply(|d| set_default(d, &id), cx);
    }

    /// ←/→ on the Companions row: the next or previous companion becomes the default.
    pub(super) fn agents_step_default(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(dir) = self.agents_dir() else { return };
        let roster = load_roster(&dir);
        let ids: Vec<AgentId> = roster.companions().iter().map(|c| c.id().clone()).collect();
        let Some(cur) = roster.default_companion().and_then(|c| ids.iter().position(|i| i == c.id())) else { return };
        let next = ids[if forward { (cur + 1) % ids.len() } else { (cur + ids.len() - 1) % ids.len() }].clone();
        self.agents.selected = Some(next.clone());
        self.agents_set_default(next, cx);
    }

    fn agents_add_site(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = self.agents_dir() else { return };
        let roster = load_roster(&dir);
        let Some(id) = self.agents_selected(&roster) else { return };
        let site = self.agents.site.read(cx).text().to_string();
        let who = if self.agents.new_all { Who::All } else { Who::One(id) };
        let level = self.agents.new_level;
        if self.agents_apply(|d| set_level(d, &who, &site, level), cx) {
            self.agents.site.update(cx, |i, cx| i.set_text("", cx));
        }
    }

    /// Reread what the page shows (it opened, or the profile changed).
    pub(super) fn agents_reload(&mut self, cx: &mut Context<Self>) {
        self.agents.selected = None;
        self.agents.note = None;
        self.servers_reload();
        cx.notify();
    }

    /// A settings card without a keyboard item: label and help left, controls right.
    fn plain_row(&self, label: &str, help: String, controls: gpui::AnyElement) -> gpui::AnyElement {
        let t = self.theme;
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(16.))
            .px(px(12.))
            .py(px(8.))
            .rounded(px(8.))
            .bg(rgb(t.surface))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().text_size(px(13.)).child(label.to_string()))
                    .child(div().text_size(px(12.)).text_color(rgb(t.text_secondary)).child(help)),
            )
            .child(div().flex_none().max_w_full().child(controls))
            .into_any_element()
    }

    pub(super) fn agents_rows(&mut self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let t = self.theme;
        let Some(dir) = self.agents_dir() else {
            return vec![
                div().text_size(px(12.)).text_color(rgb(t.text_secondary)).child("Open a profile to set up its companions.").into_any_element(),
            ];
        };
        let roster = load_roster(&dir);
        let Some(selected) = self.agents_selected(&roster) else { return Vec::new() };
        let profile = roster.get(&selected).cloned();
        let grants = grants(&dir).ok();
        let default_id = roster.default_companion().map(|c| c.id().clone());
        let dim = |s: String| div().text_size(px(12.)).text_color(rgb(t.text_secondary)).child(s);
        let mut rows: Vec<gpui::AnyElement> = Vec::new();

        // Companions: pick one to edit; the star marks the default.
        let picker = div().flex().flex_row().flex_wrap().gap(px(6.)).children(roster.companions().iter().enumerate().map(|(i, c)| {
            let id = c.id().clone();
            let label = if Some(c.id()) == default_id.as_ref() { format!("★ {}", c.name()) } else { c.name().to_string() };
            chip(t, ("agents-pick", i), label, *c.id() == selected).on_click(cx.listener(move |this, _, _, cx| {
                this.agents.selected = Some(id.clone());
                cx.notify();
            }))
        }));
        rows.push(self.stacked_row(
            Item::DefaultCompanion,
            "Companions",
            Some("★ is the default: `> question` in the address bar asks it. ←/→ here change the default; click a name to edit it below.".into()),
            picker.into_any_element(),
            cx,
        ));

        if let Some((reason, instead)) = cpu_notice(self.agents.cpu.as_ref()) {
            let mode = load_settings(&dir).offload.mode;
            let chips = div().flex().flex_row().gap(px(6.)).children(
                [(OffloadMode::Auto, "Automatic"), (OffloadMode::Always, "Another device")].into_iter().enumerate().map(|(i, (m, label))| {
                    chip(t, ("agents-offload", i), label, mode == m).on_click(cx.listener(move |this, _, _, cx| {
                        this.agents_apply(|d| set_offload_mode(d, m), cx);
                    }))
                }),
            );
            rows.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .px(px(12.))
                    .py(px(10.))
                    .rounded(px(8.))
                    .bg(rgb(t.surface))
                    .child(div().text_size(px(14.)).text_color(rgb(t.error)).child(reason))
                    .child(dim(instead))
                    .child(chips)
                    .child(dim("Saved now; turns use it once linked-device routing is wired up.".into()))
                    .into_any_element(),
            );
        }
        if let Some(p) = profile {
            let is_default = default_id.as_ref() == Some(p.id());
            let model = p.model.clone().unwrap_or_else(|| ModelChoice::default_for(Platform::Desktop));
            let state = model_state(&model, &ModelStore::from_env());
            let model_chips = div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap(px(6.))
                .child(
                    chip(t, "agents-model-e4b", "Gemma 4 E4B", model == ModelChoice::Gemma4E4B)
                        .on_click(cx.listener(|this, _, _, cx| this.agents_set_model(ModelChoice::Gemma4E4B, cx))),
                )
                .child(
                    chip(t, "agents-model-e2b", "Gemma 4 E2B", model == ModelChoice::Gemma4E2B)
                        .on_click(cx.listener(|this, _, _, cx| this.agents_set_model(ModelChoice::Gemma4E2B, cx))),
                )
                .child(chip(t, "agents-model-custom", "Custom…", matches!(model, ModelChoice::Custom(_))).on_click(cx.listener(|this, _, _, cx| {
                    this.agents.custom_open = true;
                    cx.notify();
                })));
            let rename_row = div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .child(div().w(px(200.)).child(self.agents.rename.clone()))
                .child(button(t, "agents-rename", "Rename", false).on_click(cx.listener(|this, _, _, cx| {
                    let name = this.agents.rename.read(cx).text().to_string();
                    this.agents_rename(&name, cx);
                })))
                .children((!is_default).then(|| {
                    let id = p.id().clone();
                    button(t, "agents-make-default", "Make default", false)
                        .on_click(cx.listener(move |this, _, _, cx| this.agents_set_default(id.clone(), cx)))
                }));
            let local_ok = local_models_offered(self.agents.cpu.as_ref());
            let custom_row = (local_ok && (self.agents.custom_open || matches!(model, ModelChoice::Custom(_)))).then(|| {
                div().flex().flex_row().items_center().gap(px(8.)).child(div().w(px(240.)).child(self.agents.custom_model.clone())).child(
                    button(t, "agents-custom-set", "Use", false).on_click(cx.listener(|this, _, _, cx| {
                        let id = this.agents.custom_model.read(cx).text().trim().to_string();
                        if !id.is_empty() {
                            this.agents_set_model(ModelChoice::Custom(id), cx);
                        }
                    })),
                )
            });
            rows.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .px(px(12.))
                    .py(px(10.))
                    .rounded(px(8.))
                    .bg(rgb(t.surface))
                    .child(div().text_size(px(14.)).child(format!("{}{}", p.name(), if is_default { " (default)" } else { "" })))
                    .child(rename_row)
                    .child(dim("Model".into()))
                    .children(local_ok.then_some(model_chips))
                    .children(custom_row)
                    .child(dim(match &self.agents.cpu {
                        Some(cpu) => format!("Local models are off: {}.", cpu.reason()),
                        None => format!("{}: {}", model.label(), state.text()),
                    }))
                    .into_any_element(),
            );

            // Per-site levels for this companion.
            let sites = grants.as_ref().map(|g| g.sites_for(p.id())).unwrap_or_default();
            let mut card = div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .px(px(12.))
                .py(px(10.))
                .rounded(px(8.))
                .bg(rgb(t.surface))
                .child(div().text_size(px(14.)).child(format!("Sites for {}", p.name())))
                .child(dim("Off: no actions, research skips it. Ask (the default): every action asks. Act: navigate and interact run without asking. Reading a page is always an explicit attach.".into()));
            if sites.is_empty() {
                card = card.child(dim("Every site is Ask.".into()));
            }
            for (i, (site, level)) in sites.iter().enumerate() {
                let who = Who::One(p.id().clone());
                card = card.child(
                    div().flex().flex_row().items_center().gap(px(6.)).child(div().flex_1().text_size(px(13.)).child(site.clone())).children(
                        LEVELS.into_iter().enumerate().map(|(j, l)| {
                            let (who, site) = (who.clone(), site.clone());
                            chip(t, ("agents-level", i * 10 + j), level_label(l), l == *level).on_click(cx.listener(move |this, _, _, cx| {
                                let (who, site) = (who.clone(), site.clone());
                                this.agents_apply(|d| set_level(d, &who, &site, l), cx);
                            }))
                        }),
                    ),
                );
            }
            let add = div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap(px(6.))
                .child(div().w(px(180.)).child(self.agents.site.clone()))
                .children(LEVELS.into_iter().enumerate().map(|(j, l)| {
                    chip(t, ("agents-new-level", j), level_label(l), self.agents.new_level == l).on_click(cx.listener(move |this, _, _, cx| {
                        this.agents.new_level = l;
                        cx.notify();
                    }))
                }))
                .child(chip(t, "agents-new-all", "All companions", self.agents.new_all).on_click(cx.listener(|this, _, _, cx| {
                    this.agents.new_all = !this.agents.new_all;
                    cx.notify();
                })))
                .child(button(t, "agents-add-site", "Set", false).on_click(cx.listener(|this, _, _, cx| this.agents_add_site(cx))));
            let id = p.id().clone();
            card = card.child(add).child(div().flex().flex_row().child(
                button(t, "agents-forget", format!("Forget {}'s grants", p.name()), false).on_click(cx.listener(move |this, _, _, cx| {
                    let id = id.clone();
                    this.agents_apply(|d| forget(d, &id), cx);
                })),
            ));
            rows.push(card.into_any_element());
        }

        // Always allow.
        let allows = grants.as_ref().map(|g| g.allows()).unwrap_or_default();
        let mut card = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .px(px(12.))
            .py(px(10.))
            .rounded(px(8.))
            .bg(rgb(t.surface))
            .child(div().text_size(px(14.)).child("Always allow"))
            .child(dim("Standing allows skip the approval for navigate (one site or everywhere) and research (optionally one provider). Off sites, budgets and signed-in research are never overridden. Revoking takes effect at the next tool call.".into()));
        if allows.is_empty() {
            card = card.child(dim("None. Always allow appears on approval cards.".into()));
        }
        for (i, a) in allows.iter().enumerate() {
            let (what, detail) = describe_allow(a, &roster);
            let id = a.id;
            card = card.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(div().flex_1().flex().flex_col().child(div().text_size(px(13.)).child(what)).child(dim(detail)))
                    .child(
                        div()
                            .id(("agents-revoke", i))
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
                            .child("Revoke")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.agents_apply(|d| revoke(d, id).map(|_| ()), cx);
                            })),
                    ),
            );
        }
        // Add: navigate everywhere or research, for the selected companion or all of them.
        if let Some(sel) = self.agents_selected(&roster) {
            let name = roster.get(&sel).map(|c| c.name().to_string()).unwrap_or_default();
            let adds = [
                (format!("+ Navigate everywhere · {name}"), Who::One(sel.clone()), AllowRule::Navigate { site: None }),
                ("+ Navigate everywhere · all".to_string(), Who::All, AllowRule::Navigate { site: None }),
                (format!("+ Research · {name}"), Who::One(sel.clone()), AllowRule::Research { provider: None }),
                ("+ Research · all".to_string(), Who::All, AllowRule::Research { provider: None }),
            ];
            card =
                card.child(div().flex().flex_row().flex_wrap().gap(px(6.)).children(adds.into_iter().enumerate().map(|(i, (label, who, rule))| {
                    button(t, ("agents-add-allow", i), label, false).on_click(cx.listener(move |this, _, _, cx| {
                        let (who, rule) = (who.clone(), rule.clone());
                        this.agents_apply(|d| add_allow(d, &who, rule), cx);
                    }))
                })));
        }
        if allows.len() > 1 {
            card = card.child(div().flex().flex_row().child(button(t, "agents-revoke-all", "Revoke all", false).on_click(cx.listener(
                |this, _, _, cx| {
                    this.agents_apply(revoke_all, cx);
                },
            ))));
        }
        rows.push(card.into_any_element());

        // Research provider.
        let current = load_settings(&dir).search_provider;
        let chips = div().flex().flex_row().gap(px(6.)).children(SearchProvider::ALL.into_iter().enumerate().map(|(i, p)| {
            chip(t, ("agents-provider", i), p.label(), current == p).on_click(cx.listener(move |this, _, _, cx| {
                this.agents_apply(|d| set_provider(d, p), cx);
            }))
        }));
        rows.push(
            self.plain_row(
                "Research search",
                "Research searches your Library (bookmarks and history) first, then this provider. Saved now; research uses it once it is wired up."
                    .into(),
                chips.into_any_element(),
            ),
        );

        // Site rate: the most requests a second to any one site, for every companion and research.
        let rate = load_settings(&dir).site_rate();
        let rate_chips = div().flex().flex_row().flex_wrap().gap(px(6.)).children(morph_ai::SITE_RATE_STEPS.into_iter().enumerate().map(|(i, r)| {
            chip(t, ("agents-site-rate", i), site_rate_label(r), (rate - r).abs() < 1e-9).on_click(cx.listener(move |this, _, _, cx| {
                this.agents_apply(|d| set_site_rate(d, r), cx);
            }))
        }));
        rows.push(self.plain_row(
            "Site rate",
            format!(
                "The most requests a second Morph makes to any one site, for every companion and research project, with or without Always allow. Each wait is at least 1 / rate, plus a random 0 to 10% so visits look less mechanical. A 429 or 503 backs off on top. Now {}. Syncs with your other agent settings.",
                site_rate_label(rate)
            ),
            rate_chips.into_any_element(),
        ));

        // Serve this device's model to the user's other devices (off until turned on).
        let serve = load_settings(&dir).offload.serve;
        let served = roster_model(&dir);
        let note = serving_note(serve, self.agents.cpu.as_ref(), &served, &model_state(&served, &ModelStore::from_env()));
        let serve_chips =
            div().flex().flex_row().gap(px(6.)).children([(true, "On"), (false, "Off")].into_iter().enumerate().map(|(i, (on, label))| {
                chip(t, ("agents-serve", i), label, serve == on).on_click(cx.listener(move |this, _, _, cx| {
                    this.agents_apply(|d| set_serve_models(d, on), cx);
                }))
            }));
        rows.push(self.plain_row(
            "Serve my models to my other devices",
            format!(
                "Lets your linked, unrevoked devices (a phone, say) use this device's local model for their turns. Only your own devices can ask, never while this profile is locked, and one turn at a time. {note} Syncs with your other agent settings, so a device that cannot run local models serves nothing."
            ),
            serve_chips.into_any_element(),
        ));

        // Connected servers: MCP servers the companions use (not Morph's own server, below).
        rows.extend(self.servers_rows(&dir, cx));

        // MCP placeholder: there is no server to turn on yet.
        rows.push(self.plain_row(
            "Morph MCP server",
            "Would let Mouser and other local agents use this browser's tools. Off, and coming soon: no server exists yet, so this cannot be turned on.".into(),
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .child(div().text_size(px(12.)).text_color(rgb(t.text_secondary)).child("Coming soon"))
                .child(div().w(px(36.)).h(px(20.)).rounded(px(10.)).bg(rgb(t.surface_variant)).border_1().border_color(rgb(t.border)).opacity(0.6))
                .into_any_element(),
        ));

        if let Some(note) = &self.agents.note {
            rows.push(div().text_size(px(12.)).text_color(rgb(t.error)).child(SharedString::from(note.clone())).into_any_element());
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("companions");
        (d, p)
    }

    fn local() -> AgentId {
        AgentId::new("local")
    }

    #[test]
    fn rename_default_and_model_are_saved_and_validated() {
        let (_d, p) = tmp();
        let id = load_roster(&p).default_companion().unwrap().id().clone();
        rename(&p, &id, "Atlas").unwrap();
        set_model(&p, &id, ModelChoice::Gemma4E2B).unwrap();
        let r = load_roster(&p);
        assert_eq!(r.get(&id).unwrap().name(), "Atlas");
        assert_eq!(r.get(&id).unwrap().model, Some(ModelChoice::Gemma4E2B));
        assert!(rename(&p, &id, "  ").is_err());
        assert!(rename(&p, &AgentId::new("nope"), "X").is_err());
        assert!(set_default(&p, &AgentId::new("nope")).is_err());
    }

    #[test]
    fn site_levels_apply_to_one_companion_or_all_and_survive_a_reload() {
        let (_d, p) = tmp();
        let mut r = load_roster(&p);
        r.add(morph_ai::CompanionProfile::local("scout", "Scout", ModelChoice::Gemma4E4B)).unwrap();
        store(&p).save_roster(&r).unwrap();
        set_level(&p, &Who::One(local()), "https://www.Example.com/a?b", SiteLevel::Off).unwrap();
        set_level(&p, &Who::All, "tokio.rs", SiteLevel::Act).unwrap();
        let g = grants(&p).unwrap();
        assert_eq!(g.sites_for(&local()), [("example.com".to_string(), SiteLevel::Off), ("tokio.rs".to_string(), SiteLevel::Act)]);
        assert_eq!(g.sites_for(&AgentId::new("scout")), [("tokio.rs".to_string(), SiteLevel::Act)]);
        set_level(&p, &Who::One(local()), "example.com", SiteLevel::Ask).unwrap();
        assert_eq!(grants(&p).unwrap().sites_for(&local()).len(), 1);
        assert!(set_level(&p, &Who::All, "not a site", SiteLevel::Off).is_err());
        forget(&p, &local()).unwrap();
        assert!(grants(&p).unwrap().sites_for(&local()).is_empty());
        assert_eq!(grants(&p).unwrap().sites_for(&AgentId::new("scout")).len(), 1);
    }

    #[test]
    fn site_input_accepts_urls_and_hosts_only() {
        assert_eq!(site_from_input(" https://User@WWW.Docs.rs:8080/x ").as_deref(), Some("docs.rs"));
        assert_eq!(site_from_input("www.Example.com/a").as_deref(), Some("example.com"));
        assert_eq!(site_from_input("localhost:3000").as_deref(), Some("localhost"));
        for bad in ["", "   ", "two words.com", "nodots", "https://"] {
            assert_eq!(site_from_input(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn allows_list_describe_and_revoke() {
        let (_d, p) = tmp();
        let g = grants(&p).unwrap();
        let a = g.add_allow(CompanionScope::One(local()), AllowRule::Navigate { site: Some("tokio.rs".into()) }, 1_795_000_000_000).unwrap();
        let b = g.add_allow(CompanionScope::All, AllowRule::Research { provider: Some("duckduckgo".into()) }, 1_795_000_000_000).unwrap();
        g.add_allow(CompanionScope::All, AllowRule::Navigate { site: None }, 0).unwrap();
        let roster = load_roster(&p);
        let all = grants(&p).unwrap().allows();
        assert_eq!(describe_allow(&all[0], &roster), ("Navigate on tokio.rs".to_string(), "for Local · since 2026-11-18".to_string()));
        assert_eq!(describe_allow(&all[1], &roster).0, "Research with duckduckgo");
        assert_eq!(describe_allow(&all[2], &roster), ("Navigate everywhere".to_string(), "for all companions · since 1970-01-01".to_string()));
        assert!(revoke(&p, a).unwrap());
        assert!(!revoke(&p, a).unwrap());
        assert_eq!(grants(&p).unwrap().allows().len(), 2);
        revoke_all(&p).unwrap();
        assert!(grants(&p).unwrap().allows().is_empty());
        let _ = b;
    }

    #[test]
    fn dates_are_utc_civil_dates() {
        assert_eq!(date_of(0), "1970-01-01");
        assert_eq!(date_of(951_782_400_000), "2000-02-29");
        assert_eq!(date_of(1_795_000_000_000), "2026-11-18");
    }

    #[test]
    fn the_research_provider_defaults_to_duckduckgo_and_is_stored() {
        let (_d, p) = tmp();
        assert_eq!(load_settings(&p).search_provider, SearchProvider::DuckDuckGo);
        set_provider(&p, SearchProvider::Google).unwrap();
        assert_eq!(load_settings(&p).search_provider, SearchProvider::Google);
    }

    #[test]
    fn a_failing_cpu_check_hides_local_models_and_shows_one_reason_line() {
        let weak = CpuLimit::from_check(Err(weirdnet_model::Error::UnsupportedCpu { missing: vec!["avx2".into(), "fma".into()] }));
        assert!(!local_models_offered(weak.as_ref()), "the picker and setup do not offer local models");
        let (line, instead) = cpu_notice(weak.as_ref()).unwrap();
        assert_eq!(line, "This CPU lacks AVX2/FMA needed for local models.");
        assert!(instead.contains("another device") && instead.contains("offload"));
        // A CPU that passes sees today's page.
        let ok = CpuLimit::from_check(Ok(()));
        assert!(local_models_offered(ok.as_ref()) && cpu_notice(ok.as_ref()).is_none());
    }

    #[test]
    fn the_offload_choice_is_saved_through_the_agent_settings() {
        let (_d, p) = tmp();
        assert_eq!(load_settings(&p).offload.mode, OffloadMode::Auto);
        set_offload_mode(&p, OffloadMode::Always).unwrap();
        assert_eq!(load_settings(&p).offload.mode, OffloadMode::Always);
        assert_eq!(load_settings(&p).search_provider, SearchProvider::DuckDuckGo, "other settings stay");
    }

    #[test]
    fn serving_is_off_by_default_saved_with_the_settings_and_explained() {
        let (_d, p) = tmp();
        assert!(!load_settings(&p).offload.serve, "off until turned on");
        set_serve_models(&p, true).unwrap();
        assert!(load_settings(&p).offload.serve);
        assert_eq!(load_settings(&p).offload.mode, OffloadMode::Auto, "the offload choice stays");
        assert_eq!(roster_model(&p), ModelChoice::Gemma4E4B);
        let m = ModelChoice::Gemma4E4B;
        assert!(serving_note(false, None, &m, &ModelState::Installed).starts_with("Off"));
        assert!(serving_note(true, None, &m, &ModelState::Installed).contains("Gemma 4 E4B is served"));
        assert!(serving_note(true, None, &m, &ModelState::NotDownloaded { size: 1, partial: 0 }).contains("not downloaded"));
        let cpu = CpuLimit { missing: vec!["avx2".into()] };
        assert!(serving_note(true, Some(&cpu), &m, &ModelState::Installed).contains("AVX2"));
    }

    #[test]
    fn model_state_reports_missing_partial_installed_and_unknown() {
        let d = tempfile::tempdir().unwrap();
        let models = ModelStore::new(d.path());
        let spec = catalog::find(catalog::GEMMA_4_E2B).unwrap();
        assert_eq!(model_state(&ModelChoice::Gemma4E2B, &models), ModelState::NotDownloaded { size: spec.size, partial: 0 });
        std::fs::write(d.path().join(format!("{}.part", spec.file)), vec![0u8; 1000]).unwrap();
        assert_eq!(model_state(&ModelChoice::Gemma4E2B, &models), ModelState::NotDownloaded { size: spec.size, partial: 1000 });
        std::fs::File::create(d.path().join(&spec.file)).unwrap().set_len(spec.size).unwrap();
        assert_eq!(model_state(&ModelChoice::Gemma4E2B, &models), ModelState::Installed);
        assert_eq!(model_state(&ModelChoice::Custom("org/x".into()), &models), ModelState::NotInCatalog);
        assert!(ModelState::NotInCatalog.text().contains("cannot") || ModelState::NotInCatalog.text().contains("Not a model"));
        assert!(model_state(&ModelChoice::Gemma4E4B, &models).text().contains("GB"));
    }

    #[test]
    fn allows_can_be_added_by_hand_for_one_companion_or_all() {
        let home = tempfile::tempdir().unwrap();
        let d = home.path().join("companions");
        add_allow(&d, &Who::One(AgentId::new("local")), AllowRule::Navigate { site: None }).unwrap();
        add_allow(&d, &Who::All, AllowRule::Research { provider: None }).unwrap();
        let allows = grants(&d).unwrap().allows();
        assert_eq!(allows.len(), 2);
        assert_eq!(allows[0].companion, CompanionScope::One(AgentId::new("local")));
        assert_eq!(allows[1].companion, CompanionScope::All);
        assert!(revoke(&d, allows[0].id).unwrap());
        assert_eq!(grants(&d).unwrap().allows().len(), 1);
    }
}
