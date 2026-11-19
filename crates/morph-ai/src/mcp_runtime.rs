//! Connects the configured MCP servers (feature `mcp`): one `weirdnet_mcp::agent::McpSource` per
//! server that is on, runs on this device and has been approved, merged into a
//! `weirdnet_agent::ToolRegistry`. Every server ends in a [`ServerStatus`] the Settings page shows.
//!
//! Local commands start only after the user approved them on this machine ([`McpApprovals`]).
//! Secret values are read from this machine's [`McpSecrets`] at connect time and go straight to the
//! child's environment or the request headers; they are not copied anywhere else.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use weirdnet_agent::{Tier, ToolRegistry, ToolSource};
use weirdnet_mcp::agent::{McpSource, SourceOptions};
use weirdnet_mcp::client::{ClientOptions, HttpConfig, Secret, StdioCommand};

use crate::mcp::{McpConfigs, McpSecrets, McpServer, McpTransport, McpValue};
use crate::mcp_local::McpApprovals;

/// One tool of a connected server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInfo {
    /// The server's own name for the tool (what `tools_off` holds).
    pub name: String,
    /// The name the model sees (with the server's prefix).
    pub exposed: String,
    /// Description, as the server gave it.
    pub description: String,
    /// The tier the tool runs at.
    pub tier: Tier,
    /// Whether the user has it on.
    pub enabled: bool,
}

/// Where one server stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerStatus {
    /// Switched off.
    Off,
    /// Set to run on other devices only.
    NotOnThisDevice,
    /// A local command the user has not approved here yet. `warning` is what to show them.
    NeedsApproval { warning: String },
    /// Secret values this device does not have yet (names only).
    MissingSecrets(Vec<String>),
    /// Starting or listing tools failed.
    Failed(String),
    /// Connected, with its tools.
    Connected { tools: Vec<ToolInfo>, warning: Option<String> },
}

impl ServerStatus {
    /// A one-line summary for the list.
    pub fn summary(&self) -> String {
        match self {
            Self::Off => "Off".into(),
            Self::NotOnThisDevice => "Runs on other devices".into(),
            Self::NeedsApproval { .. } => "Needs your OK to start".into(),
            Self::MissingSecrets(names) => format!("Missing on this device: {}", names.join(", ")),
            Self::Failed(e) => format!("Could not connect: {e}"),
            Self::Connected { tools, .. } => {
                let on = tools.iter().filter(|t| t.enabled).count();
                format!("Connected · {on} of {} tools on", tools.len())
            }
        }
    }
}

/// The registry of connected servers and every server's status.
pub struct McpHub {
    /// The tools of the connected servers that apply to every companion. Tools the user switched
    /// off are disabled in it.
    pub registry: Arc<ToolRegistry>,
    /// For each companion some connected server is limited to: the shared tools plus the servers
    /// limited to that companion. Other companions use [`registry`](Self::registry).
    pub scoped: BTreeMap<String, Arc<ToolRegistry>>,
    /// Status by server id.
    pub statuses: BTreeMap<String, ServerStatus>,
}

impl McpHub {
    /// The tools `companion` may use.
    pub fn registry_for(&self, companion: &str) -> Arc<ToolRegistry> {
        self.scoped.get(companion).unwrap_or(&self.registry).clone()
    }
}

/// What [`connect`] needs from this machine.
pub struct ConnectContext<'a> {
    /// This machine's secrets.
    pub secrets: &'a dyn McpSecrets,
    /// What the user approved on this machine.
    pub approvals: &'a McpApprovals,
    /// This device's id (a peer id) for `McpServer::devices`.
    pub device: &'a str,
    /// Time allowed for starting and listing one server.
    pub timeout: Duration,
}

/// A tool-name prefix made of the server's name: `GitHub Docs` becomes `github_docs_`.
/// `taken` holds prefixes already used; a repeat gets the server id's tail so names stay unique.
pub fn prefix_for(server: &McpServer, taken: &[String]) -> String {
    let mut slug: String = server.name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
    slug = slug.split('_').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("_");
    if slug.is_empty() {
        slug = "mcp".into();
    }
    slug.truncate(24);
    let mut prefix = format!("{slug}_");
    if taken.contains(&prefix) {
        let tail: String = server.id.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().filter(char::is_ascii_alphanumeric).collect();
        prefix = format!("{slug}_{}_", tail.to_ascii_lowercase());
    }
    prefix
}

fn resolve(server: &McpServer, values: &BTreeMap<String, McpValue>, secrets: &dyn McpSecrets) -> Result<Vec<(String, Secret)>, Vec<String>> {
    let mut out = Vec::new();
    let mut missing = Vec::new();
    for (name, v) in values {
        match v {
            McpValue::Plain { value } => out.push((name.clone(), Secret::new(value.clone()))),
            McpValue::Secret { name: secret } => match secrets.get(&server.id, secret) {
                Some(value) => out.push((name.clone(), Secret::new(value))),
                None => missing.push(secret.clone()),
            },
        }
    }
    if missing.is_empty() { Ok(out) } else { Err(missing) }
}

/// The warning to show before a server's command is started.
pub fn warning_for(server: &McpServer) -> Option<String> {
    match &server.transport {
        McpTransport::Stdio { command, args, .. } => {
            Some(StdioCommand { program: command.clone(), args: args.clone(), env: Vec::new(), cwd: None }.warning())
        }
        McpTransport::Http { url, .. } => {
            Some(format!("Morph will send this server's requests, and any secret in its headers, to {url}. Only allow servers you trust."))
        }
    }
}

async fn open(server: &McpServer, secrets: &dyn McpSecrets, timeout: Duration, options: SourceOptions) -> Result<McpSource, ServerStatus> {
    let client_options = ClientOptions { timeout, ..ClientOptions::default() };
    let source = match &server.transport {
        McpTransport::Stdio { command, args, env } => {
            let env = resolve(server, env, secrets).map_err(ServerStatus::MissingSecrets)?;
            let mut cmd = StdioCommand::new(command.clone());
            cmd.args = args.clone();
            cmd.env = env;
            McpSource::connect_stdio(server.id.clone(), &cmd, client_options).await
        }
        McpTransport::Http { url, headers } => {
            let headers = resolve(server, headers, secrets).map_err(ServerStatus::MissingSecrets)?;
            let mut cfg = HttpConfig::new(url.clone());
            cfg.headers = headers;
            McpSource::connect_http(server.id.clone(), &cfg, client_options).await
        }
    };
    source.map(|s| s.with_options(options)).map_err(|e| ServerStatus::Failed(e.to_string()))
}

/// Connects every configured server and builds the registry. One server failing never stops the
/// others; its status says why.
pub async fn connect(configs: &McpConfigs, ctx: &ConnectContext<'_>) -> McpHub {
    let mut statuses = BTreeMap::new();
    let mut sources: Vec<(McpServer, Arc<McpSource>)> = Vec::new();
    let mut prefixes: Vec<String> = Vec::new();
    for server in &configs.servers {
        let status = if !server.enabled {
            ServerStatus::Off
        } else if !server.runs_on(ctx.device) {
            ServerStatus::NotOnThisDevice
        } else if !ctx.approvals.is_approved(server, ctx.secrets) {
            ServerStatus::NeedsApproval { warning: warning_for(server).unwrap_or_default() }
        } else {
            let prefix = prefix_for(server, &prefixes);
            let options = SourceOptions { name_prefix: Some(prefix.clone()), ..SourceOptions::default() };
            let attempt = async {
                let source = open(server, ctx.secrets, ctx.timeout, options).await?;
                let specs = source.list().await.map_err(|e| ServerStatus::Failed(e.to_string()))?;
                Ok::<_, ServerStatus>((source, specs))
            };
            match tokio::time::timeout(ctx.timeout + Duration::from_secs(5), attempt).await {
                Err(_) => ServerStatus::Failed("timed out".into()),
                Ok(Err(status)) => status,
                Ok(Ok((source, specs))) => {
                    let warning = source.warning().map(str::to_string);
                    let tools = specs
                        .iter()
                        .map(|s| {
                            let name = s.name.strip_prefix(&prefix).unwrap_or(&s.name).to_string();
                            ToolInfo {
                                enabled: !server.tools_off.contains(&name),
                                name,
                                exposed: s.name.clone(),
                                description: s.description.clone(),
                                tier: s.tier,
                            }
                        })
                        .collect();
                    prefixes.push(prefix);
                    sources.push((server.clone(), Arc::new(source)));
                    ServerStatus::Connected { tools, warning }
                }
            }
        };
        statuses.insert(server.id.clone(), status);
    }
    // Companions some server is limited to get a registry of their own; the sources are shared.
    let named: std::collections::BTreeSet<String> = sources.iter().filter_map(|(s, _)| s.companions.clone()).flatten().collect();
    let shared: Vec<&(McpServer, Arc<McpSource>)> = sources.iter().filter(|(s, _)| s.companions.is_none()).collect();
    // Every added source listed fine a moment ago; a failure here (a tool name two servers share
    // despite the prefixes) is reported on every connected server rather than dropping them all.
    let mut failure = None;
    let registry = match build_registry(&shared, &statuses).await {
        Ok(r) => r,
        Err(e) => {
            failure = Some(e);
            Arc::new(ToolRegistry::new())
        }
    };
    let mut scoped = BTreeMap::new();
    for companion in named {
        let picked: Vec<&(McpServer, Arc<McpSource>)> = sources.iter().filter(|(s, _)| s.applies_to(&companion)).collect();
        match build_registry(&picked, &statuses).await {
            Ok(r) => {
                scoped.insert(companion, r);
            }
            Err(e) => failure = Some(e),
        }
    }
    if let Some(e) = failure {
        for status in statuses.values_mut() {
            if matches!(status, ServerStatus::Connected { .. }) {
                *status = ServerStatus::Failed(e.to_string());
            }
        }
        return McpHub { registry: Arc::new(ToolRegistry::new()), scoped: BTreeMap::new(), statuses };
    }
    McpHub { registry, scoped, statuses }
}

/// A registry over `picked` with the user's switched-off tools disabled.
async fn build_registry(
    picked: &[&(McpServer, Arc<McpSource>)],
    statuses: &BTreeMap<String, ServerStatus>,
) -> Result<Arc<ToolRegistry>, weirdnet_agent::RegistryError> {
    let mut registry = ToolRegistry::new();
    for (_, source) in picked {
        registry.add_source(source.clone());
    }
    registry.refresh().await?;
    for (server, _) in picked {
        if let Some(ServerStatus::Connected { tools, .. }) = statuses.get(&server.id) {
            for t in tools.iter().filter(|t| !t.enabled) {
                registry.set_tool_enabled(&t.exposed, false);
            }
        }
    }
    Ok(Arc::new(registry))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::mcp_local::MemorySecrets;

    /// A shell "server" that answers `initialize` and `tools/list` any number of times (the id is
    /// copied from the request), with two tools named after `$MCP_NAME`.
    const SCRIPT: &str = r#"while read l; do
  id=$(echo "$l" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$l" in
    *'"initialize"'*) echo '{"jsonrpc":"2.0","id":'$id',"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"sh","version":"1"}}}' ;;
    *'"tools/list"'*) echo '{"jsonrpc":"2.0","id":'$id',"result":{"tools":[{"name":"'"$MCP_NAME"'_read","annotations":{"readOnlyHint":true,"openWorldHint":false}},{"name":"'"$MCP_NAME"'_write"}]}}' ;;
  esac
done"#;

    fn stdio(name: &str, env: &[(&str, McpValue)]) -> McpServer {
        let env = env.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        McpServer::new(name, McpTransport::Stdio { command: "sh".into(), args: vec!["-c".into(), SCRIPT.into()], env })
    }

    fn plain(v: &str) -> McpValue {
        McpValue::Plain { value: v.into() }
    }

    async fn run(configs: &McpConfigs, secrets: &dyn McpSecrets, approvals: &McpApprovals) -> McpHub {
        connect(configs, &ConnectContext { secrets, approvals, device: "desk", timeout: Duration::from_secs(5) }).await
    }

    #[tokio::test]
    async fn connects_approved_servers_with_prefixes_secrets_and_tiers() {
        let secrets = MemorySecrets::default();
        let mut s = stdio("Files Server", &[("MCP_NAME", McpValue::Secret { name: "MCP_NAME".into() })]);
        secrets.set(&s.id, "MCP_NAME", "files");
        s.tools_off = BTreeSet::from(["files_write".to_string()]);
        let mut approvals = McpApprovals::default();
        approvals.approve(&s, &secrets);
        let mut configs = McpConfigs::default();
        configs.upsert(s.clone());
        let hub = run(&configs, &secrets, &approvals).await;
        let ServerStatus::Connected { tools, warning } = &hub.statuses[&s.id] else { panic!("{:?}", hub.statuses) };
        assert!(warning.as_deref().unwrap().contains("local command"));
        let names: Vec<_> = tools.iter().map(|t| (t.name.as_str(), t.exposed.as_str(), t.tier, t.enabled)).collect();
        assert_eq!(
            names,
            [("files_read", "files_server_files_read", Tier::Research, true), ("files_write", "files_server_files_write", Tier::Interact, false)]
        );
        // The registry offers the tools by their prefixed names; the switched-off one is hidden.
        let offered: Vec<String> = hub.registry.specs().into_iter().map(|t| t.name).collect();
        assert_eq!(offered, ["files_server_files_read"]);
        assert!(hub.registry.lookup("files_server_files_write").is_err());
        assert_eq!(hub.statuses[&s.id].summary(), "Connected · 1 of 2 tools on");
    }

    #[tokio::test]
    async fn every_other_state_is_reported_and_none_stops_the_rest() {
        let secrets = MemorySecrets::default();
        let good = stdio("Good", &[("MCP_NAME", plain("good"))]);
        let off = {
            let mut s = stdio("Off", &[("MCP_NAME", plain("off"))]);
            s.enabled = false;
            s
        };
        let elsewhere = {
            let mut s = stdio("Elsewhere", &[("MCP_NAME", plain("e"))]);
            s.devices = Some(BTreeSet::from(["phone".to_string()]));
            s
        };
        let unapproved = stdio("Unapproved", &[("MCP_NAME", plain("u"))]);
        let needs_secret = stdio("Needs secret", &[("MCP_NAME", McpValue::Secret { name: "MCP_NAME".into() })]);
        let missing_bin =
            McpServer::new("Missing bin", McpTransport::Stdio { command: "/nonexistent/mcp-server".into(), args: vec![], env: BTreeMap::new() });
        let unreachable = McpServer::new("Unreachable", McpTransport::Http { url: "http://127.0.0.1:9/mcp".into(), headers: BTreeMap::new() });
        let mut approvals = McpApprovals::default();
        for s in [&good, &off, &elsewhere, &needs_secret, &missing_bin, &unreachable] {
            approvals.approve(s, &secrets);
        }
        let mut configs = McpConfigs::default();
        for s in [&good, &off, &elsewhere, &unapproved, &needs_secret, &missing_bin, &unreachable] {
            configs.upsert((*s).clone());
        }
        let hub = run(&configs, &secrets, &approvals).await;
        let st = |s: &McpServer| hub.statuses[&s.id].clone();
        assert!(matches!(st(&good), ServerStatus::Connected { .. }));
        assert_eq!(st(&off), ServerStatus::Off);
        assert_eq!(st(&elsewhere), ServerStatus::NotOnThisDevice);
        assert!(matches!(st(&unapproved), ServerStatus::NeedsApproval { warning } if warning.contains("runs with your permissions")));
        assert_eq!(st(&needs_secret), ServerStatus::MissingSecrets(vec!["MCP_NAME".into()]));
        assert!(matches!(st(&missing_bin), ServerStatus::Failed(e) if e.contains("could not start")));
        assert!(matches!(st(&unreachable), ServerStatus::Failed(_)));
        assert_eq!(hub.registry.specs().len(), 2, "only the good server's tools are offered");
        assert!(st(&needs_secret).summary().contains("MCP_NAME"));
    }

    #[tokio::test]
    async fn a_server_limited_to_a_companion_is_offered_to_that_companion_only() {
        let secrets = MemorySecrets::default();
        let shared = stdio("Shared", &[("MCP_NAME", plain("all"))]);
        let mut mine = stdio("Mine", &[("MCP_NAME", plain("mine"))]);
        mine.companions = Some(BTreeSet::from(["scout".to_string()]));
        let mut approvals = McpApprovals::default();
        approvals.approve(&shared, &secrets);
        approvals.approve(&mine, &secrets);
        let mut configs = McpConfigs::default();
        configs.upsert(shared.clone());
        configs.upsert(mine.clone());
        let hub = run(&configs, &secrets, &approvals).await;
        let names = |r: Arc<ToolRegistry>| r.specs().into_iter().map(|t| t.name).collect::<Vec<_>>();
        assert_eq!(names(hub.registry_for("local")), ["shared_all_read", "shared_all_write"]);
        assert_eq!(names(hub.registry_for("scout")), ["mine_mine_read", "mine_mine_write", "shared_all_read", "shared_all_write"]);
        assert!(hub.registry_for("local").lookup("mine_mine_read").is_err());
        assert!(matches!(hub.statuses[&mine.id], ServerStatus::Connected { .. }), "the server connects once, whoever may use it");
        // The scope is part of the synced config and an old config without it means everyone.
        let json = serde_json::to_string(&mine).unwrap();
        assert!(json.contains("\"companions\":[\"scout\"]"));
        let old: McpServer = serde_json::from_str(&serde_json::to_string(&shared).unwrap()).unwrap();
        assert!(old.companions.is_none() && old.applies_to("anyone"));
    }

    #[tokio::test]
    async fn a_synced_change_of_an_environment_value_stops_the_server_until_the_user_agrees_again() {
        let secrets = MemorySecrets::default();
        let mut s = stdio("Docs", &[("MCP_NAME", plain("docs")), ("NODE_OPTIONS", plain("--max-old-space-size=64"))]);
        let mut approvals = McpApprovals::default();
        approvals.approve(&s, &secrets);
        let mut configs = McpConfigs::default();
        configs.upsert(s.clone());
        let hub = run(&configs, &secrets, &approvals).await;
        assert!(matches!(hub.statuses[&s.id], ServerStatus::Connected { .. }));
        // The config arrives again from another device with a different NODE_OPTIONS.
        if let McpTransport::Stdio { env, .. } = &mut s.transport {
            env.insert("NODE_OPTIONS".into(), plain("--require /tmp/evil.js"));
        }
        configs.upsert(s.clone());
        let hub = run(&configs, &secrets, &approvals).await;
        assert!(matches!(hub.statuses[&s.id], ServerStatus::NeedsApproval { .. }), "{:?}", hub.statuses);
        assert!(hub.registry.specs().is_empty(), "nothing of it runs");
    }

    #[test]
    fn prefixes_are_safe_names_and_stay_unique() {
        let a = McpServer::new("GitHub Docs!", McpTransport::Http { url: "https://x".into(), headers: BTreeMap::new() });
        let mut b = a.clone();
        b.id = "mcp-abcd1234".into();
        assert_eq!(prefix_for(&a, &[]), "github_docs_");
        assert_eq!(prefix_for(&b, &["github_docs_".to_string()]), "github_docs_1234_");
        let weird = McpServer::new("日本語", McpTransport::Http { url: "https://x".into(), headers: BTreeMap::new() });
        assert_eq!(prefix_for(&weird, &[]), "mcp_");
        assert!(warning_for(&a).unwrap().contains("https://x"));
    }
}
