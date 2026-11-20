//! Which device runs a research project, and steers from the others (design: "owner lease").
//!
//! One device owns a project at a time. Only the owner runs its steps, applies its steers,
//! posts its check-ins and writes its body to sync. Any other device can look at it and queue
//! steers; they travel as their own records and the owner takes them in at its next step
//! boundary. Taking over is an explicit act of the user on the new device: the old owner sees
//! the new owner in the merged body and stops. There is no timeout: a lease never lapses by
//! itself, so a laptop that is asleep keeps its projects until you take them over.
//!
//! The device id is made once per install and kept in `companions/research-device.json`; it is
//! never synced. `signed_in` and `always_allow` are per-device choices and do not travel either.

use serde::{Deserialize, Serialize};

use crate::ids::new_id;
use crate::research::{ProjectState, Projects, ResearchProject, StepStatus};
use crate::steer::{Steer, SteerRecord};

/// The file (under `companions/`) that holds this install's device id.
pub const DEVICE_FILE: &str = "research-device.json";

/// Most remote steer ids remembered per project.
const SEEN_MAX: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
}

impl Device {
    pub fn new(name: &str) -> Self {
        Self { id: new_id("device"), name: name.into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Owner {
    pub device: String,
    pub name: String,
    pub since_ms: u64,
}

/// A steer waiting for the project's owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedSteer {
    pub project: String,
    pub record: SteerRecord,
}

impl QueuedSteer {
    /// The sync record id for this steer.
    pub fn key(&self) -> String {
        format!("{}/{}", self.project, self.record.id)
    }
}

impl ResearchProject {
    /// Whether `device` runs this project (a project nobody claimed yet counts as everyone's).
    pub fn owned_by(&self, device: &str) -> bool {
        self.owner.as_ref().is_none_or(|o| o.device == device)
    }

    /// Makes `device` the owner.
    pub fn claim(&mut self, device: &Device, now: u64) {
        self.owner = Some(Owner { device: device.id.clone(), name: device.name.clone(), since_ms: now });
    }

    /// Takes the project over from another device: it is paused, a step that was in flight there
    /// goes back to the queue, and the history says from whom. Returns false when it is ours already.
    pub fn take_over(&mut self, device: &Device, now: u64) -> bool {
        if self.owned_by(&device.id) && self.owner.is_some() {
            return false;
        }
        let from = self.owner.as_ref().map(|o| o.name.clone()).unwrap_or_else(|| "nobody".into());
        self.claim(device, now);
        for s in &mut self.plan {
            if s.status == StepStatus::Running {
                s.status = StepStatus::Queued;
            }
        }
        if self.state == ProjectState::Running {
            self.state = ProjectState::Paused;
        }
        self.log_steer(now, format!("taken over by {} from {from}; paused, resume when ready", device.name));
        true
    }

    /// Takes in a steer another device queued: recorded as it was made (id, time, device) and
    /// applied at the next boundary. A steer seen before is ignored. Returns whether it was new.
    pub fn accept_remote(&mut self, rec: SteerRecord, now: u64) -> bool {
        if self.steering.seen_remote.contains(&rec.id) {
            return false;
        }
        self.steering.seen_remote.push(rec.id.clone());
        let extra = self.steering.seen_remote.len().saturating_sub(SEEN_MAX);
        self.steering.seen_remote.drain(..extra);
        if let Steer::CheckIn { id, choice } = &rec.steer
            && let Some(c) = self.steering.checkins.iter_mut().find(|c| c.id == *id && c.answer.is_none())
        {
            c.answer = Some(choice.clone());
        }
        self.log_steer(now, format!("steer ({}): {}", rec.device, rec.steer.describe()));
        self.steering.pending.push(SteerRecord { applied_ms: None, ..rec });
        true
    }
}

impl Projects {
    /// Whether this device runs `p` (always, when no device is set).
    pub fn mine(&self, p: &ResearchProject) -> bool {
        self.device.is_empty() || p.owned_by(&self.device)
    }

    /// Whether a project this device runs is running now (not paused, waiting for the user,
    /// done or stopped): the model is wanted for the whole run, between its steps too.
    pub fn running_here(&self) -> bool {
        self.projects.iter().any(|p| p.state == crate::research::ProjectState::Running && self.mine(p))
    }

    /// A steer for project `id` made on this device: queued for the owner when another device
    /// owns it. Returns false when this device owns it (the caller records it directly) or the
    /// project is unknown.
    pub fn queue_steer(&mut self, id: &str, device: &Device, now: u64, steer: Steer) -> bool {
        let Some(p) = self.get(id) else { return false };
        if p.owned_by(&device.id) {
            return false;
        }
        let owner = p.owner.as_ref().map(|o| o.name.clone()).unwrap_or_default();
        let rec = SteerRecord { id: new_id("steer"), at_ms: now, device: device.name.clone(), steer, applied_ms: None };
        let said = rec.steer.describe();
        self.queued.push(QueuedSteer { project: id.into(), record: rec });
        // Not written to the project: the body belongs to its owner.
        let _ = (owner, said);
        true
    }

    /// The steers queued for projects this device owns are taken into them (and leave the
    /// queue). Returns how many were new.
    pub fn drain_queued(&mut self, now: u64) -> usize {
        let queued = std::mem::take(&mut self.queued);
        let mut rest = Vec::new();
        let mut taken = 0;
        for q in queued {
            let device_mine = self.get(&q.project).is_some_and(|p| self.mine(p));
            if device_mine {
                if let Some(p) = self.get_mut(&q.project)
                    && p.accept_remote(q.record, now)
                {
                    taken += 1;
                }
            } else {
                rest.push(q);
            }
        }
        self.queued = rest;
        taken
    }

    /// How many steers wait for the owner of `project`.
    pub fn waiting(&self, project: &str) -> usize {
        self.queued.iter().filter(|q| q.project == project).count()
    }

    /// The files' content after a sync merge. `remote` and `queued` are what the merge holds;
    /// `current` is what the file has now (this device's own).
    ///
    /// - A project this device owns, in the merge as ours too, keeps this device's copy: it is
    ///   the only writer, and the merge may be a step behind.
    /// - Any other project is the merge's copy (so a takeover on another device is followed),
    ///   with this device's `signed_in` and `always_allow` kept (never synced; new ones are off).
    /// - A project or steer that is new here since the last pass (`known` says what the last
    ///   pass had) is kept, so nothing made in between is dropped.
    /// - A steer already taken in by the owner is not queued again.
    pub fn merged(current: Option<Projects>, remote: Vec<ResearchProject>, queued: Vec<QueuedSteer>, known: &dyn Fn(&str) -> bool) -> Projects {
        let current = current.unwrap_or_default();
        let device = current.device.clone();
        let mut out = Projects { device, always_allow_all: current.always_allow_all, ..Default::default() };
        for mut r in remote {
            match current.get(&r.id) {
                Some(l) if l.owned_by(&current.device) && r.owned_by(&current.device) && l.owner.is_some() => out.projects.push(l.clone()),
                Some(l) => {
                    r.signed_in = l.signed_in;
                    r.always_allow = l.always_allow;
                    out.projects.push(r);
                }
                None => {
                    r.signed_in = false;
                    r.always_allow = false;
                    out.projects.push(r);
                }
            }
        }
        for l in &current.projects {
            if !out.projects.iter().any(|p| p.id == l.id) && !known(&format!("p/{}", l.id)) {
                out.projects.push(l.clone());
            }
        }
        out.projects.sort_by(|a, b| b.created_ms.cmp(&a.created_ms).then_with(|| a.id.cmp(&b.id)));
        let mut all = queued;
        for l in &current.queued {
            if !all.iter().any(|q| q.key() == l.key()) && !known(&format!("q/{}", l.key())) {
                all.push(l.clone());
            }
        }
        all.retain(|q| out.get(&q.project).or_else(|| current.get(&q.project)).is_none_or(|p| !p.steering.seen_remote.contains(&q.record.id)));
        all.sort_by(|a, b| a.record.at_ms.cmp(&b.record.at_ms).then_with(|| a.record.id.cmp(&b.record.id)));
        out.queued = all;
        out
    }

    /// The project as it travels: without this device's own choices.
    pub fn synced_copy(p: &ResearchProject) -> ResearchProject {
        ResearchProject { signed_in: false, always_allow: false, ..p.clone() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(n: &str) -> Device {
        Device { id: format!("id-{n}"), name: n.into() }
    }

    fn owned(by: &Device) -> ResearchProject {
        let mut p = ResearchProject::from_question("How does Tokio cancel a task?", "atlas", 1);
        p.claim(by, 1);
        p.state = ProjectState::Running;
        p
    }

    fn projects_for(d: &Device, ps: Vec<ResearchProject>) -> Projects {
        Projects { device: d.id.clone(), projects: ps, ..Default::default() }
    }

    #[test]
    fn a_project_counts_as_running_here_only_while_it_runs_and_is_ours() {
        let (me, other) = (dev("me"), dev("other"));
        let mut projects = projects_for(&me, vec![owned(&me)]);
        assert!(projects.running_here(), "running and ours");
        for state in [ProjectState::Paused, ProjectState::NeedsYou, ProjectState::Done, ProjectState::Stopped] {
            projects.projects[0].state = state;
            assert!(!projects.running_here(), "{state:?} releases the model");
        }
        let theirs = projects_for(&me, vec![owned(&other)]);
        assert!(!theirs.running_here(), "another device's project does not hold this one's model");
    }

    #[test]
    fn only_the_owner_runs_a_project() {
        let (a, b) = (dev("desktop"), dev("laptop"));
        let p = owned(&a);
        let ps = projects_for(&a, vec![p.clone()]);
        assert!(crate::research::next_action(&ps, &Default::default(), 5) != crate::research::Next::Idle);
        let ps = projects_for(&b, vec![p]);
        assert_eq!(crate::research::next_action(&ps, &Default::default(), 5), crate::research::Next::Idle, "a lease held elsewhere");
    }

    #[test]
    fn a_steer_on_another_device_is_queued_and_the_owner_takes_it_in_once() {
        let (a, b) = (dev("desktop"), dev("laptop"));
        let p = owned(&a);
        let id = p.id.clone();
        let mut on_b = projects_for(&b, vec![p.clone()]);
        assert!(on_b.queue_steer(&id, &b, 10, Steer::Note { text: "only 2022 and later".into() }));
        assert_eq!(on_b.waiting(&id), 1);
        assert!(on_b.get(&id).unwrap().steering.pending.is_empty(), "the owner's body is not touched");
        let mut on_a = projects_for(&a, vec![p]);
        assert!(!on_a.queue_steer(&id, &a, 11, Steer::Note { text: "x".into() }), "the owner records its own steers directly");
        on_a.queued = on_b.queued.clone();
        assert_eq!(on_a.drain_queued(20), 1);
        assert!(on_a.queued.is_empty());
        let pa = on_a.get_mut(&id).unwrap();
        assert!(pa.log.iter().any(|l| l.steer && l.text == "steer (laptop): note: only 2022 and later"));
        pa.apply_pending(21);
        assert!(pa.step_prompt(0).contains("only 2022 and later"));
        // The same steer again (the merge may still hold it) is ignored.
        on_a.queued = on_b.queued.clone();
        assert_eq!(on_a.drain_queued(30), 0);
        assert!(on_a.queued.is_empty());
    }

    #[test]
    fn a_queued_steer_waits_while_its_owner_is_someone_else() {
        let (a, b, c) = (dev("a"), dev("b"), dev("c"));
        let p = owned(&a);
        let mut on_c = projects_for(&c, vec![p.clone()]);
        on_c.queued.push(QueuedSteer {
            project: p.id.clone(),
            record: SteerRecord { id: "s1".into(), at_ms: 1, device: "b".into(), steer: Steer::Note { text: "x".into() }, applied_ms: None },
        });
        assert_eq!(on_c.drain_queued(5), 0);
        assert_eq!(on_c.waiting(&p.id), 1);
        let _ = b;
    }

    #[test]
    fn a_takeover_pauses_requeues_the_step_in_flight_and_says_from_whom() {
        let (a, b) = (dev("desktop"), dev("laptop"));
        let mut p = owned(&a);
        p.plan[0].status = StepStatus::Running;
        assert!(!p.take_over(&a, 5), "already ours");
        assert!(p.take_over(&b, 6));
        assert!(p.owned_by(&b.id) && !p.owned_by(&a.id));
        assert_eq!((p.state, p.plan[0].status), (ProjectState::Paused, StepStatus::Queued));
        assert!(p.log.iter().any(|l| l.steer && l.text.contains("taken over by laptop from desktop")));
    }

    #[test]
    fn the_merge_keeps_our_own_copy_follows_a_takeover_and_keeps_device_choices_local() {
        let (a, b) = (dev("desktop"), dev("laptop"));
        // Ours, a step ahead of the merge.
        let mut ours = owned(&a);
        ours.findings.push(crate::research::Finding { step: "s".into(), text: "t".into(), sources: vec![], at_ms: 2 });
        ours.always_allow = true;
        ours.signed_in = true;
        let behind = Projects::synced_copy(&{
            let mut x = ours.clone();
            x.findings.clear();
            x
        });
        let cur = projects_for(&a, vec![ours.clone()]);
        let m = Projects::merged(Some(cur.clone()), vec![behind.clone()], vec![], &|_| true);
        assert_eq!(m.projects[0].findings.len(), 1, "the owner's copy is the newer one");
        // Taken over elsewhere: the merge wins, our device choices stay.
        let mut taken = behind.clone();
        taken.take_over(&b, 9);
        let m = Projects::merged(Some(cur), vec![Projects::synced_copy(&taken)], vec![], &|_| true);
        assert!(m.projects[0].owned_by(&b.id) && !m.projects[0].owned_by(&a.id));
        assert!(m.projects[0].always_allow && m.projects[0].signed_in, "never synced, kept here");
        // A project that is new here since the last pass is not dropped; a new one from the merge starts with both off.
        let fresh = owned(&a);
        let cur = projects_for(&a, vec![fresh.clone()]);
        let m = Projects::merged(Some(cur), vec![Projects::synced_copy(&taken)], vec![], &|_| false);
        assert_eq!(m.projects.len(), 2);
        assert!(m.projects.iter().all(|p| p.id == fresh.id || (!p.signed_in && !p.always_allow) || p.id == taken.id));
        assert!(!Projects::synced_copy(&ours).signed_in && !Projects::synced_copy(&ours).always_allow);
    }

    #[test]
    fn the_merge_drops_a_steer_the_owner_already_took_in() {
        let a = dev("a");
        let mut p = owned(&a);
        let rec = SteerRecord { id: "s1".into(), at_ms: 1, device: "b".into(), steer: Steer::Note { text: "x".into() }, applied_ms: None };
        p.accept_remote(rec.clone(), 2);
        let q = QueuedSteer { project: p.id.clone(), record: rec };
        let m = Projects::merged(Some(projects_for(&a, vec![p])), vec![], vec![q], &|_| true);
        assert!(m.queued.is_empty());
    }
}
