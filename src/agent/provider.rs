use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::pin::Pin;

pub type DynError = Box<dyn Error + Send + Sync>;
pub type BoxStream =
    Pin<Box<dyn futures_util::Stream<Item = Result<StreamEvent, DynError>> + Send>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    Tool,
    System,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn tool_result(id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            tool_calls: None,
            tool_call_id: Some(id.into()),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolCall {
    #[serde(default)]
    pub id: String,
    pub name: String,
    /// Raw JSON string exactly as produced by the model.
    #[serde(default)]
    pub arguments: String,
    /// Position in the tool-call array; providers stream fragments out of order.
    #[serde(default)]
    pub index: usize,
}

/// Reassembles streamed tool-call fragments (name and arguments arrive in pieces).
#[derive(Debug, Default)]
pub struct ToolCallAccumulator {
    slots: Vec<ToolCall>,
}

impl ToolCallAccumulator {
    pub fn push(&mut self, fragment: ToolCall) {
        let idx = fragment.index;
        let slot = match self.slots.get_mut(idx) {
            Some(slot) => slot,
            None => {
                self.slots.resize(idx + 1, ToolCall::default());
                &mut self.slots[idx]
            }
        };
        if !fragment.id.is_empty() {
            slot.id = fragment.id;
        }
        if !fragment.name.is_empty() {
            slot.name.push_str(&fragment.name);
        }
        slot.arguments.push_str(&fragment.arguments);
    }

    pub fn finish(self) -> Vec<ToolCall> {
        self.slots
            .into_iter()
            .filter(|c| !c.name.is_empty())
            .enumerate()
            .map(|(i, mut c)| {
                c.index = i;
                c
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub r#type: String,
    pub function: Function,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Function {
    pub name: String,
    pub description: Option<String>,
    pub parameters: serde_json::Value,
}

impl Tool {
    pub fn new(name: &str, description: &str, parameters: serde_json::Value) -> Self {
        Self {
            r#type: "function".to_string(),
            function: Function {
                name: name.to_string(),
                description: Some(description.to_string()),
                parameters,
            },
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

impl Usage {
    pub fn add(&mut self, other: &Usage) {
        self.prompt_tokens = self.prompt_tokens.saturating_add(other.prompt_tokens);
        self.completion_tokens = self
            .completion_tokens
            .saturating_add(other.completion_tokens);
        self.total_tokens = self.total_tokens.saturating_add(other.total_tokens);
    }
}

/// Incremental pieces of a completion.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    Delta(String),
    /// A reasoning model's thought token (`delta.reasoning_content`) -
    /// streamed separately from the answer so the wait stays visible.
    ReasoningDelta(String),
    ToolCall(ToolCall),
    Usage(Usage),
    Done,
}

#[async_trait]
pub trait AgentProvider: Send + Sync {
    async fn stream_chat(
        &self,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
    ) -> Result<BoxStream, DynError>;

    /// Models the endpoint advertises. Providers that cannot enumerate return an empty list.
    async fn list_models(&self) -> Result<Vec<String>, DynError> {
        Ok(Vec::new())
    }

    /// Non-streaming convenience wrapper: drains [`AgentProvider::stream_chat`].
    async fn chat_completion(
        &self,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
    ) -> Result<(String, Vec<ToolCall>, Usage), DynError> {
        use futures_util::StreamExt;
        let mut stream = self.stream_chat(messages, tools).await?;
        let mut content = String::new();
        let mut calls = Vec::new();
        let mut usage = Usage::default();
        while let Some(event) = stream.next().await {
            match event? {
                StreamEvent::Delta(d) => content.push_str(&d),
                // Reasoning thoughts are ephemeral display data; the
                // aggregated completion only carries the final answer.
                StreamEvent::ReasoningDelta(_) => {}
                StreamEvent::ToolCall(c) => calls.push(c),
                StreamEvent::Usage(u) => usage.add(&u),
                StreamEvent::Done => break,
            }
        }
        Ok((content, calls, usage))
    }
}
