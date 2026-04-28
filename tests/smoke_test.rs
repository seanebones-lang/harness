//! Smoke tests for the harness crates.
//! These run without a live API key — they test local logic only.

use harness_memory::{MemoryStore, Session, SessionStore};
use harness_provider_core::{
    ChatRequest, Message, Role, ToolCall, ToolCallFunction, ToolDefinition,
};
use harness_tools::{ToolExecutor, ToolRegistry};
use harness_tools::tools::{PatchFileTool, ReadFileTool, SearchCodeTool, ShellTool, WriteFileTool};
use tempfile::tempdir;

// ── Session / store ───────────────────────────────────────────────────────────

#[tokio::test]
async fn session_round_trip() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("sessions.db");
    let store = SessionStore::open(&db).unwrap();

    let mut session = Session::new("grok-3-fast");
    session.push(Message::user("hello"));
    session.push(Message::assistant("hi there"));

    store.save(&session).unwrap();

    let loaded = store.load(&session.id).unwrap().expect("session should exist");
    assert_eq!(loaded.id, session.id);
    assert_eq!(loaded.messages.len(), 2);
    assert_eq!(loaded.messages[0].content.as_str(), "hello");
}

#[tokio::test]
async fn session_find_by_prefix() {
    let dir = tempdir().unwrap();
    let store = SessionStore::open(dir.path().join("s.db")).unwrap();

    let mut s = Session::new("grok-3-fast").with_name("my-coding-session");
    s.push(Message::user("test"));
    store.save(&s).unwrap();

    // Find by id prefix
    let found = store.find(&s.id[..6]).unwrap();
    assert!(found.is_some());

    // Find by name
    let found2 = store.find("my-coding-session").unwrap();
    assert!(found2.is_some());
    assert_eq!(found2.unwrap().id, s.id);

    // Not found
    let missing = store.find("no-such-session").unwrap();
    assert!(missing.is_none());
}

#[tokio::test]
async fn session_list() {
    let dir = tempdir().unwrap();
    let store = SessionStore::open(dir.path().join("s.db")).unwrap();

    for i in 0..5 {
        let mut s = Session::new("grok-3-fast");
        s.push(Message::user(format!("message {i}")));
        store.save(&s).unwrap();
    }

    let list = store.list(10).unwrap();
    assert_eq!(list.len(), 5);
}

// ── Memory (vector) store ─────────────────────────────────────────────────────

#[tokio::test]
async fn memory_store_insert_and_search() {
    let dir = tempdir().unwrap();
    let mem = MemoryStore::open(dir.path().join("mem.db")).unwrap();

    // Insert a memory with a known embedding direction
    let session_a = "session-a";
    let session_b = "session-b";
    let emb_a: Vec<f32> = vec![1.0, 0.0, 0.0];
    let emb_query: Vec<f32> = vec![0.9, 0.1, 0.0]; // close to emb_a
    let emb_far: Vec<f32> = vec![0.0, 1.0, 0.0];   // orthogonal

    mem.insert(session_a, "text close to query", &emb_a).unwrap();
    mem.insert(session_b, "orthogonal text", &emb_far).unwrap();

    // Search excluding session_a itself
    let results = mem.search(&emb_query, "current-session", 5).unwrap();
    assert_eq!(results.len(), 2);
    // Most similar should be emb_a
    assert_eq!(results[0].0.text, "text close to query");
    assert!(results[0].1 > results[1].1);
}

// ── Provider core types ───────────────────────────────────────────────────────

#[test]
fn message_constructors() {
    let sys = Message::system("you are a helper");
    assert!(matches!(sys.role, Role::System));

    let user = Message::user("hello");
    assert!(matches!(user.role, Role::User));
    assert_eq!(user.content.as_str(), "hello");

    let asst = Message::assistant("hi");
    assert!(matches!(asst.role, Role::Assistant));

    let tool = Message::tool_result("call-1", "output");
    assert!(matches!(tool.role, Role::Tool));
    assert_eq!(tool.tool_call_id.as_deref(), Some("call-1"));
}

#[test]
fn tool_call_arg_parsing() {
    let call = ToolCall {
        id: "c1".into(),
        kind: "function".into(),
        function: ToolCallFunction {
            name: "read_file".into(),
            arguments: r#"{"path": "/tmp/foo.txt"}"#.into(),
        },
    };
    let args = call.args().unwrap();
    assert_eq!(args["path"], "/tmp/foo.txt");
}

#[test]
fn chat_request_builder() {
    let req = ChatRequest::new("grok-3-fast")
        .with_system("be helpful")
        .with_messages(vec![Message::user("hi")])
        .with_tools(vec![
            ToolDefinition::new("my_tool", "does stuff", serde_json::json!({"type":"object"}))
        ]);

    assert_eq!(req.model, "grok-3-fast");
    assert_eq!(req.system.as_deref(), Some("be helpful"));
    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.tools.len(), 1);
}

// ── Tool execution ────────────────────────────────────────────────────────────

#[tokio::test]
async fn read_write_file_tools() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("test.txt");

    let mut registry = ToolRegistry::new();
    registry.register(WriteFileTool);
    registry.register(ReadFileTool);
    let executor = ToolExecutor::new(registry);

    // Write
    let write_call = ToolCall {
        id: "w1".into(),
        kind: "function".into(),
        function: ToolCallFunction {
            name: "write_file".into(),
            arguments: serde_json::json!({
                "path": file_path.to_str().unwrap(),
                "content": "hello world\nline 2\n"
            }).to_string(),
        },
    };
    let result = executor.execute(&write_call).await;
    assert!(result.contains("bytes"), "write should report bytes: {result}");

    // Read back
    let read_call = ToolCall {
        id: "r1".into(),
        kind: "function".into(),
        function: ToolCallFunction {
            name: "read_file".into(),
            arguments: serde_json::json!({
                "path": file_path.to_str().unwrap()
            }).to_string(),
        },
    };
    let result = executor.execute(&read_call).await;
    assert!(result.contains("hello world"), "read should contain content: {result}");
}

#[tokio::test]
async fn shell_tool_basic() {
    let mut registry = ToolRegistry::new();
    registry.register(ShellTool);
    let executor = ToolExecutor::new(registry);

    let call = ToolCall {
        id: "s1".into(),
        kind: "function".into(),
        function: ToolCallFunction {
            name: "shell".into(),
            arguments: r#"{"command": "echo 'harness ok'"}"#.into(),
        },
    };
    let result = executor.execute(&call).await;
    assert!(result.contains("harness ok"), "unexpected output: {result}");
}

#[tokio::test]
async fn shell_tool_timeout() {
    let mut registry = ToolRegistry::new();
    registry.register(ShellTool);
    let executor = ToolExecutor::new(registry);

    let call = ToolCall {
        id: "s2".into(),
        kind: "function".into(),
        function: ToolCallFunction {
            name: "shell".into(),
            arguments: r#"{"command": "sleep 10", "timeout_secs": 1}"#.into(),
        },
    };
    let result = executor.execute(&call).await;
    assert!(result.contains("timed out"), "expected timeout: {result}");
}

#[tokio::test]
async fn search_code_tool() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("main.rs"), "fn main() { println!(\"hello\"); }").unwrap();

    let mut registry = ToolRegistry::new();
    registry.register(SearchCodeTool);
    let executor = ToolExecutor::new(registry);

    let call = ToolCall {
        id: "sc1".into(),
        kind: "function".into(),
        function: ToolCallFunction {
            name: "search_code".into(),
            arguments: serde_json::json!({
                "pattern": "println",
                "path": dir.path().to_str().unwrap()
            }).to_string(),
        },
    };
    let result = executor.execute(&call).await;
    assert!(result.contains("println"), "expected match: {result}");
}

#[tokio::test]
async fn patch_file_tool() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("code.rs");
    std::fs::write(&path, "fn foo() {\n    let x = 1;\n    x\n}\n").unwrap();

    let mut registry = ToolRegistry::new();
    registry.register(PatchFileTool);
    let executor = ToolExecutor::new(registry);

    // Successful patch
    let call = ToolCall {
        id: "p1".into(),
        kind: "function".into(),
        function: ToolCallFunction {
            name: "patch_file".into(),
            arguments: serde_json::json!({
                "path": path.to_str().unwrap(),
                "old_content": "    let x = 1;",
                "new_content": "    let x = 42;"
            }).to_string(),
        },
    };
    let result = executor.execute(&call).await;
    assert!(result.contains("Patched"), "expected patch success: {result}");
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("42"), "file should be updated: {content}");

    // Not found
    let call2 = ToolCall {
        id: "p2".into(),
        kind: "function".into(),
        function: ToolCallFunction {
            name: "patch_file".into(),
            arguments: serde_json::json!({
                "path": path.to_str().unwrap(),
                "old_content": "this does not exist",
                "new_content": "replacement"
            }).to_string(),
        },
    };
    let result2 = executor.execute(&call2).await;
    assert!(result2.contains("not found"), "expected not-found: {result2}");
}

#[tokio::test]
async fn unknown_tool_returns_error_message() {
    let registry = ToolRegistry::new();
    let executor = ToolExecutor::new(registry);

    let call = ToolCall {
        id: "u1".into(),
        kind: "function".into(),
        function: ToolCallFunction {
            name: "nonexistent_tool".into(),
            arguments: "{}".into(),
        },
    };
    let result = executor.execute(&call).await;
    assert!(result.contains("Unknown tool"), "expected unknown tool message: {result}");
}
