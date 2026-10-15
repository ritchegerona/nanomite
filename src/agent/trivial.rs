use crate::agent::provider::{AgentProvider, ChatMessage, Role, StreamEvent, Tool};
use async_trait::async_trait;
use futures_util::stream::{self, Stream, StreamExt};
use std::pin::Pin;
use std::error::Error;

/// A very simple test provider that returns hard‑coded text.
pub struct TrivialProvider;

#[async_trait]
impl AgentProvider for TrivialProvider {
    /// Returns the string `"Hello from TrivialProvider"` split into delta events.
    async fn stream_chat(
        &self,
        _messages: Vec<ChatMessage>,
        _tools: Option<Vec<Tool>>,
    ) -> Result<Box<dyn Stream<Item = Result<StreamEvent, Box<dyn Error + Send + Sync>>> + Send>, Box<dyn Error + Send + Sync>> {
        let chunks = vec!["Hello", " ", "from", " ", "TrivialProvider", "!"];
        let stream = stream::iter(chunks.into_iter().map(|c| Ok(StreamEvent::Delta(c.to_string()))));
        Ok(Box::pin(stream))
    }

    async fn list_models(&self) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        Ok(vec!["trivial-model".to_string()])
    }
}
