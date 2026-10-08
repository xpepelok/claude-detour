use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

pub const DIR_NAME: &str = ".claude-detour";
pub const FILE_NAME: &str = "config.toml";
pub const LOG_NAME: &str = "claude-detour.log";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Http,
    Socks5,
}

impl Protocol {
    pub const ALL: [Protocol; 2] = [Protocol::Http, Protocol::Socks5];
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Protocol::Http => "http",
            Protocol::Socks5 => "socks5",
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub protocol: Protocol,
    pub host: String,
    pub port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pass: Option<String>,
    #[serde(default = "default_local_port")]
    pub local_port: u16,
    #[serde(default = "default_max_upstream")]
    pub max_upstream: usize,
}

fn default_local_port() -> u16 {
    18080
}

fn default_max_upstream() -> usize {
    6
}

impl Config {
    pub fn credentials(&self) -> Option<(&str, &str)> {
        match (&self.user, &self.pass) {
            (Some(u), Some(p)) if !u.is_empty() => Some((u, p)),
            _ => None,
        }
    }

    pub fn load(path: &Path) -> Result<Config> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let cfg: Config =
            toml::from_str(&text).with_context(|| format!("parsing config {}", path.display()))?;
        Ok(cfg)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let text = toml::to_string_pretty(self).context("serializing config")?;
        fs::write(path, text).with_context(|| format!("writing config {}", path.display()))?;
        Ok(())
    }
}

pub fn base_dir() -> PathBuf {
    let home = env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(DIR_NAME)
}

pub fn default_path() -> PathBuf {
    base_dir().join(FILE_NAME)
}

pub fn log_path() -> PathBuf {
    base_dir().join(LOG_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_with_credentials() {
        let cfg = Config {
            protocol: Protocol::Socks5,
            host: "1.2.3.4".into(),
            port: 1080,
            user: Some("alice".into()),
            pass: Some("secret".into()),
            local_port: 18080,
            max_upstream: 6,
        };
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.protocol, Protocol::Socks5);
        assert_eq!(back.host, "1.2.3.4");
        assert_eq!(back.port, 1080);
        assert_eq!(back.credentials(), Some(("alice", "secret")));
    }

    #[test]
    fn defaults_fill_in_missing_fields() {
        let cfg: Config =
            toml::from_str("protocol = \"http\"\nhost = \"h\"\nport = 8080\n").unwrap();
        assert_eq!(cfg.local_port, 18080);
        assert_eq!(cfg.max_upstream, 6);
        assert_eq!(cfg.credentials(), None);
    }

    #[test]
    fn empty_user_is_not_credentials() {
        let cfg = Config {
            protocol: Protocol::Http,
            host: "h".into(),
            port: 8080,
            user: Some(String::new()),
            pass: Some(String::new()),
            local_port: 18080,
            max_upstream: 6,
        };
        assert_eq!(cfg.credentials(), None);
    }
}
