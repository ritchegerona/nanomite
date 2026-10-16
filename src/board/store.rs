// SQLite-backed store for the multi-agent message board.
//
// Follows the conventions of `memory::store`: a synchronous connection behind a
// `Mutex` (single-process TUI, one writer), errors propagated with `?` into a
// small error enum, and `bundled` rusqlite so no system libsqlite3 is needed.
// The UI is expected to drive it from `spawn_blocking`, same as memory.

use rusqlite::{params, Connection, OptionalExtension, Row};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

/// Persona marker used for the round-0 message that states the topic.
///
/// Convention: every thread starts with exactly one round-0 message whose
/// persona is `"topic"`; its body is the thread's topic line.
pub const TOPIC_PERSONA: &str = "topic";

/// Absolute ceiling on the page size of [`BoardStore::list_threads`].
pub const HARD_THREAD_LIMIT: usize = 1_000;

/// Every persisted error class for board operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoardError {
    /// Input failed validation (empty thread id, empty persona, ...).
    Invalid(String),
    /// Underlying database failure.
    Database(String),
    /// Filesystem failure (creating the database directory, ...).
    Io(String),
}

impl std::fmt::Display for BoardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BoardError::Invalid(msg) => write!(f, "invalid board input: {msg}"),
            BoardError::Database(msg) => write!(f, "board database error: {msg}"),
            BoardError::Io(msg) => write!(f, "board io error: {msg}"),
        }
    }
}

impl std::error::Error for BoardError {}

impl From<rusqlite::Error> for BoardError {
    fn from(err: rusqlite::Error) -> Self {
        BoardError::Database(err.to_string())
    }
}

/// One message on the board. `id` is `None` until the message has been
/// appended; the store fills it from the row id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardMessage {
    pub id: Option<i64>,
    pub thread_id: String,
    pub persona: String,
    pub round: u32,
    pub body: String,
    pub created_at: String,
}

/// A coarse per-thread summary for listing board threads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadSummary {
    pub thread_id: String,
    /// Body of the round-0 `topic` message; empty when a thread lacks one.
    pub topic: String,
    /// Earliest `created_at` in the thread (RFC 3339 style strings sort well).
    pub started_at: String,
    pub message_count: i64,
}

/// A message board store bound to one database file.
///
/// Sync + `Mutex` rather than a pool: this is a single-process TUI with one
/// writer, mirroring [`crate::memory`]'s store.
pub struct BoardStore {
    conn: Mutex<Connection>,
    path: Option<PathBuf>,
}

impl BoardStore {
    /// Opens (creating if needed) the database at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, BoardError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| BoardError::Io(format!("create {}: {e}", parent.display())))?;
            }
        }
        let conn = Connection::open(&path)
            .map_err(|e| BoardError::Database(format!("open {}: {e}", path.display())))?;
        let store = Self {
            conn: Mutex::new(conn),
            path: Some(path),
        };
        store.init_schema()?;
        Ok(store)
    }

    /// Private, non-persistent database. Used by tests and throwaway flows.
    pub fn open_in_memory() -> Result<Self, BoardError> {
        let conn = Connection::open_in_memory()
            .map_err(|e| BoardError::Database(format!("open in-memory: {e}")))?;
        let store = Self {
            conn: Mutex::new(conn),
            path: None,
        };
        store.init_schema()?;
        Ok(store)
    }

    /// Locks the connection. A poisoned mutex is recovered rather than
    /// propagated: one panicking caller must not wedge the store for the rest
    /// of the session.
    fn lock(&self) -> MutexGuard<'_, Connection> {
        match self.conn.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// Database path, if this store is file-backed.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn init_schema(&self) -> Result<(), BoardError> {
        let conn = self.lock();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS board_messages (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                thread_id  TEXT    NOT NULL,
                persona    TEXT    NOT NULL,
                round      INTEGER NOT NULL,
                body       TEXT    NOT NULL,
                created_at TEXT    NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_board_messages_thread
                ON board_messages(thread_id);",
        )?;
        Ok(())
    }

    /// Persists a message and returns its new row id.
    pub fn append(&self, message: &BoardMessage) -> Result<i64, BoardError> {
        if message.thread_id.is_empty() {
            return Err(BoardError::Invalid("thread_id must not be empty".into()));
        }
        if message.persona.is_empty() {
            return Err(BoardError::Invalid("persona must not be empty".into()));
        }
        if message.created_at.is_empty() {
            return Err(BoardError::Invalid("created_at must not be empty".into()));
        }
        let conn = self.lock();
        conn.execute(
            "INSERT INTO board_messages (thread_id, persona, round, body, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                message.thread_id,
                message.persona,
                i64::from(message.round),
                message.body,
                message.created_at
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// All messages of one thread, oldest first (insertion order).
    pub fn thread_messages(&self, thread_id: &str) -> Result<Vec<BoardMessage>, BoardError> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, thread_id, persona, round, body, created_at
             FROM board_messages WHERE thread_id = ?1 ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![thread_id], map_message)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(BoardError::from)
    }

    /// Thread id of the most recently appended message, if the board is not empty.
    pub fn latest_thread_id(&self) -> Result<Option<String>, BoardError> {
        let conn = self.lock();
        let mut stmt =
            conn.prepare("SELECT thread_id FROM board_messages ORDER BY id DESC LIMIT 1")?;
        let id = stmt
            .query_row([], |row| row.get::<_, String>(0))
            .optional()?;
        Ok(id)
    }

    /// Most recent threads, newest first. The topic is the thread's round-0
    /// message with persona [`TOPIC_PERSONA`] (empty when missing).
    pub fn list_threads(&self, limit: usize) -> Result<Vec<ThreadSummary>, BoardError> {
        let limit = limit.min(HARD_THREAD_LIMIT) as i64;
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT b.thread_id,
                    COALESCE(MAX(CASE WHEN b.round = 0 AND b.persona = ?1
                                      THEN b.body END), '') AS topic,
                    MIN(b.created_at) AS started_at,
                    COUNT(*) AS message_count
             FROM board_messages b
             GROUP BY b.thread_id
             ORDER BY MIN(b.id) DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![TOPIC_PERSONA, limit], |row| {
            Ok(ThreadSummary {
                thread_id: row.get(0)?,
                topic: row.get(1)?,
                started_at: row.get(2)?,
                message_count: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(BoardError::from)
    }
}

fn map_message(row: &Row<'_>) -> rusqlite::Result<BoardMessage> {
    let round: i64 = row.get(3)?;
    Ok(BoardMessage {
        id: Some(row.get(0)?),
        thread_id: row.get(1)?,
        persona: row.get(2)?,
        round: u32::try_from(round).unwrap_or(0),
        body: row.get(4)?,
        created_at: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(thread: &str, persona: &str, round: u32, body: &str, at: &str) -> BoardMessage {
        BoardMessage {
            id: None,
            thread_id: thread.into(),
            persona: persona.into(),
            round,
            body: body.into(),
            created_at: at.into(),
        }
    }

    #[test]
    fn messages_round_trip_in_insertion_order() {
        let store = BoardStore::open_in_memory().unwrap();
        let id0 = store
            .append(&msg(
                "t1",
                TOPIC_PERSONA,
                0,
                "Should we split the crate?",
                "2026-01-01T00:00:00Z",
            ))
            .unwrap();
        let id1 = store
            .append(&msg(
                "t1",
                "Architect",
                1,
                "Split by concern.",
                "2026-01-01T00:00:01Z",
            ))
            .unwrap();
        // Same created_at as the previous message: ordering must still hold.
        let id2 = store
            .append(&msg(
                "t1",
                "Critic",
                1,
                "The split cost is underestimated.",
                "2026-01-01T00:00:01Z",
            ))
            .unwrap();
        assert!(id0 < id1 && id1 < id2);

        let loaded = store.thread_messages("t1").unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(
            loaded[0],
            msg(
                "t1",
                TOPIC_PERSONA,
                0,
                "Should we split the crate?",
                "2026-01-01T00:00:00Z"
            )
            .with_id(id0)
        );
        assert_eq!(loaded[1].persona, "Architect");
        assert_eq!(loaded[1].round, 1);
        assert_eq!(loaded[2].body, "The split cost is underestimated.");
        assert_eq!(loaded[2].id, Some(id2));
    }

    #[test]
    fn list_threads_summarizes_by_topic_message() {
        let store = BoardStore::open_in_memory().unwrap();
        store
            .append(&msg(
                "t1",
                TOPIC_PERSONA,
                0,
                "First topic",
                "2026-01-01T00:00:00Z",
            ))
            .unwrap();
        store
            .append(&msg("t1", "Architect", 1, "One", "2026-01-01T00:00:01Z"))
            .unwrap();
        store
            .append(&msg("t1", "Critic", 1, "Two", "2026-01-01T00:00:02Z"))
            .unwrap();
        store
            .append(&msg(
                "t2",
                TOPIC_PERSONA,
                0,
                "Second topic",
                "2026-01-02T00:00:00Z",
            ))
            .unwrap();

        assert_eq!(store.latest_thread_id().unwrap().as_deref(), Some("t2"));

        let threads = store.list_threads(10).unwrap();
        assert_eq!(threads.len(), 2);
        // Newest thread first.
        assert_eq!(threads[0].thread_id, "t2");
        assert_eq!(threads[0].topic, "Second topic");
        assert_eq!(threads[0].message_count, 1);
        assert_eq!(threads[1].thread_id, "t1");
        assert_eq!(threads[1].topic, "First topic");
        assert_eq!(threads[1].message_count, 3);
        assert_eq!(threads[1].started_at, "2026-01-01T00:00:00Z");
    }

    #[test]
    fn validation_and_empty_results_are_explicit() {
        let store = BoardStore::open_in_memory().unwrap();
        assert!(matches!(
            store.append(&msg("", "Architect", 0, "x", "2026-01-01T00:00:00Z")),
            Err(BoardError::Invalid(_))
        ));
        assert!(matches!(
            store.append(&msg("t1", "", 0, "x", "2026-01-01T00:00:00Z")),
            Err(BoardError::Invalid(_))
        ));
        assert!(store.thread_messages("missing").unwrap().is_empty());
        assert_eq!(store.latest_thread_id().unwrap(), None);
        assert!(store.list_threads(0).unwrap().is_empty());
    }

    impl BoardMessage {
        fn with_id(mut self, id: i64) -> Self {
            self.id = Some(id);
            self
        }
    }
}
