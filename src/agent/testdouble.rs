use super::provider::{
    AgentProvider, BoxStream, ChatMessage, DynError, Role, StreamEvent, Tool, ToolCall, Usage,
};
use async_trait::async_trait;
use futures_util::stream;
use std::time::Duration;

/// Offline provider: deterministic, streaming, and tool-call aware so the TUI can be
/// exercised end to end without credentials.
pub struct ScriptedProvider {
    delay: Duration,
}

impl Default for ScriptedProvider {
    fn default() -> Self {
        Self {
            delay: Duration::from_millis(16),
        }
    }
}

impl ScriptedProvider {
    /// Scripted reply for a prompt: a short preamble, any tool calls, then the answer.
    fn plan(prompt: &str) -> (Vec<String>, Vec<ToolCall>) {
        let lower = prompt.to_ascii_lowercase();
        let wants_files = ["list", "files", "ls", "structure", "tree"]
            .iter()
            .any(|k| lower.contains(k));
        let wants_read = ["read", "cat ", "show me", "contents of", "open "]
            .iter()
            .any(|k| lower.contains(k));

        let mut words: Vec<String> = Vec::new();
        let mut calls = Vec::new();

        if wants_files {
            words.push("Looking at the workspace.".to_string());
            calls.push(ToolCall {
                id: "scripted-call-1".into(),
                name: "list_files".into(),
                arguments: "{\"path\":\"src\"}".into(),
                index: 0,
            });
        }
        if wants_read {
            words.push("Reading that file now.".to_string());
            calls.push(ToolCall {
                id: "scripted-call-2".into(),
                name: "read_file".into(),
                arguments: "{\"path\":\"README.md\"}".into(),
                index: 0,
            });
        }

        words.extend(
            [
                "",
                "## Summary",
                "",
                "Scripted test reply: this is the offline test double, never part of the shipped binary. Point",
                "rendered from **local fixtures**. Point `NANOMITE_API_KEY` + `NANOMITE_MODEL` at an",
                "OpenAI-compatible endpoint to talk to a real model.",
                "",
                "```rust",
                "let runtime = AgentRuntime::new(provider, workspace, 200_000);",
                "let reply = runtime.run(input, cancel, tx).await;",
                "```",
                "",
                "- streaming deltas arrive incrementally",
                "- tool calls render as rows",
                "- `esc` interrupts, `ctrl+t` starts a new session",
            ]
            .iter()
            .map(|s| s.to_string()),
        );

        (words, calls)
    }
}

#[async_trait]
impl AgentProvider for ScriptedProvider {
    async fn list_models(&self) -> Result<Vec<String>, DynError> {
        Ok(vec!["scripted-model".to_string()])
    }

    async fn stream_chat(
        &self,
        messages: Vec<ChatMessage>,
        _tools: Option<Vec<Tool>>,
    ) -> Result<BoxStream, DynError> {
        let prompt = messages
            .iter()
            .rev()
            .find(|m| m.role == Role::User)
            .map(|m| m.content.clone())
            .unwrap_or_default();
        let (words, calls) = Self::plan(&prompt);

        // Script the turn up front, then trickle it out so streaming is observable.
        let mut script: Vec<StreamEvent> = Vec::new();
        if let Some(first) = words.first() {
            if !first.is_empty() {
                script.push(StreamEvent::Delta(first.clone()));
            }
        }
        script.extend(calls.into_iter().map(StreamEvent::ToolCall));
        let body = words.iter().skip(1).cloned().collect::<Vec<_>>().join("\n");
        for chunk in chunk_text(&body, 48) {
            script.push(StreamEvent::Delta(chunk));
        }
        script.push(StreamEvent::Usage(Usage {
            prompt_tokens: 412,
            completion_tokens: 96,
            total_tokens: 508,
        }));
        script.push(StreamEvent::Done);

        let delay = self.delay;
        Ok(Box::pin(stream::unfold(
            script.into_iter(),
            move |mut script| async move {
                match script.next() {
                    Some(event) => {
                        tokio::time::sleep(delay).await;
                        Some((Ok(event), script))
                    }
                    None => None,
                }
            },
        )))
    }
}

/// Splits text into small streaming slices, preferring to break at whitespace.
fn chunk_text(text: &str, approx_chars: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut buf = String::new();
    for word in text.split_inclusive(' ') {
        buf.push_str(word);
        if buf.chars().count() >= approx_chars {
            chunks.push(std::mem::take(&mut buf));
        }
    }
    if !buf.is_empty() {
        chunks.push(buf);
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_tool_calls_from_the_prompt() {
        let (words, calls) = ScriptedProvider::plan("list the files in src");
        assert!(!words.is_empty());
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "list_files");

        let (_, calls) = ScriptedProvider::plan("hello");
        assert!(calls.is_empty(), "plain prompts must not call tools");
    }

    #[test]
    fn chunks_break_on_whitespace() {
        let chunks = chunk_text("alpha beta gamma delta", 12);
        assert!(chunks.len() > 1);
        assert_eq!(chunks.concat(), "alpha beta gamma delta");
    }
}
