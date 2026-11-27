//! Settings > Agents > Link to Ymir: how this profile and a [Ymir](https://github.com/WeirdBrains/ymir)
//! pool find each other.
//!
//! Ymir has no id list to type a device into: every Ymir node admits only linked devices of its
//! own Weirdnet identity (its roster check), and Morph's [`YmirPool`](morph_services::ymir_pool)
//! dials only this profile's linked devices. So Morph and the pool have to share one identity,
//! and linking is the ordinary device link with a code and a six-digit check:
//!
//! - **Ymir node into this profile's identity**: `ymirctl pool link` on the node prints a
//!   `weirdnet-ymir://link/…` code; pasted here it opens Identity > Add a device with it filled
//!   in (password, then the six-digit check).
//! - **This profile into the pool's identity** (a profile without an identity yet): Identity >
//!   Link from another device shows a `weirdnet-morph://link/…` code; `ymirctl pool add 'CODE'` on
//!   a Ymir node that holds the identity links it ([`ymir_add_command`]).
//!
//! The page shows the identity id and this device's peer id with Copy buttons, so they can be
//! compared with the first line of `ymirctl pool devices` (and its device list) on a node.

use std::path::Path;
use std::time::{Duration, Instant};

use gpui::{ClipboardItem, Context, div, prelude::*, px, rgb};
use morph_identity::device_link::{OfferApp, is_invite, offer_scheme};
use morph_services::sync::DeviceView;
use morph_ui::actions::Action;
use weirdnet_agent::remote::pool_model::PoolStatus;

use super::SettingsView;
use crate::shell::ShellRequest;
use crate::widgets::button;

/// How long a Copy button reads "Copied".
pub const COPIED_FOR: Duration = Duration::from_secs(2);

/// What a Ymir node needs to see to let this device in, read from the profile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinkInfo {
    /// The profile's Weirdnet identity (the 32 hex digits `ymirctl pool devices` prints as
    /// `Identity …`); `None` when the profile has none yet.
    pub identity: Option<String>,
    /// This device's peer id (`12D3KooW…`), as it shows in the node's device list.
    pub peer: Option<String>,
    /// Linked Ymir nodes in this profile's roster: (label, peer id).
    pub nodes: Vec<(String, String)>,
}

/// Reads [`LinkInfo`] from the profile's public identity files and its roster. Nothing secret is
/// opened: `identity.json` and `node.cert.json` are public, and a locked profile simply shows no
/// devices.
pub fn link_info(profile_dir: &Path, devices: &[DeviceView]) -> LinkInfo {
    let identity = morph_identity::profile_identity::load(profile_dir).ok().flatten().map(|i| i.user_id);
    let peer = devices
        .iter()
        .find(|d| d.this_device)
        .map(|d| d.peer_id.clone())
        .or_else(|| morph_identity::profile_identity::load_node_cert(profile_dir).ok().flatten().map(|c| c.peer_id));
    LinkInfo { identity: identity.clone(), peer: identity.and(peer), nodes: ymir_nodes(devices) }
}

/// The linked, unrevoked Ymir nodes in a roster. Ymir names its devices "Ymir on <node>"
/// (`ymirctl pool link`), and Add a device names one so when its code came from Ymir.
pub fn ymir_nodes(devices: &[DeviceView]) -> Vec<(String, String)> {
    devices
        .iter()
        .filter(|d| !d.this_device && d.revoked_ms.is_none() && d.label.to_ascii_lowercase().contains("ymir"))
        .map(|d| (d.label.clone(), d.peer_id.clone()))
        .collect()
}

/// A pasted Ymir node's code, checked by its prefix (the pairing checks the rest), or what to
/// say instead.
pub fn ymir_code(text: &str) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Paste the weirdnet-ymir://link/… code that `ymirctl pool link` prints on the Ymir node.".into());
    }
    match offer_scheme(text) {
        Some((OfferApp::Ymir, _)) => Ok(text.to_string()),
        Some((OfferApp::Morph, _)) => {
            Err("That is a Morph device's code. Open it under Identity > Add a device, or on a Ymir node with `ymirctl pool add`.".into())
        }
        None if is_invite(text) => Err("That is a Morph invite, not a Ymir node's code. Ymir codes start with weirdnet-ymir://link/.".into()),
        None => {
            Err("That is not a Ymir node's code. Run `ymirctl pool link` on the node and paste the weirdnet-ymir://link/… line it prints.".into())
        }
    }
}

/// The command that links the device showing `link` (a `weirdnet-morph://link/…` code) from a
/// Ymir node holding the identity. Ymir reads Morph's codes as its own.
pub fn ymir_add_command(link: &str) -> String {
    format!("ymirctl pool add '{}'", link.trim())
}

/// A Copy button's label: "Copied" for [`COPIED_FOR`] after `key` was copied.
pub fn copy_label(copied: Option<&(&'static str, Instant)>, key: &str, now: Instant) -> &'static str {
    match copied {
        Some((k, at)) if *k == key && now.saturating_duration_since(*at) < COPIED_FOR => "Copied",
        _ => "Copy",
    }
}

/// The card's explanation for what the profile has.
pub fn link_help(info: &LinkInfo) -> String {
    if info.identity.is_none() {
        return "This profile has no Weirdnet identity yet. To join your Ymir pool, open Identity, choose Link from another device, and run \
                `ymirctl pool add 'CODE'` on a Ymir node with the code it shows (Copy for Ymir copies the whole command). Compare the six digits on both."
            .into();
    }
    "Ymir lets in only devices of its own Weirdnet identity; there is no separate id to type into it. On a Ymir node, the first line of \
     `ymirctl pool devices` is its identity: when it is the one below, this device is in the pool (its peer id is in that list). To add a Ymir \
     node that isn't linked yet, run `ymirctl pool link` on it and paste its code here. If the node shows a different identity, its pool \
     has one of its own: link a Morph profile without an identity to it (Identity > Link from another device, then `ymirctl pool add 'CODE'` on the node)."
        .into()
}

/// What the card says about the pool itself, when Run on is the pool.
pub fn nodes_line(info: &LinkInfo, status: Option<&PoolStatus>) -> String {
    let nodes = match info.nodes.as_slice() {
        [] => "No Ymir node is linked to this profile yet.".to_string(),
        list => format!("Linked Ymir nodes: {}.", list.iter().map(|(label, _)| label.as_str()).collect::<Vec<_>>().join(", ")),
    };
    match status {
        Some(PoolStatus::Live { .. }) => format!("{nodes} The pool answers this device."),
        Some(PoolStatus::Unreachable { .. }) if !info.nodes.is_empty() => {
            format!("{nodes} None of them answers right now; check that ymird is running.")
        }
        _ => nodes,
    }
}

impl SettingsView {
    fn ymir_copy(&mut self, key: &'static str, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.agents.copied = Some((key, Instant::now()));
        self.agents.copied_reset = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED_FOR).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        }));
        cx.notify();
    }

    /// The pasted Ymir code: Identity > Add a device with it filled in.
    pub(super) fn ymir_link_code(&mut self, cx: &mut Context<Self>) {
        let text = self.agents.ymir_code.read(cx).text().to_string();
        match ymir_code(&text) {
            Ok(code) => {
                self.agents.ymir_note = None;
                self.agents.ymir_code.update(cx, |i, cx| i.set_text("", cx));
                cx.emit(ShellRequest::OpenPairingLink(code));
            }
            Err(e) => self.agents.ymir_note = Some(e),
        }
        cx.notify();
    }

    /// Settings > Agents > Link to Ymir.
    pub(super) fn ymir_link_row(&mut self, status: Option<&PoolStatus>, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = self.theme;
        let Some(profile) = self.profile_id() else { return div().into_any_element() };
        let info = link_info(&morph_core::profiles::profile_dir(&self.home, &profile), &self.devices);
        let now = Instant::now();
        let dim = |s: String| div().text_size(px(12.)).text_color(rgb(t.text_secondary)).child(s);
        let id_line = |key: &'static str,
                       name: &str,
                       value: Option<String>,
                       missing: &str,
                       copied: Option<&(&'static str, Instant)>,
                       cx: &mut Context<Self>| {
            let row = div().flex().flex_row().items_center().gap(px(8.)).child(div().w(px(110.)).text_size(px(12.)).child(name.to_string()));
            match value {
                Some(v) => row.child(div().flex_1().min_w_0().text_size(px(12.)).font_family("monospace").overflow_hidden().child(v.clone())).child(
                    button(t, ("agents-ymir-copy", key.len()), copy_label(copied, key, now), false)
                        .on_click(cx.listener(move |this, _, _, cx| this.ymir_copy(key, v.clone(), cx))),
                ),
                None => row.child(dim(missing.to_string())),
            }
        };
        let copied = self.agents.copied;
        let mut card = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .px(px(12.))
            .py(px(10.))
            .rounded(px(8.))
            .bg(rgb(t.surface))
            .child(div().text_size(px(14.)).child("Link to Ymir"))
            .child(dim(link_help(&info)));
        if info.identity.is_some() {
            card = card
                .child(id_line("identity", "Identity", info.identity.clone(), "", copied.as_ref(), cx))
                .child(id_line("peer", "This device", info.peer.clone(), "Unlock the library to see this device's peer id.", copied.as_ref(), cx))
                .child(dim(nodes_line(&info, status)))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.))
                        .child(div().w(px(320.)).child(self.agents.ymir_code.clone()))
                        .child(button(t, "agents-ymir-link", "Link Ymir node", false).on_click(cx.listener(|this, _, _, cx| this.ymir_link_code(cx))))
                        .child(
                            button(
                                t,
                                "agents-ymir-cmd",
                                match copy_label(copied.as_ref(), "cmd", now) {
                                    "Copied" => "Copied",
                                    _ => "Copy `ymirctl pool link`",
                                },
                                false,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.ymir_copy("cmd", "ymirctl pool link".into(), cx))),
                        ),
                );
        } else {
            card = card.child(
                div().flex().flex_row().child(
                    button(t, "agents-ymir-identity", "Open Identity", false)
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(ShellRequest::Run(Action::ShowIdentity)))),
                ),
            );
        }
        if let Some(note) = &self.agents.ymir_note {
            card = card.child(div().text_size(px(12.)).text_color(rgb(t.error)).child(note.clone()));
        }
        card.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(peer: &str, label: &str, this_device: bool, revoked: bool) -> DeviceView {
        DeviceView {
            peer_id: peer.into(),
            label: label.into(),
            issued_ms: 1,
            revoked_ms: revoked.then_some(1),
            this_device,
            needs_admission: false,
            awaiting_first_sync: false,
            has_key: true,
        }
    }

    #[test]
    fn ymir_nodes_are_the_linked_unrevoked_ymir_devices() {
        let devices = [
            device("12D3KooWMe", "Morph on desk", true, false),
            device("12D3KooWGpu", "Ymir on gpu-box", false, false),
            device("12D3KooWOld", "Ymir on old-box", false, true),
            device("12D3KooWPhone", "Phone", false, false),
            device("12D3KooWBare", "ymir", false, false),
        ];
        assert_eq!(
            ymir_nodes(&devices),
            [("Ymir on gpu-box".to_string(), "12D3KooWGpu".to_string()), ("ymir".to_string(), "12D3KooWBare".to_string())]
        );
    }

    #[test]
    fn only_a_ymir_nodes_code_is_taken() {
        assert_eq!(ymir_code("  weirdnet-ymir://link/abc \n").as_deref(), Ok("weirdnet-ymir://link/abc"));
        assert_eq!(ymir_code("WEIRDNET-YMIR://LINK/abc").as_deref(), Ok("WEIRDNET-YMIR://LINK/abc"), "the shell lowercases the scheme");
        assert!(ymir_code("").unwrap_err().contains("ymirctl pool link"));
        assert!(ymir_code("weirdnet-morph://link/abc").unwrap_err().contains("Morph device's code"));
        assert!(ymir_code("weirdnet-morph://invite/abc").unwrap_err().contains("invite"));
        assert!(ymir_code("12D3KooWQH96LGJd33JquZXNyrmrs7HAeaeviiFqdwnzwfdmj1VY").unwrap_err().contains("not a Ymir node's code"));
    }

    #[test]
    fn the_ymir_command_quotes_the_code() {
        assert_eq!(ymir_add_command(" weirdnet-morph://link/AbC_-1 "), "ymirctl pool add 'weirdnet-morph://link/AbC_-1'");
    }

    #[test]
    fn copy_reads_copied_for_a_moment_on_the_copied_button_only() {
        let at = Instant::now();
        let copied = ("identity", at);
        assert_eq!(copy_label(Some(&copied), "identity", at), "Copied");
        assert_eq!(copy_label(Some(&copied), "peer", at), "Copy");
        assert_eq!(copy_label(Some(&copied), "identity", at + COPIED_FOR), "Copy");
        assert_eq!(copy_label(None, "identity", at), "Copy");
    }

    #[test]
    fn link_info_reads_the_public_identity_files_only() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        assert_eq!(link_info(p, &[]), LinkInfo::default(), "no identity, nothing to show");
        assert!(link_help(&LinkInfo::default()).contains("Link from another device"));

        let password = morph_identity::profile_identity::SecretString::from("a long test password".to_string());
        let created = morph_identity::profile_identity::create(p, &password, 1_795_000_000_000).unwrap();
        let user = created.identity.user_id.clone();
        let info = link_info(p, &[]);
        assert_eq!(info.identity.as_deref(), Some(user.as_str()));
        let cert = info.peer.clone().expect("a new identity certifies this device's node: its peer id shows while the library is locked");
        assert!(cert.starts_with("12D3KooW"), "{cert}");
        let devices = [device("12D3KooWMe", "Morph on desk", true, false), device("12D3KooWGpu", "Ymir on gpu-box", false, false)];
        let info = link_info(p, &devices);
        assert_eq!(info.peer.as_deref(), Some("12D3KooWMe"), "the roster's own entry comes first");
        assert_eq!(info.nodes.len(), 1);
        assert!(link_help(&info).contains("ymirctl pool devices"));
        assert_eq!(nodes_line(&info, None), "Linked Ymir nodes: Ymir on gpu-box.");
        assert!(nodes_line(&LinkInfo::default(), None).starts_with("No Ymir node"));
    }
}
