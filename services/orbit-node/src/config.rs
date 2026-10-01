//! Node configuration (TOML). Unknown keys are rejected so typos surface.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Directory for the node key and the mailbox database.
    pub data_dir: PathBuf,
    /// UDP addresses to listen on (QUIC). One per address family.
    #[serde(default = "default_listen")]
    pub listen: Vec<SocketAddr>,
    /// Addresses advertised to clients, for example the VPS public IP.
    /// Defaults to the bound listen addresses.
    #[serde(default)]
    pub public_addrs: Vec<SocketAddr>,
    #[serde(default)]
    pub mailbox: MailboxConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct MailboxConfig {
    /// How long an undelivered envelope is kept.
    pub ttl_seconds: u64,
    pub max_items_per_mailbox: u64,
    pub max_bytes_per_mailbox: u64,
    pub max_mailboxes: u64,
    pub max_tokens_per_mailbox: u32,
    /// When set, creating a mailbox requires this code. Recommended for a
    /// private node: without it anyone who can reach the node may register.
    pub registration_code: Option<String>,
    pub sweep_interval_seconds: u64,
    pub max_requests_per_minute: u32,
    pub max_connections: usize,
}

impl Default for MailboxConfig {
    fn default() -> Self {
        Self {
            ttl_seconds: 14 * 24 * 3600,
            max_items_per_mailbox: 10_000,
            max_bytes_per_mailbox: 64 * 1024 * 1024,
            max_mailboxes: 1_000,
            max_tokens_per_mailbox: 256,
            registration_code: None,
            sweep_interval_seconds: 600,
            max_requests_per_minute: 600,
            max_connections: 1_000,
        }
    }
}

fn default_listen() -> Vec<SocketAddr> {
    vec![
        "0.0.0.0:7443".parse().expect("valid address"),
        "[::]:7443".parse().expect("valid address"),
    ]
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read {path}: {source}")]
    Read { path: PathBuf, source: std::io::Error },
    #[error("invalid config: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("invalid config: {0}")]
    Invalid(&'static str),
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_owned(),
            source,
        })?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let config: Config = toml::from_str(text)?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        let m = &self.mailbox;
        if self.listen.is_empty() {
            return Err(ConfigError::Invalid("listen must contain at least one address"));
        }
        if m.ttl_seconds == 0 || m.max_items_per_mailbox == 0 || m.max_bytes_per_mailbox == 0 {
            return Err(ConfigError::Invalid("mailbox limits must be positive"));
        }
        if m.sweep_interval_seconds == 0 || m.max_requests_per_minute == 0 || m.max_connections == 0 {
            return Err(ConfigError::Invalid("intervals and rates must be positive"));
        }
        if m.registration_code.as_deref().is_some_and(|c| c.len() < 12) {
            return Err(ConfigError::Invalid(
                "registration_code must have at least 12 characters",
            ));
        }
        Ok(())
    }
}

/// Commented example written by `orbit-node init`.
pub const EXAMPLE: &str = r#"# orbit-node configuration
data_dir = "/var/lib/orbit-node"

# UDP ports for QUIC. Open them in the firewall.
listen = ["0.0.0.0:7443", "[::]:7443"]

# Addresses clients should dial, e.g. the public IP of the VPS.
# public_addrs = ["203.0.113.10:7443"]

[mailbox]
ttl_seconds = 1209600            # 14 days
max_items_per_mailbox = 10000
max_bytes_per_mailbox = 67108864 # 64 MiB
max_mailboxes = 1000
max_tokens_per_mailbox = 256
# Required to create a mailbox. Share it only with your users.
# registration_code = "change-me-to-a-long-random-string"
sweep_interval_seconds = 600
max_requests_per_minute = 600
max_connections = 1000
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_parses() {
        let config = Config::parse(EXAMPLE).unwrap();
        assert_eq!(config.listen.len(), 2);
        assert_eq!(config.mailbox.ttl_seconds, 14 * 24 * 3600);
    }

    #[test]
    fn minimal_config_uses_defaults() {
        let config = Config::parse("data_dir = \"/tmp/x\"").unwrap();
        assert_eq!(config.listen, default_listen());
        assert!(config.mailbox.registration_code.is_none());
    }

    #[test]
    fn rejects_unknown_keys_and_bad_values() {
        assert!(Config::parse("data_dir = \"/x\"\nlisen = []").is_err());
        assert!(Config::parse("data_dir = \"/x\"\nlisten = []").is_err());
        assert!(Config::parse("data_dir = \"/x\"\n[mailbox]\nttl_seconds = 0").is_err());
        assert!(Config::parse("data_dir = \"/x\"\n[mailbox]\nregistration_code = \"short\"").is_err());
    }
}
