// The convene engine: runs a multi-persona discussion around a topic and
// persists every message.
//
// `convene` is deliberately synchronous: the UI will call it from
// `spawn_blocking`, so the engine bridges to the async providers with
// `block_on` instead of being async itself. Never call `convene` from an async
// worker thread (blocking there panics); from a blocking thread or a plain
// thread it reuses the ambient runtime when one is visible and otherwise builds
// a private current-thread runtime.

use crate::agent::provider::ChatMessage;
use crate::agent::swarm::Swarm;
use crate::board::personas::{CHAIR, PERSONAS};
use crate::board::store::{BoardError, BoardMessage, BoardStore, TOPIC_PERSONA};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Hard cap on discussion rounds per convene.
pub const MAX_ROUNDS: usize = 4;

/// Hard cap on a stored message body, in characters.
const MAX_BODY_CHARS: usize = 1200;

/// Cap on the transcript handed to each persona, in characters (tail window).
const TRANSCRIPT_MAX_CHARS: usize = 8000;

/// Cap on the error text quoted inside a "(provider error: ...)" note.
const ERROR_NOTE_MAX_CHARS: usize = 200;

/// Persona used for system marker messages (cancellation, ...).
const SYSTEM_PERSONA: &str = "system";

/// Body of the marker message posted when a convene is cancelled mid-flight.
const CANCEL_MARKER_BODY: &str = "cancelled by user";

/// Cooperative cancellation flag, shared between the UI and the engine.
#[derive(Debug, Default)]
pub struct CancelFlag(AtomicBool);

impl CancelFlag {
    pub fn new() -> Self {
        Self(AtomicBool::new(false))
    }

    /// Ask the engine to stop at the next checkpoint (between rounds and after
    /// each posted message).
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// A board engine: a store plus the provider pool that plays the personas.
pub struct BoardEngine {
    store: BoardStore,
    providers: Swarm,
    thread_counter: AtomicU64,
}

impl BoardEngine {
    pub fn new(store: BoardStore, providers: Swarm) -> Self {
        Self {
            store,
            providers,
            thread_counter: AtomicU64::new(0),
        }
    }

    /// The backing store, for tests and for reading threads back.
    pub fn store(&self) -> &BoardStore {
        &self.store
    }

    /// Convenes the board on `topic`.
    ///
    /// Flow: a round-0 topic message, then `rounds` (capped at
    /// [`MAX_ROUNDS`]) discussion rounds in which all four discussant personas
    /// answer in parallel via the [`Swarm`], then one chair synthesis message.
    /// Every posted message is appended to the store and passed to
    /// `on_message` in order.
    ///
    /// Cancellation is cooperative and checked between rounds and after each
    /// message. Choice: on cancellation the engine returns `Ok(thread_id)`
    /// after posting a marker message (persona "system", body
    /// "cancelled by user") rather than a dedicated error enum — the caller
    /// already holds the `CancelFlag` it set, so a distinct return type would
    /// add nothing, and the thread itself records why it stopped.
    ///
    /// Per-provider failures do not abort the convene: they are stored as a
    /// "(provider error: ...)" note attributed to that persona so the
    /// transcript stays complete.
    pub fn convene(
        &self,
        topic: &str,
        rounds: usize,
        cancel: &CancelFlag,
        mut on_message: impl FnMut(&BoardMessage),
    ) -> Result<String, BoardError> {
        if self.providers.is_empty() {
            return Err(BoardError::Invalid(
                "board engine needs at least one provider in its swarm".into(),
            ));
        }
        let topic = topic.trim();
        if topic.is_empty() {
            return Err(BoardError::Invalid("topic must not be empty".into()));
        }
        let rounds = rounds.min(MAX_ROUNDS);
        let blocker = Blocker::acquire()?;
        let thread_id = self.new_thread_id();

        // Round 0: the topic itself.
        self.post(&thread_id, TOPIC_PERSONA, 0, topic, &mut on_message)?;

        // Transcript lines ("Architect: ..."), discussion messages only; the
        // topic is passed separately below.
        let mut transcript: Vec<String> = Vec::new();

        for round in 1..=rounds {
            if cancel.is_cancelled() {
                return self.abort(
                    &thread_id,
                    u32::try_from(round).unwrap_or(MAX_ROUNDS as u32),
                    &mut on_message,
                );
            }
            let prompt = build_prompt(topic, &transcript);
            let conversations = PERSONAS
                .iter()
                .map(|persona| {
                    vec![
                        ChatMessage::system(persona.system_prompt),
                        ChatMessage::user(prompt.clone()),
                    ]
                })
                .collect::<Vec<_>>();
            let results = blocker.block_on(self.providers.run_conversations(conversations, None));
            for (persona, (_, result)) in PERSONAS.iter().zip(results) {
                if cancel.is_cancelled() {
                    return self.abort(
                        &thread_id,
                        u32::try_from(round).unwrap_or(MAX_ROUNDS as u32),
                        &mut on_message,
                    );
                }
                let body = match result {
                    Ok(body) => body,
                    Err(err) => format!(
                        "(provider error: {})",
                        cap_chars(&err.to_string(), ERROR_NOTE_MAX_CHARS)
                    ),
                };
                let message = self.post(
                    &thread_id,
                    persona.name,
                    round as u32,
                    &body,
                    &mut on_message,
                )?;
                transcript.push(format!("{}: {}", persona.name, message.body));
            }
        }

        if cancel.is_cancelled() {
            return self.abort(&thread_id, rounds as u32, &mut on_message);
        }

        // Chair synthesis over the same context. The chair posts at
        // round = rounds + 1 to mark it as synthesis rather than discussion.
        let prompt = build_prompt(topic, &transcript);
        let conversations = vec![vec![
            ChatMessage::system(CHAIR.system_prompt),
            ChatMessage::user(prompt),
        ]];
        let results = blocker.block_on(self.providers.run_conversations(conversations, None));
        let chair_body = match results.into_iter().next() {
            Some((_, Ok(body))) => body,
            Some((_, Err(err))) => format!(
                "(provider error: {})",
                cap_chars(&err.to_string(), ERROR_NOTE_MAX_CHARS)
            ),
            None => "(no reply)".to_string(),
        };
        self.post(
            &thread_id,
            CHAIR.name,
            rounds as u32 + 1,
            &chair_body,
            &mut on_message,
        )?;
        Ok(thread_id)
    }

    /// Posts a message: cap the body, append to the store, notify the caller.
    fn post(
        &self,
        thread_id: &str,
        persona: &str,
        round: u32,
        body: &str,
        on_message: &mut impl FnMut(&BoardMessage),
    ) -> Result<BoardMessage, BoardError> {
        let message = BoardMessage {
            id: None,
            thread_id: thread_id.to_string(),
            persona: persona.to_string(),
            round,
            body: cap_chars(body, MAX_BODY_CHARS),
            created_at: utc_now_string(),
        };
        let id = self.store.append(&message)?;
        let message = BoardMessage {
            id: Some(id),
            ..message
        };
        on_message(&message);
        Ok(message)
    }

    /// Cancelled mid-convene: post the marker and report the partial thread.
    fn abort(
        &self,
        thread_id: &str,
        round: u32,
        on_message: &mut impl FnMut(&BoardMessage),
    ) -> Result<String, BoardError> {
        self.post(
            thread_id,
            SYSTEM_PERSONA,
            round,
            CANCEL_MARKER_BODY,
            on_message,
        )?;
        Ok(thread_id.to_string())
    }

    /// Timestamp + monotonic counter: unique within the process, sortable, and
    /// stable even when two convenes start in the same millisecond.
    fn new_thread_id(&self) -> String {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let counter = self.thread_counter.fetch_add(1, Ordering::Relaxed) + 1;
        format!("board-{millis:013}-{counter:04}")
    }
}

/// Bridges the sync engine onto async providers.
enum Blocker {
    /// A runtime is already visible (e.g. we are on a `spawn_blocking` thread
    /// of the UI's runtime): reuse it.
    Ambient(tokio::runtime::Handle),
    /// No runtime anywhere (plain thread, tests): build a private one.
    Owned(tokio::runtime::Runtime),
}

impl Blocker {
    fn acquire() -> Result<Self, BoardError> {
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => Ok(Blocker::Ambient(handle)),
            Err(_) => tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map(Blocker::Owned)
                .map_err(|e| BoardError::Io(format!("build tokio runtime: {e}"))),
        }
    }

    fn block_on<F: std::future::Future>(&self, fut: F) -> F::Output {
        match self {
            Blocker::Ambient(handle) => handle.block_on(fut),
            Blocker::Owned(rt) => rt.block_on(fut),
        }
    }
}

/// The user message every persona sees: topic plus the capped transcript.
fn build_prompt(topic: &str, transcript: &[String]) -> String {
    let mut prompt = format!("Topic under discussion:\n{topic}\n\n");
    if transcript.is_empty() {
        prompt.push_str("No messages yet. Open the discussion with your position.");
    } else {
        prompt.push_str("Transcript so far, oldest first:\n");
        prompt.push_str(&cap_tail(&transcript.join("\n"), TRANSCRIPT_MAX_CHARS));
    }
    prompt
}

/// Hard-caps `text` to its first `max_chars` characters (char-boundary safe).
fn cap_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect()
}

/// Keeps the last `max_chars` characters of `text` (char-boundary safe).
fn cap_tail(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    text.chars().skip(count - max_chars).collect()
}

/// Current UTC time as an RFC 3339 style string, without a time crate.
pub fn utc_now_string() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_utc(secs)
}

/// RFC 3339 style `YYYY-MM-DDTHH:MM:SSZ` for an epoch-seconds value.
fn format_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to (y, m, d).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::provider::{AgentProvider, BoxStream, DynError, StreamEvent, Tool};
    use crate::agent::testdouble::ScriptedProvider;
    use async_trait::async_trait;

    /// Replies with 2000 identical characters: exercises the body cap.
    struct LongProvider;

    #[async_trait]
    impl AgentProvider for LongProvider {
        async fn stream_chat(
            &self,
            _messages: Vec<ChatMessage>,
            _tools: Option<Vec<Tool>>,
        ) -> Result<BoxStream, DynError> {
            Ok(Box::pin(futures_util::stream::once(async {
                Ok::<StreamEvent, DynError>(StreamEvent::Delta("x".repeat(2000)))
            })))
        }
    }

    fn engine_with(providers: &mut Swarm) -> BoardEngine {
        BoardEngine::new(
            BoardStore::open_in_memory().unwrap(),
            std::mem::take(providers),
        )
    }

    #[test]
    fn two_round_convene_produces_topic_replies_and_chair() {
        let mut swarm = Swarm::new();
        swarm.add(ScriptedProvider::default());
        swarm.add(ScriptedProvider::default());
        let engine = engine_with(&mut swarm);

        let cancel = CancelFlag::new();
        let mut seen: Vec<(String, u32)> = Vec::new();
        let thread_id = engine
            .convene("Should we rewrite the parser?", 2, &cancel, |message| {
                seen.push((message.persona.clone(), message.round));
            })
            .unwrap();

        assert!(thread_id.starts_with("board-"));
        // 1 topic + 4 personas x 2 rounds + 1 chair.
        assert_eq!(seen.len(), 1 + PERSONAS.len() * 2 + 1);
        assert_eq!(seen[0], (TOPIC_PERSONA.to_string(), 0));
        for (index, (persona, round)) in seen.iter().enumerate().skip(1) {
            if index < seen.len() - 1 {
                assert_eq!(persona, PERSONAS[(index - 1) % PERSONAS.len()].name);
                assert_eq!(*round, ((index - 1) / PERSONAS.len() + 1) as u32);
            }
        }
        assert_eq!(seen.last().unwrap().0, CHAIR.name);

        // Everything persisted, bodies within the cap.
        let stored = engine.store().thread_messages(&thread_id).unwrap();
        assert_eq!(stored.len(), seen.len());
        for message in &stored {
            assert!(
                message.body.chars().count() <= MAX_BODY_CHARS,
                "body must respect the hard cap"
            );
        }
        assert_eq!(stored.last().unwrap().persona, CHAIR.name);
    }

    #[test]
    fn bodies_are_hard_capped_at_1200_chars() {
        let mut swarm = Swarm::new();
        swarm.add(LongProvider);
        let engine = engine_with(&mut swarm);

        let cancel = CancelFlag::new();
        let thread_id = engine.convene("cap check", 1, &cancel, |_| {}).unwrap();
        let stored = engine.store().thread_messages(&thread_id).unwrap();
        // 1 topic + 4 personas + 1 chair.
        assert_eq!(stored.len(), 1 + PERSONAS.len() + 1);
        for message in &stored {
            assert!(message.body.chars().count() <= MAX_BODY_CHARS);
        }
        assert!(stored
            .iter()
            .any(|m| m.body.chars().count() == MAX_BODY_CHARS));
    }

    #[test]
    fn cancel_between_rounds_stops_with_a_marker_message() {
        let mut swarm = Swarm::new();
        swarm.add(ScriptedProvider::default());
        let engine = engine_with(&mut swarm);

        let cancel = CancelFlag::new();
        let mut count = 0usize;
        let thread_id = engine
            .convene("long debate", 3, &cancel, |_| {
                count += 1;
                // topic + 4 round-1 replies are out; stop before round 2.
                if count == 1 + PERSONAS.len() {
                    cancel.cancel();
                }
            })
            .unwrap();

        let stored = engine.store().thread_messages(&thread_id).unwrap();
        assert_eq!(stored.len(), 1 + PERSONAS.len() + 1);
        let last = stored.last().unwrap();
        assert_eq!(last.persona, SYSTEM_PERSONA);
        assert_eq!(last.body, CANCEL_MARKER_BODY);
    }

    #[test]
    fn invalid_inputs_are_rejected_before_any_post() {
        let mut swarm = Swarm::new();
        swarm.add(ScriptedProvider::default());
        let engine = engine_with(&mut swarm);

        let cancel = CancelFlag::new();
        assert!(matches!(
            engine.convene("   ", 2, &cancel, |_| {}),
            Err(BoardError::Invalid(_))
        ));
        let empty = BoardEngine::new(BoardStore::open_in_memory().unwrap(), Swarm::new());
        assert!(matches!(
            empty.convene("topic", 2, &cancel, |_| {}),
            Err(BoardError::Invalid(_))
        ));
    }

    #[test]
    fn utc_format_matches_known_epochs() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc(1_000_000_000), "2001-09-09T01:46:40Z");
        assert_eq!(format_utc(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(format_utc(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn caps_and_tail_window_are_char_boundary_safe() {
        assert_eq!(cap_chars("hello", 10), "hello");
        assert_eq!(cap_chars("hello", 3), "hel");
        // 2-byte characters: never split one in half.
        let hiragana = "あああああ";
        assert_eq!(cap_chars(hiragana, 3).chars().count(), 3);
        assert_eq!(cap_tail(hiragana, 2).chars().count(), 2);
        assert_eq!(cap_tail("abcdef", 4), "cdef");
        assert_eq!(cap_tail("abc", 10), "abc");
    }
}
