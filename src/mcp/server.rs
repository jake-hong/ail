use crate::config;
use crate::core::context::{self, DetailLevel};
use crate::core::db::Database;
use anyhow::Result;
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

pub fn run_mcp_server() -> Result<()> {
    let db_path = config::db_path();
    let db = Database::open(&db_path)?;

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    // MCP uses Content-Length framed JSON-RPC over stdio
    let reader = stdin.lock();
    let mut buf_reader = io::BufReader::new(reader);

    loop {
        // Read Content-Length header
        let mut header = String::new();
        loop {
            header.clear();
            let bytes_read = buf_reader.read_line(&mut header)?;
            if bytes_read == 0 {
                return Ok(()); // EOF
            }
            let trimmed = header.trim();
            if trimmed.is_empty() {
                break; // End of headers
            }
        }

        // Read content-length from previous headers
        // Simple approach: try to read a line as JSON directly
        let mut line = String::new();
        let bytes_read = buf_reader.read_line(&mut line)?;
        if bytes_read == 0 {
            return Ok(());
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Parse JSON-RPC request
        let request: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let id = request.get("id").cloned();
        let method = request
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or("");
        let params = request.get("params").cloned().unwrap_or(json!({}));

        let response = match method {
            "initialize" => handle_initialize(id.clone()),
            "tools/list" => handle_tools_list(id.clone()),
            "tools/call" => handle_tools_call(id.clone(), &params, &db),
            "notifications/initialized" => continue, // No response needed
            "ping" => json!({ "jsonrpc": "2.0", "id": id, "result": {} }),
            _ => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": format!("Method not found: {}", method) }
            }),
        };

        let response_str = serde_json::to_string(&response)?;
        let content_length = response_str.len();
        write!(
            stdout,
            "Content-Length: {}\r\n\r\n{}",
            content_length, response_str
        )?;
        stdout.flush()?;
    }
}

fn handle_initialize(id: Option<Value>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "ail",
                "version": env!("CARGO_PKG_VERSION")
            }
        }
    })
}

fn handle_tools_list(id: Option<Value>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "tools": [
                {
                    "name": "search_sessions",
                    "description": "Search AI coding sessions by keyword, agent, date range, and project",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "keyword": { "type": "string", "description": "Search keyword" },
                            "agent": { "type": "string", "description": "Agent filter: claude-code, codex, cursor" },
                            "from": { "type": "string", "description": "Start date (ISO 8601)" },
                            "to": { "type": "string", "description": "End date (ISO 8601)" },
                            "project": { "type": "string", "description": "Project path filter" },
                            "limit": { "type": "integer", "description": "Max results (default 20)" }
                        }
                    }
                },
                {
                    "name": "get_session_history",
                    "description": "Get the full conversation history of a specific session",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "session_id": { "type": "string", "description": "Session ID" }
                        },
                        "required": ["session_id"]
                    }
                },
                {
                    "name": "get_changed_files",
                    "description": "Get list of files changed in a session with change types",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "session_id": { "type": "string", "description": "Session ID" }
                        },
                        "required": ["session_id"]
                    }
                },
                {
                    "name": "get_session_summary",
                    "description": "Get session metadata: agent, project, time, message count, etc.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "session_id": { "type": "string", "description": "Session ID" }
                        },
                        "required": ["session_id"]
                    }
                },
                {
                    "name": "get_stats",
                    "description": "Get statistics for a time period: session count, by agent, by project, file changes",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "from": { "type": "string", "description": "Start date (ISO 8601)" },
                            "to": { "type": "string", "description": "End date (ISO 8601)" },
                            "project": { "type": "string", "description": "Project path filter" }
                        }
                    }
                },
                {
                    "name": "export_context",
                    "description": "Export session context as markdown for sharing between agents",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "session_id": { "type": "string", "description": "Session ID" },
                            "detail": { "type": "string", "description": "Detail level: full, summary, minimal" }
                        },
                        "required": ["session_id"]
                    }
                },
                {
                    "name": "get_full_session",
                    "description": "Get full untruncated session content for summarization. Use this to get complete session messages, file changes, and metadata so the calling agent can generate its own summary.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "session_id": { "type": "string", "description": "Session ID" }
                        },
                        "required": ["session_id"]
                    }
                }
            ]
        }
    })
}

fn handle_tools_call(id: Option<Value>, params: &Value, db: &Database) -> Value {
    let tool_name = params
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("");
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

    let result = match tool_name {
        "search_sessions" => tool_search_sessions(&arguments, db),
        "get_session_history" => tool_get_session_history(&arguments, db),
        "get_changed_files" => tool_get_changed_files(&arguments, db),
        "get_session_summary" => tool_get_session_summary(&arguments, db),
        "get_stats" => tool_get_stats(&arguments, db),
        "export_context" => tool_export_context(&arguments, db),
        "get_full_session" => tool_get_full_session(&arguments, db),
        _ => Err(anyhow::anyhow!("Unknown tool: {}", tool_name)),
    };

    match result {
        Ok(content) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "content": [{
                    "type": "text",
                    "text": content
                }]
            }
        }),
        Err(e) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "content": [{
                    "type": "text",
                    "text": format!("Error: {}", e)
                }],
                "isError": true
            }
        }),
    }
}

fn tool_search_sessions(args: &Value, db: &Database) -> Result<String> {
    let keyword = args.get("keyword").and_then(|k| k.as_str());
    let agent = args.get("agent").and_then(|a| a.as_str());
    let from = args
        .get("from")
        .and_then(|f| f.as_str())
        .and_then(crate::core::db::parse_datetime);
    let to = args
        .get("to")
        .and_then(|t| t.as_str())
        .and_then(crate::core::db::parse_datetime);
    let project = args.get("project").and_then(|p| p.as_str());
    let limit = args
        .get("limit")
        .and_then(|l| l.as_u64())
        .unwrap_or(20) as usize;

    if let Some(kw) = keyword {
        let results = db.search_messages(kw, agent, project, from, to, limit)?;
        let output: Vec<Value> = results
            .iter()
            .map(|r| {
                json!({
                    "session_id": r.session_id,
                    "agent": r.agent,
                    "project": r.project_name,
                    "role": r.role,
                    "content_preview": r.content.chars().take(200).collect::<String>(),
                    "started_at": r.started_at,
                })
            })
            .collect();
        Ok(serde_json::to_string_pretty(&output)?)
    } else {
        let sessions = db.list_sessions(agent, project, from, to, limit)?;
        let output: Vec<Value> = sessions
            .iter()
            .map(|s| {
                json!({
                    "id": s.id,
                    "agent": s.agent,
                    "project": s.project_name,
                    "summary": s.summary,
                    "started_at": s.started_at,
                    "message_count": s.message_count,
                })
            })
            .collect();
        Ok(serde_json::to_string_pretty(&output)?)
    }
}

fn tool_get_session_history(args: &Value, db: &Database) -> Result<String> {
    let session_id = args
        .get("session_id")
        .and_then(|s| s.as_str())
        .ok_or_else(|| anyhow::anyhow!("session_id is required"))?;

    let messages = db.get_messages(session_id)?;
    let output: Vec<Value> = messages
        .iter()
        .filter(|m| m.role != "tool")
        .map(|m| {
            json!({
                "role": m.role,
                "content": m.content,
                "timestamp": m.timestamp,
            })
        })
        .collect();
    Ok(serde_json::to_string_pretty(&output)?)
}

fn tool_get_changed_files(args: &Value, db: &Database) -> Result<String> {
    let session_id = args
        .get("session_id")
        .and_then(|s| s.as_str())
        .ok_or_else(|| anyhow::anyhow!("session_id is required"))?;

    let tool_calls = db.get_tool_calls(session_id)?;
    let mut files = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for tc in &tool_calls {
        if let Some(ref fp) = tc.file_path {
            if seen.insert(fp.clone()) {
                let change_type = match tc.tool_name.as_str() {
                    "Write" | "create_file" => "created",
                    "Edit" | "edit_file" => "modified",
                    "delete_file" => "deleted",
                    _ => "other",
                };
                files.push(json!({
                    "path": fp,
                    "change_type": change_type,
                }));
            }
        }
    }

    Ok(serde_json::to_string_pretty(&files)?)
}

fn tool_get_session_summary(args: &Value, db: &Database) -> Result<String> {
    let session_id = args
        .get("session_id")
        .and_then(|s| s.as_str())
        .ok_or_else(|| anyhow::anyhow!("session_id is required"))?;

    let session = db
        .get_session(session_id)?
        .ok_or_else(|| anyhow::anyhow!("Session not found: {}", session_id))?;

    let output = json!({
        "id": session.id,
        "agent": session.agent,
        "project_path": session.project_path,
        "project_name": session.project_name,
        "summary": session.summary,
        "work_summary": session.work_summary,
        "llm_summary": session.llm_summary,
        "started_at": session.started_at,
        "ended_at": session.ended_at,
        "message_count": session.message_count,
        "files_created": session.files_created,
        "files_modified": session.files_modified,
        "files_deleted": session.files_deleted,
        "tags": session.tags,
    });

    Ok(serde_json::to_string_pretty(&output)?)
}

fn tool_get_stats(args: &Value, db: &Database) -> Result<String> {
    let from = args
        .get("from")
        .and_then(|f| f.as_str())
        .and_then(crate::core::db::parse_datetime);
    let to = args
        .get("to")
        .and_then(|t| t.as_str())
        .and_then(crate::core::db::parse_datetime);
    let project = args.get("project").and_then(|p| p.as_str());

    let stats = db.get_stats(from, to, project)?;

    let output = json!({
        "total_sessions": stats.total_sessions,
        "sessions_by_agent": stats.sessions_by_agent,
        "sessions_by_project": stats.sessions_by_project,
        "files_created": stats.total_files_created,
        "files_modified": stats.total_files_modified,
        "files_deleted": stats.total_files_deleted,
        "most_modified_files": stats.most_modified_files,
    });

    Ok(serde_json::to_string_pretty(&output)?)
}

fn tool_get_full_session(args: &Value, db: &Database) -> Result<String> {
    let session_id = args
        .get("session_id")
        .and_then(|s| s.as_str())
        .ok_or_else(|| anyhow::anyhow!("session_id is required"))?;

    let session = db
        .get_session(session_id)?
        .ok_or_else(|| anyhow::anyhow!("Session not found: {}", session_id))?;

    let messages = db.get_messages(session_id)?;
    let tool_calls = db.get_tool_calls(session_id)?;

    // Build full untruncated messages
    let full_messages: Vec<Value> = messages
        .iter()
        .filter(|m| m.role != "tool")
        .map(|m| {
            json!({
                "role": m.role,
                "content": m.content,
                "timestamp": m.timestamp,
            })
        })
        .collect();

    // Build file changes
    let mut files = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for tc in &tool_calls {
        if let Some(ref fp) = tc.file_path {
            if seen.insert(fp.clone()) {
                let change_type = match tc.tool_name.as_str() {
                    "Write" | "create_file" => "created",
                    "Edit" | "edit_file" => "modified",
                    "delete_file" => "deleted",
                    _ => "other",
                };
                files.push(json!({ "path": fp, "change_type": change_type }));
            }
        }
    }

    let output = json!({
        "id": session.id,
        "agent": session.agent,
        "project_path": session.project_path,
        "project_name": session.project_name,
        "summary": session.summary,
        "work_summary": session.work_summary,
        "llm_summary": session.llm_summary,
        "started_at": session.started_at,
        "ended_at": session.ended_at,
        "message_count": session.message_count,
        "messages": full_messages,
        "files_changed": files,
        "tags": session.tags,
    });

    Ok(serde_json::to_string_pretty(&output)?)
}

fn tool_export_context(args: &Value, db: &Database) -> Result<String> {
    let session_id = args
        .get("session_id")
        .and_then(|s| s.as_str())
        .ok_or_else(|| anyhow::anyhow!("session_id is required"))?;

    let detail = args
        .get("detail")
        .and_then(|d| d.as_str())
        .unwrap_or("summary");

    context::export_context(db, session_id, DetailLevel::from_str(detail))
}
