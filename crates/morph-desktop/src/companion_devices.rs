//! The panel's view of the user's other devices: a `morph-services` `node_router` for the
//! profile while its sync node runs, with what each linked device says it can do, and where a
//! companion's task runs.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use morph_ai::{CompanionProfile, CompanionRoute, NodeRouter, Platform, route_companion};
use morph_services::node_router::{self, Capabilities, DeviceRouter, NodeDirectory};
use weirdnet_agent::AgentKind;

/// How often the panel checks the sync node and asks the devices again.
const EVERY: Duration = Duration::from_secs(10);
const ASK_TIMEOUT: Duration = Duration::from_secs(3);

struct Links {
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
}

/// What this desktop tells its linked devices. It does not serve models to them yet (that
/// needs the `remote::infer` service, which comes with the `weirdnet-agent` 0.2.0 pin), so
/// phones do not offload here.
fn this_device() -> Capabilities {
    Capabilities { platform: Platform::Desktop, serves_models: false }
}

impl CompanionDevices {
    pub fn new(home: PathBuf, profile: String) -> Self {
        Self { home, profile, links: None, checked: None, asking: Arc::new(AtomicBool::new(false)) }
    }

    /// From the panel's poll: at most every [`EVERY`], follow the sync node (a router while it
    /// runs, none while it does not) and ask the linked devices what they can do.
    pub fn tick(&mut self) {
        if self.checked.is_some_and(|t| t.elapsed() < EVERY) {
            return;
        }
        self.checked = Some(Instant::now());
        let Some(started_at) = morph_services::sync::net_status(&self.home, &self.profile).map(|s| s.started_at) else {
            self.links = None;
            return;
        };
        if self.links.as_ref().is_none_or(|l| l.started_at != started_at) {
            self.links = node_router::for_profile(&self.home, &self.profile, this_device)
                .map(|(router, directory)| Links { router, directory, started_at })
                .map_err(|e| eprintln!("morph-companions: linked devices: {e}"))
                .ok();
        }
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

    /// Where `companion`'s task runs.
    pub fn route(&self, companion: &CompanionProfile) -> CompanionRoute {
        let nodes = self.links.as_ref().map(|l| l.router.nodes()).unwrap_or_default();
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
    fn without_sync_a_node_companion_says_why_and_local_ones_run() {
        let tmp = std::env::temp_dir().join(format!("morph-companion-devices-{}", std::process::id()));
        let mut d = CompanionDevices::new(tmp, "p1".into());
        d.tick();
        let atlas = CompanionProfile::on_node("atlas", "Atlas", "12D3KooWDesk", ModelChoice::Gemma4E4B);
        assert!(d.not_here(&atlas).is_some_and(|r| r.contains("turn on sync")));
        let local = CompanionProfile::local("local", "Local", ModelChoice::Gemma4E2B);
        assert_eq!(d.not_here(&local), None);
        assert_eq!(d.route(&local), CompanionRoute::Here);
    }
}
