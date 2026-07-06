use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub servers: Vec<ServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub nick: String,
    pub user: Option<String>,
    pub realname: Option<String>,
    pub password: Option<String>,
    pub use_tls: bool,
    pub channels: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            servers: vec![ServerConfig {
                name: "Libera.Chat".into(),
                host: "irc.libera.chat".into(),
                port: 6697,
                nick: "cosmic-user".into(),
                user: None,
                realname: None,
                password: None,
                use_tls: true,
                channels: vec!["#cosmic-chat".into()],
            }],
        }
    }
}

impl Config {
    pub fn config_path() -> PathBuf {
        let base = if let Ok(p) = std::env::var("XDG_CONFIG_HOME") {
            PathBuf::from(p)
        } else {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".config")
        };
        base.join("cosmic-chat").join("config.toml")
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        match std::fs::read_to_string(&path) {
            Ok(s) => toml::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(s) = toml::to_string_pretty(self) {
            let _ = std::fs::write(&path, s);
        }
    }
}
