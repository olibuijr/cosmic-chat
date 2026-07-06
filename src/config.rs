use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

// ── Root config ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub profile: Profiles,
    #[serde(default)]
    pub servers: Vec<ServerConfig>,
    #[serde(default)]
    pub layout: LayoutConfig,
}

// ── User profiles ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Profiles {
    #[serde(default)]
    pub default: UserProfile,
    #[serde(flatten)]
    pub named: HashMap<String, UserProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    #[serde(default = "default_nick")]
    pub nick: String,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub realname: Option<String>,
}

fn default_nick() -> String {
    "cosmic-user".into()
}

impl Default for UserProfile {
    fn default() -> Self {
        Self {
            nick: default_nick(),
            user: None,
            realname: None,
        }
    }
}

impl Profiles {
    /// Resolve a profile name to its UserProfile. Falls back to "default".
    pub fn get(&self, name: &str) -> &UserProfile {
        self.named.get(name).unwrap_or(&self.default)
    }

    /// All available profile names.
    pub fn names(&self) -> Vec<&String> {
        let mut names: Vec<&String> = self.named.keys().collect();
        names.sort();
        names
    }
}

// ── Server configuration ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Display name for the nav sidebar.
    pub name: String,
    /// IRC server hostname.
    pub host: String,
    /// Port (6697 for TLS, 6667 for plain).
    #[serde(default = "default_port")]
    pub port: u16,
    /// Use TLS.
    #[serde(default = "default_true")]
    pub tls: bool,
    /// Profile name to use for nick/user/realname.
    #[serde(default = "default_profile_name")]
    pub profile: String,
    /// Server password (NICKSERV or SASL).
    #[serde(default)]
    pub password: Option<String>,
    /// Channels to auto-join on connect.
    #[serde(default)]
    pub channels: Vec<String>,
    /// Connect automatically on app startup.
    #[serde(default)]
    pub auto_connect: bool,
    /// Reconnect automatically on disconnect.
    #[serde(default)]
    pub reconnect: bool,
    /// Seconds to wait before reconnecting.
    #[serde(default = "default_reconnect_delay")]
    pub reconnect_delay_secs: u64,
    /// SASL authentication username (if different from nick).
    #[serde(default)]
    pub sasl_user: Option<String>,
    /// SASL authentication password.
    #[serde(default)]
    pub sasl_pass: Option<String>,
}
impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            host: "irc.libera.chat".into(),
            port: 6697,
            tls: true,
            profile: "default".into(),
            password: None,
            channels: vec!["#cosmic-chat".into()],
            auto_connect: false,
            reconnect: false,
            reconnect_delay_secs: 10,
            sasl_user: None,
            sasl_pass: None,
        }
    }
}



fn default_port() -> u16 {
    6697
}

fn default_true() -> bool {
    true
}

fn default_profile_name() -> String {
    "default".into()
}

fn default_reconnect_delay() -> u64 {
    10
}

// ── Layout configuration ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutConfig {
    /// Sidebar width in logical pixels.
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: u16,
    /// Font scale factor (0.8–1.5).
    #[serde(default = "default_font_scale")]
    pub font_scale: f32,
    /// Timestamp format string (chrono-style: "%H:%M", "%H:%M:%S", etc.).
    #[serde(default = "default_timestamp_format")]
    pub timestamp_format: String,
    /// Show join/part/quit messages in channel view.
    #[serde(default = "default_true")]
    pub show_join_part: bool,
    /// Show timestamps next to each message.
    #[serde(default = "default_true")]
    pub show_timestamps: bool,
    /// Maximum messages to keep in scrollback per channel.
    #[serde(default = "default_scrollback")]
    pub max_scrollback: usize,
}

fn default_sidebar_width() -> u16 {
    220
}

fn default_font_scale() -> f32 {
    1.0
}

fn default_timestamp_format() -> String {
    "%H:%M".into()
}

fn default_scrollback() -> usize {
    5000
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            sidebar_width: default_sidebar_width(),
            font_scale: default_font_scale(),
            timestamp_format: default_timestamp_format(),
            show_join_part: default_true(),
            show_timestamps: default_true(),
            max_scrollback: default_scrollback(),
        }
    }
}

// ── Default config ──────────────────────────────────────────────────────────

impl Default for Config {
    fn default() -> Self {
        Self {
            profile: Profiles::default(),
            servers: vec![ServerConfig {
                name: "Libera.Chat".into(),
                host: "irc.libera.chat".into(),
                port: 6697,
                tls: true,
                profile: "default".into(),
                password: None,
                channels: vec!["#cosmic-chat".into()],
                auto_connect: false,
                reconnect: false,
                reconnect_delay_secs: 10,
                sasl_user: None,
                sasl_pass: None,
            }],
            layout: LayoutConfig::default(),
        }
    }
}

// ── Persistence ─────────────────────────────────────────────────────────────

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
            Ok(s) => toml::from_str(&s).unwrap_or_else(|e| {
                log::warn!("Config parse error: {e}; using defaults");
                Self::default()
            }),
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

    /// Resolve a server's effective nick/user/realname from its profile reference.
    pub fn resolve_profile(&self, server: &ServerConfig) -> &UserProfile {
        self.profile.get(&server.profile)
    }

    /// Generate a default config file on disk if none exists.
    pub fn ensure_default() -> Self {
        let cfg = Self::load();
        let path = Self::config_path();
        if !path.exists() {
            cfg.save();
        }
        cfg
    }
}
