//! Integration tests for public APIs.
//!
//! These tests exercise the public APIs: ToolSet, session persistence,
//! memory engine, and config loading.

use nanomite::app::{Entry, EntryRole};
use nanomite::config::Config;
use nanomite::memory::model::{
    MemoryCategory, MemorySource, NewMemory, SearchQuery, Sensitivity, SourceKind,
};
use nanomite::memory::policy::MemoryPolicy;
use nanomite::memory::MemoryEngine;
use nanomite::session::{self, StoredSession};
use nanomite::tools::ToolSet;
use tempfile::TempDir;

#[tokio::test]
async fn tool_set_file_operations() {
    let tmp = TempDir::new().unwrap();
    let tools = ToolSet::new(tmp.path().to_path_buf(), 4 * 1024 * 1024);

    // Write file
    let result = tools
        .files
        .write_file(nanomite::tools::file_tools::WriteFileArgs {
            path: "test.txt".into(),
            content: "hello world".into(),
            append: false,
        })
        .unwrap();
    assert!(result.bytes_written > 0);

    // Read file
    let result = tools
        .files
        .read_file(nanomite::tools::file_tools::ReadFileArgs {
            path: "test.txt".into(),
            offset: None,
            limit: None,
        })
        .unwrap();
    assert_eq!(result.content, "hello world");

    // List files
    let result = tools
        .files
        .list_files(nanomite::tools::file_tools::ListFilesArgs {
            path: Some(".".into()),
        })
        .unwrap();
    assert!(result.files.iter().any(|e| e == "test.txt"));

    // Search
    let result = tools
        .files
        .search(nanomite::tools::file_tools::SearchArgs {
            query: "world".into(),
            path: None,
        })
        .unwrap();
    assert!(!result.hits.is_empty());

    // Edit file
    let result = tools
        .files
        .edit_file(nanomite::tools::file_tools::EditFileArgs {
            path: "test.txt".into(),
            old_string: "world".into(),
            new_string: "rust".into(),
            replace_all: false,
        })
        .unwrap();
    assert_eq!(result.replacements_made, 1);

    let result = tools
        .files
        .read_file(nanomite::tools::file_tools::ReadFileArgs {
            path: "test.txt".into(),
            offset: None,
            limit: None,
        })
        .unwrap();
    assert_eq!(result.content, "hello rust");
}

#[tokio::test]
async fn tool_set_exec_operations() {
    let tmp = TempDir::new().unwrap();
    let tools = ToolSet::new(tmp.path().to_path_buf(), 4 * 1024 * 1024);

    // Run a simple command
    let result = tools
        .exec
        .run_command(nanomite::tools::exec_tools::RunCommandArgs {
            command: "echo hello".into(),
            cwd: None,
            timeout_ms: Some(5000),
            sudo_password: None,
        })
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("hello"));
}

#[tokio::test]
async fn session_persist_and_restore() {
    let tmp = TempDir::new().unwrap();

    // Create a session using the capture function
    let entries = vec![
        Entry::new(EntryRole::User, "Hello"),
        Entry::new(EntryRole::Assistant, "Hi there!"),
    ];

    let session = StoredSession::capture(
        &entries,
        "test-provider",
        "test-model",
        tmp.path(),
        std::time::SystemTime::now(),
    );

    // Save session
    let path = session::save(&session).unwrap();
    assert!(path.exists());

    // List sessions
    let sessions = session::list();
    assert!(!sessions.is_empty());

    // Load session
    let loaded = session::load(&path).unwrap();
    assert_eq!(loaded.entries.len(), 2);
    assert_eq!(loaded.entries[0].text, "Hello");
    assert_eq!(loaded.entries[1].text, "Hi there!");
}

#[tokio::test]
async fn memory_engine_in_memory() {
    let mem = MemoryEngine::open_in_memory(MemoryPolicy::default()).expect("memory engine");

    // Save a memory using NewMemory
    let new_mem = NewMemory {
        category: MemoryCategory::Episodic,
        content: "Test memory content".into(),
        summary: None,
        tags: vec!["tag1".into()],
        source: MemorySource::new(SourceKind::UserInstruction),
        project: Some("test-project".into()),
        confidence: None,
        importance: None,
        expires_at: None,
        sensitivity: None,
        mirror_markdown: true,
    };

    let save_outcome = mem.save(&new_mem).expect("save memory");
    let id = save_outcome.record.id;

    // Search for it
    let query = SearchQuery {
        text: Some("Test memory".into()),
        categories: vec![],
        tags: vec![],
        project: Some("test-project".into()),
        project_unscoped: false,
        min_importance: None,
        max_sensitivity: Sensitivity::Confidential,
        include_expired: false,
        order: nanomite::memory::model::SearchOrder::Relevance,
        limit: Some(10),
        offset: 0,
    };

    let (results, _markdown) = mem.search(&query, false).expect("search memory");
    assert!(!results.items.is_empty());
    assert_eq!(results.items[0].record.id, id);

    // Get by ID
    let record = mem.get(&id).expect("get memory");
    assert_eq!(record.content, "Test memory content");
}

#[tokio::test]
async fn config_load() {
    let cfg = Config::load();
    // Config should load without panicking
    assert!(!cfg.workspace.as_os_str().is_empty());
}

#[tokio::test]
async fn config_from_env() {
    // Config::from_env() uses current directory as workspace by default
    // We can't easily change current dir in a test, so just verify it loads
    let cfg = Config::from_env();
    // Should load without error and have a workspace
    assert!(!cfg.workspace.as_os_str().is_empty());
}

#[tokio::test]
async fn document_extraction_text() {
    let tmp = TempDir::new().unwrap();
    let tools = ToolSet::new(tmp.path().to_path_buf(), 4 * 1024 * 1024);

    // Create a text file
    std::fs::write(tmp.path().join("doc.txt"), "Hello, world!\nThis is a test.").unwrap();

    // Extract text
    let result = tools
        .documents
        .read_document(nanomite::tools::document_tools::ReadDocumentArgs {
            path: "doc.txt".into(),
        })
        .unwrap();

    assert!(result.text.contains("Hello, world!"));
    assert!(result.text.contains("This is a test."));
}

#[test]
fn session_v2_roundtrip_carries_settings_and_plans() {
    use nanomite::session::{self, StoredPlan, StoredPlanTask, StoredSession, StoredSettings};

    let dir = std::env::temp_dir().join(format!("nanomite-sess-v2-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let mut session = StoredSession::capture(
        &[],
        "openai",
        "test-model",
        &dir,
        std::time::SystemTime::now(),
    );
    session.settings = Some(StoredSettings {
        tool_policy: Some("plan".into()),
        disabled_tools: Some(vec!["web_search".into()]),
        reasoning: Some("high".into()),
        theme: Some("midnight".into()),
    });
    session.plans = vec![StoredPlan {
        name: "feature-x".into(),
        description: "do x".into(),
        updated_at: "2026-10-27T00:00:00Z".into(),
        tasks: vec![StoredPlanTask {
            id: 1,
            description: "step one".into(),
            status: "done".into(),
            depends_on: vec![],
            tags: vec!["api".into()],
        }],
    }];

    let path = session::save(&session).unwrap();
    let loaded = session::load(&path).unwrap();
    assert_eq!(loaded.version, 2);
    let settings = loaded.settings.expect("settings stored");
    assert_eq!(settings.tool_policy.as_deref(), Some("plan"));
    assert_eq!(
        settings.disabled_tools.as_deref(),
        Some(&["web_search".to_string()][..])
    );
    assert_eq!(loaded.plans.len(), 1);
    assert_eq!(loaded.plans[0].tasks[0].status, "done");
    assert!(loaded.workspace.is_some());

    // The listing carries the workspace slug for per-workspace grouping.
    let metas = session::list();
    assert!(metas.iter().any(|m| m.workspace.is_some()));

    let _ = std::fs::remove_dir_all(&dir);
}
