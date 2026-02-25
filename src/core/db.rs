use crate::adapters::traits::*;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

pub struct Database {
    conn: Connection,
}

#[derive(Debug, Clone)]
pub struct SessionRow {
    pub id: String,
    pub conversation_id: Option<String>,
    pub agent: String,
    pub project_path: Option<String>,
    pub project_name: Option<String>,
    pub summary: Option<String>,
    pub work_summary: Option<String>,
    pub llm_summary: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub message_count: i64,
    pub files_created: i64,
    pub files_modified: i64,
    pub files_deleted: i64,
    pub tags: String,
}

#[derive(Debug, Clone)]
pub struct MessageRow {
    pub id: i64,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub timestamp: Option<String>,
    pub files_changed: String,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub session_id: String,
    pub agent: String,
    pub project_name: Option<String>,
    pub project_path: Option<String>,
    pub role: String,
    pub content: String,
    pub timestamp: Option<String>,
    pub summary: Option<String>,
    pub started_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ToolCallRow {
    pub id: i64,
    pub session_id: String,
    pub tool_name: String,
    pub file_path: Option<String>,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub total_sessions: i64,
    pub sessions_by_agent: Vec<(String, i64)>,
    pub sessions_by_project: Vec<(String, i64)>,
    pub total_files_created: i64,
    pub total_files_modified: i64,
    pub total_files_deleted: i64,
    pub most_modified_files: Vec<(String, i64)>,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open database at {}", path.display()))?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;

        let db = Self { conn };
        db.init_schema()?;
        db.migrate()?;
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.init_schema()?;
        db.migrate()?;
        Ok(db)
    }

    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                agent TEXT NOT NULL,
                project_path TEXT,
                project_name TEXT,
                summary TEXT,
                work_summary TEXT,
                started_at TEXT,
                ended_at TEXT,
                message_count INTEGER DEFAULT 0,
                files_created INTEGER DEFAULT 0,
                files_modified INTEGER DEFAULT 0,
                files_deleted INTEGER DEFAULT 0,
                tags TEXT DEFAULT ''
            );

            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                timestamp TEXT,
                files_changed TEXT DEFAULT '[]'
            );

            CREATE TABLE IF NOT EXISTS tool_calls (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                tool_name TEXT NOT NULL,
                file_path TEXT,
                timestamp TEXT
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
                session_id UNINDEXED,
                role UNINDEXED,
                content,
                tokenize='unicode61'
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS sessions_fts USING fts5(
                session_id UNINDEXED,
                summary,
                work_summary,
                project_name,
                tags,
                tokenize='unicode61'
            );

            CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id);
            CREATE INDEX IF NOT EXISTS idx_tool_calls_session ON tool_calls(session_id);
            CREATE INDEX IF NOT EXISTS idx_sessions_agent ON sessions(agent);
            CREATE INDEX IF NOT EXISTS idx_sessions_project ON sessions(project_path);
            CREATE INDEX IF NOT EXISTS idx_sessions_started ON sessions(started_at);
            CREATE INDEX IF NOT EXISTS idx_tool_calls_file ON tool_calls(file_path);
            ",
        )?;
        Ok(())
    }

    fn migrate(&self) -> Result<()> {
        // Safe migration: add columns if they don't exist
        self.conn
            .execute("ALTER TABLE sessions ADD COLUMN llm_summary TEXT", [])
            .ok();
        self.conn
            .execute("ALTER TABLE sessions ADD COLUMN conversation_id TEXT", [])
            .ok();
        Ok(())
    }

    pub fn insert_session(&self, session: &SessionData) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO sessions (id, conversation_id, agent, project_path, project_name, summary, work_summary, started_at, ended_at, message_count, files_created, files_modified, files_deleted, tags)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                session.id,
                session.conversation_id,
                session.agent.as_str(),
                session.project_path.as_ref().map(|p| p.to_string_lossy().to_string()),
                session.project_name,
                session.summary,
                session.work_summary,
                session.started_at.map(|t| t.to_rfc3339()),
                session.ended_at.map(|t| t.to_rfc3339()),
                session.message_count() as i64,
                session.files_created() as i64,
                session.files_modified() as i64,
                session.files_deleted() as i64,
                session.tags.join(","),
            ],
        )?;

        // Insert into sessions FTS
        self.conn.execute(
            "INSERT OR REPLACE INTO sessions_fts (session_id, summary, work_summary, project_name, tags)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                session.id,
                session.summary.as_deref().unwrap_or(""),
                session.work_summary.as_deref().unwrap_or(""),
                session.project_name.as_deref().unwrap_or(""),
                session.tags.join(" "),
            ],
        )?;

        // Insert messages
        for msg in &session.messages {
            let _msg_id = self.conn.execute(
                "INSERT INTO messages (session_id, role, content, timestamp, files_changed)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    session.id,
                    msg.role.as_str(),
                    msg.content,
                    msg.timestamp.map(|t| t.to_rfc3339()),
                    serde_json::to_string(&msg.files_changed).unwrap_or_default(),
                ],
            )?;

            // Insert into messages FTS (use last_insert_rowid for the rowid)
            self.conn.execute(
                "INSERT INTO messages_fts (session_id, role, content)
                 VALUES (?1, ?2, ?3)",
                params![session.id, msg.role.as_str(), msg.content],
            )?;
        }

        // Insert tool calls
        for tc in &session.tool_calls {
            self.conn.execute(
                "INSERT INTO tool_calls (session_id, tool_name, file_path, timestamp)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    session.id,
                    tc.tool_name,
                    tc.file_path,
                    tc.timestamp.map(|t| t.to_rfc3339()),
                ],
            )?;
        }

        Ok(())
    }

    pub fn delete_session(&self, session_id: &str) -> Result<()> {
        // Delete FTS entries first
        self.conn.execute(
            "DELETE FROM messages_fts WHERE session_id = ?1",
            params![session_id],
        )?;
        self.conn.execute(
            "DELETE FROM sessions_fts WHERE session_id = ?1",
            params![session_id],
        )?;
        // Delete from main tables (CASCADE handles messages and tool_calls)
        self.conn.execute(
            "DELETE FROM tool_calls WHERE session_id = ?1",
            params![session_id],
        )?;
        self.conn.execute(
            "DELETE FROM messages WHERE session_id = ?1",
            params![session_id],
        )?;
        self.conn.execute(
            "DELETE FROM sessions WHERE id = ?1",
            params![session_id],
        )?;
        Ok(())
    }

    pub fn clear_all(&self) -> Result<()> {
        self.conn.execute_batch(
            "DELETE FROM messages_fts;
             DELETE FROM sessions_fts;
             DELETE FROM tool_calls;
             DELETE FROM messages;
             DELETE FROM sessions;",
        )?;
        Ok(())
    }

    pub fn get_session(&self, session_id: &str) -> Result<Option<SessionRow>> {
        self.conn
            .query_row(
                "SELECT id, conversation_id, agent, project_path, project_name, summary, work_summary, llm_summary, started_at, ended_at, message_count, files_created, files_modified, files_deleted, tags
                 FROM sessions WHERE id = ?1",
                params![session_id],
                |row| Self::row_to_session(row),
            )
            .optional()
            .map_err(Into::into)
    }

    fn row_to_session(row: &rusqlite::Row) -> rusqlite::Result<SessionRow> {
        Ok(SessionRow {
            id: row.get(0)?,
            conversation_id: row.get(1)?,
            agent: row.get(2)?,
            project_path: row.get(3)?,
            project_name: row.get(4)?,
            summary: row.get(5)?,
            work_summary: row.get(6)?,
            llm_summary: row.get(7)?,
            started_at: row.get(8)?,
            ended_at: row.get(9)?,
            message_count: row.get(10)?,
            files_created: row.get(11)?,
            files_modified: row.get(12)?,
            files_deleted: row.get(13)?,
            tags: row.get::<_, String>(14)?,
        })
    }

    pub fn list_sessions(
        &self,
        agent: Option<&str>,
        project: Option<&str>,
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
        limit: usize,
    ) -> Result<Vec<SessionRow>> {
        let mut sql = String::from(
            "SELECT id, conversation_id, agent, project_path, project_name, summary, work_summary, llm_summary, started_at, ended_at, message_count, files_created, files_modified, files_deleted, tags
             FROM sessions WHERE 1=1",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(a) = agent {
            sql.push_str(" AND agent = ?");
            param_values.push(Box::new(a.to_string()));
        }
        if let Some(p) = project {
            let abs_project = std::fs::canonicalize(p)
                .unwrap_or_else(|_| std::path::PathBuf::from(p));
            sql.push_str(" AND project_path = ?");
            param_values.push(Box::new(abs_project.to_string_lossy().to_string()));
        }
        if let Some(f) = from {
            sql.push_str(" AND started_at >= ?");
            param_values.push(Box::new(f.to_rfc3339()));
        }
        if let Some(t) = to {
            sql.push_str(" AND started_at <= ?");
            param_values.push(Box::new(t.to_rfc3339()));
        }

        sql.push_str(" ORDER BY started_at DESC");
        sql.push_str(&format!(" LIMIT {}", limit));

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_refs.as_slice(), |row| Self::row_to_session(row))?;

        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(row?);
        }
        Ok(sessions)
    }

    pub fn get_messages(&self, session_id: &str) -> Result<Vec<MessageRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, role, content, timestamp, files_changed
             FROM messages WHERE session_id = ?1 ORDER BY id ASC",
        )?;

        let rows = stmt.query_map(params![session_id], |row| {
            Ok(MessageRow {
                id: row.get(0)?,
                session_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                timestamp: row.get(4)?,
                files_changed: row.get::<_, String>(5)?,
            })
        })?;

        let mut messages = Vec::new();
        for row in rows {
            messages.push(row?);
        }
        Ok(messages)
    }

    pub fn get_tool_calls(&self, session_id: &str) -> Result<Vec<ToolCallRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, tool_name, file_path, timestamp
             FROM tool_calls WHERE session_id = ?1 ORDER BY id ASC",
        )?;

        let rows = stmt.query_map(params![session_id], |row| {
            Ok(ToolCallRow {
                id: row.get(0)?,
                session_id: row.get(1)?,
                tool_name: row.get(2)?,
                file_path: row.get(3)?,
                timestamp: row.get(4)?,
            })
        })?;

        let mut tool_calls = Vec::new();
        for row in rows {
            tool_calls.push(row?);
        }
        Ok(tool_calls)
    }

    pub fn search_messages(
        &self,
        keyword: &str,
        agent: Option<&str>,
        project: Option<&str>,
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        let mut sql = String::from(
            "SELECT mf.session_id, s.agent, s.project_name, s.project_path, mf.role, mf.content, s.started_at, s.summary, s.started_at
             FROM messages_fts mf
             JOIN sessions s ON s.id = mf.session_id
             WHERE messages_fts MATCH ?1",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        param_values.push(Box::new(keyword.to_string()));

        if let Some(a) = agent {
            sql.push_str(" AND s.agent = ?");
            param_values.push(Box::new(a.to_string()));
        }
        if let Some(p) = project {
            let abs_project = std::fs::canonicalize(p)
                .unwrap_or_else(|_| std::path::PathBuf::from(p));
            sql.push_str(" AND s.project_path = ?");
            param_values.push(Box::new(abs_project.to_string_lossy().to_string()));
        }
        if let Some(f) = from {
            sql.push_str(" AND s.started_at >= ?");
            param_values.push(Box::new(f.to_rfc3339()));
        }
        if let Some(t) = to {
            sql.push_str(" AND s.started_at <= ?");
            param_values.push(Box::new(t.to_rfc3339()));
        }

        sql.push_str(&format!(" LIMIT {}", limit));

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok(SearchResult {
                session_id: row.get(0)?,
                agent: row.get(1)?,
                project_name: row.get(2)?,
                project_path: row.get(3)?,
                role: row.get(4)?,
                content: row.get(5)?,
                timestamp: row.get(6)?,
                summary: row.get(7)?,
                started_at: row.get(8)?,
            })
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    pub fn search_by_file(
        &self,
        file_path: &str,
        limit: usize,
    ) -> Result<Vec<SessionRow>> {
        let pattern = format!("%{}%", file_path);
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT s.id, s.conversation_id, s.agent, s.project_path, s.project_name, s.summary, s.work_summary, s.llm_summary, s.started_at, s.ended_at, s.message_count, s.files_created, s.files_modified, s.files_deleted, s.tags
             FROM sessions s
             JOIN tool_calls tc ON tc.session_id = s.id
             WHERE tc.file_path LIKE ?1
             ORDER BY s.started_at DESC
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![pattern, limit as i64], |row| Self::row_to_session(row))?;

        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(row?);
        }
        Ok(sessions)
    }

    pub fn update_tags(&self, session_id: &str, tags: &[String]) -> Result<()> {
        let tag_str = tags.join(",");
        self.conn.execute(
            "UPDATE sessions SET tags = ?1 WHERE id = ?2",
            params![tag_str, session_id],
        )?;
        // Update FTS
        self.conn.execute(
            "UPDATE sessions_fts SET tags = ?1 WHERE session_id = ?2",
            params![tags.join(" "), session_id],
        )?;
        Ok(())
    }

    pub fn get_tags(&self, session_id: &str) -> Result<Vec<String>> {
        let tags: String = self
            .conn
            .query_row(
                "SELECT tags FROM sessions WHERE id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .unwrap_or_default();

        Ok(tags
            .split(',')
            .filter(|t| !t.is_empty())
            .map(|t| t.to_string())
            .collect())
    }

    pub fn get_stats(
        &self,
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
        project: Option<&str>,
    ) -> Result<Stats> {
        let mut where_clause = String::from("WHERE 1=1");
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(f) = from {
            where_clause.push_str(" AND started_at >= ?");
            param_values.push(Box::new(f.to_rfc3339()));
        }
        if let Some(t) = to {
            where_clause.push_str(" AND started_at <= ?");
            param_values.push(Box::new(t.to_rfc3339()));
        }
        if let Some(p) = project {
            let abs_project = std::fs::canonicalize(p)
                .unwrap_or_else(|_| std::path::PathBuf::from(p));
            where_clause.push_str(" AND project_path = ?");
            param_values.push(Box::new(abs_project.to_string_lossy().to_string()));
        }

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        // Total sessions
        let total_sessions: i64 = self.conn.query_row(
            &format!("SELECT COUNT(*) FROM sessions {}", where_clause),
            params_refs.as_slice(),
            |row| row.get(0),
        )?;

        // By agent
        let mut stmt = self.conn.prepare(&format!(
            "SELECT agent, COUNT(*) FROM sessions {} GROUP BY agent ORDER BY COUNT(*) DESC",
            where_clause
        ))?;
        let sessions_by_agent: Vec<(String, i64)> = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        // By project
        let mut stmt = self.conn.prepare(&format!(
            "SELECT COALESCE(project_name, 'unknown'), COUNT(*) FROM sessions {} GROUP BY project_name ORDER BY COUNT(*) DESC",
            where_clause
        ))?;
        let sessions_by_project: Vec<(String, i64)> = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        // File stats
        let file_stats: (i64, i64, i64) = self.conn.query_row(
            &format!(
                "SELECT COALESCE(SUM(files_created),0), COALESCE(SUM(files_modified),0), COALESCE(SUM(files_deleted),0) FROM sessions {}",
                where_clause
            ),
            params_refs.as_slice(),
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;

        // Most modified files
        let mut file_where = String::from("WHERE 1=1");
        let mut file_params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        if let Some(f) = from {
            file_where.push_str(" AND tc.timestamp >= ?");
            file_params.push(Box::new(f.to_rfc3339()));
        }
        if let Some(t) = to {
            file_where.push_str(" AND tc.timestamp <= ?");
            file_params.push(Box::new(t.to_rfc3339()));
        }
        let file_params_refs: Vec<&dyn rusqlite::types::ToSql> =
            file_params.iter().map(|p| p.as_ref()).collect();

        let mut stmt = self.conn.prepare(&format!(
            "SELECT tc.file_path, COUNT(*) as cnt FROM tool_calls tc {} AND tc.file_path IS NOT NULL GROUP BY tc.file_path ORDER BY cnt DESC LIMIT 10",
            file_where
        ))?;
        let most_modified_files: Vec<(String, i64)> = stmt
            .query_map(file_params_refs.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(Stats {
            total_sessions,
            sessions_by_agent,
            sessions_by_project,
            total_files_created: file_stats.0,
            total_files_modified: file_stats.1,
            total_files_deleted: file_stats.2,
            most_modified_files,
        })
    }

    pub fn update_llm_summary(&self, session_id: &str, llm_summary: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET llm_summary = ?1 WHERE id = ?2",
            params![llm_summary, session_id],
        )?;
        Ok(())
    }

    pub fn session_exists(&self, session_id: &str) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn session_message_count(&self, session_id: &str) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT message_count FROM sessions WHERE id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    /// Update an existing session with new data (upsert pattern).
    /// Replaces messages and tool_calls entirely.
    pub fn update_session(&self, session: &SessionData) -> Result<()> {
        // Update session metadata
        self.conn.execute(
            "UPDATE sessions SET conversation_id = ?1, summary = ?2, work_summary = ?3, ended_at = ?4, message_count = ?5, files_created = ?6, files_modified = ?7, files_deleted = ?8
             WHERE id = ?9",
            params![
                session.conversation_id,
                session.summary,
                session.work_summary,
                session.ended_at.map(|t| t.to_rfc3339()),
                session.message_count() as i64,
                session.files_created() as i64,
                session.files_modified() as i64,
                session.files_deleted() as i64,
                session.id,
            ],
        )?;

        // Replace messages: delete old, insert new
        self.conn.execute("DELETE FROM messages_fts WHERE session_id = ?1", params![session.id])?;
        self.conn.execute("DELETE FROM messages WHERE session_id = ?1", params![session.id])?;
        for msg in &session.messages {
            self.conn.execute(
                "INSERT INTO messages (session_id, role, content, timestamp, files_changed) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    session.id,
                    msg.role.as_str(),
                    msg.content,
                    msg.timestamp.map(|t| t.to_rfc3339()),
                    serde_json::to_string(&msg.files_changed).unwrap_or_default(),
                ],
            )?;
            self.conn.execute(
                "INSERT INTO messages_fts (session_id, role, content) VALUES (?1, ?2, ?3)",
                params![session.id, msg.role.as_str(), msg.content],
            )?;
        }

        // Replace tool calls
        self.conn.execute("DELETE FROM tool_calls WHERE session_id = ?1", params![session.id])?;
        for tc in &session.tool_calls {
            self.conn.execute(
                "INSERT INTO tool_calls (session_id, tool_name, file_path, timestamp) VALUES (?1, ?2, ?3, ?4)",
                params![session.id, tc.tool_name, tc.file_path, tc.timestamp.map(|t| t.to_rfc3339())],
            )?;
        }

        // Update sessions FTS
        self.conn.execute(
            "UPDATE sessions_fts SET summary = ?1, work_summary = ?2 WHERE session_id = ?3",
            params![
                session.summary.as_deref().unwrap_or(""),
                session.work_summary.as_deref().unwrap_or(""),
                session.id,
            ],
        )?;

        Ok(())
    }

    pub fn session_count(&self) -> Result<i64> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))?;
        Ok(count)
    }

    pub fn clean_sessions(
        &self,
        before: DateTime<Utc>,
        agent: Option<&str>,
    ) -> Result<usize> {
        let mut sql = String::from("SELECT id FROM sessions WHERE started_at < ?1");
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        param_values.push(Box::new(before.to_rfc3339()));

        if let Some(a) = agent {
            sql.push_str(" AND agent = ?2");
            param_values.push(Box::new(a.to_string()));
        }

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let mut stmt = self.conn.prepare(&sql)?;
        let ids: Vec<String> = stmt
            .query_map(params_refs.as_slice(), |row| row.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();

        let count = ids.len();
        for id in &ids {
            self.delete_session(id)?;
        }

        Ok(count)
    }
}

pub fn parse_datetime(s: &str) -> Option<DateTime<Utc>> {
    // Try RFC3339 first
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    // Try YYYY-MM-DD
    if let Ok(nd) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let ndt = nd.and_hms_opt(0, 0, 0)?;
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc));
    }
    None
}

/// Parse a duration string like "7d", "2w", "1m" into a chrono::Duration
pub fn parse_duration(s: &str) -> Option<chrono::Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let (num_str, unit) = s.split_at(s.len() - 1);
    let num: i64 = num_str.parse().ok()?;

    match unit {
        "d" => Some(chrono::Duration::days(num)),
        "w" => Some(chrono::Duration::weeks(num)),
        "m" => Some(chrono::Duration::days(num * 30)),
        "h" => Some(chrono::Duration::hours(num)),
        _ => None,
    }
}
