use crate::config::SummarizeConfig;
use crate::core::db::{Database, SessionRow};
use anyhow::{bail, Result};

/// Resolve the API key: config value takes precedence, then ANTHROPIC_API_KEY env var
fn resolve_api_key(config: &SummarizeConfig) -> Result<String> {
    if let Some(ref key) = config.api_key {
        if !key.is_empty() {
            return Ok(key.clone());
        }
    }
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        if !key.is_empty() {
            return Ok(key);
        }
    }
    bail!(
        "No API key found. Set ANTHROPIC_API_KEY environment variable or add api_key to [report.summarize] in config."
    )
}

/// Call Claude API to generate a one-line summary of a session
fn call_claude_summarize(
    api_key: &str,
    model: &str,
    session_text: &str,
    max_input_chars: usize,
) -> Result<String> {
    // Truncate input to max_input_chars
    let input: String = session_text.chars().take(max_input_chars).collect();

    let body = serde_json::json!({
        "model": model,
        "max_tokens": 300,
        "messages": [{
            "role": "user",
            "content": format!(
                "Summarize this AI coding session. Focus on what was accomplished.\nIf multiple distinct tasks were done, list each as a bullet point (max 3 bullets, each under 80 chars).\nIf only one task, use a single sentence (max 100 chars).\nReply with ONLY the summary, no quotes or prefixes.\n\nExample (multi-task):\n- Implemented user authentication with JWT\n- Fixed database migration bug in users table\n\nExample (single task):\nAdded dark mode toggle to application settings\n\n{}",
                input
            )
        }]
    });

    let resp = ureq::post("https://api.anthropic.com/v1/messages")
        .set("x-api-key", api_key)
        .set("anthropic-version", "2023-06-01")
        .set("content-type", "application/json")
        .send_json(body);

    let resp = match resp {
        Ok(r) => r,
        Err(ureq::Error::Status(code, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            bail!("API error ({}): {}", code, body);
        }
        Err(e) => bail!("Request failed: {}", e),
    };

    let json: serde_json::Value = resp.into_json()?;

    // Extract text from response
    let text = json
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    if text.is_empty() {
        bail!("Empty response from API");
    }

    Ok(text)
}

/// Build a text representation of a session for summarization
fn build_session_text(db: &Database, session: &SessionRow) -> String {
    let mut text = String::new();

    if let Some(ref project) = session.project_name {
        text.push_str(&format!("Project: {}\n", project));
    }
    if let Some(ref summary) = session.summary {
        text.push_str(&format!("Request: {}\n", summary));
    }
    if let Some(ref work) = session.work_summary {
        text.push_str(&format!("Work: {}\n", work));
    }

    // Add user messages (primary signal) and short AI summaries
    if let Ok(messages) = db.get_messages(&session.id) {
        for msg in &messages {
            if msg.role == "tool" {
                continue;
            }
            let role_label = if msg.role == "user" { "User" } else { "AI" };
            // User messages get more space, AI messages are truncated shorter
            let max_chars = if msg.role == "user" { 500 } else { 200 };
            let content: String = msg.content.chars().take(max_chars).collect();
            text.push_str(&format!("\n{}: {}", role_label, content));
        }
    }

    text
}

/// Summarize sessions that don't already have an llm_summary.
/// Shows progress and continues on individual failures.
pub fn summarize_sessions(
    db: &Database,
    sessions: &[SessionRow],
    config: &SummarizeConfig,
) -> Result<usize> {
    let api_key = resolve_api_key(config)?;

    // Filter to sessions without llm_summary
    let to_summarize: Vec<&SessionRow> = sessions
        .iter()
        .filter(|s| s.llm_summary.is_none())
        .collect();

    if to_summarize.is_empty() {
        eprintln!("All sessions already have LLM summaries.");
        return Ok(0);
    }

    let total = to_summarize.len();
    let mut success_count = 0;

    for (i, session) in to_summarize.iter().enumerate() {
        eprint!("Summarizing {}/{}...\r", i + 1, total);

        let session_text = build_session_text(db, session);
        match call_claude_summarize(&api_key, &config.model, &session_text, config.max_input_chars)
        {
            Ok(summary) => {
                if let Err(e) = db.update_llm_summary(&session.id, &summary) {
                    eprintln!("\nFailed to save summary for {}: {}", &session.id[..8], e);
                } else {
                    success_count += 1;
                }
            }
            Err(e) => {
                eprintln!(
                    "\nFailed to summarize session {}: {}",
                    &session.id[..session.id.len().min(8)],
                    e
                );
            }
        }
    }

    eprintln!("Summarized {}/{} sessions.", success_count, total);
    Ok(success_count)
}
