use crate::core::db::{Database, SessionRow, Stats};
use anyhow::Result;
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, Utc};
use std::collections::HashMap;
use std::fmt::Write;

#[derive(Debug, Clone)]
pub enum ReportPeriod {
    Day(NaiveDate),
    Week(NaiveDate, NaiveDate),
    Month(i32, u32),
    Quarter(i32, u8),
    Custom(DateTime<Utc>, DateTime<Utc>),
}

#[derive(Debug, Clone, Copy)]
pub enum ReportFormat {
    Markdown,
    Slack,
    Json,
}

impl ReportFormat {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "slack" => ReportFormat::Slack,
            "json" => ReportFormat::Json,
            _ => ReportFormat::Markdown,
        }
    }
}

pub fn generate_report(
    db: &Database,
    period: &ReportPeriod,
    project: Option<&str>,
    format: ReportFormat,
) -> Result<String> {
    let (from, to) = period_to_range(period);
    let sessions = db.list_sessions(None, project, Some(from), Some(to), 1000)?;
    let stats = db.get_stats(Some(from), Some(to), project)?;

    match format {
        ReportFormat::Markdown => generate_markdown(&sessions, &stats, period, db),
        ReportFormat::Slack => generate_slack(&sessions, &stats, period, db),
        ReportFormat::Json => generate_json(&sessions, &stats, period),
    }
}

fn period_to_range(period: &ReportPeriod) -> (DateTime<Utc>, DateTime<Utc>) {
    match period {
        ReportPeriod::Day(date) => {
            let start = date.and_hms_opt(0, 0, 0).unwrap();
            let end = date.and_hms_opt(23, 59, 59).unwrap();
            (
                DateTime::<Utc>::from_naive_utc_and_offset(start, Utc),
                DateTime::<Utc>::from_naive_utc_and_offset(end, Utc),
            )
        }
        ReportPeriod::Week(start, end) => (
            DateTime::<Utc>::from_naive_utc_and_offset(start.and_hms_opt(0, 0, 0).unwrap(), Utc),
            DateTime::<Utc>::from_naive_utc_and_offset(end.and_hms_opt(23, 59, 59).unwrap(), Utc),
        ),
        ReportPeriod::Month(year, month) => {
            let start = NaiveDate::from_ymd_opt(*year, *month, 1).unwrap();
            let end = if *month == 12 {
                NaiveDate::from_ymd_opt(*year + 1, 1, 1).unwrap() - Duration::days(1)
            } else {
                NaiveDate::from_ymd_opt(*year, *month + 1, 1).unwrap() - Duration::days(1)
            };
            (
                DateTime::<Utc>::from_naive_utc_and_offset(start.and_hms_opt(0, 0, 0).unwrap(), Utc),
                DateTime::<Utc>::from_naive_utc_and_offset(end.and_hms_opt(23, 59, 59).unwrap(), Utc),
            )
        }
        ReportPeriod::Quarter(year, quarter) => {
            let start_month = (quarter - 1) * 3 + 1;
            let end_month = start_month + 2;
            let start = NaiveDate::from_ymd_opt(*year, start_month as u32, 1).unwrap();
            let end = if end_month == 12 {
                NaiveDate::from_ymd_opt(*year + 1, 1, 1).unwrap() - Duration::days(1)
            } else {
                NaiveDate::from_ymd_opt(*year, end_month as u32 + 1, 1).unwrap() - Duration::days(1)
            };
            (
                DateTime::<Utc>::from_naive_utc_and_offset(start.and_hms_opt(0, 0, 0).unwrap(), Utc),
                DateTime::<Utc>::from_naive_utc_and_offset(end.and_hms_opt(23, 59, 59).unwrap(), Utc),
            )
        }
        ReportPeriod::Custom(from, to) => (*from, *to),
    }
}

fn generate_markdown(
    sessions: &[SessionRow],
    stats: &Stats,
    period: &ReportPeriod,
    db: &Database,
) -> Result<String> {
    let mut out = String::new();
    let (_from, _to) = period_to_range(period);

    // Title
    writeln!(out, "# AI Work Report ({})", period_label(period))?;
    writeln!(out)?;

    // Summary stats
    writeln!(out, "## Summary")?;
    writeln!(
        out,
        "- Total: {} sessions, {} projects",
        stats.total_sessions,
        stats.sessions_by_project.len()
    )?;
    for (agent, count) in &stats.sessions_by_agent {
        writeln!(out, "- {}: {} sessions", agent_display(agent), count)?;
    }
    writeln!(
        out,
        "- Files: {} created, {} modified, {} deleted",
        stats.total_files_created, stats.total_files_modified, stats.total_files_deleted
    )?;
    writeln!(out)?;

    // Group sessions by project
    let mut by_project: HashMap<String, Vec<&SessionRow>> = HashMap::new();
    for session in sessions {
        let project = session
            .project_name
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        by_project.entry(project).or_default().push(session);
    }

    writeln!(out, "## Activity by Project")?;
    writeln!(out)?;

    for (project, project_sessions) in &by_project {
        writeln!(out, "### {} ({} sessions)", project, project_sessions.len())?;
        writeln!(out)?;
        writeln!(out, "| Request | AI Work Summary | Changes |")?;
        writeln!(out, "|---------|----------------|---------|")?;

        for session in project_sessions {
            let request = session
                .summary
                .as_deref()
                .unwrap_or("-")
                .chars()
                .take(60)
                .collect::<String>();
            let work = session
                .work_summary
                .as_deref()
                .unwrap_or("-")
                .chars()
                .take(80)
                .collect::<String>();

            // Get file changes
            let file_changes = get_session_file_changes(db, &session.id);
            let changes_str = file_changes
                .iter()
                .map(|(path, prefix)| format!("{}{}", prefix, short_path(path)))
                .collect::<Vec<_>>()
                .join(" ");

            writeln!(out, "| {} | {} | {} |", request, work, changes_str)?;
        }

        // Project file totals
        let project_created: i64 = project_sessions.iter().map(|s| s.files_created).sum();
        let project_modified: i64 = project_sessions.iter().map(|s| s.files_modified).sum();
        writeln!(out)?;
        writeln!(
            out,
            "Session total: {} created, {} modified",
            project_created, project_modified
        )?;
        writeln!(out)?;
    }

    Ok(out)
}

fn generate_slack(
    sessions: &[SessionRow],
    stats: &Stats,
    period: &ReportPeriod,
    _db: &Database,
) -> Result<String> {
    let mut out = String::new();

    writeln!(out, "*AI Work Report ({})*", period_label(period))?;
    writeln!(out)?;
    writeln!(
        out,
        "> {} sessions across {} projects",
        stats.total_sessions,
        stats.sessions_by_project.len()
    )?;
    for (agent, count) in &stats.sessions_by_agent {
        writeln!(out, "> {} {} sessions", agent_display(agent), count)?;
    }
    writeln!(out)?;

    let mut by_project: HashMap<String, Vec<&SessionRow>> = HashMap::new();
    for session in sessions {
        let project = session
            .project_name
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        by_project.entry(project).or_default().push(session);
    }

    for (project, project_sessions) in &by_project {
        writeln!(out, "*{}* ({} sessions)", project, project_sessions.len())?;
        for session in project_sessions {
            let request = session.summary.as_deref().unwrap_or("-");
            let work = session.work_summary.as_deref().unwrap_or("-");
            writeln!(out, "  - {} → {}", request, work)?;
        }
        writeln!(out)?;
    }

    Ok(out)
}

fn generate_json(
    sessions: &[SessionRow],
    stats: &Stats,
    period: &ReportPeriod,
) -> Result<String> {
    let (from, to) = period_to_range(period);

    let report = serde_json::json!({
        "period": {
            "label": period_label(period),
            "from": from.to_rfc3339(),
            "to": to.to_rfc3339(),
        },
        "stats": {
            "total_sessions": stats.total_sessions,
            "sessions_by_agent": stats.sessions_by_agent,
            "sessions_by_project": stats.sessions_by_project,
            "files_created": stats.total_files_created,
            "files_modified": stats.total_files_modified,
            "files_deleted": stats.total_files_deleted,
        },
        "sessions": sessions.iter().map(|s| serde_json::json!({
            "id": s.id,
            "agent": s.agent,
            "project": s.project_name,
            "summary": s.summary,
            "work_summary": s.work_summary,
            "started_at": s.started_at,
            "files_created": s.files_created,
            "files_modified": s.files_modified,
            "files_deleted": s.files_deleted,
            "tags": s.tags,
        })).collect::<Vec<_>>(),
    });

    Ok(serde_json::to_string_pretty(&report)?)
}

fn get_session_file_changes(db: &Database, session_id: &str) -> Vec<(String, &'static str)> {
    let tool_calls = db.get_tool_calls(session_id).unwrap_or_default();
    let mut files = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for tc in &tool_calls {
        if let Some(ref fp) = tc.file_path {
            if seen.insert(fp.clone()) {
                let prefix = match tc.tool_name.as_str() {
                    "Write" | "create_file" => "+",
                    "Edit" | "edit_file" => "~",
                    "delete_file" => "-",
                    _ => "~",
                };
                files.push((fp.clone(), prefix));
            }
        }
    }
    files
}

fn short_path(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() <= 1 {
        path.to_string()
    } else {
        parts.last().unwrap_or(&path).to_string()
    }
}

fn agent_display(agent: &str) -> &str {
    match agent {
        "claude-code" => "Claude Code",
        "codex" => "Codex",
        "cursor" => "Cursor",
        _ => agent,
    }
}

fn period_label(period: &ReportPeriod) -> String {
    match period {
        ReportPeriod::Day(date) => date.format("%Y-%m-%d").to_string(),
        ReportPeriod::Week(start, end) => {
            format!("{} ~ {}", start.format("%Y.%m.%d"), end.format("%m.%d"))
        }
        ReportPeriod::Month(year, month) => format!("{}-{:02}", year, month),
        ReportPeriod::Quarter(year, q) => format!("{} Q{}", year, q),
        ReportPeriod::Custom(from, to) => {
            format!(
                "{} ~ {}",
                from.format("%Y.%m.%d"),
                to.format("%m.%d")
            )
        }
    }
}

pub fn resolve_period(
    day: bool,
    date: Option<&str>,
    week: bool,
    month: bool,
    quarter: Option<&str>,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<ReportPeriod> {
    let today = Local::now().date_naive();

    if let (Some(from_str), Some(to_str)) = (from, to) {
        let from_dt = crate::core::db::parse_datetime(from_str)
            .ok_or_else(|| anyhow::anyhow!("Invalid --from date: {}", from_str))?;
        let to_dt = crate::core::db::parse_datetime(to_str)
            .ok_or_else(|| anyhow::anyhow!("Invalid --to date: {}", to_str))?;
        return Ok(ReportPeriod::Custom(from_dt, to_dt));
    }

    if let Some(q) = quarter {
        let q_num: u8 = q
            .trim_start_matches(|c: char| !c.is_ascii_digit())
            .parse()
            .unwrap_or(1);
        return Ok(ReportPeriod::Quarter(today.year(), q_num));
    }

    if month {
        return Ok(ReportPeriod::Month(today.year(), today.month()));
    }

    if week {
        let weekday = today.weekday().num_days_from_monday();
        let start = today - Duration::days(weekday as i64);
        let end = start + Duration::days(6);
        return Ok(ReportPeriod::Week(start, end));
    }

    if day {
        let d = if let Some(date_str) = date {
            NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                .map_err(|_| anyhow::anyhow!("Invalid date format: {}", date_str))?
        } else {
            today
        };
        return Ok(ReportPeriod::Day(d));
    }

    // Default to weekly
    let weekday = today.weekday().num_days_from_monday();
    let start = today - Duration::days(weekday as i64);
    let end = start + Duration::days(6);
    Ok(ReportPeriod::Week(start, end))
}
