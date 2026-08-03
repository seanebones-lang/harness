//! `database` tool — read-focused SQLite queries under the workspace sandbox.
//!
//! Default mode is **read-only**: only `SELECT` / `WITH` / `PRAGMA` / `EXPLAIN` statements
//! are allowed. Non-sqlite schemes return a clear error (sqlite-first; no extra drivers).

use async_trait::async_trait;
use harness_provider_core::ToolDefinition;
use rusqlite::types::ValueRef;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::registry::Tool;
use crate::workspace_root::WorkspaceRoot;

/// Default maximum rows returned by a query.
pub const DEFAULT_MAX_ROWS: usize = 500;

/// Configuration for [`DatabaseTool`].
#[derive(Debug, Clone)]
pub struct DatabaseToolConfig {
    /// When true (default), reject non-readonly SQL.
    pub readonly: bool,
    /// Clamp on result rows (default 500).
    pub max_rows: usize,
}

impl Default for DatabaseToolConfig {
    fn default() -> Self {
        Self {
            readonly: true,
            max_rows: DEFAULT_MAX_ROWS,
        }
    }
}

/// Structured SQLite access within the workspace.
pub struct DatabaseTool {
    /// Workspace root for path resolution.
    pub workspace: Arc<WorkspaceRoot>,
    /// Tool options (readonly, max_rows).
    pub config: DatabaseToolConfig,
}

impl DatabaseTool {
    /// Create a database tool with the given workspace and config.
    pub fn new(workspace: Arc<WorkspaceRoot>, config: DatabaseToolConfig) -> Self {
        Self { workspace, config }
    }
}

#[async_trait]
impl Tool for DatabaseTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "database",
            "Query SQLite databases under the workspace. Default is READ-ONLY \
             (SELECT/WITH/PRAGMA/EXPLAIN only). Actions: query, list_tables, schema. \
             Provide `path` to a .db/.sqlite file (relative to workspace). \
             Other SQL schemes (postgres, mysql, …) are not supported.",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["query", "list_tables", "schema"],
                        "description": "Database operation."
                    },
                    "path": {
                        "type": "string",
                        "description": "Path to SQLite database file under the workspace."
                    },
                    "sql": {
                        "type": "string",
                        "description": "SQL statement (for action=query)."
                    },
                    "table": {
                        "type": "string",
                        "description": "Table name (for action=schema)."
                    },
                    "max_rows": {
                        "type": "integer",
                        "description": "Max rows to return (clamped; default from config, typically 500)."
                    }
                },
                "required": ["action", "path"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<String> {
        let action = args["action"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing action"))?;
        let path_raw = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing path"))?;

        // Reject non-file / remote schemes early.
        if let Some(scheme) = connection_scheme(path_raw) {
            if scheme != "sqlite" && scheme != "file" {
                anyhow::bail!(
                    "database tool is sqlite-only; scheme '{scheme}' is not supported. \
                     Use a workspace-relative path to a .db/.sqlite file."
                );
            }
        }

        let db_path = resolve_sqlite_path(&self.workspace, path_raw)?;
        let max_rows = args["max_rows"]
            .as_u64()
            .map(|n| n as usize)
            .unwrap_or(self.config.max_rows)
            .clamp(1, self.config.max_rows.max(1));
        let readonly = self.config.readonly;
        let sql = args["sql"].as_str().map(|s| s.to_string());
        let table = args["table"].as_str().map(|s| s.to_string());
        let action = action.to_string();

        tokio::task::spawn_blocking(move || {
            run_database_action(&action, &db_path, sql.as_deref(), table.as_deref(), readonly, max_rows)
        })
        .await
        .map_err(|e| anyhow::anyhow!("database task join error: {e}"))?
    }
}

fn connection_scheme(s: &str) -> Option<&str> {
    let s = s.trim();
    let idx = s.find("://")?;
    let scheme = &s[..idx];
    if scheme.chars().all(|c| c.is_ascii_alphabetic()) {
        Some(scheme)
    } else {
        None
    }
}

fn resolve_sqlite_path(workspace: &WorkspaceRoot, path_raw: &str) -> anyhow::Result<PathBuf> {
    let cleaned = path_raw
        .trim()
        .trim_start_matches("sqlite://")
        .trim_start_matches("file://");
    workspace.resolve(cleaned)
}

fn run_database_action(
    action: &str,
    db_path: &Path,
    sql: Option<&str>,
    table: Option<&str>,
    readonly: bool,
    max_rows: usize,
) -> anyhow::Result<String> {
    if !db_path.exists() {
        anyhow::bail!("database file not found: {}", db_path.display());
    }

    let open_flags = if readonly {
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX
    } else {
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
            | rusqlite::OpenFlags::SQLITE_OPEN_CREATE
            | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX
    };

    let conn = Connection::open_with_flags(db_path, open_flags)
        .map_err(|e| anyhow::anyhow!("failed to open {}: {e}", db_path.display()))?;

    match action {
        "query" => {
            let sql = sql.ok_or_else(|| anyhow::anyhow!("query requires `sql`"))?;
            if readonly && !is_readonly_sql(sql) {
                anyhow::bail!(
                    "readonly mode: only SELECT / WITH / PRAGMA / EXPLAIN statements are allowed \
                     (got: {})",
                    sql.lines().next().unwrap_or(sql).chars().take(80).collect::<String>()
                );
            }
            execute_query(&conn, sql, max_rows)
        }
        "list_tables" => {
            let sql = "SELECT name FROM sqlite_master WHERE type IN ('table','view') \
                       AND name NOT LIKE 'sqlite_%' ORDER BY name";
            execute_query(&conn, sql, max_rows)
        }
        "schema" => {
            let table = table.ok_or_else(|| anyhow::anyhow!("schema requires `table`"))?;
            validate_ident(table)?;
            let sql = format!("PRAGMA table_info({table})");
            execute_query(&conn, &sql, max_rows)
        }
        other => anyhow::bail!("unknown database action: {other}"),
    }
}

/// True when SQL is allowed under readonly=true.
pub fn is_readonly_sql(sql: &str) -> bool {
    // Strip leading comments / whitespace, then check first keyword.
    let stripped = strip_sql_leading(sql);
    let upper = stripped.to_ascii_uppercase();
    upper.starts_with("SELECT")
        || upper.starts_with("WITH")
        || upper.starts_with("PRAGMA")
        || upper.starts_with("EXPLAIN")
}

fn strip_sql_leading(sql: &str) -> &str {
    let mut s = sql.trim_start();
    loop {
        if s.starts_with("--") {
            s = s.split_once('\n').map(|(_, rest)| rest).unwrap_or("").trim_start();
            continue;
        }
        if s.starts_with("/*") {
            s = s
                .split_once("*/")
                .map(|(_, rest)| rest)
                .unwrap_or("")
                .trim_start();
            continue;
        }
        break;
    }
    s
}

fn validate_ident(name: &str) -> anyhow::Result<()> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        anyhow::bail!("invalid identifier: {name:?} (use letters, digits, underscore only)");
    }
    Ok(())
}

fn execute_query(conn: &Connection, sql: &str, max_rows: usize) -> anyhow::Result<String> {
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| anyhow::anyhow!("prepare failed: {e}"))?;

    let col_count = stmt.column_count();
    let headers: Vec<String> = (0..col_count)
        .map(|i| stmt.column_name(i).unwrap_or("?").to_string())
        .collect();

    let mut rows_out: Vec<Vec<String>> = Vec::new();
    let mut truncated = false;

    {
        let mut rows = stmt
            .query([])
            .map_err(|e| anyhow::anyhow!("query failed: {e}"))?;
        while let Some(row) = rows
            .next()
            .map_err(|e| anyhow::anyhow!("row error: {e}"))?
        {
            if rows_out.len() >= max_rows {
                truncated = true;
                break;
            }
            let mut vals = Vec::with_capacity(col_count);
            for i in 0..col_count {
                let v = row.get_ref(i).unwrap_or(ValueRef::Null);
                vals.push(value_ref_to_string(v));
            }
            rows_out.push(vals);
        }
    }

    // Non-SELECT statements (when readonly=false) may have no columns.
    if col_count == 0 {
        let changes = conn.changes();
        return Ok(format!("OK — {changes} row(s) affected"));
    }

    let mut out = String::new();
    out.push_str(&headers.join(" | "));
    out.push('\n');
    out.push_str(&"-".repeat(headers.join(" | ").len().max(8)));
    out.push('\n');
    for r in &rows_out {
        out.push_str(&r.join(" | "));
        out.push('\n');
    }
    out.push_str(&format!("\n({} row(s)", rows_out.len()));
    if truncated {
        out.push_str(&format!(", truncated at max_rows={max_rows}"));
    }
    out.push(')');
    Ok(out)
}

fn value_ref_to_string(v: ValueRef<'_>) -> String {
    match v {
        ValueRef::Null => "NULL".into(),
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(f) => f.to_string(),
        ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
        ValueRef::Blob(b) => format!("<blob {} bytes>", b.len()),
    }
}

/// Whether a database tool call mutates data (for checkpoint policy).
pub fn database_action_is_mutating(args: &Value, readonly_config: bool) -> bool {
    if readonly_config {
        return false;
    }
    match args.get("action").and_then(Value::as_str) {
        Some("list_tables" | "schema") => false,
        Some("query") => {
            let sql = args.get("sql").and_then(Value::as_str).unwrap_or("");
            !is_readonly_sql(sql)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace_root::{SandboxMode, WorkspaceRoot};
    use serde_json::json;
    use tempfile::tempdir;

    fn setup_db() -> (tempfile::TempDir, Arc<WorkspaceRoot>, PathBuf) {
        let dir = tempdir().unwrap();
        let ws = Arc::new(
            WorkspaceRoot::new(dir.path().to_path_buf(), SandboxMode::Strict).expect("ws"),
        );
        let db = dir.path().join("test.db");
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
                 INSERT INTO items (name) VALUES ('alpha'), ('beta'), ('gamma');",
            )
            .unwrap();
        }
        (dir, ws, db)
    }

    #[test]
    fn readonly_sql_allows_select_with_pragma_explain() {
        assert!(is_readonly_sql("SELECT 1"));
        assert!(is_readonly_sql("  with cte as (select 1) select * from cte"));
        assert!(is_readonly_sql("PRAGMA table_info(items)"));
        assert!(is_readonly_sql("EXPLAIN QUERY PLAN SELECT 1"));
        assert!(is_readonly_sql("-- comment\nSELECT 1"));
        assert!(is_readonly_sql("/* c */ SELECT 1"));
        assert!(!is_readonly_sql("INSERT INTO t VALUES (1)"));
        assert!(!is_readonly_sql("DELETE FROM t"));
        assert!(!is_readonly_sql("UPDATE t SET x=1"));
        assert!(!is_readonly_sql("DROP TABLE t"));
    }

    #[tokio::test]
    async fn query_select_and_list_tables() {
        let (_dir, ws, _db) = setup_db();
        let tool = DatabaseTool::new(ws, DatabaseToolConfig::default());

        let out = tool
            .execute(json!({
                "action": "query",
                "path": "test.db",
                "sql": "SELECT name FROM items ORDER BY id"
            }))
            .await
            .expect("query");
        assert!(out.contains("alpha"));
        assert!(out.contains("beta"));
        assert!(out.contains("3 row(s)"));

        let tables = tool
            .execute(json!({"action": "list_tables", "path": "test.db"}))
            .await
            .expect("list");
        assert!(tables.contains("items"));
    }

    #[tokio::test]
    async fn schema_and_reject_insert_when_readonly() {
        let (_dir, ws, _db) = setup_db();
        let tool = DatabaseTool::new(ws, DatabaseToolConfig::default());

        let schema = tool
            .execute(json!({
                "action": "schema",
                "path": "test.db",
                "table": "items"
            }))
            .await
            .expect("schema");
        assert!(schema.contains("name") || schema.to_lowercase().contains("name"));

        let err = tool
            .execute(json!({
                "action": "query",
                "path": "test.db",
                "sql": "INSERT INTO items (name) VALUES ('nope')"
            }))
            .await
            .expect_err("insert blocked");
        assert!(err.to_string().contains("readonly"));
    }

    #[tokio::test]
    async fn max_rows_clamp_and_path_escape() {
        let (_dir, ws, _db) = setup_db();
        let tool = DatabaseTool::new(
            ws.clone(),
            DatabaseToolConfig {
                readonly: true,
                max_rows: 2,
            },
        );

        let out = tool
            .execute(json!({
                "action": "query",
                "path": "test.db",
                "sql": "SELECT name FROM items ORDER BY id",
                "max_rows": 999
            }))
            .await
            .expect("query");
        assert!(out.contains("truncated") || out.contains("2 row"));

        let err = tool
            .execute(json!({
                "action": "query",
                "path": "../outside.db",
                "sql": "SELECT 1"
            }))
            .await
            .expect_err("escape");
        assert!(err.to_string().contains("escapes") || err.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn rejects_postgres_scheme() {
        let (_dir, ws, _db) = setup_db();
        let tool = DatabaseTool::new(ws, DatabaseToolConfig::default());
        let err = tool
            .execute(json!({
                "action": "query",
                "path": "postgres://localhost/db",
                "sql": "SELECT 1"
            }))
            .await
            .expect_err("pg");
        assert!(err.to_string().contains("sqlite-only"));
    }

    #[test]
    fn mutating_policy_helper() {
        assert!(!database_action_is_mutating(
            &json!({"action": "query", "sql": "SELECT 1"}),
            true
        ));
        assert!(!database_action_is_mutating(
            &json!({"action": "query", "sql": "INSERT INTO t VALUES (1)"}),
            true
        ));
        assert!(database_action_is_mutating(
            &json!({"action": "query", "sql": "INSERT INTO t VALUES (1)"}),
            false
        ));
        assert!(!database_action_is_mutating(
            &json!({"action": "list_tables"}),
            false
        ));
    }
}
