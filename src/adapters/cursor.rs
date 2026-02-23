use super::traits::*;
use anyhow::Result;
use std::fs;
use std::path::PathBuf;

pub struct CursorAdapter {
    data_dir: PathBuf,
}

impl CursorAdapter {
    pub fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        Self {
            data_dir: home.join(".cursor"),
        }
    }
}

impl AgentAdapter for CursorAdapter {
    fn agent_type(&self) -> AgentType {
        AgentType::Cursor
    }

    fn data_dir(&self) -> PathBuf {
        self.data_dir.clone()
    }

    fn is_installed(&self) -> bool {
        self.data_dir.exists()
    }

    fn scan_sessions(&self) -> Result<Vec<SessionData>> {
        if !self.is_installed() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();

        // Cursor stores AI chat in various locations depending on version
        // Check projects/ directory for session data
        let projects_dir = self.data_dir.join("projects");
        if projects_dir.exists() {
            scan_cursor_dir(&projects_dir, &mut sessions)?;
        }

        // Check sessions/ directory
        let sessions_dir = self.data_dir.join("sessions");
        if sessions_dir.exists() {
            scan_cursor_dir(&sessions_dir, &mut sessions)?;
        }

        Ok(sessions)
    }

    fn get_session(&self, session_id: &str) -> Result<Option<SessionData>> {
        // Search through known locations
        let sessions = self.scan_sessions()?;
        Ok(sessions.into_iter().find(|s| s.id == session_id))
    }

    fn resume_command(&self, _session_id: &str, project_path: Option<&str>) -> String {
        match project_path {
            Some(p) => format!("cursor {}", p),
            None => "cursor .".to_string(),
        }
    }
}

fn scan_cursor_dir(dir: &std::path::Path, sessions: &mut Vec<SessionData>) -> Result<()> {
    if !dir.exists() || !dir.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext == "json" || ext == "jsonl" {
                match parse_cursor_session(&path) {
                    Ok(Some(session)) => sessions.push(session),
                    Ok(None) => {}
                    Err(_) => {}
                }
            }
        }
    }

    Ok(())
}

fn parse_cursor_session(path: &std::path::Path) -> Result<Option<SessionData>> {
    let content = fs::read_to_string(path)?;
    let session_id = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut messages = Vec::new();

    // Try parsing as JSON array first
    if let Ok(serde_json::Value::Array(arr)) = serde_json::from_str::<serde_json::Value>(&content)
    {
        for item in &arr {
            let role_str = item.get("role").and_then(|r| r.as_str()).unwrap_or("");
            let text = item
                .get("content")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string();

            if !role_str.is_empty() && !text.is_empty() {
                messages.push(MessageData {
                    role: Role::from_str(role_str),
                    content: text,
                    timestamp: None,
                    files_changed: Vec::new(),
                });
            }
        }
    } else {
        // Try JSONL
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                let role_str = v.get("role").and_then(|r| r.as_str()).unwrap_or("");
                let text = v
                    .get("content")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string();

                if !role_str.is_empty() && !text.is_empty() {
                    messages.push(MessageData {
                        role: Role::from_str(role_str),
                        content: text,
                        timestamp: None,
                        files_changed: Vec::new(),
                    });
                }
            }
        }
    }

    if messages.is_empty() {
        return Ok(None);
    }

    let mut session = SessionData {
        id: session_id,
        agent: AgentType::Cursor,
        project_path: None,
        project_name: None,
        summary: None,
        work_summary: None,
        started_at: None,
        ended_at: None,
        messages,
        tool_calls: Vec::new(),
        tags: Vec::new(),
    };

    session.summary = session.first_user_message().map(|s| {
        let end = s.char_indices().nth(200).map(|(i, _)| i).unwrap_or(s.len());
        s[..end].to_string()
    });
    session.work_summary = session.extract_work_summary();

    Ok(Some(session))
}
