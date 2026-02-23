use super::traits::*;
use anyhow::Result;
use std::fs;
use std::path::PathBuf;

pub struct CodexAdapter {
    data_dir: PathBuf,
}

impl CodexAdapter {
    pub fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        Self {
            data_dir: home.join(".codex"),
        }
    }
}

impl AgentAdapter for CodexAdapter {
    fn agent_type(&self) -> AgentType {
        AgentType::Codex
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
        let sessions_dir = self.data_dir.join("sessions");

        if !sessions_dir.exists() {
            return Ok(sessions);
        }

        for entry in fs::read_dir(&sessions_dir)? {
            let entry = entry?;
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext != "jsonl" && ext != "json" {
                continue;
            }

            let session_id = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();

            match parse_codex_session(&path, &session_id) {
                Ok(Some(session)) => sessions.push(session),
                Ok(None) => {}
                Err(e) => {
                    eprintln!("Warning: Failed to parse Codex session {}: {}", path.display(), e);
                }
            }
        }

        Ok(sessions)
    }

    fn get_session(&self, session_id: &str) -> Result<Option<SessionData>> {
        let sessions_dir = self.data_dir.join("sessions");
        if !sessions_dir.exists() {
            return Ok(None);
        }

        for ext in &["jsonl", "json"] {
            let path = sessions_dir.join(format!("{}.{}", session_id, ext));
            if path.exists() {
                return parse_codex_session(&path, session_id);
            }
        }

        Ok(None)
    }

    fn resume_command(&self, session_id: &str, project_path: Option<&str>) -> String {
        let mut cmd = format!("codex --resume {}", session_id);
        if let Some(p) = project_path {
            cmd = format!("cd {} && {}", p, cmd);
        }
        cmd
    }
}

fn parse_codex_session(path: &std::path::Path, session_id: &str) -> Result<Option<SessionData>> {
    let content = fs::read_to_string(path)?;

    let mut messages = Vec::new();
    let tool_calls = Vec::new();
    let mut started_at = None;
    let mut ended_at = None;
    let mut project_path = None;

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let role_str = v
            .get("role")
            .or_else(|| v.get("message").and_then(|m| m.get("role")))
            .and_then(|r| r.as_str())
            .unwrap_or("");

        let content_text = v
            .get("content")
            .or_else(|| v.get("message").and_then(|m| m.get("content")))
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();

        if project_path.is_none() {
            if let Some(cwd) = v.get("cwd").and_then(|c| c.as_str()) {
                project_path = Some(PathBuf::from(cwd));
            }
        }

        let ts = v
            .get("timestamp")
            .and_then(|t| t.as_str())
            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
            .map(|t| t.with_timezone(&chrono::Utc));

        if let Some(t) = ts {
            if started_at.is_none() || t < started_at.unwrap() {
                started_at = Some(t);
            }
            if ended_at.is_none() || t > ended_at.unwrap() {
                ended_at = Some(t);
            }
        }

        if !role_str.is_empty() && !content_text.is_empty() {
            messages.push(MessageData {
                role: Role::from_str(role_str),
                content: content_text,
                timestamp: ts,
                files_changed: Vec::new(),
            });
        }
    }

    if messages.is_empty() {
        return Ok(None);
    }

    let project_name = project_path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string());

    let mut session = SessionData {
        id: session_id.to_string(),
        agent: AgentType::Codex,
        project_path,
        project_name,
        summary: None,
        work_summary: None,
        started_at,
        ended_at,
        messages,
        tool_calls,
        tags: Vec::new(),
    };

    session.summary = session.first_user_message().map(|s| {
        let end = s.char_indices().nth(200).map(|(i, _)| i).unwrap_or(s.len());
        s[..end].to_string()
    });
    session.work_summary = session.extract_work_summary();

    Ok(Some(session))
}
