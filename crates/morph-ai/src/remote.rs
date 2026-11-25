//! Routing to the user's other devices: Node companions (a companion whose agent loop runs on
//! another linked device) and inference offload (a phone, or any device, sending model turns to
//! a stronger one).
//!
//! The transport sits behind [`NodeRouter`] (calling) and [`LinkHandler`] (serving), so this
//! code does not depend on how devices are reached. `morph-services` `node_router` implements
//! both over Weirdnet streams on the device's identity (`device_calls::DeviceCaller`), and the
//! generic protocols on top (`weirdnet-agent` `remote::infer` and `remote::task`) run over the
//! [`Link`] it opens.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use weirdnet_agent::AgentKind;

use crate::model::Platform;
use crate::roster::CompanionProfile;

/// One of the user's linked, unrevoked devices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedNode {
    pub peer_id: String,
    pub label: String,
    pub platform: Platform,
    /// Reachable now (seen by discovery or a relay).
    pub online: bool,
    pub this_device: bool,
    /// Has a local model it will serve to the user's other devices.
    pub serves_models: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RouteError {
    #[error("{0} is not one of your linked devices")]
    NotLinked(String),
    #[error("{0} is not reachable")]
    Unreachable(String),
    /// The device does not accept the protocol (a build from before it): ask an older one.
    #[error("{0} is not reachable")]
    Unsupported(String),
    #[error("the connection closed")]
    Closed,
    #[error("{0}")]
    Failed(String),
}

#[async_trait]
pub trait LinkTx: Send {
    async fn send(&mut self, message: Vec<u8>) -> Result<(), RouteError>;
}

/// `recv` must be cancel-safe (it is raced against cancellation).
#[async_trait]
pub trait LinkRx: Send {
    async fn recv(&mut self) -> Result<Option<Vec<u8>>, RouteError>;
}

/// A message channel to one linked device for one app protocol.
pub struct Link {
    pub tx: Box<dyn LinkTx>,
    pub rx: Box<dyn LinkRx>,
}

/// How Morph reaches the user's other devices.
#[async_trait]
pub trait NodeRouter: Send + Sync {
    /// Linked, unrevoked devices from the sync roster, this one included and marked.
    fn nodes(&self) -> Vec<LinkedNode>;

    /// Open a channel to a linked device. Implementations authenticate the far end as one of
    /// the user's linked devices and refuse anything else; the serving side checks the caller
    /// the same way before it runs anything.
    async fn open(&self, peer: &str, protocol: &str) -> Result<Link, RouteError>;
}

/// The serving side: runs one protocol session on a link a linked device opened. The router
/// has checked the caller against the roster before this is called.
#[async_trait]
pub trait LinkHandler: Send + Sync {
    /// `from` is the caller's peer id (transport-authenticated). Returning ends the session.
    async fn handle(&self, from: String, link: Link);
}

/// Where a companion's task runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompanionRoute {
    /// On this device.
    Here,
    /// On another linked device.
    Node { peer: String, label: String },
    /// Cannot run now; the reason is shown in the panel.
    Unavailable { reason: String },
}

/// Pick where a companion runs, given the linked devices.
pub fn route_companion(profile: &CompanionProfile, nodes: &[LinkedNode]) -> CompanionRoute {
    match profile.identity.kind {
        AgentKind::Local => CompanionRoute::Here,
        AgentKind::External => CompanionRoute::Unavailable { reason: format!("{} connects from outside Morph (over MCP)", profile.name()) },
        AgentKind::Node => {
            let Some(host) = profile.host.as_deref() else {
                return CompanionRoute::Unavailable { reason: format!("{} has no device to run on; choose one in Settings, Agents", profile.name()) };
            };
            match nodes.iter().find(|n| n.peer_id == host) {
                Some(n) if n.this_device => CompanionRoute::Here,
                Some(n) if n.online => CompanionRoute::Node { peer: n.peer_id.clone(), label: n.label.clone() },
                Some(n) => CompanionRoute::Unavailable { reason: format!("{} runs on {}, which is offline", profile.name(), n.label) },
                None => CompanionRoute::Unavailable { reason: format!("{} ran on a device that is no longer linked", profile.name()) },
            }
        }
        // `AgentKind` is non-exhaustive: a kind this build does not know cannot be routed.
        _ => CompanionRoute::Unavailable { reason: format!("{} is a kind of companion this version cannot run", profile.name()) },
    }
}

/// When model turns go to another device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OffloadMode {
    /// Phones send turns to an online desktop that serves models; desktops run locally.
    #[default]
    Auto,
    /// Send turns to another device whenever one that serves models is online.
    Always,
    /// Always run here.
    Never,
}

/// Offload settings (part of [`crate::AgentSettings`], so they sync).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OffloadSettings {
    #[serde(default)]
    pub mode: OffloadMode,
    /// A device (peer id) to try first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefer: Option<String>,
    /// Run here when no device can take the turn.
    #[serde(default = "yes")]
    pub fallback_local: bool,
    /// "Serve my models to my other devices": this device answers model turns from the user's
    /// linked devices with its local model. Off until the user turns it on; synced with the rest.
    #[serde(default, skip_serializing_if = "is_false")]
    pub serve: bool,
    /// "Run on: Ymir pool": model turns go to the user's Ymir model cloud (inference nodes among
    /// the linked devices) instead of following `mode`. When the pool can't take a turn before
    /// any text, this device's model runs it. Devices from before this field ignore it and
    /// keep following `mode`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub pool: bool,
    /// The pool model to ask for (a catalog id); `None` lets the pool choose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pool_model: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn yes() -> bool {
    true
}

impl Default for OffloadSettings {
    fn default() -> Self {
        Self { mode: OffloadMode::Auto, prefer: None, fallback_local: true, serve: false, pool: false, pool_model: None }
    }
}

/// Devices to try for a model turn, in order, and whether to fall back to this device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffloadPlan {
    pub nodes: Vec<String>,
    pub fallback_local: bool,
}

impl OffloadPlan {
    pub fn runs_here(&self) -> bool {
        self.nodes.is_empty()
    }
}

/// Plan where this device's model turns run ("Run on" in Settings > Agents).
///
/// - `Never` (This device): here.
/// - `Always` with `prefer` (a chosen device): that device when it serves models and is online,
///   then here when `fallback_local` and the local model is there; with no `prefer`, any serving
///   device, preferred ones first.
/// - `Auto` (Automatic): here while the local model can run (`local_available`), except on a
///   phone, which offloads to a desktop first; otherwise a serving device.
///
/// The asker gives the local model as the fallback only when it is available.
pub fn offload_plan(settings: &OffloadSettings, platform: Platform, nodes: &[LinkedNode], local_available: bool) -> OffloadPlan {
    let offload = match settings.mode {
        OffloadMode::Never => false,
        OffloadMode::Always => true,
        OffloadMode::Auto => platform == Platform::Phone || !local_available,
    };
    if !offload {
        return OffloadPlan { nodes: Vec::new(), fallback_local: true };
    }
    let mut candidates: Vec<&LinkedNode> = nodes.iter().filter(|n| !n.this_device && n.online && n.serves_models).collect();
    // A chosen device is the one asked, not just the first.
    if let (OffloadMode::Always, Some(chosen)) = (settings.mode, settings.prefer.as_deref()) {
        candidates.retain(|n| n.peer_id == chosen);
    }
    // Preferred first, then desktops before phones, then by label.
    candidates.sort_by_key(|n| (settings.prefer.as_deref() != Some(n.peer_id.as_str()), n.platform == Platform::Phone, n.label.to_lowercase()));
    let nodes: Vec<String> = candidates.into_iter().map(|n| n.peer_id.clone()).collect();
    // With nothing to offload to, a device runs locally whatever the fallback says.
    let fallback_local = settings.fallback_local || nodes.is_empty();
    OffloadPlan { nodes, fallback_local }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ModelChoice;

    fn node(id: &str, platform: Platform, online: bool) -> LinkedNode {
        LinkedNode { peer_id: id.into(), label: id.to_uppercase(), platform, online, this_device: false, serves_models: true }
    }

    #[test]
    fn local_companions_run_here_and_node_companions_follow_their_device() {
        let here = LinkedNode { this_device: true, ..node("phone", Platform::Phone, true) };
        let nodes = vec![here, node("desk", Platform::Desktop, true), node("lap", Platform::Desktop, false)];
        assert_eq!(route_companion(&CompanionProfile::local("l", "Local", ModelChoice::Gemma4E2B), &nodes), CompanionRoute::Here);
        let atlas = CompanionProfile::on_node("atlas", "Atlas", "desk", ModelChoice::Gemma4E4B);
        assert_eq!(route_companion(&atlas, &nodes), CompanionRoute::Node { peer: "desk".into(), label: "DESK".into() });
        let mut off = atlas.clone();
        off.host = Some("lap".into());
        assert_eq!(route_companion(&off, &nodes), CompanionRoute::Unavailable { reason: "Atlas runs on LAP, which is offline".into() });
        off.host = Some("revoked".into());
        assert!(matches!(route_companion(&off, &nodes), CompanionRoute::Unavailable { reason } if reason.contains("no longer linked")));
        off.host = Some("phone".into());
        assert_eq!(route_companion(&off, &nodes), CompanionRoute::Here, "a Node companion on this device runs here");
        off.host = None;
        assert!(matches!(route_companion(&off, &nodes), CompanionRoute::Unavailable { .. }));
    }

    #[test]
    fn phones_offload_to_desktops_by_default_and_desktops_run_locally() {
        let nodes = vec![node("phone2", Platform::Phone, true), node("desk-b", Platform::Desktop, true), node("desk-a", Platform::Desktop, true)];
        let auto = OffloadSettings::default();
        let plan = offload_plan(&auto, Platform::Phone, &nodes, true);
        assert_eq!(plan.nodes, ["desk-a", "desk-b", "phone2"], "desktops first");
        assert!(plan.fallback_local);
        assert!(offload_plan(&auto, Platform::Desktop, &nodes, true).runs_here());
        assert_eq!(
            offload_plan(&auto, Platform::Desktop, &nodes, false).nodes,
            ["desk-a", "desk-b", "phone2"],
            "a desktop whose model cannot run asks the others"
        );

        let prefer = OffloadSettings { mode: OffloadMode::Always, prefer: Some("desk-b".into()), fallback_local: false, ..Default::default() };
        let plan = offload_plan(&prefer, Platform::Desktop, &nodes, true);
        assert_eq!(plan.nodes, ["desk-b"], "the chosen device only");
        assert!(!plan.fallback_local);

        let never = OffloadSettings { mode: OffloadMode::Never, ..prefer.clone() };
        assert!(offload_plan(&never, Platform::Phone, &nodes, false).runs_here());
    }

    #[test]
    fn offline_non_serving_and_this_device_are_never_offload_targets() {
        let mut quiet = node("quiet", Platform::Desktop, true);
        quiet.serves_models = false;
        let me = LinkedNode { this_device: true, ..node("me", Platform::Desktop, true) };
        let nodes = vec![quiet, me, node("off", Platform::Desktop, false)];
        let s = OffloadSettings { mode: OffloadMode::Always, prefer: None, fallback_local: false, ..Default::default() };
        let plan = offload_plan(&s, Platform::Phone, &nodes, true);
        assert!(plan.runs_here());
        assert!(plan.fallback_local, "nothing to offload to: run here");
    }

    #[test]
    fn the_pool_choice_is_off_the_wire_by_default_and_older_settings_read_without_it() {
        let old: OffloadSettings = serde_json::from_str(r#"{"mode":"always","prefer":"desk","fallback_local":false}"#).unwrap();
        assert!(!old.pool && old.pool_model.is_none());
        let json = serde_json::to_value(OffloadSettings::default()).unwrap();
        assert!(json.get("pool").is_none() && json.get("pool_model").is_none(), "{json}");
        let pool = OffloadSettings { pool: true, pool_model: Some("qwen3-8b".into()), ..OffloadSettings::default() };
        let json = serde_json::to_string(&pool).unwrap();
        assert_eq!(serde_json::from_str::<OffloadSettings>(&json).unwrap(), pool);
        // A device from before the pool reads it as its `mode`.
        #[derive(serde::Deserialize)]
        struct Before {
            mode: OffloadMode,
        }
        assert_eq!(serde_json::from_str::<Before>(&json).unwrap().mode, OffloadMode::Auto);
    }

    #[test]
    fn offload_settings_default_and_tolerate_missing_fields() {
        let s: OffloadSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, OffloadSettings::default());
    }
}
