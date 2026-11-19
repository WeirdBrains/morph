//! The panel's view of the user's other devices: a `morph-services` `node_router` for the
//! profile while its sync node runs, with what each linked device says it can do, and where a
//! companion's task runs.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use morph_ai::{
    AiStore, CompanionProfile, CompanionRoute, LocalInferHost, LocalModels, ModelChoice, ModelProvider, ModelStatus, NodeRouter, Platform,
    route_companion, serving_choice,
};
use morph_services::infer_serving::{InferServing, serve_inference};
use morph_services::node_router::{self, Capabilities, DeviceRouter, NodeDirectory};
use weirdnet_agent::AgentKind;
use weirdnet_agent::remote::infer::Admission;

use crate::companion_host::{LockCheck, profile_lock};

/// How often the panel checks the sync node and asks the devices again.
const EVERY: Duration = Duration::from_secs(10);
const ASK_TIMEOUT: Duration = Duration::from_secs(3);

struct Links {
    /// This device's model served to the linked ones, while the setting is on (and the claim
    /// that makes this panel the process's one registrar).
    serving: Option<(InferServing, Claim)>,
    router: DeviceRouter,
    directory: NodeDirectory,
    /// The node's start time: a restarted node needs a new router.
    started_at: i64,
}

pub struct CompanionDevices {
    home: PathBuf,
    profile: String,
    links: Option<Links>,
    checked: Option<Instant>,
    asking: Arc<AtomicBool>,
    locked: LockCheck,
    serving: Option<ModelServing>,
    /// This panel, among the process's windows (see [`Claim`]).
    id: u64,
}

/// What serving this device's model needs from the panel: the models, the companions folder
/// (the setting and the roster live there) and the worker's busy flag.
#[derive(Clone)]
pub struct ModelServing {
    pub models: Arc<LocalModels>,
    pub companions: PathBuf,
    pub busy: Arc<AtomicBool>,
    /// A research project of this device is running (held for the whole run, released while
    /// paused), so a remote turn cannot slip in between its steps.
    pub project: Arc<AtomicBool>,
}

impl ModelServing {
    /// "Serve my models to my other devices" (Settings > Agents), read live: it syncs and is
    /// toggled from the settings page.
    fn enabled(&self) -> bool {
        morph_services::profile_io::file_store(&self.companions).load_settings().ok().flatten().is_some_and(|s| s.offload.serve)
    }

    /// The model served: the default local companion's.
    fn choice(&self) -> ModelChoice {
        let roster = morph_services::profile_io::file_store(&self.companions).load_roster().ok().flatten();
        roster.map(|r| serving_choice(&r)).unwrap_or_else(|| ModelChoice::default_for(Platform::Desktop))
    }

    /// What this desktop tells its linked devices: it serves models when the setting is on and
    /// the model is downloaded and runs on this CPU, so a device never offloads to one that
    /// would refuse.
    fn capabilities(&self) -> Capabilities {
        let ready = matches!(self.models.status(&self.choice()), ModelStatus::Ready { .. });
        Capabilities { platform: Platform::Desktop, serves_models: self.enabled() && ready }
    }
}

/// Without model serving (tests) a device serves none.
fn capabilities_of(serving: &Option<ModelServing>) -> Capabilities {
    serving.as_ref().map(ModelServing::capabilities).unwrap_or(Capabilities { platform: Platform::Desktop, serves_models: false })
}

/// Inference is registered once per process and profile, not once per window: the first panel to
/// register holds the claim until it stops serving (setting off, lock, node restart, window
/// closed), when another window's panel takes over at its next tick.
static CLAIMS: std::sync::Mutex<Vec<(String, u64)>> = std::sync::Mutex::new(Vec::new());
static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

struct Claim {
    key: String,
    id: u64,
}

impl Claim {
    /// The claim on `key` for panel `id`, unless another panel holds it.
    fn take(key: &str, id: u64) -> Option<Self> {
        let mut claims = CLAIMS.lock().unwrap_or_else(|p| p.into_inner());
        if claims.iter().any(|(k, _)| k == key) {
            return None;
        }
        claims.push((key.to_string(), id));
        Some(Self { key: key.to_string(), id })
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        CLAIMS.lock().unwrap_or_else(|p| p.into_inner()).retain(|(k, i)| !(*k == self.key && *i == self.id));
    }
}

/// Whether inference is registered now: the setting is on and the profile is unlocked.
fn wants_serving(enabled: bool, locked: bool) -> bool {
    enabled && !locked
}

impl CompanionDevices {
    pub fn new(home: PathBuf, profile: String) -> Self {
        let locked = profile_lock(home.clone(), profile.clone());
        Self {
            home,
            profile,
            links: None,
            checked: None,
            asking: Arc::new(AtomicBool::new(false)),
            locked,
            serving: None,
            id: NEXT_ID.fetch_add(1, Ordering::SeqCst),
        }
    }

    /// Follow the panel's profile lock check (the one its hosts refuse on), so the two never
    /// disagree; tests set their own.
    pub fn with_lock(self, locked: LockCheck) -> Self {
        Self { locked, ..self }
    }

    /// Serve this device's model to the linked devices while the setting is on (and the profile
    /// is unlocked); the setting itself is read live by the host, so turning it off refuses at
    /// once and the registration follows at the next tick.
    pub fn with_model_serving(self, serving: ModelServing) -> Self {
        Self { serving: Some(serving), ..self }
    }

    /// What serving this device's model needs, when it is set up.
    pub fn model_serving(&self) -> Option<&ModelServing> {
        self.serving.as_ref()
    }

    /// From the panel's poll: at most every [`EVERY`], follow the sync node (a router while it
    /// runs, none while it does not) and ask the linked devices what they can do.
    pub fn tick(&mut self) {
        if self.checked.is_some_and(|t| t.elapsed() < EVERY) {
            return;
        }
        self.checked = Some(Instant::now());
        // A locked profile reaches no device and serves nothing: the registration goes with it.
        if (self.locked)() {
            self.links = None;
            return;
        }
        let Some(started_at) = morph_services::sync::net_status(&self.home, &self.profile).map(|s| s.started_at) else {
            self.links = None;
            return;
        };
        if self.links.as_ref().is_none_or(|l| l.started_at != started_at) {
            let mine = self.serving.clone();
            self.links = node_router::for_profile(&self.home, &self.profile, move || capabilities_of(&mine))
                .map(|(router, directory)| Links { serving: None, router, directory, started_at })
                .map_err(|e| {
                    if !matches!(e, morph_services::device_calls::DeviceCallError::Locked) {
                        eprintln!("morph-companions: linked devices: {e}");
                    }
                })
                .ok();
        }
        self.follow_serving();
        if let Some(l) = &self.links
            && !self.asking.swap(true, Ordering::SeqCst)
        {
            let (caller, directory, asking) = (l.router.caller().clone(), l.directory.clone(), self.asking.clone());
            std::thread::spawn(move || {
                directory.refresh_blocking(&caller, ASK_TIMEOUT);
                asking.store(false, Ordering::SeqCst);
            });
        }
    }

    /// Register inference on the router while it should be served, drop it when it should not
    /// (setting off) or when it ended (the node stopped accepting).
    fn follow_serving(&mut self) {
        let (Some(l), Some(s)) = (self.links.as_mut(), self.serving.as_ref()) else { return };
        if l.serving.as_ref().is_some_and(|(x, _)| !x.running()) {
            l.serving = None;
        }
        if !wants_serving(s.enabled(), (self.locked)()) {
            l.serving = None;
            return;
        }
        if l.serving.is_none() {
            let Some(claim) = Claim::take(&format!("{}|{}", self.home.display(), self.profile), self.id) else { return };
            let s = s.clone();
            let make = move || {
                let (on, choice, busy, project) = (s.clone(), s.clone(), s.busy.clone(), s.project.clone());
                LocalInferHost::new(s.models.clone() as Arc<dyn ModelProvider>, move || choice.choice(), move || on.enabled())
                    .with_local_priority(move || busy.load(Ordering::SeqCst) || project.load(Ordering::SeqCst))
            };
            match serve_inference(&l.router, make, Some((self.home.clone(), self.profile.clone())), Admission::with_queue(1, 2)) {
                Ok(serving) => l.serving = Some((serving, claim)),
                Err(e) => eprintln!("morph-companions: serving models to linked devices: {e}"),
            }
        }
    }

    /// Where `companion`'s task runs.
    pub fn route(&self, companion: &CompanionProfile) -> CompanionRoute {
        let nodes = self.links.as_ref().map(|l| l.router.nodes()).unwrap_or_default();
        // A locked profile reaches no device (`DeviceCaller::for_profile` refuses it).
        if nodes.is_empty() && companion.identity.kind == AgentKind::Node && (self.locked)() {
            return CompanionRoute::Unavailable {
                reason: format!("{} runs on another device; this profile is locked, so unlock it with its password first", companion.name()),
            };
        }
        if nodes.is_empty() && companion.identity.kind == AgentKind::Node {
            return CompanionRoute::Unavailable {
                reason: format!("{} runs on another device; turn on sync in Settings to reach it", companion.name()),
            };
        }
        route_companion(companion, &nodes)
    }

    /// Why `companion`'s task can't run from this panel now, for Node companions (others keep
    /// their own handling).
    pub fn not_here(&self, companion: &CompanionProfile) -> Option<String> {
        if companion.identity.kind != AgentKind::Node {
            return None;
        }
        match self.route(companion) {
            CompanionRoute::Here => None,
            CompanionRoute::Node { label, .. } => {
                Some(format!("{} runs on {label}. Asking it from here comes in the next update; until then, ask it on {label}.", companion.name()))
            }
            CompanionRoute::Unavailable { reason } => Some(reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use morph_ai::ModelChoice;

    #[test]
    fn inference_is_registered_only_while_the_setting_is_on_and_the_profile_unlocked() {
        assert!(wants_serving(true, false));
        assert!(!wants_serving(false, false), "off");
        assert!(!wants_serving(true, true), "locked");
        assert!(!capabilities_of(&None).serves_models, "no model serving: nothing is offered to offload to");
    }

    #[test]
    fn one_panel_registers_inference_per_profile_and_another_takes_over_when_it_stops() {
        let (a, b) = (NEXT_ID.fetch_add(1, Ordering::SeqCst), NEXT_ID.fetch_add(1, Ordering::SeqCst));
        let first = Claim::take("home-x|p1", a).expect("the first window registers");
        assert!(Claim::take("home-x|p1", b).is_none(), "the second window does not");
        assert!(Claim::take("home-x|p2", b).is_some(), "another profile has its own node");
        drop(first);
        assert!(Claim::take("home-x|p1", b).is_some(), "the claim is free once the first stopped serving");
    }

    #[test]
    fn without_sync_a_node_companion_says_why_and_local_ones_run() {
        let tmp = std::env::temp_dir().join(format!("morph-companion-devices-{}", std::process::id()));
        let mut d = CompanionDevices::new(tmp.clone(), "p1".into()).with_lock(Arc::new(|| false));
        d.tick();
        let atlas = CompanionProfile::on_node("atlas", "Atlas", "12D3KooWDesk", ModelChoice::Gemma4E4B);
        assert!(d.not_here(&atlas).is_some_and(|r| r.contains("turn on sync")));
        let locked = CompanionDevices::new(tmp, "p1".into()).with_lock(Arc::new(|| true));
        assert!(locked.not_here(&atlas).is_some_and(|r| r.contains("this profile is locked")), "{:?}", locked.not_here(&atlas));
        let local = CompanionProfile::local("local", "Local", ModelChoice::Gemma4E2B);
        assert_eq!(d.not_here(&local), None);
        assert_eq!(d.route(&local), CompanionRoute::Here);
    }
}
