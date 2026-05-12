use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::Connection;

/// Initialize the per-project database.
/// Locates the project root (git repo root or cwd) and stores the DB
/// at `<project_root>/.codereview/state.db`.
pub fn init() -> Result<Connection> {
    let project_root = find_project_root()?;
    init_at(&project_root)
}

/// Initialize the database at a specific project root.
pub fn init_at(project_root: &Path) -> Result<Connection> {
    let db_path = project_db_path(project_root);

    // Ensure .codereview/ directory exists
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("Failed to create database directory: {}", parent.display())
        })?;
    }

    // Best-effort: add .codereview/ to .gitignore
    let _ = ensure_gitignore_entry(project_root);

    let conn = Connection::open(&db_path)
        .with_context(|| format!("Failed to open database: {}", db_path.display()))?;

    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;

    run_migrations(&conn)?;

    Ok(conn)
}

/// Determine the project root directory.
///
/// 1. Try git repo root via `git2::Repository::discover`
/// 2. Fall back to current working directory
pub fn find_project_root() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("Cannot determine current directory")?;

    match git2::Repository::discover(&cwd) {
        Ok(repo) => repo
            .workdir()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| anyhow::anyhow!("Bare git repository has no working directory")),
        Err(_) => Ok(cwd),
    }
}

/// Per-project database path: `<root>/.codereview/state.db`
fn project_db_path(project_root: &Path) -> PathBuf {
    project_root.join(".codereview").join("state.db")
}

/// Append `.codereview/` to `.gitignore` if not already present.
/// Best-effort — never fails the caller. Only appends to existing `.gitignore`.
fn ensure_gitignore_entry(project_root: &Path) -> Result<()> {
    let gitignore_path = project_root.join(".gitignore");

    if !gitignore_path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(&gitignore_path).context("Failed to read .gitignore")?;

    let already_present = content.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == ".codereview/"
            || trimmed == ".codereview"
            || trimmed == "/.codereview/"
            || trimmed == "/.codereview"
    });

    if already_present {
        return Ok(());
    }

    let mut addition = String::new();
    if !content.ends_with('\n') {
        addition.push('\n');
    }
    addition.push_str(".codereview/\n");

    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&gitignore_path)?;
    file.write_all(addition.as_bytes())?;

    Ok(())
}

/// Initialize an in-memory database with migrations (for testing).
pub fn init_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
    run_migrations(&conn)?;
    Ok(conn)
}

/// Run all pending migrations.
fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER PRIMARY KEY
        );",
    )?;

    let current_version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let migrations: &[(i64, &str)] = &[(
        1,
        "CREATE TABLE IF NOT EXISTS onboarding (
                id         INTEGER PRIMARY KEY CHECK (id = 1),
                state_json TEXT    NOT NULL,
                version    INTEGER NOT NULL DEFAULT 1,
                updated_at TEXT    NOT NULL
            );",
    )];

    for (version, sql) in migrations {
        if *version > current_version {
            conn.execute_batch(sql)?;
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                rusqlite::params![version],
            )?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_in_memory_creates_schema() {
        let conn = init_in_memory().unwrap();
        let version: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_version",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, 1);
    }

    #[test]
    fn init_in_memory_creates_onboarding_table() {
        let conn = init_in_memory().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM onboarding", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn migrations_are_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap();

        let version: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_version",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, 1);
    }

    #[test]
    fn onboarding_table_enforces_singleton() {
        let conn = init_in_memory().unwrap();
        conn.execute(
            "INSERT INTO onboarding (id, state_json, version, updated_at) VALUES (1, '{}', 1, '0')",
            [],
        )
        .unwrap();

        let result = conn.execute(
            "INSERT INTO onboarding (id, state_json, version, updated_at) VALUES (2, '{}', 1, '0')",
            [],
        );
        assert!(result.is_err());
    }

    // ── Per-project DB tests ──

    #[test]
    fn project_db_path_correct() {
        let root = PathBuf::from("/tmp/my-project");
        let path = project_db_path(&root);
        assert_eq!(path, PathBuf::from("/tmp/my-project/.codereview/state.db"));
    }

    #[test]
    fn init_at_creates_codereview_dir_and_db() {
        let tmp = tempfile::tempdir().unwrap();
        let conn = init_at(tmp.path()).unwrap();

        // DB should be functional
        let version: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_version",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, 1);

        // .codereview/state.db should exist
        assert!(tmp.path().join(".codereview").join("state.db").exists());
    }

    #[test]
    fn ensure_gitignore_appends_when_not_present() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(".gitignore"), "/target\n").unwrap();

        ensure_gitignore_entry(tmp.path()).unwrap();

        let content = std::fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        assert!(content.contains(".codereview/"));
    }

    #[test]
    fn ensure_gitignore_no_duplicate() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(".gitignore"), "/target\n.codereview/\n").unwrap();

        ensure_gitignore_entry(tmp.path()).unwrap();

        let content = std::fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        assert_eq!(content.matches(".codereview/").count(), 1);
    }

    #[test]
    fn ensure_gitignore_noop_when_no_file() {
        let tmp = tempfile::tempdir().unwrap();
        ensure_gitignore_entry(tmp.path()).unwrap();
        assert!(!tmp.path().join(".gitignore").exists());
    }

    #[test]
    fn init_at_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let _conn1 = init_at(tmp.path()).unwrap();
        let conn2 = init_at(tmp.path()).unwrap();

        let version: i64 = conn2
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_version",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, 1);
    }
}
