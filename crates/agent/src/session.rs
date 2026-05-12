use dashmap::DashMap;
use tokio_util::sync::CancellationToken;

pub struct SessionManager {
    tokens: DashMap<String, CancellationToken>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            tokens: DashMap::new(),
        }
    }

    pub fn register(&self, session_id: String, token: CancellationToken) {
        self.tokens.insert(session_id, token);
    }

    pub fn cancel(&self, session_id: &str) -> bool {
        if let Some((_, token)) = self.tokens.remove(session_id) {
            token.cancel();
            true
        } else {
            false
        }
    }

    pub fn remove(&self, session_id: &str) {
        self.tokens.remove(session_id);
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}
