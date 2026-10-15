// Swarm module – placeholder for parallel agent execution support.

use crate::agent::provider::{AgentProvider, ChatMessage, DynError, Tool};
use std::sync::Arc;

/// Holds multiple `AgentProvider`s and can run them concurrently.
pub struct Swarm {
    providers: Vec<Arc<dyn AgentProvider>>,
}

impl Swarm {
    /// Create an empty swarm.
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    /// Add a provider to the swarm.
    pub fn add<P>(&mut self, provider: P)
    where
        P: AgentProvider + 'static,
    {
        self.providers.push(Arc::new(provider));
    }

    /// Run all providers in parallel.
    ///
    /// The concrete implementation is a TODO – for now it simply returns `Ok(())`.
    /// Future work will spawn a task per provider and collect their streams.
    pub async fn run_parallel(
        &self,
        _messages: Vec<ChatMessage>,
        _tools: Option<Vec<Tool>>,
    ) -> Result<(), DynError> {
        // Placeholder: real parallel execution to be added.
        Ok(())
    }
}
