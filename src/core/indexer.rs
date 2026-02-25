use crate::adapters::{self, traits::AgentAdapter};
use crate::core::db::Database;
use anyhow::Result;

pub struct IndexResult {
    pub agent: String,
    pub sessions_found: usize,
    pub sessions_new: usize,
    pub sessions_updated: usize,
}

pub fn index_all(db: &Database) -> Result<Vec<IndexResult>> {
    let adapters = adapters::installed_adapters();
    let mut results = Vec::new();

    for adapter in &adapters {
        let result = index_adapter(db, adapter.as_ref())?;
        results.push(result);
    }

    Ok(results)
}

pub fn index_agent(db: &Database, agent_name: &str) -> Result<Option<IndexResult>> {
    if let Some(adapter) = adapters::get_adapter(agent_name) {
        if adapter.is_installed() {
            let result = index_adapter(db, adapter.as_ref())?;
            return Ok(Some(result));
        }
    }
    Ok(None)
}

pub fn rebuild_all(db: &Database) -> Result<Vec<IndexResult>> {
    db.clear_all()?;
    index_all(db)
}

fn index_adapter(db: &Database, adapter: &dyn AgentAdapter) -> Result<IndexResult> {
    let agent_name = adapter.agent_type().as_str().to_string();
    eprintln!("  Scanning {} sessions...", agent_name);
    let sessions = adapter.scan_sessions()?;
    let sessions_found = sessions.len();
    let mut sessions_new = 0;
    let mut sessions_updated = 0;

    for session in sessions {
        if db.session_exists(&session.id)? {
            // Update if message count changed (session grew)
            let old_count = db.session_message_count(&session.id).unwrap_or(0);
            if session.messages.len() as i64 != old_count {
                db.update_session(&session)?;
                sessions_updated += 1;
            }
        } else {
            db.insert_session(&session)?;
            sessions_new += 1;
        }
    }

    Ok(IndexResult {
        agent: agent_name,
        sessions_found,
        sessions_new,
        sessions_updated,
    })
}
