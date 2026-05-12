use std::collections::HashMap;
use std::sync::Arc;

use super::provider::{AgentProvider, ProviderInfo};

pub struct Registry {
    providers: HashMap<String, Arc<dyn AgentProvider>>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    pub fn register(&mut self, provider: Arc<dyn AgentProvider>) {
        let info = provider.info();
        self.providers.insert(info.id, provider);
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn AgentProvider>> {
        self.providers.get(id).cloned()
    }

    pub fn list(&self) -> Vec<ProviderInfo> {
        let mut infos: Vec<ProviderInfo> = self.providers.values().map(|p| p.info()).collect();
        infos.sort_by(|a, b| a.id.cmp(&b.id));
        infos
    }

    pub fn default_providers() -> Self {
        use super::providers::{AnthropicApi, ClaudeCodeCli, CodexCli, OpenAiApi};
        let mut r = Registry::new();
        r.register(Arc::new(ClaudeCodeCli::new()));
        r.register(Arc::new(CodexCli::new()));
        r.register(Arc::new(AnthropicApi::new()));
        r.register(Arc::new(OpenAiApi::new()));
        r
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::default_providers()
    }
}
