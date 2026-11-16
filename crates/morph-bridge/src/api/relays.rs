//! The relays device linking uses and whether Morph is connected to them now
//! (Settings → Relays, and the status line on the link screens). The state is
//! read from the running link nodes or a relay check; with none running,
//! `nodes` is 0 and every relay reads `no-node`.
use morph_services::device_link::Links;
use morph_services::relays::RelayReport;

/// One configured relay. `state` is `no-node`, `not-connected`,
/// `connected` or `reserved`; `label` is what a screen says about it.
pub struct RelayStatusView {
    pub peer: String,
    pub addrs: Vec<String>,
    /// Its display name (from its well-known info), else its host.
    pub title: String,
    pub host: Option<String>,
    pub state: String,
    pub label: String,
    pub connected: bool,
    pub reserved: bool,
    pub rtt_ms: Option<u64>,
}

/// The configured relays now. `nodes`: running nodes the state came from;
/// `checking`: a relay check runs; `summary`: one line for a link screen.
pub struct RelayReportView {
    pub nodes: u32,
    pub checking: bool,
    pub summary: String,
    pub relays: Vec<RelayStatusView>,
}

impl From<RelayReport> for RelayReportView {
    fn from(r: RelayReport) -> Self {
        let relays = r
            .relays
            .iter()
            .map(|v| {
                let status = r.status(v);
                RelayStatusView {
                    peer: v.peer.clone(),
                    addrs: v.addrs.clone(),
                    title: v.title(),
                    host: v.host.clone(),
                    state: status.as_str().into(),
                    label: status.label().into(),
                    connected: v.connected,
                    reserved: v.reserved,
                    rtt_ms: v.rtt_ms,
                }
            })
            .collect();
        RelayReportView { nodes: r.nodes as u32, checking: r.checking, summary: r.summary(), relays }
    }
}

/// The relays as the running nodes see them. Cheap and never waits on the
/// network (a relay's name is fetched in the background and shows on a
/// later call).
#[flutter_rust_bridge::frb(sync)]
pub fn relay_report() -> RelayReportView {
    Links::global().relays().into()
}

/// Start a relay check: for 30 s a node like a link's reaches the relays
/// (and reserves a slot), so [`relay_report`] shows real state while no link
/// runs. Runs off the UI isolate; a running check is kept.
pub fn relay_check_start() -> Result<(), String> {
    Links::global().relay_check_start().map_err(|e| e.to_string())
}

pub fn relay_check_stop() {
    Links::global().relay_check_stop();
}

#[cfg(test)]
mod tests {
    use super::*;
    use morph_identity::device_link::{Multiaddr, NetStatus, PUBLIC_RELAYS};

    #[test]
    fn the_view_carries_the_mapped_state() {
        let relays: Vec<Multiaddr> = PUBLIC_RELAYS.iter().map(|a| a.parse().unwrap()).collect();
        let none: RelayReportView = RelayReport::build(&relays, &[], false, |_, _| None).into();
        assert_eq!(none.nodes, 0);
        assert_eq!(none.relays.len(), 1);
        assert_eq!(none.relays[0].state, "no-node");
        assert_eq!(none.relays[0].title, "relay.net.weirdbrains.com");
        assert_eq!(none.relays[0].addrs.len(), 2);

        let peer = none.relays[0].peer.clone();
        let up = NetStatus { connected: vec![peer.clone()], reservations: vec![peer], ..Default::default() };
        let view: RelayReportView = RelayReport::build(&relays, &[up], true, |_, _| Some("WeirdBrains public relay".into())).into();
        assert_eq!(view.relays[0].state, "reserved");
        assert_eq!(view.relays[0].label, "Connected, slot reserved");
        assert_eq!(view.relays[0].title, "WeirdBrains public relay");
        assert!(view.checking && view.relays[0].rtt_ms.is_none());
        assert_eq!(view.summary, "Relay WeirdBrains public relay: connected, slot reserved");
    }
}
