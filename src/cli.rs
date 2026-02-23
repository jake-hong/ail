use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "ail", about = "AI Log — AI development activity intelligence", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Output as JSON
    #[arg(long, global = true)]
    pub json: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initial setup: detect agents and index sessions
    Setup,

    /// List sessions
    List {
        /// Filter by agent (claude-code, codex, cursor)
        #[arg(short, long)]
        agent: Option<String>,

        /// Filter by project path
        #[arg(short, long)]
        project: Option<String>,

        /// Filter by time period (e.g. 7d, 2w, 1m)
        #[arg(long)]
        last: Option<String>,

        /// Fuzzy search query
        #[arg(short, long)]
        query: Option<String>,
    },

    /// Resume a session
    Resume {
        /// Session ID to resume
        session_id: Option<String>,

        /// Resume the most recent session
        #[arg(long)]
        last: bool,

        /// Filter by agent when using --last
        #[arg(short, long)]
        agent: Option<String>,

        /// Context file to inject
        #[arg(long)]
        context: Option<String>,
    },

    /// Change to a session's project directory
    Cd {
        /// Session ID
        session_id: String,
    },

    /// Search conversation history
    History {
        /// Keyword search (FTS)
        #[arg(short = 'k', long)]
        keyword: Option<String>,

        /// Filter by agent
        #[arg(short, long)]
        agent: Option<String>,

        /// Filter by project path
        #[arg(short, long)]
        project: Option<String>,

        /// Filter by time period
        #[arg(long)]
        last: Option<String>,

        /// Search by file path
        #[arg(long)]
        file: Option<String>,
    },

    /// Show full session conversation
    Show {
        /// Session ID
        session_id: String,

        /// Show only changed files
        #[arg(long)]
        files: bool,
    },

    /// Manage session tags
    Tag {
        /// Session ID
        session_id: String,

        /// Tags to add
        tags: Vec<String>,

        /// Remove tags instead of adding
        #[arg(long)]
        remove: bool,
    },

    /// Clean old sessions
    Clean {
        /// Remove sessions older than duration (e.g. 30d, 4w)
        #[arg(long)]
        older_than: Option<String>,

        /// Filter by agent
        #[arg(short, long)]
        agent: Option<String>,

        /// Interactive mode
        #[arg(long)]
        interactive: bool,
    },

    /// Generate work reports
    Report {
        /// Daily report
        #[arg(long)]
        day: bool,

        /// Specific date for daily report (YYYY-MM-DD)
        #[arg(long)]
        date: Option<String>,

        /// Weekly report
        #[arg(long)]
        week: bool,

        /// Monthly report
        #[arg(long)]
        month: bool,

        /// Quarterly report (Q1, Q2, Q3, Q4)
        #[arg(long)]
        quarter: Option<String>,

        /// Custom range start
        #[arg(long)]
        from: Option<String>,

        /// Custom range end
        #[arg(long)]
        to: Option<String>,

        /// Filter by project
        #[arg(short, long)]
        project: Option<String>,

        /// Output to file
        #[arg(short, long)]
        output: Option<String>,

        /// Output format (markdown, slack, json)
        #[arg(long, default_value = "markdown")]
        format: String,
    },

    /// Export session context
    Export {
        /// Session ID
        session_id: String,

        /// Copy to clipboard
        #[arg(long)]
        clipboard: bool,

        /// Output to stdout
        #[arg(long)]
        stdout: bool,

        /// Detail level (full, summary, minimal)
        #[arg(long, default_value = "summary")]
        detail: String,
    },

    /// Inject context into CLAUDE.md
    Inject {
        /// Session ID
        session_id: Option<String>,

        /// Auto-inject latest context for current project
        #[arg(long)]
        auto: bool,
    },

    /// Rebuild or update the index
    Index {
        /// Only index specific agent
        #[arg(long)]
        agent: Option<String>,

        /// Full rebuild
        #[arg(long)]
        rebuild: bool,
    },

    /// Start MCP server or show MCP setup guide
    Serve {
        /// Start MCP server (stdio transport)
        #[arg(long)]
        mcp: bool,
    },

    /// View or edit configuration
    Config {
        /// Open config in editor
        #[arg(long)]
        edit: bool,
    },
}
