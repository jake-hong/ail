use crate::core::db::{Database, SearchResult, SessionRow};
use anyhow::Result;
use chrono::{DateTime, Utc};

pub struct SearchOptions {
    pub keyword: Option<String>,
    pub agent: Option<String>,
    pub project: Option<String>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub file: Option<String>,
    pub limit: usize,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            keyword: None,
            agent: None,
            project: None,
            from: None,
            to: None,
            file: None,
            limit: 100,
        }
    }
}

pub fn search_history(db: &Database, opts: &SearchOptions) -> Result<Vec<SearchResult>> {
    if let Some(ref keyword) = opts.keyword {
        db.search_messages(
            keyword,
            opts.agent.as_deref(),
            opts.project.as_deref(),
            opts.from,
            opts.to,
            opts.limit,
        )
    } else {
        Ok(Vec::new())
    }
}

pub fn search_by_file(db: &Database, file_path: &str, limit: usize) -> Result<Vec<SessionRow>> {
    db.search_by_file(file_path, limit)
}

pub fn list_sessions(db: &Database, opts: &SearchOptions) -> Result<Vec<SessionRow>> {
    db.list_sessions(
        opts.agent.as_deref(),
        opts.project.as_deref(),
        opts.from,
        opts.to,
        opts.limit,
    )
}
