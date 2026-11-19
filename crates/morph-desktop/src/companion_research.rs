//! The Research tab of the Companions panel (design: "Long-term Research Projects"): the list
//! of projects with their state, and one project open with its plan, Pause / Resume / Stop, the
//! Always allow choice for the research tools (this project, or all projects) and its log.
//! The state lives in `companions/research.json` (`morph_ai::research`); this file draws it.

use gpui::{Context, div, prelude::*, px, rgb};
use morph_ai::FileStore;
use morph_ai::research::{ProjectState, Projects, ResearchProject, StepStatus};
use morph_ui::theme::Theme;

use crate::companion_view::CompanionView;
use crate::widgets::{button, chip};

/// Where the projects are kept, in the profile's `companions/` directory.
pub const FILE: &str = "research.json";

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or_default()
}

/// The Research tab's state.
pub struct ResearchState {
    pub projects: Projects,
    /// The project shown open.
    pub open: Option<String>,
}

impl ResearchState {
    pub fn load(store: &FileStore) -> Self {
        let bytes = store.read_file(FILE).ok().flatten();
        let projects = Projects::from_bytes(bytes.as_deref(), now_ms()).unwrap_or_default();
        Self { projects, open: None }
    }

    pub fn save(&self, store: &FileStore) {
        if let Ok(bytes) = self.projects.to_bytes() {
            let _ = store.write_file(FILE, &bytes);
        }
    }

    /// A new project from a question for `companion`; it is opened and starts paused.
    pub fn add(&mut self, question: &str, companion: &str) -> String {
        let p = ResearchProject::from_question(question, companion, now_ms());
        let id = p.id.clone();
        self.projects.projects.insert(0, p);
        self.open = Some(id.clone());
        id
    }

    pub fn set_state(&mut self, id: &str, state: ProjectState) {
        let now = now_ms();
        if let Some(p) = self.projects.get_mut(id) {
            p.state = state;
            p.log(now, format!("{state:?}").to_lowercase());
        }
    }

    pub fn toggle_allow(&mut self, id: &str) {
        let now = now_ms();
        if let Some(p) = self.projects.get_mut(id) {
            p.always_allow = !p.always_allow;
            let on = p.always_allow;
            p.log(now, if on { "Always allow for the research tools: on (no budget caps)" } else { "Always allow: off (budget caps apply)" });
        }
    }

    pub fn toggle_allow_all(&mut self) {
        self.projects.always_allow_all = !self.projects.always_allow_all;
    }

    /// Made-up projects for the headless screenshot check (`MORPH_COMPANION_DEMO_RESEARCH=1`).
    pub fn demo(&mut self) {
        use morph_ai::research::{PlanStep, Usage};
        let now = now_ms();
        let mut a = ResearchProject::from_question("How do cancellation and drop interact in Tokio, and what are the open proposals?", "atlas", now);
        a.state = ProjectState::Running;
        a.always_allow = true;
        a.plan.push(PlanStep::new("Compare the two async-drop proposals"));
        a.plan[0].status = StepStatus::Done;
        a.plan[1].status = StepStatus::Running;
        a.usage = Usage { pages: 41, ..Usage::default() };
        a.log(now, "session 2 started (Always allow: no budget caps)");
        a.log(now, "done: Search the Library and the web for the question: 3 sources");
        let mut b = ResearchProject::from_question("Local-first sync papers worth reading", "scout", now);
        b.state = ProjectState::Paused;
        b.usage = Usage { pages: 18, ..Usage::default() };
        let mut c = ResearchProject::from_question("WebView2 bridge prior art", "scout", now);
        c.state = ProjectState::Done;
        for s in &mut c.plan {
            s.status = StepStatus::Done;
        }
        self.projects.projects = vec![a.clone(), b, c];
        self.open = Some(a.id);
    }

    /// Projects that are running or waiting for the user (the tab's count).
    pub fn active(&self) -> usize {
        self.projects.projects.iter().filter(|p| matches!(p.state, ProjectState::Running | ProjectState::NeedsYou)).count()
    }
}

fn state_label(s: ProjectState) -> (&'static str, fn(Theme) -> u32) {
    match s {
        ProjectState::Running => ("Running", |t| t.accent),
        ProjectState::Paused => ("Paused", |t| t.text_secondary),
        ProjectState::NeedsYou => ("Needs you", |t| t.accent_alt),
        ProjectState::Done => ("Done", |t| t.accent),
        ProjectState::Stopped => ("Stopped", |t| t.error),
    }
}

fn steps_done(p: &ResearchProject) -> (usize, usize) {
    (p.plan.iter().filter(|s| s.status == StepStatus::Done).count(), p.plan.len())
}

/// The tab's body: a hint for the composer, the project list, and the open project.
pub fn body(t: Theme, state: &ResearchState, companion: &str, cx: &mut Context<CompanionView>) -> gpui::Div {
    let allow_all = state.projects.always_allow_all;
    let mut col = div().flex().flex_col().flex_1().gap(px(6.)).overflow_hidden();
    col = col.child(
        div()
            .text_size(px(11.))
            .text_color(rgb(t.text_secondary))
            .child(format!("Type what {companion} should research below, then Start project. A project keeps its plan, sources and log.")),
    );
    col = col.child(
        chip(
            t,
            "research-allow-all",
            if allow_all { "Always allow research tools: all projects" } else { "Always allow for all projects: off" },
            allow_all,
        )
        .on_click(cx.listener(|this, _, _, cx| this.research_toggle_allow_all(cx))),
    );
    if state.projects.projects.is_empty() {
        return col.child(div().text_size(px(12.)).text_color(rgb(t.text_secondary)).child("No research projects yet."));
    }
    for (i, p) in state.projects.projects.iter().enumerate() {
        let open = state.open.as_deref() == Some(p.id.as_str());
        let (label, color) = state_label(p.state);
        let (done, total) = steps_done(p);
        let id = p.id.clone();
        let allowed = state.projects.allowed(p);
        let mut card = div()
            .id(("research-row", i))
            .flex()
            .flex_col()
            .gap(px(4.))
            .p(px(8.))
            .rounded(px(8.))
            .bg(rgb(t.surface_variant))
            .border_1()
            .border_color(rgb(if open { t.accent } else { t.surface_variant }))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| this.research_open(id.clone(), cx)))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(6.))
                    .child(div().flex_1().min_w(px(0.)).overflow_hidden().text_size(px(13.)).text_color(rgb(t.text_primary)).child(p.title.clone()))
                    .child(div().flex_shrink_0().text_size(px(11.)).text_color(rgb(color(t))).child(label)),
            )
            .child(div().text_size(px(11.)).text_color(rgb(t.text_secondary)).child(format!(
                "{} · step {done} of {total} · {} pages{}{}",
                p.companion,
                p.usage.pages,
                if allowed { " · Always allow" } else { "" },
                if p.signed_in { " · signed in" } else { "" }
            )));
        card =
            card.child(div().h(px(4.)).rounded(px(2.)).bg(rgb(t.background)).child(
                div().h(px(4.)).rounded(px(2.)).bg(rgb(color(t))).w(gpui::relative(if total == 0 { 0. } else { done as f32 / total as f32 })),
            ));
        if open {
            card = card.child(project_detail(t, state, p, i, cx));
        }
        col = col.child(card);
    }
    col
}

fn project_detail(t: Theme, state: &ResearchState, p: &ResearchProject, i: usize, cx: &mut Context<CompanionView>) -> gpui::Div {
    let id = p.id.clone();
    let mut d = div().flex().flex_col().gap(px(6.)).mt(px(4.));
    d = d.child(div().text_size(px(12.)).text_color(rgb(t.text_primary)).child(p.brief.clone()));
    for s in &p.plan {
        let (mark, c) = match s.status {
            StepStatus::Done => ("✓", t.accent),
            StepStatus::Running => ("…", t.accent_alt),
            StepStatus::Failed => ("✗", t.error),
            StepStatus::Skipped => ("–", t.text_secondary),
            StepStatus::Queued => ("·", t.text_secondary),
        };
        d = d.child(div().text_size(px(11.)).text_color(rgb(c)).child(format!("{mark} {}", s.title)));
    }
    let running = p.state == ProjectState::Running;
    let ended = matches!(p.state, ProjectState::Done | ProjectState::Stopped);
    let (a, b, c) = (id.clone(), id.clone(), id.clone());
    let mut row = div().flex().flex_row().flex_wrap().gap(px(4.));
    if !ended {
        row = row.child(if running {
            button(t, ("research-pause", i), "Pause", false)
                .on_click(cx.listener(move |this, _, _, cx| this.research_set(a.clone(), ProjectState::Paused, cx)))
        } else {
            button(t, ("research-resume", i), "Resume", true)
                .on_click(cx.listener(move |this, _, _, cx| this.research_set(a.clone(), ProjectState::Running, cx)))
        });
        row = row.child(
            button(t, ("research-stop", i), "Stop", false)
                .on_click(cx.listener(move |this, _, _, cx| this.research_set(b.clone(), ProjectState::Stopped, cx))),
        );
    }
    row = row.child(
        chip(t, ("research-allow", i), if p.always_allow { "Always allow: on" } else { "Always allow: off" }, p.always_allow)
            .on_click(cx.listener(move |this, _, _, cx| this.research_toggle_allow(c.clone(), cx))),
    );
    d = d.child(row);
    d = d.child(div().text_size(px(10.)).text_color(rgb(t.text_secondary)).child(if state.projects.allowed(p) {
        "Always allow: no per-day, per-project or pages-per-hour caps. Off sites, signed-in choice, click and fill cards, Pause and Stop, power rules and the per-site limit still apply."
    } else {
        "Caps apply: 60 pages and 90 min a day, 300 pages, 14 days, 30 pages an hour."
    }));
    if !p.findings.is_empty() {
        d = d.child(div().text_size(px(11.)).text_color(rgb(t.text_primary)).child(format!("Findings: {}", p.findings.len())));
        if let Some(f) = p.findings.last() {
            let mut text: String = f.text.chars().take(220).collect();
            if f.text.chars().count() > 220 {
                text.push('…');
            }
            d = d.child(div().text_size(px(11.)).text_color(rgb(t.text_secondary)).child(format!("{}: {text}", f.step)));
            if !f.sources.is_empty() {
                d = d.child(div().text_size(px(10.)).text_color(rgb(t.accent)).child(format!(
                    "{} sources: {}",
                    f.sources.len(),
                    f.sources.join(" · ")
                )));
            }
        }
    }
    for l in p.log.iter().rev().take(4) {
        d = d.child(div().text_size(px(10.)).text_color(rgb(t.text_secondary)).child(l.text.clone()));
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_are_added_changed_and_kept_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::new(dir.path());
        let mut s = ResearchState::load(&store);
        assert!(s.projects.projects.is_empty());
        let id = s.add("How does Tokio cancel a task?", "atlas");
        assert_eq!(s.open.as_deref(), Some(id.as_str()));
        s.set_state(&id, ProjectState::Running);
        assert_eq!(s.active(), 1);
        s.toggle_allow(&id);
        s.toggle_allow_all();
        s.save(&store);
        let back = ResearchState::load(&store);
        let p = back.projects.get(&id).unwrap();
        assert!(p.always_allow && back.projects.always_allow_all);
        // A project that was running when the app closed comes back paused.
        assert_eq!(p.state, ProjectState::Paused);
        assert_eq!(back.active(), 0);
        assert!(!p.signed_in, "Always allow never turns on the signed-in session");
    }
}
