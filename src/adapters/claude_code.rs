use super::traits::*;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub struct ClaudeCodeAdapter {
    data_dir: PathBuf,
}

impl ClaudeCodeAdapter {
    pub fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        Self {
            data_dir: home.join(".claude"),
        }
    }

    pub fn with_data_dir(data_dir: PathBuf) -> Self {
        Self { data_dir }
    }

    fn projects_dir(&self) -> PathBuf {
        self.data_dir.join("projects")
    }

    /// Decode the project directory name back to a filesystem path.
    /// `-Users-sungeun-Documents-GitHub-rovn-app` → `/Users/sungeun/Documents/GitHub/rovn-app`
    fn decode_project_path(dir_name: &str) -> Option<PathBuf> {
        if dir_name.is_empty() {
            return None;
        }
        // The directory name is the path with '/' replaced by '-'
        // The first '-' represents the leading '/'
        let path_str = dir_name.replacen('-', "/", 1);
        // Now we need to figure out which remaining '-' are path separators
        // vs. hyphens in directory names. We try to resolve the path by
        // checking which interpretation yields an existing directory.
        let full_path = resolve_encoded_path(&path_str);
        Some(full_path)
    }

    fn extract_project_name(project_path: &Path) -> String {
        project_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }

    fn parse_session_file(path: &Path, project_path: Option<&Path>) -> Result<SessionData> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read session file: {}", path.display()))?;

        let session_id = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        let mut messages: Vec<MessageData> = Vec::new();
        let mut tool_calls: Vec<ToolCallData> = Vec::new();
        let mut started_at: Option<DateTime<Utc>> = None;
        let mut ended_at: Option<DateTime<Utc>> = None;
        let mut cwd: Option<String> = None;

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let v: Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let msg_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("");

            // Extract cwd from first user message
            if cwd.is_none() {
                if let Some(c) = v.get("cwd").and_then(|c| c.as_str()) {
                    cwd = Some(c.to_string());
                }
            }

            // Parse timestamp
            let ts = v
                .get("timestamp")
                .and_then(|t| t.as_str())
                .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                .map(|t| t.with_timezone(&Utc));

            if let Some(t) = ts {
                if started_at.is_none() || t < started_at.unwrap() {
                    started_at = Some(t);
                }
                if ended_at.is_none() || t > ended_at.unwrap() {
                    ended_at = Some(t);
                }
            }

            match msg_type {
                "user" => {
                    let content_text = extract_message_content(&v);
                    if !content_text.is_empty() {
                        messages.push(MessageData {
                            role: Role::User,
                            content: content_text,
                            timestamp: ts,
                            files_changed: Vec::new(),
                        });
                    }
                }
                "assistant" => {
                    let msg = v.get("message").unwrap_or(&v);
                    let content_arr = msg.get("content");

                    let mut text_parts: Vec<String> = Vec::new();
                    let mut file_changes: Vec<String> = Vec::new();

                    if let Some(Value::Array(arr)) = content_arr {
                        for item in arr {
                            match item.get("type").and_then(|t| t.as_str()) {
                                Some("text") => {
                                    if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                                        text_parts.push(text.to_string());
                                    }
                                }
                                Some("tool_use") => {
                                    let tool_name = item
                                        .get("name")
                                        .and_then(|n| n.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    let file_path = item
                                        .get("input")
                                        .and_then(|i| {
                                            i.get("file_path")
                                                .or_else(|| i.get("path"))
                                                .and_then(|p| p.as_str())
                                        })
                                        .map(|s| s.to_string());

                                    if matches!(
                                        tool_name.as_str(),
                                        "Write" | "Edit" | "Read" | "create_file" | "edit_file" | "delete_file"
                                    ) {
                                        if let Some(ref fp) = file_path {
                                            file_changes.push(fp.clone());
                                        }
                                    }

                                    tool_calls.push(ToolCallData {
                                        tool_name,
                                        file_path,
                                        timestamp: ts,
                                    });
                                }
                                _ => {}
                            }
                        }
                    } else if let Some(Value::String(s)) = content_arr {
                        text_parts.push(s.clone());
                    }

                    let combined_text = text_parts.join("\n");
                    if !combined_text.is_empty() {
                        messages.push(MessageData {
                            role: Role::Assistant,
                            content: combined_text,
                            timestamp: ts,
                            files_changed: file_changes,
                        });
                    }
                }
                _ => {}
            }
        }

        let resolved_project = cwd
            .as_deref()
            .map(PathBuf::from)
            .or_else(|| project_path.map(|p| p.to_path_buf()));

        let project_name = resolved_project
            .as_ref()
            .map(|p| Self::extract_project_name(p));

        let mut session = SessionData {
            id: session_id,
            agent: AgentType::ClaudeCode,
            project_path: resolved_project,
            project_name,
            summary: None,
            work_summary: None,
            started_at,
            ended_at,
            messages,
            tool_calls,
            tags: Vec::new(),
        };

        // Extract summary from first user message (first sentence, 120 chars)
        session.summary = session.extract_summary();

        // Extract work summary
        session.work_summary = session.extract_work_summary();

        Ok(session)
    }
}

impl AgentAdapter for ClaudeCodeAdapter {
    fn agent_type(&self) -> AgentType {
        AgentType::ClaudeCode
    }

    fn data_dir(&self) -> PathBuf {
        self.data_dir.clone()
    }

    fn is_installed(&self) -> bool {
        self.data_dir.exists() && self.projects_dir().exists()
    }

    fn scan_sessions(&self) -> Result<Vec<SessionData>> {
        let projects_dir = self.projects_dir();
        if !projects_dir.exists() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();

        for project_entry in fs::read_dir(&projects_dir)? {
            let project_entry = project_entry?;
            let project_dir = project_entry.path();

            if !project_dir.is_dir() {
                continue;
            }

            let dir_name = project_entry
                .file_name()
                .to_string_lossy()
                .to_string();
            let project_path = Self::decode_project_path(&dir_name);

            eprintln!("  Scanning project: {}", dir_name);

            // Scan for .jsonl files directly in project dir
            for entry in fs::read_dir(&project_dir)? {
                let entry = entry?;
                let path = entry.path();

                if path.extension().map_or(false, |ext| ext == "jsonl")
                    && path.is_file()
                {
                    // Skip subagent files
                    if path.to_string_lossy().contains("subagent") {
                        continue;
                    }

                    // Skip very large files (>10MB) to avoid hanging
                    if let Ok(meta) = fs::metadata(&path) {
                        if meta.len() > 10 * 1024 * 1024 {
                            eprintln!("  Skipping large file ({:.1}MB): {}", meta.len() as f64 / 1_048_576.0, path.file_name().unwrap_or_default().to_string_lossy());
                            continue;
                        }
                    }

                    match Self::parse_session_file(&path, project_path.as_deref()) {
                        Ok(session) => {
                            // Only include sessions with at least one message
                            if !session.messages.is_empty() {
                                sessions.push(session);
                            }
                        }
                        Err(e) => {
                            eprintln!(
                                "Warning: Failed to parse session {}: {}",
                                path.display(),
                                e
                            );
                        }
                    }
                }
            }

            // Also check sessions/ subdirectory
            let sessions_dir = project_dir.join("sessions");
            if sessions_dir.exists() && sessions_dir.is_dir() {
                for entry in fs::read_dir(&sessions_dir)? {
                    let entry = entry?;
                    let path = entry.path();

                    if path.extension().map_or(false, |ext| ext == "jsonl") && path.is_file() {
                        // Skip very large files (>10MB) to avoid hanging
                        if let Ok(meta) = fs::metadata(&path) {
                            if meta.len() > 10 * 1024 * 1024 {
                                eprintln!("  Skipping large file ({:.1}MB): {}", meta.len() as f64 / 1_048_576.0, path.file_name().unwrap_or_default().to_string_lossy());
                                continue;
                            }
                        }

                        match Self::parse_session_file(&path, project_path.as_deref()) {
                            Ok(session) => {
                                if !session.messages.is_empty() {
                                    sessions.push(session);
                                }
                            }
                            Err(e) => {
                                eprintln!(
                                    "Warning: Failed to parse session {}: {}",
                                    path.display(),
                                    e
                                );
                            }
                        }
                    }
                }
            }
        }

        Ok(sessions)
    }

    fn get_session(&self, session_id: &str) -> Result<Option<SessionData>> {
        let projects_dir = self.projects_dir();
        if !projects_dir.exists() {
            return Ok(None);
        }

        for project_entry in fs::read_dir(&projects_dir)? {
            let project_entry = project_entry?;
            let project_dir = project_entry.path();

            if !project_dir.is_dir() {
                continue;
            }

            let dir_name = project_entry.file_name().to_string_lossy().to_string();
            let project_path = Self::decode_project_path(&dir_name);

            // Check direct .jsonl file
            let session_file = project_dir.join(format!("{}.jsonl", session_id));
            if session_file.exists() {
                return Self::parse_session_file(&session_file, project_path.as_deref()).map(Some);
            }

            // Check sessions/ subdirectory
            let session_file = project_dir.join("sessions").join(format!("{}.jsonl", session_id));
            if session_file.exists() {
                return Self::parse_session_file(&session_file, project_path.as_deref()).map(Some);
            }
        }

        Ok(None)
    }

    fn resume_command(&self, session_id: &str, project_path: Option<&str>) -> String {
        let mut cmd = format!("claude --resume {}", session_id);
        if let Some(p) = project_path {
            cmd = format!("cd {} && {}", p, cmd);
        }
        cmd
    }
}

fn extract_message_content(v: &Value) -> String {
    // Try message.content as string first
    if let Some(content) = v
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
    {
        return content.to_string();
    }

    // Try message.content as array
    if let Some(Value::Array(arr)) = v.get("message").and_then(|m| m.get("content")) {
        let mut parts = Vec::new();
        for item in arr {
            if let Some("text") = item.get("type").and_then(|t| t.as_str()) {
                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                    parts.push(text.to_string());
                }
            }
        }
        return parts.join("\n");
    }

    String::new()
}

/// Try to resolve an encoded path by checking if directories exist
fn resolve_encoded_path(encoded: &str) -> PathBuf {
    // The path has had all '/' replaced with '-', then we replaced the first one back.
    // Now we need to figure out which '-' chars are path separators.
    // Strategy: greedily try to match existing directories from left to right.
    let parts: Vec<&str> = encoded.splitn(2, '/').collect();
    if parts.len() < 2 {
        return PathBuf::from(encoded);
    }

    let prefix = parts[0]; // empty string (before leading /)
    let rest = parts[1];

    // Split the rest by '-' and try to greedily reconstruct path
    let segments: Vec<&str> = rest.split('-').collect();
    if segments.is_empty() {
        return PathBuf::from(encoded);
    }

    let mut result = PathBuf::from(format!("/{}", prefix));
    let mut current = String::new();

    for (i, seg) in segments.iter().enumerate() {
        if current.is_empty() {
            current = seg.to_string();
        } else {
            current = format!("{}-{}", current, seg);
        }

        // Check if treating this as a complete path component works
        let mut test_path = result.clone();
        test_path.push(&current);

        if test_path.exists() || i == segments.len() - 1 {
            result.push(&current);
            current.clear();
        }
    }

    if !current.is_empty() {
        result.push(&current);
    }

    result
}
