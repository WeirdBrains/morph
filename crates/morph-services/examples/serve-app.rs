//! A Weirdnet app on this machine, for headless checks of `weirdnet-<app>://`
//! addresses (scripts/xvfb-check.py) and as the smallest example of serving
//! an app to Morph: a weirdnet-net node on a throwaway key that listens on
//! nothing of its own, reserves a slot on the relays in `MORPH_LINK_RELAYS`
//! (comma-separated multiaddrs ending in `/p2p/<relay>`, as Morph takes
//! them) and serves the app `morph-test`, so Morph reaches it only through a
//! relay circuit. Prints `peer <peer id>`, then `ready` once a relay holds
//! its slot, and runs until stdin closes.
//!
//! Paths: `/hello` (HTML), `/data.json`, `/pic.png`, `/notes.txt`;
//! `/refuse` refuses; anything else is a 404 with no body.
//!
//! `MORPH_LINK_RELAYS=… cargo run -p morph-services --example serve-app`,
//! then open `weirdnet-morph-test://<peer id>/hello` in Morph with the same
//! `MORPH_LINK_RELAYS`.
use std::io::{BufRead, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use weirdnet_net::profile::WEIRDNET;
use weirdnet_net::{AppError, AppRequest, AppResponse, Keypair, Multiaddr, Node, NodeConfig, PeerId};

/// The app it serves.
const APP: &str = "morph-test";

/// A 1×1 PNG.
const PIXEL: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
    0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0,
    0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99, 0x3d, 0x1d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

struct NoRequests;

impl weirdnet_net::Handler<(), ()> for NoRequests {
    fn request(&self, _from: &PeerId, _req: ()) {}
}

async fn answer(peer: PeerId, req: AppRequest) -> Result<AppResponse, AppError> {
    let path = req.path_with_query.split('?').next().unwrap_or("/").to_string();
    Ok(match path.as_str() {
        "/hello" => AppResponse::new(200).header("content-type", "text/html; charset=utf-8").body(format!(
            "<!doctype html><html><head><title>Hello from {APP}</title></head><body><h1>Hello from {APP}</h1><p>{} {} from {}</p><p><a href=\"data.json\">data</a></p></body></html>",
            req.method,
            req.path_with_query.replace('<', "&lt;"),
            weirdnet_net::short_peer_id(&peer)
        )),
        "/data.json" => AppResponse::new(200).header("content-type", "application/json").body(format!(r#"{{"app":"{APP}","ok":true}}"#)),
        "/pic.png" => AppResponse::new(200).header("content-type", "image/png").body(PIXEL.to_vec()),
        "/notes.txt" => AppResponse::new(200).header("content-type", "text/plain").body("plain <text>"),
        "/refuse" => return Err(AppError::Refused("members only".into())),
        _ => AppResponse::new(404),
    })
}

fn main() {
    let relays: Vec<Multiaddr> = std::env::var("MORPH_LINK_RELAYS").unwrap_or_default().split(',').filter_map(|a| a.trim().parse().ok()).collect();
    if relays.is_empty() {
        eprintln!("serve-app: set MORPH_LINK_RELAYS to the relays to serve through");
        std::process::exit(2);
    }
    let cfg = NodeConfig { relays, label: "morph-serve-app".into(), ..Default::default() };
    let node = Node::start(&WEIRDNET, Keypair::generate_ed25519(), cfg, Arc::new(NoRequests)).expect("the node starts");
    node.serve_app(APP, Arc::new(answer)).expect("a valid app name");
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "peer {}", node.peer_id());
    let _ = out.flush();
    let end = Instant::now() + Duration::from_secs(30);
    while node.status().reservations.is_empty() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
    }
    if node.status().reservations.is_empty() {
        eprintln!("serve-app: no relay took a reservation");
        std::process::exit(1);
    }
    let _ = writeln!(out, "ready");
    let _ = out.flush();
    drop(out);
    for line in std::io::stdin().lock().lines() {
        if line.is_err() {
            break;
        }
    }
    node.stop();
}
