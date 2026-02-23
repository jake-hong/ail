use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentType {
    ClaudeCode,
    Codex,
    Cursor,
}

impl AgentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentType::ClaudeCode => "claude-code",
            AgentType::Codex => "codex",
            AgentType::Cursor => "cursor",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            AgentType::ClaudeCode => "Claude Code",
            AgentType::Codex => "Codex",
            AgentType::Cursor => "Cursor",
        }
    }

    pub fn from_str(s: &str) -> Option<AgentType> {
        match s.to_lowercase().as_str() {
            "claude-code" | "claude_code" | "claude" => Some(AgentType::ClaudeCode),
            "codex" => Some(AgentType::Codex),
            "cursor" => Some(AgentType::Cursor),
            _ => None,
        }
    }
}

impl fmt::Display for AgentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    User,
    Assistant,
    Tool,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }

    pub fn from_str(s: &str) -> Role {
        match s.to_lowercase().as_str() {
            "user" => Role::User,
            "assistant" => Role::Assistant,
            _ => Role::Tool,
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub id: String,
    pub agent: AgentType,
    pub project_path: Option<PathBuf>,
    pub project_name: Option<String>,
    pub summary: Option<String>,
    pub work_summary: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub messages: Vec<MessageData>,
    pub tool_calls: Vec<ToolCallData>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageData {
    pub role: Role,
    pub content: String,
    pub timestamp: Option<DateTime<Utc>>,
    pub files_changed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallData {
    pub tool_name: String,
    pub file_path: Option<String>,
    pub timestamp: Option<DateTime<Utc>>,
}

impl SessionData {
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    pub fn files_created(&self) -> usize {
        self.tool_calls
            .iter()
            .filter(|tc| tc.tool_name == "Write" || tc.tool_name == "create_file")
            .count()
    }

    pub fn files_modified(&self) -> usize {
        self.tool_calls
            .iter()
            .filter(|tc| tc.tool_name == "Edit" || tc.tool_name == "edit_file")
            .count()
    }

    pub fn files_deleted(&self) -> usize {
        self.tool_calls
            .iter()
            .filter(|tc| tc.tool_name == "delete_file")
            .count()
    }

    pub fn first_user_message(&self) -> Option<&str> {
        self.messages
            .iter()
            .find(|m| m.role == Role::User)
            .map(|m| m.content.as_str())
    }

    pub fn last_assistant_message(&self) -> Option<&str> {
        self.messages
            .iter()
            .rev()
            .find(|m| m.role == Role::Assistant && !m.content.is_empty())
            .map(|m| m.content.as_str())
    }

    /// Extract work summary from the last assistant message using rule-based extraction
    pub fn extract_work_summary(&self) -> Option<String> {
        let last_msg = self.last_assistant_message()?;
        // Look for completion patterns
        let patterns = ["완료", "구현", "추가", "수정", "생성", "삭제", "변경",
                        "complete", "implement", "added", "modified", "created", "fixed"];

        let lines: Vec<&str> = last_msg.lines().collect();
        for line in &lines {
            let lower = line.to_lowercase();
            if patterns.iter().any(|p| lower.contains(p)) {
                let trimmed = line.trim();
                if !trimmed.is_empty() && trimmed.len() > 5 {
                    // Return first 200 chars
                    let end = trimmed.char_indices().nth(200).map(|(i, _)| i).unwrap_or(trimmed.len());
                    return Some(trimmed[..end].to_string());
                }
            }
        }

        // Fallback: first non-empty line up to 200 chars
        for line in &lines {
            let trimmed = line.trim();
            if !trimmed.is_empty() && trimmed.len() > 5 {
                let end = trimmed.char_indices().nth(200).map(|(i, _)| i).unwrap_or(trimmed.len());
                return Some(trimmed[..end].to_string());
            }
        }

        None
    }

    pub fn changed_file_paths(&self) -> Vec<(String, &'static str)> {
        let mut files: Vec<(String, &'static str)> = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for tc in &self.tool_calls {
            if let Some(ref fp) = tc.file_path {
                let short = shorten_path(fp);
                if seen.insert(short.clone()) {
                    let prefix = match tc.tool_name.as_str() {
                        "Write" | "create_file" => "+",
                        "Edit" | "edit_file" => "~",
                        "delete_file" => "-",
                        _ => "~",
                    };
                    files.push((short, prefix));
                }
            }
        }
        files
    }
}

fn shorten_path(path: &str) -> String {
    // Extract just the filename or last 2 components
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() <= 2 {
        path.to_string()
    } else {
        parts[parts.len() - 2..].join("/")
    }
}

pub trait AgentAdapter: Send + Sync {
    fn agent_type(&self) -> AgentType;
    fn data_dir(&self) -> PathBuf;
    fn is_installed(&self) -> bool;
    fn scan_sessions(&self) -> anyhow::Result<Vec<SessionData>>;
    fn get_session(&self, session_id: &str) -> anyhow::Result<Option<SessionData>>;
    fn resume_command(&self, session_id: &str, project_path: Option<&str>) -> String;
}
