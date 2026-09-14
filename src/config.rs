use anyhow::{Result, ensure};
use serde::Deserialize;
use std::{collections::BTreeMap, net::SocketAddr, path::PathBuf};

#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub listen: SocketAddr,
    pub default_model: String,
    pub models: BTreeMap<String, String>,
    pub timeout_seconds: u64,
    pub max_concurrent_requests: usize,
    pub backend_url: String,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:8787".parse().unwrap(),
            default_model: "gpt-5.6-luna".into(),
            models: BTreeMap::new(),
            timeout_seconds: 300,
            max_concurrent_requests: 8,
            backend_url: "https://chatgpt.com/backend-api/codex/responses".into(),
        }
    }
}
impl Config {
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let config: Self = if path.exists() {
            toml::from_str(&std::fs::read_to_string(path)?)?
        } else {
            Self::default()
        };
        ensure!(
            config.timeout_seconds > 0 && config.max_concurrent_requests > 0,
            "timeout and concurrency must be positive"
        );
        let url = url::Url::parse(&config.backend_url)?;
        ensure!(
            url.scheme() == "https"
                || (url.scheme() == "http"
                    && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))),
            "backend must use HTTPS or loopback HTTP"
        );
        Ok(config)
    }
}
pub fn home() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("CODEXY_HOME") {
        return Ok(path.into());
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
        .ok_or_else(|| anyhow::anyhow!("Set CODEXY_HOME"))?;
    Ok(base.join("codexy"))
}
