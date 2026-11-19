//! Long-term research projects (design: "Morph Companions — Long-term Research Projects"),
//! slice 0: a Morph-local spike with no UI. A project is a brief, a plan of steps, a usage
//! ledger and a log, saved after every step so a crash or restart resumes at the next
//! unfinished step. [`run_session`] is the one-session runner; it asks a [`StepRunner`] to do
//! each step (the model and browser hookup comes in a later slice).
//!
//! Always allow (decided 2026-11-19): a project, or all projects, may be allowed to use the
//! research tools without the budget caps. It lifts the daily and project caps, the
//! pages-per-hour limit and the budget check-ins. It does not lift: Off sites (the step runner
//! and hosts still enforce them), logged-out by default (`signed_in` is its own opt-in),
//! cards for clicks, fills and submits, Pause and Stop, the power and battery rules, and the
//! per-site rate limit ([`SiteLimiter`], always on).
//!
//! Generic parts (the plan and checkpoint model, multi-level caps, the limiter, run policy)
//! are meant to move to `weirdnet-agent` later; that release needs m3talsmith's OK.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use weirdnet_agent::{Budget, CancelToken};

use crate::file_io::FileIo;
use crate::ids::new_id;
use crate::store::StoreError;

const HOUR_MS: u64 = 3_600_000;
const DAY_MS: u64 = 24 * HOUR_MS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectState {
    Running,
    Paused,
    /// A card or a direction check-in waits for the user.
    NeedsYou,
    Done,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Queued,
    Running,
    Done,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStep {
    pub title: String,
    /// The site the step will touch, for the per-site rate limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<String>,
    pub status: StepStatus,
    /// How many times it was started (a step in flight at a crash is retried once).
    #[serde(default)]
    pub attempts: u32,
}

impl PlanStep {
    pub fn new(title: &str) -> Self {
        Self { title: title.into(), site: None, status: StepStatus::Queued, attempts: 0 }
    }
}

/// What a project has used. Day and hour windows are kept as timestamps, not clocks, so a
/// restart keeps them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub pages: u32,
    pub tool_ms: u64,
    /// `(at_ms, pages, tool_ms)` per finished step in the last day.
    #[serde(default)]
    pub recent: Vec<(u64, u32, u64)>,
}

impl Usage {
    fn record(&mut self, now: u64, pages: u32, tool_ms: u64) {
        self.pages += pages;
        self.tool_ms += tool_ms;
        self.recent.retain(|(at, ..)| now.saturating_sub(*at) < DAY_MS);
        self.recent.push((now, pages, tool_ms));
    }

    fn within(&self, now: u64, window: u64) -> (u32, u64) {
        self.recent.iter().filter(|(at, ..)| now.saturating_sub(*at) < window).fold((0, 0), |(p, t), (_, pages, ms)| (p + pages, t + ms))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    pub at_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResearchProject {
    pub id: String,
    pub title: String,
    pub brief: String,
    /// Companion name (roster entry) that owns the work.
    pub companion: String,
    pub state: ProjectState,
    /// Always allow for the research tools in this project (see the module docs).
    #[serde(default)]
    pub always_allow: bool,
    /// Research in the profile's signed-in session. Its own opt-in; never implied by Always allow.
    #[serde(default)]
    pub signed_in: bool,
    pub plan: Vec<PlanStep>,
    #[serde(default)]
    pub usage: Usage,
    /// Sessions run so far.
    #[serde(default)]
    pub sessions: u32,
    #[serde(default)]
    pub notes: Vec<LogEntry>,
    #[serde(default)]
    pub log: Vec<LogEntry>,
    pub created_ms: u64,
}

impl ResearchProject {
    pub fn new(title: &str, brief: &str, companion: &str, plan: Vec<PlanStep>, now: u64) -> Self {
        Self {
            id: new_id("research"),
            title: title.into(),
            brief: brief.into(),
            companion: companion.into(),
            state: ProjectState::Paused,
            always_allow: false,
            signed_in: false,
            plan,
            usage: Usage::default(),
            sessions: 0,
            notes: Vec::new(),
            log: Vec::new(),
            created_ms: now,
        }
    }

    pub fn log(&mut self, now: u64, text: impl Into<String>) {
        self.log.push(LogEntry { at_ms: now, text: text.into() });
    }

    /// A note from the user; it shapes the next step (steering, slice 4a).
    pub fn add_note(&mut self, now: u64, text: &str) {
        self.notes.push(LogEntry { at_ms: now, text: text.into() });
        self.log(now, format!("note: {text}"));
    }

    /// The next step to run: the first that is queued.
    pub fn next_step(&self) -> Option<usize> {
        self.plan.iter().position(|s| s.status == StepStatus::Queued)
    }

    /// After a crash or restart: a step left `Running` goes back to `Queued` once, then fails.
    pub fn recover(&mut self, now: u64) {
        for i in 0..self.plan.len() {
            if self.plan[i].status == StepStatus::Running {
                let retry = self.plan[i].attempts < 2;
                self.plan[i].status = if retry { StepStatus::Queued } else { StepStatus::Failed };
                let title = self.plan[i].title.clone();
                self.log(now, format!("{} after a restart: {title}", if retry { "retrying" } else { "gave up" }));
            }
        }
        if self.state == ProjectState::Running {
            self.state = ProjectState::Paused;
        }
    }
}

/// Every project, and the setting that applies Always allow to all of them. One file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Projects {
    #[serde(default)]
    pub projects: Vec<ResearchProject>,
    /// "Apply Always allow to all projects".
    #[serde(default)]
    pub always_allow_all: bool,
}

impl Projects {
    pub fn load(io: &dyn FileIo, path: &std::path::Path, now: u64) -> Result<Self, StoreError> {
        let mut me: Self = match io.read(path)? {
            Some(bytes) => serde_json::from_slice(&bytes).map_err(|e| StoreError::Io(e.to_string()))?,
            None => Self::default(),
        };
        for p in &mut me.projects {
            p.recover(now);
        }
        Ok(me)
    }

    pub fn save(&self, io: &dyn FileIo, path: &std::path::Path) -> Result<(), StoreError> {
        io.write(path, &serde_json::to_vec_pretty(self).map_err(|e| StoreError::Io(e.to_string()))?)
    }

    pub fn get(&self, id: &str) -> Option<&ResearchProject> {
        self.projects.iter().find(|p| p.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut ResearchProject> {
        self.projects.iter_mut().find(|p| p.id == id)
    }

    /// Whether the research tools run unthrottled for `project`.
    pub fn allowed(&self, project: &ResearchProject) -> bool {
        project.always_allow || self.always_allow_all
    }
}

/// The caps that apply when Always allow is off (design defaults, 2026-11-19).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    pub day_pages: u32,
    pub day_tool: Duration,
    pub total_pages: u32,
    pub max_age: Duration,
    pub pages_per_hour: u32,
}

impl Default for Caps {
    fn default() -> Self {
        Self {
            day_pages: 60,
            day_tool: Duration::from_secs(90 * 60),
            total_pages: 300,
            max_age: Duration::from_millis(14 * DAY_MS),
            pages_per_hour: 30,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CapHit {
    #[error("today's page cap is used")]
    DayPages,
    #[error("today's tool-time cap is used")]
    DayTool,
    #[error("the pages-per-hour limit is reached")]
    PagesPerHour,
    #[error("the project's page total is used")]
    TotalPages,
    #[error("the project is past its time limit")]
    TooOld,
}

/// The budget of one session. Always allow: no practical page, search, step or time limit
/// (a page still times out after `per_page`). Otherwise: 25 pages and 10 minutes of tool time.
pub fn session_budget(always_allow: bool) -> Budget {
    if always_allow {
        Budget {
            max_searches: 1_000_000,
            max_pages: 1_000_000,
            per_task: Duration::from_secs(365 * 24 * 3600),
            max_steps: 1_000_000,
            ..Budget::default()
        }
    } else {
        Budget { max_searches: 10, max_pages: 25, per_task: Duration::from_secs(600), max_steps: 64, ..Budget::default() }
    }
}

/// Which cap stops the project now, if any. Always allow stops none of them.
pub fn check_caps(project: &ResearchProject, caps: &Caps, always_allow: bool, now: u64) -> Result<(), CapHit> {
    if always_allow {
        return Ok(());
    }
    let (day_pages, day_ms) = project.usage.within(now, DAY_MS);
    let (hour_pages, _) = project.usage.within(now, HOUR_MS);
    if now.saturating_sub(project.created_ms) > caps.max_age.as_millis() as u64 {
        return Err(CapHit::TooOld);
    }
    if project.usage.pages >= caps.total_pages {
        return Err(CapHit::TotalPages);
    }
    if day_pages >= caps.day_pages {
        return Err(CapHit::DayPages);
    }
    if day_ms >= caps.day_tool.as_millis() as u64 {
        return Err(CapHit::DayTool);
    }
    if hour_pages >= caps.pages_per_hour {
        return Err(CapHit::PagesPerHour);
    }
    Ok(())
}

/// A polite per-site rate limit, always on, even with Always allow: one request per
/// `gap` per site, backing off (doubling, to five minutes) after a 429 or 503. It is not a
/// budget; it keeps the project from getting blocked.
#[derive(Debug, Clone)]
pub struct SiteLimiter {
    gap_ms: u64,
    next_ok: HashMap<String, u64>,
    strikes: HashMap<String, u32>,
}

impl SiteLimiter {
    /// One request per five seconds per site.
    pub const DEFAULT_GAP: Duration = Duration::from_secs(5);
    const MAX_BACKOFF_MS: u64 = 300_000;

    pub fn new(gap: Duration) -> Self {
        Self { gap_ms: gap.as_millis() as u64, next_ok: HashMap::new(), strikes: HashMap::new() }
    }

    /// Milliseconds to wait before `site` may be asked again (0: now).
    pub fn wait_ms(&self, site: &str, now: u64) -> u64 {
        self.next_ok.get(site).map_or(0, |at| at.saturating_sub(now))
    }

    /// A request to `site` went out.
    pub fn record(&mut self, site: &str, now: u64) {
        self.strikes.remove(site);
        self.next_ok.insert(site.into(), now + self.gap_ms);
    }

    /// The site answered 429 or 503: wait longer each time.
    pub fn back_off(&mut self, site: &str, now: u64) {
        let n = self.strikes.entry(site.into()).or_insert(0);
        *n += 1;
        let wait = (self.gap_ms << (*n).min(10)).min(Self::MAX_BACKOFF_MS);
        self.next_ok.insert(site.into(), now + wait);
    }
}

/// The device's state for the power and battery rules (never lifted by Always allow).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Power {
    pub on_ac: bool,
    /// Percent; `None` for a machine with no battery.
    pub battery_pct: Option<u8>,
    /// The device is busy with other work (the user's own tasks).
    pub busy: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Hold {
    #[error("waiting for power")]
    NeedsPower,
    #[error("battery is under 30%")]
    LowBattery,
    #[error("the device is busy")]
    Busy,
}

/// Power and CPU etiquette: run on power only, pause under 30%, defer when busy unless the
/// user pressed Run now.
pub fn may_run(power: Power, run_now: bool) -> Result<(), Hold> {
    if !power.on_ac && power.battery_pct.is_some() {
        return Err(Hold::NeedsPower);
    }
    if power.battery_pct.is_some_and(|p| p < 30) {
        return Err(Hold::LowBattery);
    }
    if power.busy && !run_now {
        return Err(Hold::Busy);
    }
    Ok(())
}

/// What a step did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StepResult {
    pub pages: u32,
    pub tool_ms: u64,
    pub summary: String,
    /// The site answered 429 or 503.
    pub rate_limited: bool,
}

/// Does one plan step. The model and browser hookup implements this in a later slice.
pub trait StepRunner {
    fn run_step(&mut self, project: &ResearchProject, step: usize) -> Result<StepResult, String>;
}

/// Why a session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEnd {
    /// Every step is done.
    Finished,
    /// The user paused or stopped.
    Interrupted,
    Capped(CapHit),
    Held(Hold),
    /// The next step's site needs this many ms of politeness.
    SiteWait(u64),
    /// A step failed; the project keeps going on the next session.
    StepFailed,
}

/// What a session needs from its caller.
pub struct SessionEnv<'a> {
    pub caps: Caps,
    pub power: Power,
    pub run_now: bool,
    pub limiter: &'a mut SiteLimiter,
    pub cancel: &'a CancelToken,
    pub clock: &'a dyn Fn() -> u64,
}

/// Runs steps until the plan is done, the user pauses or stops, a cap or the power rules
/// stop it, or the site needs a wait. Saves a checkpoint (`save`) before and after every step.
pub fn run_session(
    projects: &mut Projects,
    id: &str,
    runner: &mut dyn StepRunner,
    env: &mut SessionEnv<'_>,
    save: &mut dyn FnMut(&Projects) -> Result<(), StoreError>,
) -> Result<SessionEnd, StoreError> {
    let allow = projects.get(id).is_some_and(|p| projects.allowed(p));
    let now = (env.clock)();
    {
        let Some(p) = projects.get_mut(id) else { return Ok(SessionEnd::Interrupted) };
        p.state = ProjectState::Running;
        p.sessions += 1;
        p.log(now, format!("session {} started{}", p.sessions, if allow { " (Always allow: no budget caps)" } else { "" }));
    }
    save(projects)?;
    let end = loop {
        let now = (env.clock)();
        let Some(p) = projects.get(id) else { break SessionEnd::Interrupted };
        if env.cancel.is_cancelled() || p.state != ProjectState::Running {
            break SessionEnd::Interrupted;
        }
        let Some(i) = p.next_step() else { break SessionEnd::Finished };
        if let Err(h) = may_run(env.power, env.run_now) {
            break SessionEnd::Held(h);
        }
        if let Err(c) = check_caps(p, &env.caps, allow, now) {
            break SessionEnd::Capped(c);
        }
        let site = p.plan[i].site.clone();
        if let Some(site) = &site {
            let wait = env.limiter.wait_ms(site, now);
            if wait > 0 {
                break SessionEnd::SiteWait(wait);
            }
        }
        {
            let p = projects.get_mut(id).expect("checked above");
            p.plan[i].status = StepStatus::Running;
            p.plan[i].attempts += 1;
        }
        save(projects)?;
        let result = runner.run_step(projects.get(id).expect("checked above"), i);
        let now = (env.clock)();
        let p = projects.get_mut(id).expect("checked above");
        match result {
            Ok(r) => {
                if let Some(site) = &site {
                    if r.rate_limited {
                        env.limiter.back_off(site, now);
                    } else {
                        env.limiter.record(site, now);
                    }
                }
                p.plan[i].status = if r.rate_limited { StepStatus::Queued } else { StepStatus::Done };
                p.usage.record(now, r.pages, r.tool_ms);
                let title = p.plan[i].title.clone();
                p.log(now, format!("{}: {title} {}", if r.rate_limited { "backed off" } else { "done" }, r.summary));
                save(projects)?;
            }
            Err(e) => {
                p.plan[i].status = if p.plan[i].attempts >= 2 { StepStatus::Failed } else { StepStatus::Queued };
                let title = p.plan[i].title.clone();
                p.log(now, format!("step failed: {title}: {e}"));
                save(projects)?;
                break SessionEnd::StepFailed;
            }
        }
    };
    let now = (env.clock)();
    let p = projects.get_mut(id).expect("project exists");
    // Pause and Stop set their own state; keep it.
    if matches!(p.state, ProjectState::Running) {
        p.state = match end {
            SessionEnd::Finished => ProjectState::Done,
            _ => ProjectState::Paused,
        };
    }
    p.log(now, format!("session ended: {end:?}"));
    save(projects)?;
    Ok(end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_io::PlainIo;
    use std::cell::Cell;

    fn power() -> Power {
        Power { on_ac: true, battery_pct: None, busy: false }
    }

    fn plan(n: usize) -> Vec<PlanStep> {
        (1..=n).map(|i| PlanStep::new(&format!("step {i}"))).collect()
    }

    struct Fixed {
        pages: u32,
        tool_ms: u64,
        fail_at: Option<usize>,
        ran: Vec<usize>,
    }

    impl StepRunner for Fixed {
        fn run_step(&mut self, _: &ResearchProject, step: usize) -> Result<StepResult, String> {
            if self.fail_at == Some(step) {
                self.fail_at = None;
                return Err("network".into());
            }
            self.ran.push(step);
            Ok(StepResult { pages: self.pages, tool_ms: self.tool_ms, summary: "ok".into(), rate_limited: false })
        }
    }

    fn setup(n: usize) -> (Projects, String) {
        let mut ps = Projects::default();
        let p = ResearchProject::new("t", "brief", "atlas", plan(n), 1_000);
        let id = p.id.clone();
        ps.projects.push(p);
        (ps, id)
    }

    fn run(ps: &mut Projects, id: &str, runner: &mut Fixed, power: Power, limiter: &mut SiteLimiter, now: &Cell<u64>) -> SessionEnd {
        let cancel = CancelToken::default();
        let clock = || {
            now.set(now.get() + 1);
            now.get()
        };
        let mut env = SessionEnv { caps: Caps::default(), power, run_now: false, limiter, cancel: &cancel, clock: &clock };
        run_session(ps, id, runner, &mut env, &mut |_| Ok(())).unwrap()
    }

    #[test]
    fn a_session_runs_the_plan_and_finishes() {
        let (mut ps, id) = setup(3);
        let now = Cell::new(10_000);
        let end = run(
            &mut ps,
            &id,
            &mut Fixed { pages: 2, tool_ms: 1000, fail_at: None, ran: vec![] },
            power(),
            &mut SiteLimiter::new(Duration::ZERO),
            &now,
        );
        assert_eq!(end, SessionEnd::Finished);
        let p = ps.get(&id).unwrap();
        assert_eq!(p.state, ProjectState::Done);
        assert_eq!(p.usage.pages, 6);
    }

    #[test]
    fn resume_after_a_crash_picks_the_next_unfinished_step() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("research.json");
        let (mut ps, id) = setup(4);
        // The runner dies in step 3: its checkpoint says Running, and the process goes away.
        struct Dies;
        impl StepRunner for Dies {
            fn run_step(&mut self, p: &ResearchProject, step: usize) -> Result<StepResult, String> {
                if step == 2 {
                    panic!("killed in flight with {} steps queued", p.plan.len());
                }
                Ok(StepResult::default())
            }
        }
        let cancel = CancelToken::default();
        let now = Cell::new(2_000);
        let clock = || {
            now.set(now.get() + 1);
            now.get()
        };
        let mut limiter = SiteLimiter::new(Duration::ZERO);
        let mut env = SessionEnv { caps: Caps::default(), power: power(), run_now: false, limiter: &mut limiter, cancel: &cancel, clock: &clock };
        let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = run_session(&mut ps, &id, &mut Dies, &mut env, &mut |p| p.save(&PlainIo, &path));
        }));
        assert!(crashed.is_err());
        // A new process: load from disk. Steps 1 and 2 are done; step 3 retries once.
        let mut again = Projects::load(&PlainIo, &path, 5_000).unwrap();
        let p = again.get(&id).unwrap();
        assert_eq!(p.state, ProjectState::Paused);
        assert_eq!(p.plan[0].status, StepStatus::Done);
        assert_eq!(p.plan[1].status, StepStatus::Done);
        assert_eq!(p.plan[2].status, StepStatus::Queued);
        assert_eq!(p.next_step(), Some(2));
        let mut runner = Fixed { pages: 1, tool_ms: 10, fail_at: None, ran: vec![] };
        let end = run(&mut again, &id, &mut runner, power(), &mut SiteLimiter::new(Duration::ZERO), &now);
        assert_eq!(end, SessionEnd::Finished);
        assert_eq!(runner.ran, vec![2, 3]);
    }

    #[test]
    fn a_step_in_flight_is_retried_once_then_fails() {
        let (mut ps, id) = setup(1);
        {
            let p = ps.get_mut(&id).unwrap();
            p.plan[0].status = StepStatus::Running;
            p.plan[0].attempts = 1;
            p.recover(5);
            assert_eq!(p.plan[0].status, StepStatus::Queued);
            p.plan[0].status = StepStatus::Running;
            p.plan[0].attempts = 2;
            p.recover(6);
            assert_eq!(p.plan[0].status, StepStatus::Failed);
        }
    }

    #[test]
    fn a_failed_step_ends_the_session_and_the_next_one_retries_it() {
        let (mut ps, id) = setup(2);
        let now = Cell::new(3_000);
        let mut lim = SiteLimiter::new(Duration::ZERO);
        let mut runner = Fixed { pages: 1, tool_ms: 1, fail_at: Some(0), ran: vec![] };
        assert_eq!(run(&mut ps, &id, &mut runner, power(), &mut lim, &now), SessionEnd::StepFailed);
        assert_eq!(ps.get(&id).unwrap().plan[0].status, StepStatus::Queued);
        assert_eq!(run(&mut ps, &id, &mut runner, power(), &mut lim, &now), SessionEnd::Finished);
    }

    #[test]
    fn pause_and_stop_end_the_session() {
        let (mut ps, id) = setup(3);
        let now = Cell::new(1);
        // A runner that pauses the project after its first step.
        struct Pauses;
        impl StepRunner for Pauses {
            fn run_step(&mut self, _: &ResearchProject, _: usize) -> Result<StepResult, String> {
                Ok(StepResult::default())
            }
        }
        let cancel = CancelToken::default();
        let clock = || {
            now.set(now.get() + 1);
            now.get()
        };
        let mut lim = SiteLimiter::new(Duration::ZERO);
        let mut env = SessionEnv { caps: Caps::default(), power: power(), run_now: false, limiter: &mut lim, cancel: &cancel, clock: &clock };
        let mut saves = 0;
        let end = run_session(&mut ps, &id, &mut Pauses, &mut env, &mut |p| {
            saves += 1;
            // After the first step is saved as done, the user presses Stop.
            if p.get(&id).is_some_and(|x| x.plan[0].status == StepStatus::Done) {
                cancel.cancel();
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(end, SessionEnd::Interrupted);
        assert_eq!(ps.get(&id).unwrap().plan[1].status, StepStatus::Queued);
        assert!(saves >= 3);
    }

    #[test]
    fn caps_stop_a_project_but_always_allow_lifts_them() {
        let (mut ps, id) = setup(10);
        let now = Cell::new(10_000);
        // 40 pages a step: the second step is past the 30 pages an hour.
        let end = run(
            &mut ps,
            &id,
            &mut Fixed { pages: 40, tool_ms: 1000, fail_at: None, ran: vec![] },
            power(),
            &mut SiteLimiter::new(Duration::ZERO),
            &now,
        );
        assert_eq!(end, SessionEnd::Capped(CapHit::PagesPerHour));
        assert_eq!(ps.get(&id).unwrap().state, ProjectState::Paused);
        // Always allow for this project: the same project runs on.
        ps.get_mut(&id).unwrap().always_allow = true;
        let end = run(
            &mut ps,
            &id,
            &mut Fixed { pages: 40, tool_ms: 1000, fail_at: None, ran: vec![] },
            power(),
            &mut SiteLimiter::new(Duration::ZERO),
            &now,
        );
        assert_eq!(end, SessionEnd::Finished);
        assert_eq!(ps.get(&id).unwrap().usage.pages, 400);
    }

    #[test]
    fn always_allow_for_all_projects_applies_to_each_project() {
        let (mut ps, a) = setup(2);
        let b = ResearchProject::new("b", "b", "atlas", plan(2), 1);
        let bid = b.id.clone();
        ps.projects.push(b);
        assert!(!ps.allowed(ps.get(&a).unwrap()));
        ps.always_allow_all = true;
        assert!(ps.allowed(ps.get(&a).unwrap()) && ps.allowed(ps.get(&bid).unwrap()));
        let mut p = ps.get(&a).unwrap().clone();
        p.usage.pages = 10_000;
        assert_eq!(check_caps(&p, &Caps::default(), false, 5), Err(CapHit::TotalPages));
        assert_eq!(check_caps(&p, &Caps::default(), true, 5), Ok(()));
    }

    #[test]
    fn caps_cover_hour_day_tool_time_total_and_age() {
        let caps = Caps::default();
        let (mut ps, id) = setup(1);
        let p = ps.get_mut(&id).unwrap();
        let t = 100 * HOUR_MS;
        p.created_ms = t - HOUR_MS;
        p.usage.record(t - 1000, 30, 0);
        assert_eq!(check_caps(p, &caps, false, t), Err(CapHit::PagesPerHour));
        assert_eq!(check_caps(p, &caps, false, t + 2 * HOUR_MS), Ok(()));
        p.usage.record(t - 500, 0, 91 * 60_000);
        assert_eq!(check_caps(p, &caps, false, t + 2 * HOUR_MS), Err(CapHit::DayTool));
        p.usage = Usage::default();
        p.created_ms = 0;
        assert_eq!(check_caps(p, &caps, false, 15 * DAY_MS), Err(CapHit::TooOld));
    }

    #[test]
    fn the_site_limit_applies_even_with_always_allow() {
        let (mut ps, id) = setup(2);
        ps.get_mut(&id).unwrap().always_allow = true;
        for s in &mut ps.get_mut(&id).unwrap().plan {
            s.site = Some("docs.rs".into());
        }
        let now = Cell::new(100);
        let mut lim = SiteLimiter::new(SiteLimiter::DEFAULT_GAP);
        let mut runner = Fixed { pages: 1, tool_ms: 1, fail_at: None, ran: vec![] };
        let end = run(&mut ps, &id, &mut runner, power(), &mut lim, &now);
        assert!(matches!(end, SessionEnd::SiteWait(ms) if ms > 4_000 && ms <= 5_000), "{end:?}");
        assert_eq!(runner.ran, vec![0]);
        now.set(100 + 6_000);
        assert_eq!(run(&mut ps, &id, &mut runner, power(), &mut lim, &now), SessionEnd::Finished);
    }

    #[test]
    fn a_429_backs_off_and_the_step_stays_queued() {
        let mut lim = SiteLimiter::new(Duration::from_secs(5));
        lim.back_off("a.example", 0);
        assert_eq!(lim.wait_ms("a.example", 0), 10_000);
        lim.back_off("a.example", 0);
        assert_eq!(lim.wait_ms("a.example", 0), 20_000);
        for _ in 0..20 {
            lim.back_off("a.example", 0);
        }
        assert_eq!(lim.wait_ms("a.example", 0), 300_000);
        lim.record("a.example", 0);
        assert_eq!(lim.wait_ms("a.example", 0), 5_000);
        assert_eq!(lim.wait_ms("other.example", 0), 0);
    }

    #[test]
    fn power_rules_hold_even_with_always_allow() {
        let on = Power { on_ac: true, battery_pct: Some(80), busy: false };
        assert_eq!(may_run(on, false), Ok(()));
        assert_eq!(may_run(Power { on_ac: false, ..on }, false), Err(Hold::NeedsPower));
        assert_eq!(may_run(Power { battery_pct: Some(29), ..on }, false), Err(Hold::LowBattery));
        assert_eq!(may_run(Power { busy: true, ..on }, false), Err(Hold::Busy));
        assert_eq!(may_run(Power { busy: true, ..on }, true), Ok(()));
        assert_eq!(may_run(Power { on_ac: true, battery_pct: None, busy: false }, false), Ok(()));
        // A machine with no battery reading is a desktop on power.
        assert_eq!(may_run(Power { on_ac: false, battery_pct: None, busy: false }, false), Ok(()));
        let (mut ps, id) = setup(2);
        ps.get_mut(&id).unwrap().always_allow = true;
        let now = Cell::new(1);
        let off = Power { on_ac: false, battery_pct: Some(90), busy: false };
        let end =
            run(&mut ps, &id, &mut Fixed { pages: 1, tool_ms: 1, fail_at: None, ran: vec![] }, off, &mut SiteLimiter::new(Duration::ZERO), &now);
        assert_eq!(end, SessionEnd::Held(Hold::NeedsPower));
    }

    #[test]
    fn always_allow_never_turns_on_the_signed_in_session() {
        let (mut ps, id) = setup(1);
        ps.always_allow_all = true;
        ps.get_mut(&id).unwrap().always_allow = true;
        assert!(!ps.get(&id).unwrap().signed_in);
        let b = session_budget(true);
        assert!(b.max_pages > 100_000 && b.max_steps > 100_000);
        assert_eq!(session_budget(false).max_pages, 25);
        assert_eq!(session_budget(false).per_task, Duration::from_secs(600));
    }

    #[test]
    fn projects_round_trip_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("research.json");
        let (mut ps, id) = setup(2);
        ps.always_allow_all = true;
        ps.get_mut(&id).unwrap().add_note(5, "ignore anything before 2022");
        ps.save(&PlainIo, &path).unwrap();
        let back = Projects::load(&PlainIo, &path, 9).unwrap();
        assert!(back.always_allow_all);
        assert_eq!(back.get(&id).unwrap().notes.len(), 1);
        assert!(Projects::load(&PlainIo, &dir.path().join("none.json"), 1).unwrap().projects.is_empty());
    }
}
