//! MCP server configurations (tool sources over `weirdnet-mcp`). Configs sync across the
//! user's devices; secret values never do. A value is either plain or a reference to a secret
//! held on each device (the keystore implements [`McpSecrets`]), and plain values under names
//! that look secret are turned into references before anything is saved or synced.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// An environment variable or header value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum McpValue {
    Plain {
        value: String,
    },
    /// Looked up in this device's [`McpSecrets`] under the server id and `name`.
    Secret {
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "transport")]
pub enum McpTransport {
    /// A local process. Runs only on the devices listed in [`McpServer::devices`] (or all).
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, McpValue>,
    },
    /// Streamable HTTP.
    Http {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, McpValue>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServer {
    pub id: String,
    pub name: String,
    pub transport: McpTransport,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Tools switched off for this server.
    #[serde(default)]
    pub tools_off: BTreeSet<String>,
    /// Devices (peer ids) that run this server; `None` is every device.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub devices: Option<BTreeSet<String>>,
    /// Companions (ids) that may use this server's tools; `None` is every companion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub companions: Option<BTreeSet<String>>,
}

fn yes() -> bool {
    true
}

/// Names whose values are treated as secrets even when entered as plain text.
pub fn looks_secret(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["key", "token", "secret", "password", "passwd", "auth", "cookie", "credential", "bearer", "session"].iter().any(|w| n.contains(w))
}

/// A plain value that looks like a secret, by name or by shape (`Bearer ...`, long opaque strings).
fn secret_like(name: &str, value: &str) -> bool {
    looks_secret(name) || value.trim_start().to_ascii_lowercase().starts_with("bearer ")
}

impl McpServer {
    pub fn new(name: &str, transport: McpTransport) -> Self {
        Self {
            id: crate::ids::new_id("mcp"),
            name: name.to_string(),
            transport,
            enabled: true,
            tools_off: BTreeSet::new(),
            devices: None,
            companions: None,
        }
    }

    fn values_mut(&mut self) -> &mut BTreeMap<String, McpValue> {
        match &mut self.transport {
            McpTransport::Stdio { env, .. } => env,
            McpTransport::Http { headers, .. } => headers,
        }
    }

    fn values(&self) -> &BTreeMap<String, McpValue> {
        match &self.transport {
            McpTransport::Stdio { env, .. } => env,
            McpTransport::Http { headers, .. } => headers,
        }
    }

    /// Moves secret-looking plain values out: returns them (to store in [`McpSecrets`]) and
    /// leaves references in their place. Saving and syncing call this first.
    pub fn take_secrets(&mut self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for (name, v) in self.values_mut().iter_mut() {
            if let McpValue::Plain { value } = v
                && secret_like(name, value)
            {
                out.push((name.clone(), std::mem::take(value)));
                *v = McpValue::Secret { name: name.clone() };
            }
        }
        out
    }

    /// The copy that syncs: never a secret value.
    pub fn for_sync(&self) -> McpServer {
        let mut s = self.clone();
        s.take_secrets();
        s
    }

    /// Secret names this server needs on a device.
    pub fn secret_names(&self) -> Vec<String> {
        self.values().values().filter_map(|v| if let McpValue::Secret { name } = v { Some(name.clone()) } else { None }).collect()
    }

    /// Whether `companion` may use this server's tools.
    pub fn applies_to(&self, companion: &str) -> bool {
        self.companions.as_ref().is_none_or(|c| c.contains(companion))
    }

    /// Whether this device runs the server.
    pub fn runs_on(&self, device: &str) -> bool {
        self.enabled && self.devices.as_ref().is_none_or(|d| d.contains(device))
    }
}

/// Every configured server.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct McpConfigs {
    #[serde(default)]
    pub servers: Vec<McpServer>,
}

impl McpConfigs {
    pub fn upsert(&mut self, server: McpServer) {
        match self.servers.iter_mut().find(|s| s.id == server.id) {
            Some(s) => *s = server,
            None => self.servers.push(server),
        }
        self.servers.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)));
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.servers.len();
        self.servers.retain(|s| s.id != id);
        before != self.servers.len()
    }

    /// The copy that syncs (no secret values).
    pub fn for_sync(&self) -> McpConfigs {
        McpConfigs { servers: self.servers.iter().map(McpServer::for_sync).collect() }
    }
}

/// Per-device secret values for MCP servers. The app implements it over its keystore; values
/// never leave the device and are never logged.
pub trait McpSecrets: Send + Sync {
    fn get(&self, server: &str, name: &str) -> Option<String>;
    fn set(&self, server: &str, name: &str, value: &str);
    fn remove_server(&self, server: &str);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn http() -> McpServer {
        let mut headers = BTreeMap::new();
        headers.insert("Authorization".into(), McpValue::Plain { value: "Bearer abc".into() });
        headers.insert("X-Trace".into(), McpValue::Plain { value: "Bearer xyz".into() });
        headers.insert("Accept".into(), McpValue::Plain { value: "application/json".into() });
        McpServer::new("Docs", McpTransport::Http { url: "https://mcp.example/".into(), headers })
    }

    #[test]
    fn secrets_never_reach_the_synced_copy() {
        let s = http();
        let synced = serde_json::to_string(&s.for_sync()).unwrap();
        assert!(!synced.contains("abc") && !synced.contains("xyz"), "{synced}");
        assert!(synced.contains("application/json"));
        let mut names = s.for_sync().secret_names();
        names.sort();
        assert_eq!(names, ["Authorization", "X-Trace"]);
        let mut owned = s.clone();
        let mut taken = owned.take_secrets();
        taken.sort();
        assert_eq!(taken, [("Authorization".to_string(), "Bearer abc".to_string()), ("X-Trace".to_string(), "Bearer xyz".to_string())]);
        let mut cfgs = McpConfigs::default();
        cfgs.upsert(s);
        assert!(!serde_json::to_string(&cfgs.for_sync()).unwrap().contains("abc"));
    }

    #[test]
    fn stdio_env_names_and_devices() {
        let mut env = BTreeMap::new();
        env.insert("GITHUB_TOKEN".into(), McpValue::Plain { value: "ghp_1".into() });
        env.insert("LOG".into(), McpValue::Plain { value: "info".into() });
        let mut s = McpServer::new("GitHub", McpTransport::Stdio { command: "gh-mcp".into(), args: vec![], env });
        assert_eq!(s.for_sync().secret_names(), ["GITHUB_TOKEN"]);
        assert!(s.runs_on("any"));
        s.devices = Some(["desk".to_string()].into());
        assert!(s.runs_on("desk") && !s.runs_on("phone"));
        s.enabled = false;
        assert!(!s.runs_on("desk"));
        assert!(looks_secret("api_key") && looks_secret("X-Auth") && !looks_secret("LOG"));
    }
}
