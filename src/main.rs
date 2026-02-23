#![allow(dead_code)]

mod adapters;
mod cli;
mod config;
mod core;
mod mcp;
mod tui;

use crate::cli::{Cli, Commands};
use crate::config as cfg;
use crate::core::context::{self, DetailLevel};
use crate::core::db::{parse_duration, Database};
use crate::core::indexer;
use crate::core::report::{self, ReportFormat};
use crate::core::search::{self, SearchOptions};
use anyhow::{bail, Result};
use chrono::Utc;
use clap::Parser;
use std::io::Write;

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => {
            // Launch TUI
            tui::run_tui()?;
        }
        Some(cmd) => run_command(cmd, cli.json)?,
    }

    Ok(())
}

fn run_command(cmd: Commands, json_output: bool) -> Result<()> {
    match cmd {
        Commands::Setup => cmd_setup(),
        Commands::Index { agent, rebuild } => cmd_index(agent, rebuild),
        Commands::List {
            agent,
            project,
            last,
            query,
        } => cmd_list(agent, project, last, query, json_output),
        Commands::Resume {
            session_id,
            last,
            agent,
            context,
        } => cmd_resume(session_id, last, agent, context),
        Commands::Cd { session_id } => cmd_cd(&session_id),
        Commands::History {
            keyword,
            agent,
            project,
            last,
            file,
        } => cmd_history(keyword, agent, project, last, file, json_output),
        Commands::Show { session_id, files } => cmd_show(&session_id, files, json_output),
        Commands::Tag {
            session_id,
            tags,
            remove,
        } => cmd_tag(&session_id, tags, remove),
        Commands::Clean {
            older_than,
            agent,
            interactive,
        } => cmd_clean(older_than, agent, interactive),
        Commands::Report {
            day,
            date,
            week,
            month,
            quarter,
            from,
            to,
            project,
            output,
            format,
            summarize,
        } => cmd_report(day, date, week, month, quarter, from, to, project, output, format, summarize),
        Commands::Export {
            session_id,
            clipboard,
            stdout,
            detail,
        } => cmd_export(&session_id, clipboard, stdout, &detail),
        Commands::Inject { session_id, auto } => cmd_inject(session_id, auto),
        Commands::Serve { mcp } => cmd_serve(mcp),
        Commands::Config { edit } => cmd_config(edit),
    }
}

fn open_db() -> Result<Database> {
    cfg::ensure_data_dir()?;
    let db_path = cfg::db_path();
    Database::open(&db_path)
}

// ── Setup ──

fn cmd_setup() -> Result<()> {
    println!();
    println!("  ┌─────────────────────────────────────┐");
    println!("  │  ail — AI Log                        │");
    println!("  │  Unified AI coding session manager   │");
    println!("  └─────────────────────────────────────┘");
    println!();

    // Step 1: Detect agents and let user choose
    println!("  [1/3] Select agents to index...");
    println!();
    let all = adapters::all_adapters();
    let installed: Vec<_> = all.iter().filter(|a| a.is_installed()).collect();

    if installed.is_empty() {
        println!("    No agents found. Install Claude Code, Codex, or Cursor first.");
        println!();
        return Ok(());
    }

    let mut selected_agents: Vec<String> = Vec::new();
    for (i, adapter) in installed.iter().enumerate() {
        print!(
            "    [{}] {} ({}) — Index? (Y/n): ",
            i + 1,
            adapter.agent_type().display_name(),
            adapter.data_dir().display()
        );
        std::io::stdout().flush().ok();
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).ok();
        let answer = input.trim().to_lowercase();
        if answer.is_empty() || answer == "y" || answer == "yes" {
            println!("        -> selected");
            selected_agents.push(adapter.agent_type().as_str().to_string());
        } else {
            println!("        -> skipped");
        }
    }

    // Also show agents that are not installed
    for adapter in &all {
        if !adapter.is_installed() {
            println!(
                "    [ ] {} — not found",
                adapter.agent_type().display_name()
            );
        }
    }

    println!();
    if selected_agents.is_empty() {
        println!("    No agents selected. Run `ail setup` again to choose.");
        println!();
        return Ok(());
    }
    println!("    {} agent(s) selected.", selected_agents.len());
    println!();

    // Step 2: Index selected agents only
    println!("  [2/3] Indexing sessions...");
    println!();
    let db = open_db()?;
    let mut total_found: usize = 0;
    let mut total_new: usize = 0;
    for agent_name in &selected_agents {
        if let Some(result) = indexer::index_agent(&db, agent_name)? {
            if result.sessions_found > 0 {
                println!(
                    "    {} — {} sessions found, {} new",
                    result.agent, result.sessions_found, result.sessions_new
                );
            }
            total_found += result.sessions_found;
            total_new += result.sessions_new;
        }
    }
    println!();
    println!("    Total: {} sessions indexed ({} new)", total_found, total_new);
    println!();

    // Step 3: Config
    println!("  [3/3] Writing config...");
    let config = cfg::AilConfig::default();
    cfg::save_config(&config)?;
    let config_path = cfg::config_path();
    println!("    Config: {}", config_path.display());
    println!("    Database: {}", cfg::db_path().display());
    println!();

    // Done
    println!("  ┌─────────────────────────────────────┐");
    println!("  │  Setup complete.                     │");
    println!("  └─────────────────────────────────────┘");
    println!();
    println!("  Quick start:");
    println!("    ail               Open TUI session browser");
    println!("    ail list          List recent sessions");
    println!("    ail history -k    Search conversation history");
    println!("    ail report --week Weekly work report");
    println!();
    println!("  Tip:");
    println!("    Run `ail serve` to start the MCP server — your AI agents");
    println!("    can then query your session history programmatically.");
    println!();
    println!("  Like ail? Star it on GitHub: github.com/jake-hong/ail");
    println!();

    Ok(())
}

// ── Index ──

fn cmd_index(agent: Option<String>, rebuild: bool) -> Result<()> {
    let db = open_db()?;

    if rebuild {
        println!("Rebuilding index...");
        let results = indexer::rebuild_all(&db)?;
        for r in &results {
            println!("  {}: {} sessions", r.agent, r.sessions_found);
        }
        let total: usize = results.iter().map(|r| r.sessions_found).sum();
        println!("✓ {} sessions indexed", total);
    } else if let Some(ref agent_name) = agent {
        println!("Indexing {} sessions...", agent_name);
        if let Some(result) = indexer::index_agent(&db, agent_name)? {
            println!(
                "  {} found, {} new",
                result.sessions_found, result.sessions_new
            );
        } else {
            println!("  Agent not found or not installed: {}", agent_name);
        }
    } else {
        println!("Indexing all sessions...");
        let results = indexer::index_all(&db)?;
        for r in &results {
            if r.sessions_found > 0 {
                println!(
                    "  {}: {} found, {} new",
                    r.agent, r.sessions_found, r.sessions_new
                );
            }
        }
        let total: usize = results.iter().map(|r| r.sessions_new).sum();
        println!("✓ {} new sessions indexed", total);
    }

    Ok(())
}

// ── List ──

fn cmd_list(
    agent: Option<String>,
    project: Option<String>,
    last: Option<String>,
    query: Option<String>,
    json_output: bool,
) -> Result<()> {
    let db = open_db()?;

    let from = last.as_ref().and_then(|d| {
        parse_duration(d).map(|dur| Utc::now() - dur)
    });

    let sessions = db.list_sessions(
        agent.as_deref(),
        project.as_deref(),
        from,
        None,
        200,
    )?;

    // Apply fuzzy filter if query provided
    let sessions = if let Some(ref q) = query {
        use fuzzy_matcher::skim::SkimMatcherV2;
        use fuzzy_matcher::FuzzyMatcher;
        let matcher = SkimMatcherV2::default();
        let mut scored: Vec<_> = sessions
            .into_iter()
            .filter_map(|s| {
                let text = format!(
                    "{} {} {}",
                    s.project_name.as_deref().unwrap_or(""),
                    s.summary.as_deref().unwrap_or(""),
                    s.agent
                );
                matcher.fuzzy_match(&text, q).map(|score| (s, score))
            })
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1));
        scored.into_iter().map(|(s, _)| s).collect()
    } else {
        sessions
    };

    if json_output {
        let json_sessions: Vec<serde_json::Value> = sessions
            .iter()
            .map(|s| {
                serde_json::json!({
                    "id": s.id,
                    "agent": s.agent,
                    "project": s.project_name,
                    "project_path": s.project_path,
                    "summary": s.summary,
                    "started_at": s.started_at,
                    "message_count": s.message_count,
                    "tags": s.tags,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_sessions)?);
    } else {
        if sessions.is_empty() {
            println!("No sessions found. Run `ail setup` or `ail index` first.");
            return Ok(());
        }

        println!(
            "{:<12} {:<14} {:<20} {:<6} {}",
            "ID", "AGENT", "PROJECT", "MSGS", "SUMMARY"
        );
        println!("{}", "-".repeat(80));
        for s in &sessions {
            let short_id = &s.id[..s.id.len().min(10)];
            let summary: String = s
                .summary
                .as_deref()
                .unwrap_or("")
                .chars()
                .take(40)
                .collect();
            println!(
                "{:<12} {:<14} {:<20} {:<6} {}",
                short_id,
                s.agent,
                s.project_name.as_deref().unwrap_or("?"),
                s.message_count,
                summary
            );
        }
        println!("\n{} sessions", sessions.len());
    }

    Ok(())
}

// ── Resume ──

fn cmd_resume(
    session_id: Option<String>,
    last: bool,
    agent: Option<String>,
    context_file: Option<String>,
) -> Result<()> {
    let db = open_db()?;

    let session = if last {
        let sessions = db.list_sessions(agent.as_deref(), None, None, None, 1)?;
        sessions.into_iter().next()
    } else if let Some(ref sid) = session_id {
        db.get_session(sid)?
    } else {
        bail!("Provide a session ID or use --last");
    };

    let session = session.ok_or_else(|| anyhow::anyhow!("Session not found"))?;

    // Build resume command
    let agent_type = adapters::traits::AgentType::from_str(&session.agent)
        .ok_or_else(|| anyhow::anyhow!("Unknown agent: {}", session.agent))?;

    let cmd = match agent_type {
        adapters::traits::AgentType::ClaudeCode => {
            if let Some(ref ctx) = context_file {
                format!(
                    "cd {} && claude --resume {} --context {}",
                    session.project_path.as_deref().unwrap_or("."),
                    session.id,
                    ctx
                )
            } else {
                format!(
                    "cd {} && claude --resume {}",
                    session.project_path.as_deref().unwrap_or("."),
                    session.id
                )
            }
        }
        adapters::traits::AgentType::Codex => {
            format!(
                "cd {} && codex --resume {}",
                session.project_path.as_deref().unwrap_or("."),
                session.id
            )
        }
        adapters::traits::AgentType::Cursor => {
            format!(
                "cursor {}",
                session.project_path.as_deref().unwrap_or(".")
            )
        }
    };

    println!("{}", cmd);
    // Execute the command
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .status()?;

    if !status.success() {
        eprintln!("Command exited with status: {}", status);
    }

    Ok(())
}

// ── Cd ──

fn cmd_cd(session_id: &str) -> Result<()> {
    let db = open_db()?;
    let session = db
        .get_session(session_id)?
        .ok_or_else(|| anyhow::anyhow!("Session not found: {}", session_id))?;

    if let Some(ref p) = session.project_path {
        // Print for shell eval: eval "$(ail cd <id>)"
        println!("cd {}", p);
    } else {
        bail!("No project path for session {}", session_id);
    }
    Ok(())
}

// ── History ──

fn cmd_history(
    keyword: Option<String>,
    agent: Option<String>,
    project: Option<String>,
    last: Option<String>,
    file: Option<String>,
    json_output: bool,
) -> Result<()> {
    let db = open_db()?;

    if let Some(ref file_path) = file {
        let sessions = search::search_by_file(&db, file_path, 50)?;
        if json_output {
            println!("{}", serde_json::to_string_pretty(&serde_json::json!(
                sessions.iter().map(|s| serde_json::json!({
                    "id": s.id, "agent": s.agent, "project": s.project_name, "summary": s.summary,
                })).collect::<Vec<_>>()
            ))?);
        } else {
            println!("Sessions that modified '{}':", file_path);
            for s in &sessions {
                println!(
                    "  {} | {} | {} | {}",
                    &s.id[..s.id.len().min(10)],
                    s.agent,
                    s.project_name.as_deref().unwrap_or("?"),
                    s.summary.as_deref().unwrap_or("")
                );
            }
        }
        return Ok(());
    }

    if keyword.is_none() {
        // Launch history TUI (just use main TUI for now)
        return tui::run_tui();
    }

    let from = last.as_ref().and_then(|d| {
        parse_duration(d).map(|dur| Utc::now() - dur)
    });

    let opts = SearchOptions {
        keyword,
        agent,
        project,
        from,
        to: None,
        file: None,
        limit: 50,
    };

    let results = search::search_history(&db, &opts)?;

    if json_output {
        let json_results: Vec<serde_json::Value> = results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "session_id": r.session_id,
                    "agent": r.agent,
                    "project": r.project_name,
                    "role": r.role,
                    "content": r.content.chars().take(200).collect::<String>(),
                    "timestamp": r.timestamp,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_results)?);
    } else {
        println!("Found {} matches\n", results.len());
        for r in &results {
            let content: String = r.content.chars().take(100).collect();
            let content = content.replace('\n', " ");
            println!(
                "  {} | {} | {}",
                r.agent,
                r.project_name.as_deref().unwrap_or("?"),
                &r.session_id[..r.session_id.len().min(8)]
            );
            println!("    {}: {}", r.role, content);
            println!();
        }
    }

    Ok(())
}

// ── Show ──

fn cmd_show(session_id: &str, files_only: bool, json_output: bool) -> Result<()> {
    let db = open_db()?;

    let session = db
        .get_session(session_id)?
        .ok_or_else(|| anyhow::anyhow!("Session not found: {}", session_id))?;

    if files_only {
        let tool_calls = db.get_tool_calls(session_id)?;
        let mut seen = std::collections::HashSet::new();

        if json_output {
            let files: Vec<serde_json::Value> = tool_calls
                .iter()
                .filter_map(|tc| {
                    tc.file_path.as_ref().and_then(|fp| {
                        if seen.insert(fp.clone()) {
                            Some(serde_json::json!({
                                "path": fp,
                                "action": tc.tool_name,
                            }))
                        } else {
                            None
                        }
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&files)?);
        } else {
            println!("Files changed in session {}:", session_id);
            for tc in &tool_calls {
                if let Some(ref fp) = tc.file_path {
                    if seen.insert(fp.clone()) {
                        let prefix = match tc.tool_name.as_str() {
                            "Write" | "create_file" => "+",
                            "Edit" | "edit_file" => "~",
                            "delete_file" => "-",
                            _ => " ",
                        };
                        println!("  {} {}", prefix, fp);
                    }
                }
            }
        }
    } else {
        let messages = db.get_messages(session_id)?;

        if json_output {
            let json_msgs: Vec<serde_json::Value> = messages
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "role": m.role,
                        "content": m.content,
                        "timestamp": m.timestamp,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&json_msgs)?);
        } else {
            println!(
                "Session: {} | {} | {}\n",
                session_id,
                session.agent,
                session.project_name.as_deref().unwrap_or("?")
            );
            for m in &messages {
                if m.role == "tool" {
                    continue;
                }
                let label = if m.role == "user" { "You" } else { "AI" };
                let ts = m
                    .timestamp
                    .as_ref()
                    .map(|t| {
                        chrono::DateTime::parse_from_rfc3339(t)
                            .map(|d| d.format(" (%H:%M)").to_string())
                            .unwrap_or_default()
                    })
                    .unwrap_or_default();
                println!("--- {}{} ---", label, ts);
                println!("{}\n", m.content);
            }
        }
    }

    Ok(())
}

// ── Tag ──

fn cmd_tag(session_id: &str, tags: Vec<String>, remove: bool) -> Result<()> {
    let db = open_db()?;

    let mut current_tags = db.get_tags(session_id)?;

    if remove {
        current_tags.retain(|t| !tags.contains(t));
    } else {
        for tag in &tags {
            if !current_tags.contains(tag) {
                current_tags.push(tag.clone());
            }
        }
    }

    db.update_tags(session_id, &current_tags)?;
    println!("Tags: {}", current_tags.join(", "));

    Ok(())
}

// ── Clean ──

fn cmd_clean(
    older_than: Option<String>,
    agent: Option<String>,
    _interactive: bool,
) -> Result<()> {
    let db = open_db()?;

    let before = if let Some(ref dur_str) = older_than {
        let dur = parse_duration(dur_str)
            .ok_or_else(|| anyhow::anyhow!("Invalid duration: {}", dur_str))?;
        Utc::now() - dur
    } else {
        bail!("Specify --older-than (e.g. 30d)");
    };

    let count = db.clean_sessions(before, agent.as_deref())?;
    println!("Cleaned {} sessions", count);

    Ok(())
}

// ── Report ──

fn cmd_report(
    day: bool,
    date: Option<String>,
    week: bool,
    month: bool,
    quarter: Option<String>,
    from: Option<String>,
    to: Option<String>,
    project: Option<String>,
    output: Option<String>,
    format: String,
    summarize: bool,
) -> Result<()> {
    let db = open_db()?;
    let config = cfg::load_config()?;

    let period = report::resolve_period(
        day,
        date.as_deref(),
        week,
        month,
        quarter.as_deref(),
        from.as_deref(),
        to.as_deref(),
    )?;

    // Run LLM summarization if --summarize flag or config enabled
    if summarize || config.report.summarize.enabled {
        let (from_dt, to_dt) = report::period_to_range(&period);
        let sessions = db.list_sessions(None, project.as_deref(), Some(from_dt), Some(to_dt), 1000)?;
        crate::core::summarize::summarize_sessions(&db, &sessions, &config.report.summarize)?;
    }

    let fmt = ReportFormat::from_str(&format);
    let report_content = report::generate_report(&db, &period, project.as_deref(), fmt)?;

    if let Some(ref out_path) = output {
        std::fs::write(out_path, &report_content)?;
        println!("Report saved to {}", out_path);
    } else {
        println!("{}", report_content);
    }

    Ok(())
}

// ── Export ──

fn cmd_export(session_id: &str, clipboard: bool, stdout: bool, detail: &str) -> Result<()> {
    let db = open_db()?;
    let detail_level = DetailLevel::from_str(detail);
    let content = context::export_context(&db, session_id, detail_level)?;

    if clipboard {
        let mut clip = arboard::Clipboard::new()?;
        clip.set_text(&content)?;
        println!("Context copied to clipboard");
    } else if stdout {
        print!("{}", content);
    } else {
        let path = ".ail-context.md";
        std::fs::write(path, &content)?;
        println!("Context exported to {}", path);
    }

    Ok(())
}

// ── Inject ──

fn cmd_inject(session_id: Option<String>, auto: bool) -> Result<()> {
    let db = open_db()?;

    if auto {
        let sid = context::auto_inject(&db)?;
        println!("Auto-injected context from session {} into CLAUDE.md", sid);
    } else if let Some(ref sid) = session_id {
        let cwd = std::env::current_dir()?;
        context::inject_context(&db, sid, &cwd)?;
        println!("Injected context from session {} into CLAUDE.md", sid);
    } else {
        bail!("Provide a session ID or use --auto");
    }

    Ok(())
}

// ── Serve ──

fn cmd_serve(mcp: bool) -> Result<()> {
    if mcp {
        mcp::server::run_mcp_server()?;
    } else {
        println!("MCP Server Setup Guide");
        println!("======================");
        println!();
        println!("Add to Claude Desktop (claude_desktop_config.json):");
        println!("  or Claude Code (~/.claude/mcp_servers.json):");
        println!();
        println!("{{");
        println!("  \"mcpServers\": {{");
        println!("    \"ail\": {{");
        println!("      \"command\": \"ail\",");
        println!("      \"args\": [\"serve\", \"--mcp\"]");
        println!("    }}");
        println!("  }}");
        println!("}}");
        println!();
        println!("Then restart Claude Desktop/Code.");
    }
    Ok(())
}

// ── Config ──

fn cmd_config(edit: bool) -> Result<()> {
    if edit {
        let path = cfg::config_path();
        if !path.exists() {
            let config = cfg::AilConfig::default();
            cfg::save_config(&config)?;
        }
        cfg::open_in_editor(&path)?;
    } else {
        let config = cfg::load_config()?;
        let content = toml::to_string_pretty(&config)?;
        println!("{}", content);
    }
    Ok(())
}

