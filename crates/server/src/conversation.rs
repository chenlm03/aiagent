use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub name: String,
    pub provider_id: String,
    /// Path of the per-conversation subdirectory, relative to workspace_root.
    /// Claude is always launched with cwd = workspace_root.join(subdir).
    pub subdir: String,
    /// Provider's own session id (e.g. Claude Code's session uuid). Filled
    /// in after the first turn so subsequent turns can pass --resume <id>.
    pub provider_session_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct StoreFile {
    #[serde(default)]
    conversations: Vec<Conversation>,
}

/// Per-workspace persistent conversation list.
/// Lives at `<workspace_root>/.aiagent/conversations.json`.
pub struct ConversationStore;

impl ConversationStore {
    pub fn validate_workspace(root: &Path) -> Result<(), String> {
        let meta = root
            .metadata()
            .map_err(|e| format!("workspace not accessible: {e}"))?;
        if !meta.is_dir() {
            return Err("workspace is not a directory".into());
        }
        let probe = root.join(".aiagent-rw-probe");
        std::fs::write(&probe, b"")
            .map_err(|e| format!("workspace not writable: {e}"))?;
        let _ = std::fs::remove_file(&probe);
        Ok(())
    }

    fn metadata_path(workspace_root: &Path) -> PathBuf {
        workspace_root.join(".aiagent").join("conversations.json")
    }

    fn load(workspace_root: &Path) -> Result<StoreFile, String> {
        let path = Self::metadata_path(workspace_root);
        if !path.exists() {
            return Ok(StoreFile::default());
        }
        let bytes = std::fs::read(&path).map_err(|e| format!("read metadata: {e}"))?;
        serde_json::from_slice(&bytes).map_err(|e| format!("parse metadata: {e}"))
    }

    fn save(workspace_root: &Path, store: &StoreFile) -> Result<(), String> {
        let dir = workspace_root.join(".aiagent");
        std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir .aiagent: {e}"))?;
        let path = dir.join("conversations.json");
        let bytes = serde_json::to_vec_pretty(store).map_err(|e| format!("encode: {e}"))?;
        std::fs::write(path, bytes).map_err(|e| format!("write metadata: {e}"))?;
        Ok(())
    }

    pub fn list(workspace_root: &Path) -> Result<Vec<Conversation>, String> {
        Self::validate_workspace(workspace_root)?;
        let store = Self::load(workspace_root)?;
        Ok(store.conversations)
    }

    pub fn get(workspace_root: &Path, id: &str) -> Result<Option<Conversation>, String> {
        Self::validate_workspace(workspace_root)?;
        let store = Self::load(workspace_root)?;
        Ok(store.conversations.into_iter().find(|c| c.id == id))
    }

    pub fn create(
        workspace_root: &Path,
        provider_id: &str,
        name: Option<String>,
    ) -> Result<Conversation, String> {
        Self::validate_workspace(workspace_root)?;
        let now = unix_ts();
        let short = Uuid::new_v4().to_string()[..8].to_string();
        let id = format!("conv-{}-{}", now, short);
        let display_name = name.unwrap_or_else(|| format!("Session {}", short));

        // Create the per-conversation subdir under the workspace.
        let subdir_path = workspace_root.join(&id);
        std::fs::create_dir_all(&subdir_path)
            .map_err(|e| format!("create conv dir {}: {e}", subdir_path.display()))?;

        let conv = Conversation {
            id: id.clone(),
            name: display_name,
            provider_id: provider_id.to_string(),
            subdir: id,
            provider_session_id: None,
            created_at: now,
            updated_at: now,
        };

        let mut store = Self::load(workspace_root)?;
        store.conversations.push(conv.clone());
        Self::save(workspace_root, &store)?;
        Ok(conv)
    }

    pub fn update_provider_session_id(
        workspace_root: &Path,
        conversation_id: &str,
        provider_session_id: &str,
    ) -> Result<(), String> {
        let mut store = Self::load(workspace_root)?;
        let mut changed = false;
        for c in store.conversations.iter_mut() {
            if c.id == conversation_id {
                if c.provider_session_id.as_deref() != Some(provider_session_id) {
                    c.provider_session_id = Some(provider_session_id.to_string());
                    c.updated_at = unix_ts();
                    changed = true;
                }
                break;
            }
        }
        if changed {
            Self::save(workspace_root, &store)?;
        }
        Ok(())
    }

    pub fn touch(workspace_root: &Path, conversation_id: &str) -> Result<(), String> {
        let mut store = Self::load(workspace_root)?;
        for c in store.conversations.iter_mut() {
            if c.id == conversation_id {
                c.updated_at = unix_ts();
                break;
            }
        }
        Self::save(workspace_root, &store)
    }
}

fn unix_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
