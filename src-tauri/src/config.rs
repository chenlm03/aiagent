use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    /// Where the relay server lives. Defaults to http://127.0.0.1:8788.
    pub server_url: Option<String>,
    pub active_provider: Option<String>,
    pub active_conversation_id: Option<String>,
    /// Bearer token from /api/login. The workspace_root is now owned by the
    /// server (per user) and looked up via /api/me, so it's not stored here
    /// anymore.
    pub auth_token: Option<String>,
    /// UI theme: "light" (default) or "dark".
    pub theme: Option<String>,
}

impl AppConfig {
    fn path() -> PathBuf {
        let dir = config_dir();
        let _ = std::fs::create_dir_all(&dir);
        dir.join("config.json")
    }

    pub fn load() -> anyhow::Result<Self> {
        let path = Self::path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let bytes = std::fs::read(path)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path();
        let bytes = serde_json::to_vec_pretty(self)?;
        std::fs::write(path, bytes)?;
        Ok(())
    }
}

fn config_dir() -> PathBuf {
    if let Ok(s) = std::env::var("AIAGENT_CONFIG_DIR") {
        return PathBuf::from(s);
    }
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join("Library/Application Support"))
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(|| PathBuf::from("."))
    };
    base.join("aiagent")
}
