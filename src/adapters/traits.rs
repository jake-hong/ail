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

    /// Extract a concise summary of what the user requested.
    ///
    /// Strategy (in priority order):
    /// 1. Extract markdown header title (e.g. `# Plan: 칸반 보드 구현` → `칸반 보드 구현`)
    /// 2. Skip generic instruction prefixes, take the substantive part
    /// 3. First meaningful sentence as fallback
    /// 4. Infer from changed file names if all else fails
    pub fn extract_summary(&self) -> Option<String> {
        let first_msg = self.first_user_message()?;

        // Stage 1: Look for a markdown header with a descriptive title
        for line in first_msg.lines() {
            let trimmed = line.trim();
            // Match `# Plan: Title`, `## Title`, `# Title` etc.
            if trimmed.starts_with('#') {
                let title = trimmed.trim_start_matches('#').trim();
                // Strip common prefixes like "Plan:", "계획:"
                let title = strip_header_prefix(title);
                if title.len() > 3 {
                    return Some(truncate_str(&title, 120));
                }
            }
        }

        // Stage 2: Find first meaningful sentence, skipping generic instructions
        let generic_patterns = [
            "implement the following",
            "implement this",
            "다음을 구현",
            "아래 플랜",
            "아래 계획",
            "following plan",
            "이거보고",
            "이거 보고",
            "<local-command",
            "caveat:",
        ];

        let mut candidate_lines: Vec<&str> = Vec::new();
        for line in first_msg.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("---") {
                continue;
            }
            candidate_lines.push(trimmed);
        }

        // Try each candidate line
        for line in &candidate_lines {
            let lower = line.to_lowercase();

            // Skip if it's a generic instruction line
            if generic_patterns.iter().any(|p| lower.contains(p)) {
                continue;
            }
            // Skip markdown meta lines and XML tags
            if line.starts_with("```") || line.starts_with('|') || line.starts_with("<!--")
                || line.starts_with('<')
            {
                continue;
            }
            // Skip file paths
            if line.starts_with('/') || line.starts_with("'/") || line.starts_with("\"/")
                || line.starts_with("~/")
            {
                continue;
            }

            // Extract first sentence
            let sentence = extract_first_sentence(line);
            let cleaned = strip_markdown(&sentence);
            if cleaned.len() > 3 {
                return Some(truncate_str(&cleaned, 120));
            }
        }

        // Stage 3: Relaxed filter — accept generic instruction lines but still skip junk
        for line in &candidate_lines {
            let trimmed = line.trim();
            // Still skip XML tags and file paths
            if trimmed.starts_with('<') || trimmed.starts_with('/')
                || trimmed.starts_with("'/") || trimmed.starts_with("\"/")
                || trimmed.starts_with("~/") || trimmed.starts_with("```")
            {
                continue;
            }
            // If the line ends with ":", try to combine with next meaningful line
            if trimmed.ends_with(':') {
                if let Some(next) = candidate_lines.iter()
                    .skip_while(|l| *l != line)
                    .nth(1)
                {
                    let next_clean = strip_markdown(next.trim());
                    if next_clean.len() > 3 && !next_clean.starts_with('<')
                        && !next_clean.starts_with("```")
                    {
                        return Some(truncate_str(&next_clean, 120));
                    }
                }
            }
            let sentence = extract_first_sentence(trimmed);
            if sentence.len() > 3 {
                return Some(truncate_str(&strip_markdown(&sentence), 120));
            }
        }

        // Stage 4: Infer from file changes
        self.infer_summary_from_files()
    }

    /// Infer a summary from changed file names when no text summary is available
    fn infer_summary_from_files(&self) -> Option<String> {
        let files = self.changed_file_paths();
        if files.is_empty() {
            return None;
        }

        let created: Vec<_> = files.iter().filter(|(_, p)| *p == "+").collect();
        let modified: Vec<_> = files.iter().filter(|(_, p)| *p == "~").collect();

        let mut parts = Vec::new();
        if !created.is_empty() {
            let names: Vec<_> = created.iter().take(3).map(|(f, _)| f.as_str()).collect();
            parts.push(format!("Created {}", names.join(", ")));
        }
        if !modified.is_empty() {
            let names: Vec<_> = modified.iter().take(3).map(|(f, _)| f.as_str()).collect();
            parts.push(format!("Modified {}", names.join(", ")));
        }

        if parts.is_empty() {
            None
        } else {
            Some(truncate_str(&parts.join("; "), 120))
        }
    }

    /// Extract work summary: what the AI actually accomplished.
    ///
    /// Strategy (in priority order):
    /// 1. Find commit messages in any assistant message
    /// 2. Scan ALL assistant messages for summary sections (## Summary, etc.)
    /// 3. Keyword-scored lines across all assistant messages (later messages weighted higher)
    /// 4. Infer from file change statistics
    pub fn extract_work_summary(&self) -> Option<String> {
        let assistant_msgs: Vec<(usize, &str)> = self
            .messages
            .iter()
            .enumerate()
            .filter(|(_, m)| m.role == Role::Assistant && !m.content.is_empty())
            .map(|(i, m)| (i, m.content.as_str()))
            .collect();

        if assistant_msgs.is_empty() {
            return self.infer_work_from_files();
        }

        // Stage 1: Look for commit messages across all messages
        for (_, msg) in assistant_msgs.iter().rev() {
            if let Some(commit_msg) = extract_commit_message(msg) {
                return Some(truncate_str(&commit_msg, 120));
            }
        }

        // Stage 2: Look for summary/conclusion sections in all messages (prefer later)
        let summary_headers = [
            "## summary", "## 요약", "## result", "## 결과",
            "## done", "## 완료", "## changes", "## 변경",
            "요약:", "summary:", "완료:",
        ];

        for (_, msg) in assistant_msgs.iter().rev() {
            let lines = filter_meaningful_lines(msg);
            for (i, line) in lines.iter().enumerate() {
                let lower = line.trim().to_lowercase();
                if summary_headers.iter().any(|h| lower.starts_with(h)) {
                    // Collect the next 1-3 meaningful lines as summary
                    let summary_lines: Vec<String> = lines[i + 1..]
                        .iter()
                        .take(3)
                        .map(|l| strip_markdown(l.trim()))
                        .filter(|l| l.len() > 3)
                        .collect();
                    if !summary_lines.is_empty() {
                        return Some(truncate_str(&summary_lines.join("; "), 120));
                    }
                }
            }
        }

        // Stage 3: Keyword scoring across ALL assistant messages
        // Later messages get position bonus
        let keywords = [
            "완료", "구현", "추가", "수정", "생성", "삭제", "변경", "적용", "배포",
            "complete", "implement", "added", "modified", "created", "fixed",
            "updated", "refactored", "removed", "resolved", "deployed", "built",
        ];

        // Negative patterns — lines to skip (starts_with)
        let skip_start_patterns = [
            "let me", "i'll", "i will", "now ", "here",
            "looking", "reading", "checking",
        ];
        // Negative patterns — lines to skip (contains, for Korean)
        let skip_contains_patterns = [
            "제가", "이제", "확인", "살펴", "읽어", "파악", "분석",
            "할게요", "하겠습니다", "볼게요", "탐색", "조사",
            "핵심 발견", "확인 완료", "탐색 완료",
        ];

        let mut best: Option<(String, f64)> = None;
        let total_msgs = assistant_msgs.len();

        for (msg_idx, (_, msg)) in assistant_msgs.iter().enumerate() {
            let lines = filter_meaningful_lines(msg);
            let position_weight = (msg_idx + 1) as f64 / total_msgs as f64; // 0.0..1.0, later = higher

            for line in &lines {
                let lower = line.to_lowercase();
                let trimmed = line.trim();

                if trimmed.len() <= 5 || trimmed.starts_with('#') {
                    continue;
                }

                // Skip lines that are about planning, not doing
                if skip_start_patterns.iter().any(|p| lower.starts_with(p)) {
                    continue;
                }
                if skip_contains_patterns.iter().any(|p| lower.contains(p)) {
                    continue;
                }

                let keyword_score: usize = keywords.iter().filter(|kw| lower.contains(*kw)).count();
                if keyword_score > 0 {
                    let score = keyword_score as f64 * (1.0 + position_weight);
                    if best.is_none() || score > best.as_ref().unwrap().1 {
                        best = Some((strip_markdown(trimmed), score));
                    }
                }
            }
        }

        if let Some((line, _)) = best {
            return Some(truncate_str(&line, 120));
        }

        // Stage 4: Fallback — last meaningful line from last assistant message
        if let Some((_, last_msg)) = assistant_msgs.last() {
            let lines = filter_meaningful_lines(last_msg);
            for line in lines.iter().rev() {
                let trimmed = line.trim();
                if trimmed.len() > 10 && !trimmed.starts_with('#') {
                    let cleaned = strip_markdown(trimmed);
                    if !cleaned.is_empty() {
                        return Some(truncate_str(&cleaned, 120));
                    }
                }
            }
        }

        // Stage 5: Infer from file statistics
        self.infer_work_from_files()
    }

    /// Infer work summary from file change statistics
    fn infer_work_from_files(&self) -> Option<String> {
        let created = self.files_created();
        let modified = self.files_modified();
        let deleted = self.files_deleted();
        let total = created + modified + deleted;

        if total == 0 {
            return None;
        }

        let mut parts = Vec::new();
        if created > 0 {
            parts.push(format!("{} files created", created));
        }
        if modified > 0 {
            parts.push(format!("{} files modified", modified));
        }
        if deleted > 0 {
            parts.push(format!("{} files deleted", deleted));
        }
        Some(parts.join(", "))
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

/// Strip markdown formatting: bold, italic, list markers, heading markers
fn strip_markdown(s: &str) -> String {
    let mut result = s.to_string();
    // Remove bold/italic markers
    result = result.replace("**", "");
    result = result.replace("__", "");
    // Remove leading heading markers: "### ", "## ", "# "
    let trimmed = result.trim_start();
    if trimmed.starts_with('#') {
        result = trimmed.trim_start_matches('#').trim_start().to_string();
    }
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

/// Truncate a string to at most `max_chars` characters, avoiding mid-word/mid-backtick cuts
fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let end = s
        .char_indices()
        .nth(max_chars)
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    let mut truncated = s[..end].to_string();
    // Remove trailing incomplete backtick-quoted text
    if truncated.matches('`').count() % 2 != 0 {
        if let Some(pos) = truncated.rfind('`') {
            truncated.truncate(pos);
        }
    }
    // Trim trailing punctuation fragments
    let truncated = truncated.trim_end_matches(|c: char| c == ';' || c == ',' || c == ' ');
    truncated.to_string()
}

/// Extract the first sentence from text, splitting on `.` `!` `?`
fn extract_first_sentence(text: &str) -> String {
    let end_pos = text
        .char_indices()
        .find(|(_, c)| *c == '.' || *c == '!' || *c == '?')
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(text.len());
    text[..end_pos].trim().to_string()
}

/// Strip common header prefixes like "Plan:", "계획:", "Task:", "작업:" from titles
fn strip_header_prefix(title: &str) -> String {
    let prefixes = [
        "Plan:", "plan:", "계획:", "Task:", "task:", "작업:",
        "Feature:", "feature:", "기능:", "Bug:", "bug:", "버그:",
        "Fix:", "fix:", "수정:", "Refactor:", "refactor:", "리팩토링:",
    ];
    for prefix in &prefixes {
        if let Some(rest) = title.strip_prefix(prefix) {
            let rest = rest.trim();
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    title.to_string()
}

/// Extract a commit message from text if present.
/// Looks for patterns like `git commit -m "message"` or commit message in code blocks.
fn extract_commit_message(text: &str) -> Option<String> {
    // Pattern 1: git commit -m "..." or -m '...'
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(pos) = trimmed.find("commit -m") {
            let after = &trimmed[pos + 9..].trim_start();
            // Extract quoted string
            if let Some(msg) = extract_quoted(after) {
                if msg.len() > 5 {
                    return Some(msg);
                }
            }
        }
    }

    // Pattern 2: Lines starting with conventional commit prefixes,
    // but ONLY if they appear right after a "commit" or "committed" context line
    let commit_prefixes = [
        "feat:", "fix:", "chore:", "refactor:", "docs:", "test:", "style:", "perf:",
        "feat(", "fix(", "chore(", "refactor(", "docs(", "test(",
    ];
    let mut prev_has_commit_context = false;
    for line in text.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();

        if lower.contains("commit") || lower.contains("커밋") {
            prev_has_commit_context = true;
            continue;
        }

        if prev_has_commit_context && commit_prefixes.iter().any(|p| lower.starts_with(p)) {
            if trimmed.len() > 10 && trimmed.len() < 200 {
                return Some(strip_markdown(trimmed));
            }
        }

        if !trimmed.is_empty() {
            prev_has_commit_context = false;
        }
    }

    None
}

/// Extract a quoted string (single or double quotes) from text
fn extract_quoted(s: &str) -> Option<String> {
    let s = s.trim();
    let quote = s.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &s[1..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

/// Filter out code blocks, tables, comments from text, return meaningful lines
fn filter_meaningful_lines(text: &str) -> Vec<&str> {
    let mut in_code_block = false;
    text.lines()
        .filter(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("```") {
                in_code_block = !in_code_block;
                return false;
            }
            if in_code_block {
                return false;
            }
            if trimmed.is_empty()
                || trimmed.starts_with('|')
                || trimmed.starts_with("<!--")
                || trimmed.starts_with("---")
            {
                return false;
            }
            true
        })
        .collect()
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
