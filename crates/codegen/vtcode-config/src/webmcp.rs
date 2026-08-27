use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::path::PathBuf;

const MIN_FRAME_BYTES: usize = 1024;

/// Configuration for the opt-in VT Code WebMCP bridge.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct WebmcpConfig {
    /// Whether a session may start a WebMCP listener through an integration.
    #[serde(default)]
    pub enabled: bool,
    /// Literal loopback bind host.
    #[serde(default = "default_host")]
    pub host: String,
    /// Bind port. Zero asks the OS for an available port.
    #[serde(default)]
    pub port: u16,
    /// Exact browser origins allowed to pair.
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    /// Explicit roots available to headless mode; the current bridge serves one root per process.
    #[serde(default)]
    pub allowed_roots: Vec<PathBuf>,
    /// One-time pairing and session lifetime.
    #[serde(default = "default_pairing_ttl_secs")]
    pub pairing_ttl_secs: u64,
    /// Maximum JSON WebSocket frame size.
    #[serde(default = "default_max_frame_bytes")]
    pub max_frame_bytes: usize,
    /// Maximum concurrent bridge operations.
    #[serde(default = "default_max_in_flight_requests")]
    pub max_in_flight_requests: usize,
}

impl Default for WebmcpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            host: default_host(),
            port: 0,
            allowed_origins: Vec::new(),
            allowed_roots: Vec::new(),
            pairing_ttl_secs: default_pairing_ttl_secs(),
            max_frame_bytes: default_max_frame_bytes(),
            max_in_flight_requests: default_max_in_flight_requests(),
        }
    }
}

impl WebmcpConfig {
    /// Validate limits and origin syntax without touching the filesystem.
    pub fn validate(&self) -> Result<()> {
        if self.host.trim().is_empty() {
            bail!("webmcp.host must not be empty");
        }
        let address = self
            .host
            .parse::<IpAddr>()
            .map_err(|_error| anyhow::anyhow!("webmcp.host must be a literal IP address"))?;
        if !address.is_loopback() {
            bail!("webmcp.host must be a loopback address; use a TLS-terminating reverse proxy for remote access");
        }
        if self.pairing_ttl_secs == 0 || self.pairing_ttl_secs > 3600 {
            bail!("webmcp.pairing_ttl_secs must be between 1 and 3600");
        }
        if self.max_frame_bytes < MIN_FRAME_BYTES || self.max_frame_bytes > 16 * 1024 * 1024 {
            bail!("webmcp.max_frame_bytes must be between {MIN_FRAME_BYTES} and 16777216");
        }
        if self.max_in_flight_requests == 0 || self.max_in_flight_requests > 64 {
            bail!("webmcp.max_in_flight_requests must be between 1 and 64");
        }
        if self.allowed_origins.iter().any(|origin| !is_valid_origin(origin)) {
            bail!("webmcp.allowed_origins must contain explicit origins such as https://example.test");
        }
        if self.allowed_roots.iter().any(|root| root.as_os_str().is_empty()) {
            bail!("webmcp.allowed_roots must not contain empty paths");
        }
        Ok(())
    }
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn is_valid_origin(origin: &str) -> bool {
    let Ok(parsed) = url::Url::parse(origin) else {
        return false;
    };
    origin == origin.trim()
        && !origin.chars().any(char::is_whitespace)
        && !origin.contains('*')
        && matches!(parsed.scheme(), "http" | "https")
        && parsed.host_str().is_some_and(|host| !host.is_empty())
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && (parsed.path().is_empty() || (parsed.path() == "/" && !origin.ends_with('/')))
        && parsed.query().is_none()
        && parsed.fragment().is_none()
}

const fn default_pairing_ttl_secs() -> u64 {
    300
}

const fn default_max_frame_bytes() -> usize {
    1_048_576
}

const fn default_max_in_flight_requests() -> usize {
    8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_keep_webmcp_disabled_and_loopback_only() {
        let config = WebmcpConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 0);
        assert!(config.allowed_origins.is_empty());
        config.validate().expect("defaults should validate");
    }

    #[test]
    fn invalid_limits_and_origins_are_rejected() {
        let config = WebmcpConfig { max_in_flight_requests: 0, ..Default::default() };
        assert!(config.validate().is_err());
        let config = WebmcpConfig {
            allowed_origins: vec!["*".to_string()],
            ..Default::default()
        };
        assert!(config.validate().is_err());
        for origin in [
            "https://*",
            "ftp://example.test",
            "https://example.test/path",
            "https://user@example.test",
        ] {
            let config = WebmcpConfig {
                allowed_origins: vec![origin.to_string()],
                ..Default::default()
            };
            assert!(config.validate().is_err(), "accepted {origin}");
        }
        let config = WebmcpConfig { host: "0.0.0.0".to_string(), ..Default::default() };
        assert!(config.validate().is_err());
    }
}
