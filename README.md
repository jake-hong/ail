# ail

A unified CLI/TUI for managing AI coding agent sessions across Claude Code, Codex, and Cursor.

ail indexes your local AI session data into a single SQLite database with full-text search, letting you browse, search, export, and report on all your AI-assisted work from one place. No API keys required — everything runs locally on your machine.

---

## Why

If you use multiple AI coding agents, your session history is scattered across different directories and formats. ail brings it all together:

- **One interface** for Claude Code, Codex, and Cursor sessions
- **Full-text search** across all conversations with FTS5
- **Context sharing** between agents via `.ail-context.md` export
- **Work reports** for daily/weekly/monthly summaries — no LLM needed
- **TUI browser** with fuzzy filtering, preview panels, and keyboard-driven navigation
- **MCP server** so AI agents can query your session history programmatically

---

## Install

### From source

```bash
# Requires Rust 1.70+
git clone https://github.com/sungeun/ail.git
cd ail
cargo install --path .
```

### Build manually

```bash
cargo build --release
# Binary at target/release/ail
```

---

## Quick Start

```bash
# 1. Detect installed agents and index all sessions
ail setup

# 2. Browse sessions in the TUI
ail

# 3. Search conversation history
ail history -k "authentication"

# 4. Generate a weekly work report
ail report --week
```

---

## Usage

### Session Management

```bash
# List all sessions (most recent first)
ail list

# Filter by agent
ail list --agent claude-code
ail list --agent codex

# Filter by time
ail list --last 7d          # last 7 days
ail list --last 3h          # last 3 hours
ail list --from 2025-01-01 --to 2025-01-31

# Filter by project
ail list --project my-app

# Show session detail
ail show <session-id>
ail show <session-id> --files    # include file changes
ail show <session-id> --full     # full conversation

# Resume a session in its original agent
ail resume <session-id>

# Open a terminal at the session's project directory
ail cd <session-id>
```

### Search

```bash
# Full-text search across all conversations
ail history -k "database migration"

# Search with filters
ail history -k "auth" --agent claude-code --last 30d

# Search by file path
ail history --file src/main.rs

# JSON output for scripting
ail history -k "deploy" --json
```

### Context Sharing

Export a session's context so another agent can pick up where you left off:

```bash
# Export context to .ail-context.md
ail export <session-id>
ail export <session-id> --level full      # full conversation
ail export <session-id> --level summary   # summary only
ail export <session-id> --level minimal   # just the essentials
ail export <session-id> --stdout          # print to terminal

# Inject context into CLAUDE.md for the current project
ail inject <session-id>

# Auto-inject: find the latest session for the current directory
ail inject --auto
```

### Reports

Generate work reports without any LLM — uses rule-based extraction:

```bash
# Weekly report (default)
ail report

# Specific periods
ail report --day
ail report --day --date 2025-01-15
ail report --week
ail report --month
ail report --quarter Q1

# Custom range
ail report --from 2025-01-01 --to 2025-01-31

# Output formats
ail report --week --format markdown   # default
ail report --week --format slack      # Slack-friendly
ail report --week --format json       # machine-readable

# Filter by project
ail report --week --project my-app
```

### Tags and Cleanup

```bash
# Tag sessions for organization
ail tag <session-id> "feature" "v2"

# Clean old sessions
ail clean --before 2024-01-01
ail clean --agent codex

# Re-index everything
ail index --rebuild
```

### MCP Server

Start an MCP server so AI agents can query your session history:

```bash
ail serve
```

Available MCP tools:
- `search_sessions` — search sessions by keyword, agent, project
- `get_session_history` — get full conversation for a session
- `get_changed_files` — list files modified in a session
- `get_session_summary` — get session summary and metadata
- `get_stats` — aggregate statistics
- `export_context` — generate context markdown

### Configuration

```bash
# Show current config
ail config --show

# Set values
ail config --set db_path ~/.ail/custom.db
ail config --set default_agent claude-code
```

Config file location: `~/.config/ail/config.toml`

---

## TUI

Launch the interactive terminal UI with just `ail` (no subcommand):

```
ail
```

### Keybindings

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate up/down |
| `Enter` | Open session / select action |
| `/` | Start fuzzy search |
| `Tab` | Cycle agent filter (All / Claude / Codex / Cursor) |
| `d` | Session detail view |
| `e` | Export context |
| `r` | Resume session |
| `h` | History search (FTS) |
| `t` | Tag session |
| `q` / `Esc` | Back / quit |

### Layout

The TUI has a 3-panel layout:

- **Left panel** — Session list with agent indicators and timestamps
- **Right panel** — Preview of selected session (first user message + work summary)
- **Bottom bar** — Search input, filters, and status

---

## Supported Agents

| Agent | Data Location | Session Format |
|-------|--------------|----------------|
| Claude Code | `~/.claude/projects/` | JSONL per session |
| Codex | `~/.codex/sessions/` | JSONL / JSON |
| Cursor | `~/.cursor/projects/` | JSON / JSONL |

---

## How It Works

1. **Scan** — Adapters read each agent's native session files from disk
2. **Index** — Sessions, messages, and tool calls are stored in SQLite with FTS5 indexes
3. **Query** — CLI commands and TUI views query the database with full-text search
4. **Export** — Context can be exported as markdown and injected into other agents' config files

The database is stored at `~/.local/share/ail/ail.db` by default. All data stays local.

---

## Project Structure

```
src/
  adapters/       # Agent-specific parsers (Claude Code, Codex, Cursor)
  core/           # Database, indexer, search, context, reports
  mcp/            # MCP JSON-RPC server
  tui/            # Terminal UI (ratatui)
  cli.rs          # Command definitions (clap)
  config.rs       # TOML configuration
  main.rs         # Entry point and command handlers
```

---

## Requirements

- Rust 1.70+ (build)
- SQLite (bundled via rusqlite)
- At least one supported AI agent installed

---

## License

MIT
