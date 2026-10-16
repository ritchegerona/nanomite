// Swarm: a fixed pool of providers that can run completions concurrently.
//
// The swarm never inspects what a provider says: replies are opaque text the
// caller may store or forward.

use crate::agent::provider::{AgentProvider, ChatMessage, DynError, Tool};
use futures_util::future::join_all;
use std::sync::Arc;

/// Holds multiple `AgentProvider`s and can run them concurrently.
pub struct Swarm {
    providers: Vec<Arc<dyn AgentProvider>>,
}

impl Default for Swarm {
    fn default() -> Self {
        Self::new()
    }
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

    /// Add an already-shared provider (e.g. the runtime's `Arc<dyn AgentProvider>`)
    /// so several consumers stay on one client instance.
    pub fn add_arc(&mut self, provider: Arc<dyn AgentProvider>) {
        self.providers.push(provider);
    }

    /// Number of registered providers.
    pub fn len(&self) -> usize {
        self.providers.len()
    }

    /// True when no provider has been added.
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    /// Run a full (non-streaming) completion on every provider concurrently.
    ///
    /// Every provider receives the same conversation. One tokio task is spawned
    /// per provider; results preserve provider order as `(index, result)`, and a
    /// failing provider never poisons the others.
    pub async fn run_parallel_chat(
        &self,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
    ) -> Vec<(usize, Result<String, DynError>)> {
        let conversations: Vec<Vec<ChatMessage>> =
            self.providers.iter().map(|_| messages.clone()).collect();
        self.run_conversations(conversations, tools).await
    }

    /// Run one distinct conversation per entry, concurrently.
    ///
    /// `conversations[i]` is dispatched to provider `i % len(providers)`, so the
    /// pool may be smaller than the number of conversations (several personas
    /// can share one model). Results preserve entry order as `(index, result)`;
    /// a failing conversation yields an `Err` in its slot.
    pub async fn run_conversations(
        &self,
        conversations: Vec<Vec<ChatMessage>>,
        tools: Option<Vec<Tool>>,
    ) -> Vec<(usize, Result<String, DynError>)> {
        if self.providers.is_empty() {
            return conversations
                .into_iter()
                .enumerate()
                .map(|(index, _)| (index, Err("swarm has no providers".into())))
                .collect();
        }
        let tasks = conversations
            .into_iter()
            .enumerate()
            .map(|(index, messages)| {
                let provider = Arc::clone(&self.providers[index % self.providers.len()]);
                let tools = tools.clone();
                tokio::spawn(async move {
                    let (content, _calls, _usage) =
                        provider.chat_completion(messages, tools).await?;
                    Ok::<String, DynError>(content)
                })
            });
        join_all(tasks)
            .await
            .into_iter()
            .enumerate()
            .map(|(index, joined)| {
                let result = match joined {
                    Ok(inner) => inner,
                    Err(join_err) => Err(Box::new(join_err) as DynError),
                };
                (index, result)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::provider::BoxStream;
    use crate::agent::testdouble::ScriptedProvider;
    use async_trait::async_trait;

    struct FailingProvider;

    #[async_trait]
    impl AgentProvider for FailingProvider {
        async fn stream_chat(
            &self,
            _messages: Vec<ChatMessage>,
            _tools: Option<Vec<Tool>>,
        ) -> Result<BoxStream, DynError> {
            Err("scripted provider failure".into())
        }
    }

    #[tokio::test]
    async fn preserves_provider_order_across_results() {
        let mut swarm = Swarm::new();
        swarm.add(ScriptedProvider::default());
        swarm.add(ScriptedProvider::default());

        let results = swarm
            .run_parallel_chat(vec![ChatMessage::user("hello")], None)
            .await;

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, 0);
        assert_eq!(results[1].0, 1);
        let (Ok(a), Ok(b)) = (&results[0].1, &results[1].1) else {
            panic!("both scripted providers must succeed");
        };
        assert_eq!(a, b, "identical scripted input yields identical replies");
        assert!(a.contains("Scripted test reply"));
    }

    #[tokio::test]
    async fn reports_failing_provider_without_poisoning_the_rest() {
        let mut swarm = Swarm::new();
        swarm.add(ScriptedProvider::default());
        swarm.add(FailingProvider);
        swarm.add(ScriptedProvider::default());

        let results = swarm
            .run_parallel_chat(vec![ChatMessage::user("hello")], None)
            .await;

        assert_eq!(results.len(), 3);
        assert!(results[0].1.is_ok());
        let err = results[1].1.as_ref().expect_err("provider must fail");
        assert!(err.to_string().contains("scripted provider failure"));
        assert!(results[2].1.is_ok());
    }

    #[tokio::test]
    async fn distinct_conversations_map_round_robin_over_the_pool() {
        let mut swarm = Swarm::new();
        swarm.add(ScriptedProvider::default());
        swarm.add(ScriptedProvider::default());

        let conversations = (0..4)
            .map(|i| vec![ChatMessage::user(format!("question {i}"))])
            .collect::<Vec<_>>();
        let results = swarm.run_conversations(conversations, None).await;

        assert_eq!(results.len(), 4);
        for (index, result) in &results {
            assert!(result.is_ok(), "conversation {index} failed");
        }
    }

    #[tokio::test]
    async fn empty_swarm_reports_error_per_conversation() {
        let swarm = Swarm::new();
        let results = swarm
            .run_conversations(vec![vec![ChatMessage::user("x")]], None)
            .await;
        assert_eq!(results.len(), 1);
        assert!(results[0].1.is_err());
    }
}
