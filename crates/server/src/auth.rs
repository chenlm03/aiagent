use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use rand_core::OsRng;
use async_trait::async_trait;
use axum::extract::{FromRef, FromRequestParts};
use axum::http::header::AUTHORIZATION;
use axum::http::{request::Parts, StatusCode};
use dashmap::DashMap;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    User,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub username: String,
    pub password_hash: String,
    pub role: Role,
    pub workspace_root: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Public-facing view, never includes the password hash.
#[derive(Debug, Clone, Serialize)]
pub struct UserView {
    pub username: String,
    pub role: Role,
    pub workspace_root: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl From<&User> for UserView {
    fn from(u: &User) -> Self {
        Self {
            username: u.username.clone(),
            role: u.role,
            workspace_root: u.workspace_root.clone(),
            created_at: u.created_at,
            updated_at: u.updated_at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct UsersFile {
    #[serde(default)]
    users: Vec<User>,
}

/// File-backed user store under `~/.config/aiagent-server/users.json`.
/// All reads/writes go through an OS-level flock so concurrent server
/// processes (or async tasks) can't corrupt the file.
pub struct UserStore {
    path: PathBuf,
}

impl UserStore {
    pub fn new() -> anyhow::Result<Self> {
        let dir = server_config_dir();
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("mkdir {}", dir.display()))?;
        let store = Self {
            path: dir.join("users.json"),
        };
        store.seed_if_missing()?;
        Ok(store)
    }

    fn seed_if_missing(&self) -> anyhow::Result<()> {
        if self.path.exists() {
            return Ok(());
        }
        let now = unix_ts();
        let admin = User {
            username: "nick".into(),
            password_hash: hash_password("123456")?,
            role: Role::Admin,
            workspace_root: None,
            created_at: now,
            updated_at: now,
        };
        let f = UsersFile { users: vec![admin] };
        let bytes = serde_json::to_vec_pretty(&f)?;
        std::fs::write(&self.path, bytes)?;
        tracing::info!(
            "seeded default admin nick/123456 at {}",
            self.path.display()
        );
        Ok(())
    }

    fn with_lock<F, R>(&self, exclusive: bool, f: F) -> anyhow::Result<R>
    where
        F: FnOnce(&mut UsersFile) -> anyhow::Result<R>,
    {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&self.path)
            .with_context(|| format!("open {}", self.path.display()))?;
        if exclusive {
            file.lock_exclusive()?;
        } else {
            file.lock_shared()?;
        }
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        let mut data: UsersFile = if buf.trim().is_empty() {
            UsersFile::default()
        } else {
            serde_json::from_str(&buf)?
        };
        let result = f(&mut data)?;
        if exclusive {
            let json = serde_json::to_vec_pretty(&data)?;
            file.set_len(0)?;
            file.seek(SeekFrom::Start(0))?;
            file.write_all(&json)?;
            file.flush()?;
        }
        Ok(result)
    }

    pub fn find(&self, username: &str) -> anyhow::Result<Option<User>> {
        self.with_lock(false, |f| {
            Ok(f.users.iter().find(|u| u.username == username).cloned())
        })
    }

    pub fn list(&self) -> anyhow::Result<Vec<User>> {
        self.with_lock(false, |f| Ok(f.users.clone()))
    }

    pub fn create(
        &self,
        username: &str,
        password: &str,
        workspace_root: Option<String>,
    ) -> anyhow::Result<User> {
        validate_username(username)?;
        validate_password(password)?;
        let now = unix_ts();
        let user = User {
            username: username.into(),
            password_hash: hash_password(password)?,
            role: Role::User,
            workspace_root,
            created_at: now,
            updated_at: now,
        };
        self.with_lock(true, |f| {
            if f.users.iter().any(|u| u.username == username) {
                anyhow::bail!("用户已存在: {}", username);
            }
            f.users.push(user.clone());
            Ok(())
        })?;
        Ok(user)
    }

    pub fn delete(&self, username: &str) -> anyhow::Result<()> {
        self.with_lock(true, |f| {
            let idx = f
                .users
                .iter()
                .position(|u| u.username == username)
                .ok_or_else(|| anyhow!("用户不存在: {username}"))?;
            if f.users[idx].role == Role::Admin {
                let admins = f.users.iter().filter(|u| u.role == Role::Admin).count();
                if admins <= 1 {
                    anyhow::bail!("不能删除最后一个管理员");
                }
            }
            f.users.remove(idx);
            Ok(())
        })
    }

    pub fn set_password(&self, username: &str, password: &str) -> anyhow::Result<()> {
        validate_password(password)?;
        let hash = hash_password(password)?;
        self.with_lock(true, |f| {
            let u = f
                .users
                .iter_mut()
                .find(|u| u.username == username)
                .ok_or_else(|| anyhow!("用户不存在: {username}"))?;
            u.password_hash = hash;
            u.updated_at = unix_ts();
            Ok(())
        })
    }

    pub fn set_workspace(
        &self,
        username: &str,
        workspace_root: Option<String>,
    ) -> anyhow::Result<()> {
        self.with_lock(true, |f| {
            let u = f
                .users
                .iter_mut()
                .find(|u| u.username == username)
                .ok_or_else(|| anyhow!("用户不存在: {username}"))?;
            u.workspace_root = workspace_root;
            u.updated_at = unix_ts();
            Ok(())
        })
    }

    /// Returns Some(user) on success, None on bad credentials.
    pub fn verify(&self, username: &str, password: &str) -> anyhow::Result<Option<User>> {
        let user = match self.find(username)? {
            Some(u) => u,
            None => return Ok(None),
        };
        let parsed = PasswordHash::new(&user.password_hash)
            .map_err(|e| anyhow!("password hash parse: {e}"))?;
        if Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
        {
            Ok(Some(user))
        } else {
            Ok(None)
        }
    }
}

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow!("hash: {e}"))?
        .to_string();
    Ok(hash)
}

fn validate_username(u: &str) -> anyhow::Result<()> {
    if u.is_empty() || u.len() > 32 {
        anyhow::bail!("用户名长度必须为 1-32");
    }
    if !u
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        anyhow::bail!("用户名只能包含字母数字和 _ - .");
    }
    Ok(())
}

fn validate_password(p: &str) -> anyhow::Result<()> {
    if p.len() < 4 || p.len() > 128 {
        anyhow::bail!("密码长度必须为 4-128");
    }
    Ok(())
}

fn server_config_dir() -> PathBuf {
    if let Ok(s) = std::env::var("AIAGENT_SERVER_CONFIG_DIR") {
        return PathBuf::from(s);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("aiagent-server")
}

fn unix_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone)]
struct TokenInfo {
    username: String,
    expires_at: i64,
}

pub struct TokenStore {
    tokens: DashMap<String, TokenInfo>,
    ttl_secs: i64,
}

impl TokenStore {
    pub fn new(ttl: Duration) -> Self {
        Self {
            tokens: DashMap::new(),
            ttl_secs: ttl.as_secs() as i64,
        }
    }

    pub fn mint(&self, username: &str) -> String {
        let token = Uuid::new_v4().to_string();
        self.tokens.insert(
            token.clone(),
            TokenInfo {
                username: username.to_string(),
                expires_at: unix_ts() + self.ttl_secs,
            },
        );
        token
    }

    pub fn lookup(&self, token: &str) -> Option<String> {
        let now = unix_ts();
        if let Some(info) = self.tokens.get(token) {
            if info.expires_at > now {
                return Some(info.username.clone());
            }
        }
        self.tokens.remove(token);
        None
    }

    pub fn revoke(&self, token: &str) {
        self.tokens.remove(token);
    }

    pub fn revoke_user(&self, username: &str) {
        let to_remove: Vec<String> = self
            .tokens
            .iter()
            .filter(|kv| kv.value().username == username)
            .map(|kv| kv.key().clone())
            .collect();
        for t in to_remove {
            self.tokens.remove(&t);
        }
    }
}

/// Bundle of auth state, easy to clone into the axum AppState.
#[derive(Clone)]
pub struct AuthState {
    pub users: Arc<UserStore>,
    pub tokens: Arc<TokenStore>,
}

/// Extractor: any logged-in user.
pub struct AuthedUser(pub User);

#[async_trait]
impl<S> FromRequestParts<S> for AuthedUser
where
    AuthState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = (StatusCode, String);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth = AuthState::from_ref(state);
        let token = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .ok_or((StatusCode::UNAUTHORIZED, "未提供 Bearer token".into()))?;
        let username = auth
            .tokens
            .lookup(token)
            .ok_or((StatusCode::UNAUTHORIZED, "token 无效或已过期".into()))?;
        let user = auth
            .users
            .find(&username)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .ok_or((StatusCode::UNAUTHORIZED, "用户已被删除".into()))?;
        Ok(AuthedUser(user))
    }
}

/// Extractor: must be an admin.
pub struct AdminUser(pub User);

#[async_trait]
impl<S> FromRequestParts<S> for AdminUser
where
    AuthState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = (StatusCode, String);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let user = AuthedUser::from_request_parts(parts, state).await?.0;
        if user.role != Role::Admin {
            return Err((StatusCode::FORBIDDEN, "需要管理员权限".into()));
        }
        Ok(AdminUser(user))
    }
}
