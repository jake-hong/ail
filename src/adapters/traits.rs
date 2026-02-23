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

    /// Extract a concise summary from the first user message.
    /// Takes only the first meaningful sentence, skipping markdown headers, limited to 120 chars.
    pub fn extract_summary(&self) -> Option<String> {
        let first_msg = self.first_user_message()?;

        // Skip markdown headers and empty lines, find first content line
        let mut first_sentence = None;
        for line in first_msg.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            // Extract first sentence by splitting on sentence boundaries
            let text = trimmed;
            let end_pos = text
                .char_indices()
                .find(|(_, c)| *c == '.' || *c == '!' || *c == '?' || *c == '\n')
                .map(|(i, c)| {
                    // Include the punctuation mark itself
                    i + c.len_utf8()
                })
                .unwrap_or(text.len());
            first_sentence = Some(&text[..end_pos]);
            break;
        }

        let sentence = first_sentence?.trim();
        if sentence.is_empty() {
            return None;
        }

        // Limit to 120 chars
        let end = sentence
            .char_indices()
            .nth(120)
            .map(|(i, _)| i)
            .unwrap_or(sentence.len());
        Some(sentence[..end].to_string())
    }

    /// Extract work summary from the last assistant message using rule-based extraction.
    /// Uses a 3-stage approach: summary headers → keyword scoring → fallback.
    /// Skips code blocks, tables, and comments. Strips markdown formatting.
    pub fn extract_work_summary(&self) -> Option<String> {
        let last_msg = self.last_assistant_message()?;

        let lines: Vec<&str> = last_msg.lines().collect();

        // Filter out code blocks, tables, and comments
        let mut in_code_block = false;
        let meaningful_lines: Vec<&str> = lines
            .iter()
            .filter(|line| {
                let trimmed = line.trim();
                if trimmed.starts_with("```") {
                    in_code_block = !in_code_block;
                    return false;
                }
                if in_code_block {
                    return false;
                }
                // Skip table rows, HTML comments, empty lines
                if trimmed.starts_with('|') || trimmed.starts_with("<!--") || trimmed.is_empty() {
                    return false;
                }
                true
            })
            .copied()
            .collect();

        // Stage 1: Look for summary/conclusion headers and take the next line
        let summary_headers = ["## summary", "## 요약", "## result", "## 결과", "## done", "## 완료"];
        for (i, line) in meaningful_lines.iter().enumerate() {
            let lower = line.trim().to_lowercase();
            if summary_headers.iter().any(|h| lower.starts_with(h)) {
                // Take the next non-empty line after the header
                for next_line in &meaningful_lines[i + 1..] {
                    let cleaned = strip_markdown(next_line.trim());
                    if !cleaned.is_empty() && cleaned.len() > 3 {
                        return Some(truncate_str(&cleaned, 120));
                    }
                }
            }
        }

        // Stage 2: Keyword scoring — find the best matching line
        let keywords = [
            "완료", "구현", "추가", "수정", "생성", "삭제", "변경",
            "complete", "implement", "added", "modified", "created", "fixed",
            "updated", "refactored", "removed", "resolved",
        ];

        let mut best_line: Option<(&str, usize)> = None;
        for line in &meaningful_lines {
            let lower = line.to_lowercase();
            let trimmed = line.trim();

            // Skip short lines, markdown headers
            if trimmed.len() <= 5 || trimmed.starts_with('#') {
                continue;
            }

            let score: usize = keywords.iter().filter(|kw| lower.contains(*kw)).count();
            if score > 0 {
                if best_line.is_none() || score > best_line.unwrap().1 {
                    best_line = Some((trimmed, score));
                }
            }
        }

        if let Some((line, _)) = best_line {
            let cleaned = strip_markdown(line);
            return Some(truncate_str(&cleaned, 120));
        }

        // Stage 3: Fallback — first non-empty meaningful line
        for line in &meaningful_lines {
            let trimmed = line.trim();
            if trimmed.len() > 5 && !trimmed.starts_with('#') {
                let cleaned = strip_markdown(trimmed);
                return Some(truncate_str(&cleaned, 120));
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

/// Strip markdown formatting: bold, italic, list markers
fn strip_markdown(s: &str) -> String {
    let mut result = s.to_string();
    // Remove bold/italic markers
    result = result.replace("**", "");
    result = result.replace("__", "");
    // Remove leading list markers: "- ", "* ", "1. "
    let trimmed = result.trim_start();
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
        result = trimmed[2..].to_string();
    } else if let Some(rest) = trimmed.strip_prefix(|c: char| c.is_ascii_digit()) {
        if let Some(rest) = rest.strip_prefix(". ") {
            result = rest.to_string();
        }
    }
    result.trim().to_string()
}

/// Truncate a string to at most `max_chars` characters
fn truncate_str(s: &str, max_chars: usize) -> String {
    let end = s
        .char_indices()
        .nth(max_chars)
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    s[..end].to_string()
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
