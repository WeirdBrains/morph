//! The Research tab of the Companions panel (design: "Long-term Research Projects"): the list
//! of projects with their state, and one project open with its plan, Pause / Resume / Stop, the
//! Always allow choice for the research tools (this project, or all projects) and its log.
//! The state lives in `companions/research.json` (`morph_ai::research`); this file draws it.

use gpui::{Context, div, prelude::*, px, rgb};
use morph_ai::FileStore;
use morph_ai::research::{ProjectState, Projects, ResearchProject, StepStatus};
use morph_ai::steer::{DiffKind, SourceRule, Steer, Verdict, host_of};
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
    /// The history lists only steering entries (steers, replans, undos, check-ins).
    pub steering_only: bool,
}

impl ResearchState {
    pub fn load(store: &FileStore) -> Self {
        let bytes = store.read_file(FILE).ok().flatten();
        let projects = Projects::from_bytes(bytes.as_deref(), now_ms()).unwrap_or_default();
        Self { projects, open: None, steering_only: false }
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

    /// Records a steer for a project and applies it now when no step is running; otherwise it
    /// lands at the step boundary.
    pub fn steer(&mut self, id: &str, steer: Steer) {
        let now = now_ms();
        if let Some(p) = self.projects.get_mut(id) {
            p.submit_steer(now, "this device", steer);
            p.apply_pending(now);
        }
    }

    pub fn undo_replan(&mut self, id: &str, replan: &str) -> bool {
        self.projects.get_mut(id).is_some_and(|p| p.undo_replan(now_ms(), replan))
    }

    pub fn toggle_wait_for_me(&mut self, id: &str) {
        let now = now_ms();
        if let Some(p) = self.projects.get_mut(id) {
            p.steering.wait_for_me = !p.steering.wait_for_me;
            let on = p.steering.wait_for_me;
            p.log_steer(now, if on { "check-ins now wait for you" } else { "check-ins no longer wait for you" });
        }
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
pub fn body(t: Theme, state: &ResearchState, companion: &str, cx: &mut Context<CompanionView>) -> gpui::Stateful<gpui::Div> {
    let allow_all = state.projects.always_allow_all;
    let mut col = div().id("research-body").flex().flex_col().flex_1().min_h(px(0.)).gap(px(6.)).overflow_y_scroll();
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
            .child(
                div()
                    .id(("research-head", i))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.research_open(id.clone(), cx)))
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
    for (n, s) in p.plan.iter().enumerate() {
        let (mark, c) = match s.status {
            StepStatus::Done => ("✓", t.accent),
            StepStatus::Running => ("…", t.accent_alt),
            StepStatus::Failed => ("✗", t.error),
            StepStatus::Skipped => ("–", t.text_secondary),
            StepStatus::Queued => ("·", t.text_secondary),
        };
        let mut line = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .child(div().flex_1().min_w(px(0.)).text_size(px(11.)).text_color(rgb(c)).child(format!("{mark} {}", s.title)));
        if s.status == StepStatus::Queued {
            let rid = id.clone();
            line = line.child(
                chip(t, ("research-rm", i * 1000 + n), "Remove", false)
                    .on_click(cx.listener(move |this, _, _, cx| this.research_steer(rid.clone(), Steer::RemoveStep { index: n }, cx))),
            );
        }
        d = d.child(line);
    }
    if p.steering.plan_version > 1 || !p.steering.pending.is_empty() {
        let waiting = p.steering.pending.len();
        d = d.child(div().text_size(px(10.)).text_color(rgb(t.text_secondary)).child(format!(
            "Plan v{}{}",
            p.steering.plan_version,
            if waiting > 0 { format!(" · {waiting} steer(s) wait for the step to end") } else { String::new() }
        )));
    }
    d = d.children(replan_card(t, p, i, cx));
    d = d.children(checkin_card(t, p, i, cx));
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
    d = d.children(report_block(t, p, i, cx));
    d = d.child(steer_row(t, p, i, cx));
    d = d.children(sources_block(t, p, i, cx));
    if !p.findings.is_empty() {
        d = d.child(div().text_size(px(11.)).text_color(rgb(t.text_primary)).child(format!("Findings: {}", p.findings.len())));
        let first = p.findings.len().saturating_sub(2);
        for (n, f) in p.findings.iter().enumerate().skip(first) {
            let mut text: String = f.text.chars().take(160).collect();
            if f.text.chars().count() > 160 {
                text.push('…');
            }
            let mark = match p.steering.verdict_of(n) {
                Some(Verdict::Useful) => " · useful",
                Some(Verdict::Wrong) => " · wrong",
                Some(Verdict::DigDeeper) => " · dig deeper",
                None => "",
            };
            d = d.child(div().text_size(px(11.)).text_color(rgb(t.text_secondary)).child(format!("{}{mark}: {text}", f.step)));
            let mut fb = div().flex().flex_row().flex_wrap().gap(px(4.));
            for (k, (label, v)) in
                [("Useful", Verdict::Useful), ("Wrong", Verdict::Wrong), ("Dig deeper", Verdict::DigDeeper)].into_iter().enumerate()
            {
                let rid = id.clone();
                fb =
                    fb.child(chip(t, ("research-fb", i * 1000 + n * 10 + k), label, p.steering.verdict_of(n) == Some(v)).on_click(
                        cx.listener(move |this, _, _, cx| this.research_steer(rid.clone(), Steer::Feedback { finding: n, verdict: v }, cx)),
                    ));
            }
            d = d.child(fb);
        }
    }
    d = d.child(
        chip(t, ("research-history", i), if state.steering_only { "History: steering only" } else { "History: everything" }, state.steering_only)
            .on_click(cx.listener(|this, _, _, cx| this.research_toggle_history(cx))),
    );
    for l in p.log.iter().rev().filter(|l| !state.steering_only || l.steer).take(7) {
        d = d.child(div().text_size(px(10.)).text_color(rgb(if l.steer { t.accent_alt } else { t.text_secondary })).child(l.text.clone()));
    }
    d
}

/// The living report: its revision, what changed since the last one, and Export.
fn report_block(t: Theme, p: &ResearchProject, i: usize, cx: &mut Context<CompanionView>) -> Option<gpui::Div> {
    let last = p.reports.last()?;
    let mut c = div().flex().flex_col().gap(px(2.)).child(div().text_size(px(11.)).text_color(rgb(t.text_primary)).child(format!(
        "Report: revision {} of {} kept · {}",
        last.n,
        p.reports.len(),
        last.why
    )));
    if let Some(prev) = p.reports.iter().rev().nth(1) {
        let diff = p.report_diff(prev.n, last.n);
        let (add, rem) = (diff.iter().filter(|l| l.kind == DiffKind::Added).count(), diff.iter().filter(|l| l.kind == DiffKind::Removed).count());
        c = c.child(
            div().text_size(px(10.)).text_color(rgb(t.text_secondary)).child(format!("Since revision {}: +{add} lines, −{rem} lines", prev.n)),
        );
        for l in diff.iter().take(3) {
            let (mark, col) = if l.kind == DiffKind::Added { ("+", t.accent) } else { ("−", t.error) };
            let mut text: String = l.text.chars().take(90).collect();
            if l.text.chars().count() > 90 {
                text.push('…');
            }
            c = c.child(div().text_size(px(10.)).text_color(rgb(col)).child(format!("{mark} {text}")));
        }
    }
    let pid = p.id.clone();
    Some(
        c.child(
            div().flex().flex_row().child(
                button(t, ("research-export", i), "Export as Markdown", false)
                    .on_click(cx.listener(move |this, _, _, cx| this.research_export(pid.clone(), cx))),
            ),
        ),
    )
}

/// The latest replan: a diff of the plan with its reason, and Undo while it can be undone.
fn replan_card(t: Theme, p: &ResearchProject, i: usize, cx: &mut Context<CompanionView>) -> Option<gpui::Div> {
    let r = p.steering.replans.last()?;
    let mut c = div().flex().flex_col().gap(px(2.)).p(px(6.)).rounded(px(6.)).bg(rgb(t.background)).child(
        div().text_size(px(11.)).text_color(rgb(t.accent_alt)).child(format!(
            "Replanned (v{}){}: {}",
            r.version,
            if r.undone { " · undone" } else { "" },
            r.reason
        )),
    );
    for l in &r.diff {
        let (mark, col) = match l.kind {
            DiffKind::Added => ("+", t.accent),
            DiffKind::Removed => ("−", t.error),
            DiffKind::Changed => ("~", t.accent_alt),
        };
        c = c.child(div().text_size(px(10.)).text_color(rgb(col)).child(format!("{mark} {}", l.text)));
    }
    if p.can_undo(&r.id) {
        let (pid, rid) = (p.id.clone(), r.id.clone());
        c = c.child(
            div().flex().flex_row().child(
                button(t, ("research-undo", i), "Undo replan", false)
                    .on_click(cx.listener(move |this, _, _, cx| this.research_undo(pid.clone(), rid.clone(), cx))),
            ),
        );
    }
    Some(c)
}

/// A direction check-in waiting for an answer: its options as chips.
fn checkin_card(t: Theme, p: &ResearchProject, i: usize, cx: &mut Context<CompanionView>) -> Option<gpui::Div> {
    let c = p.steering.open_checkin()?;
    let mut row = div().flex().flex_row().flex_wrap().gap(px(4.));
    for (k, o) in c.options.iter().enumerate() {
        let (pid, cid, choice) = (p.id.clone(), c.id.clone(), o.clone());
        row = row.child(button(t, ("research-checkin", i * 100 + k), o.clone(), k == 0).on_click(
            cx.listener(move |this, _, _, cx| this.research_steer(pid.clone(), Steer::CheckIn { id: cid.clone(), choice: choice.clone() }, cx)),
        ));
    }
    Some(
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .p(px(6.))
            .rounded(px(6.))
            .border_1()
            .border_color(rgb(t.accent_alt))
            .child(div().text_size(px(11.)).text_color(rgb(t.text_primary)).child(format!("Check-in: {}", c.question)))
            .child(row)
            .child(div().text_size(px(10.)).text_color(rgb(t.text_secondary)).child(if p.steering.wait_for_me {
                "The project waits for you. Or type your own direction below and press Note."
            } else {
                "No answer: the project goes on with its plan. Or type your own direction below and press Note."
            })),
    )
}

/// Buttons that steer with what is typed in the box below.
fn steer_row(t: Theme, p: &ResearchProject, i: usize, cx: &mut Context<CompanionView>) -> gpui::Div {
    use crate::companion_view::SteerInput;
    let row =
        div().flex().flex_col().gap(px(4.)).child(
            div().text_size(px(10.)).text_color(rgb(t.text_secondary)).child("Steer with the box below (it lands when the running step ends):"),
        );
    let mut chips = div().flex().flex_row().flex_wrap().gap(px(4.));
    let kinds = [
        ("Note", SteerInput::Note),
        ("Change question", SteerInput::Brief),
        ("Add step", SteerInput::AddStep),
        ("Pin", SteerInput::Source(SourceRule::Pin)),
        ("Prefer", SteerInput::Source(SourceRule::Prefer)),
        ("Exclude", SteerInput::Source(SourceRule::Exclude)),
    ];
    for (k, (label, kind)) in kinds.into_iter().enumerate() {
        let pid = p.id.clone();
        chips = chips.child(
            button(t, ("research-steer", i * 100 + k), label, false)
                .on_click(cx.listener(move |this, _, _, cx| this.research_steer_input(pid.clone(), kind, cx))),
        );
    }
    let wid = p.id.clone();
    chips = chips.child(
        chip(t, ("research-wait", i), if p.steering.wait_for_me { "Check-ins wait for me" } else { "Check-ins don't wait" }, p.steering.wait_for_me)
            .on_click(cx.listener(move |this, _, _, cx| this.research_toggle_wait(wid.clone(), cx))),
    );
    row.child(chips)
}

/// The source rules, and Pin / Exclude for the sites of the latest finding.
fn sources_block(t: Theme, p: &ResearchProject, i: usize, cx: &mut Context<CompanionView>) -> Vec<gpui::Div> {
    let mut out = Vec::new();
    if !p.steering.sources.is_empty() {
        let mut row = div().flex().flex_row().flex_wrap().gap(px(4.));
        for (k, s) in p.steering.sources.iter().enumerate() {
            let (pid, pat) = (p.id.clone(), s.pattern.clone());
            row = row.child(
                chip(t, ("research-rule", i * 100 + k), format!("{} {} ×", s.rule.label(), s.pattern), s.rule != SourceRule::Exclude)
                    .on_click(cx.listener(move |this, _, _, cx| this.research_steer(pid.clone(), Steer::ForgetSource { pattern: pat.clone() }, cx))),
            );
        }
        out.push(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().text_size(px(10.)).text_color(rgb(t.text_secondary)).child("Source rules (click to drop)"))
                .child(row),
        );
    }
    if let Some(f) = p.findings.last() {
        let mut hosts: Vec<String> = Vec::new();
        for u in &f.sources {
            let h = host_of(u);
            if !h.is_empty() && !hosts.contains(&h) && !p.steering.sources.iter().any(|s| s.pattern.eq_ignore_ascii_case(&h)) {
                hosts.push(h);
            }
        }
        for (k, h) in hosts.into_iter().take(3).enumerate() {
            let mut row = div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(4.))
                .child(div().flex_1().min_w(px(0.)).text_size(px(10.)).text_color(rgb(t.accent)).child(h.clone()));
            for (m, (label, rule)) in
                [("Pin", SourceRule::Pin), ("Prefer", SourceRule::Prefer), ("Exclude", SourceRule::Exclude)].into_iter().enumerate()
            {
                let (pid, pat) = (p.id.clone(), h.clone());
                row =
                    row.child(chip(t, ("research-src", i * 1000 + k * 10 + m), label, false).on_click(
                        cx.listener(move |this, _, _, cx| this.research_steer(pid.clone(), Steer::Source { pattern: pat.clone(), rule }, cx)),
                    ));
            }
            out.push(row);
        }
    }
    out
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

    #[test]
    fn a_steer_lands_at_once_with_no_step_running_waits_otherwise_and_is_kept_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::new(dir.path());
        let mut s = ResearchState::load(&store);
        let id = s.add("How does Tokio cancel a task?", "atlas");
        s.steer(&id, Steer::Brief { text: "Async drop proposals".into() });
        assert_eq!(s.projects.get(&id).unwrap().brief, "Async drop proposals");
        s.projects.get_mut(&id).unwrap().plan[0].status = StepStatus::Running;
        s.steer(&id, Steer::Note { text: "only 2022 and later".into() });
        assert_eq!(s.projects.get(&id).unwrap().steering.pending.len(), 1, "a running step is not cut");
        s.toggle_wait_for_me(&id);
        s.save(&store);
        let back = ResearchState::load(&store);
        let p = back.projects.get(&id).unwrap();
        assert_eq!((p.steering.pending.len(), p.steering.replans.len(), p.steering.wait_for_me), (1, 1, true));
        let rid = p.steering.replans[0].id.clone();
        let mut back = back;
        // The step in flight went back to the queue on load, so the plan is what the replan made.
        assert!(back.undo_replan(&id, &rid));
        assert_eq!(back.projects.get(&id).unwrap().brief, "How does Tokio cancel a task?");
    }
}
