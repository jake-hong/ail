use crate::core::db::{Database, MessageRow, SessionRow, ToolCallRow};
use anyhow::{bail, Result};
use std::fmt::Write;
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub enum DetailLevel {
    Full,
    Summary,
    Minimal,
}

impl DetailLevel {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "full" => DetailLevel::Full,
            "minimal" => DetailLevel::Minimal,
            _ => DetailLevel::Summary,
        }
    }
}

pub fn export_context(
    db: &Database,
    session_id: &str,
    detail: DetailLevel,
) -> Result<String> {
    let session = db
        .get_session(session_id)?
        .ok_or_else(|| anyhow::anyhow!("Session not found: {}", session_id))?;

    let messages = db.get_messages(session_id)?;
    let tool_calls = db.get_tool_calls(session_id)?;

    generate_context_markdown(&session, &messages, &tool_calls, detail)
}

fn generate_context_markdown(
    session: &SessionRow,
    messages: &[MessageRow],
    tool_calls: &[ToolCallRow],
    detail: DetailLevel,
) -> Result<String> {
    let mut out = String::new();

    // Header
    writeln!(out, "# Session Context")?;
    writeln!(out, "- **Agent**: {}", agent_display_name(&session.agent))?;
    if let Some(ref p) = session.project_path {
        writeln!(out, "- **Project**: {}", p)?;
    }
    if let Some(ref t) = session.started_at {
        writeln!(out, "- **Date**: {}", format_date(t))?;
    }
    writeln!(out, "- **Session ID**: {}", session.id)?;
    writeln!(out)?;

    // Work summary
    writeln!(out, "## Work Summary")?;
    if let Some(ref s) = session.summary {
        writeln!(out, "**Request**: {}", s)?;
    }
    if let Some(ref ws) = session.work_summary {
        writeln!(out, "**Result**: {}", ws)?;
    }
    writeln!(out)?;

    // Changed files
    let file_changes = extract_file_changes(tool_calls);
    if !file_changes.is_empty() {
        writeln!(out, "## Changed Files")?;
        for (path, change_type) in &file_changes {
            writeln!(out, "- `{}` ({})", path, change_type)?;
        }
        writeln!(out)?;
    }

    match detail {
        DetailLevel::Minimal => {
            // Already done
        }
        DetailLevel::Summary => {
            // Add last few exchanges
            let last_messages: Vec<&MessageRow> = messages
                .iter()
                .filter(|m| m.role == "user" || m.role == "assistant")
                .rev()
                .take(6)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();

            if !last_messages.is_empty() {
                writeln!(out, "## Recent Conversation")?;
                for msg in last_messages {
                    let role_label = if msg.role == "user" { "You" } else { "AI" };
                    let content = truncate_content(&msg.content, 500);
                    writeln!(out, "**{}**: {}", role_label, content)?;
                    writeln!(out)?;
                }
            }
        }
        DetailLevel::Full => {
            // Full conversation
            writeln!(out, "## Full Conversation")?;
            for msg in messages {
                if msg.role == "tool" {
                    continue;
                }
                let role_label = if msg.role == "user" { "You" } else { "AI" };
                let ts = msg
                    .timestamp
                    .as_ref()
                    .map(|t| format_time(t))
                    .unwrap_or_default();
                writeln!(out, "### {} {}", role_label, ts)?;
                writeln!(out, "{}", msg.content)?;
                writeln!(out)?;
            }
        }
    }

    Ok(out)
}

fn extract_file_changes(tool_calls: &[ToolCallRow]) -> Vec<(String, &'static str)> {
    let mut files = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for tc in tool_calls {
        if let Some(ref fp) = tc.file_path {
            if seen.insert(fp.clone()) {
                let change_type = match tc.tool_name.as_str() {
                    "Write" | "create_file" => "created",
                    "Edit" | "edit_file" => "modified",
                    "delete_file" => "deleted",
                    _ => "modified",
                };
                files.push((fp.clone(), change_type));
            }
        }
    }
    files
}

pub fn inject_context(
    db: &Database,
    session_id: &str,
    project_path: &Path,
) -> Result<()> {
    let context = export_context(db, session_id, DetailLevel::Summary)?;
    let claude_md = project_path.join("CLAUDE.md");

    let mut content = if claude_md.exists() {
        std::fs::read_to_string(&claude_md)?
    } else {
        String::new()
    };

    // Remove existing ail context block if present
    let start_marker = "<!-- ail:context:start -->";
    let end_marker = "<!-- ail:context:end -->";
    if let Some(start) = content.find(start_marker) {
        if let Some(end) = content.find(end_marker) {
            content = format!(
                "{}{}",
                &content[..start],
                &content[end + end_marker.len()..]
            );
        }
    }

    // Append new context
    let inject_block = format!(
        "\n{}\n{}\n{}\n",
        start_marker, context, end_marker
    );
    content.push_str(&inject_block);

    std::fs::write(&claude_md, content)?;
    Ok(())
}

pub fn auto_inject(db: &Database) -> Result<String> {
    let cwd = std::env::current_dir()?;
    let cwd_str = cwd.to_string_lossy().to_string();

    // Find the most recent session for the current project
    let sessions = db.list_sessions(None, Some(&cwd_str), None, None, 1)?;

    if let Some(session) = sessions.first() {
        inject_context(db, &session.id, &cwd)?;
        Ok(session.id.clone())
    } else {
        bail!("No sessions found for current project: {}", cwd_str)
    }
}

fn agent_display_name(agent: &str) -> &str {
    match agent {
        "claude-code" => "Claude Code",
        "codex" => "Codex",
        "cursor" => "Cursor",
        _ => agent,
    }
}

fn format_date(ts: &str) -> String {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
        dt.format("%Y-%m-%d").to_string()
    } else {
        ts.to_string()
    }
}

fn format_time(ts: &str) -> String {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
        dt.format("%H:%M").to_string()
    } else {
        ts.to_string()
    }
}

fn truncate_content(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let end = s.char_indices().nth(max_chars).map(|(i, _)| i).unwrap_or(s.len());
        format!("{}...", &s[..end])
    }
}
