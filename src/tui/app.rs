use crate::config;
use crate::core::context::{self, DetailLevel};
use crate::core::db::{Database, MessageRow, SessionRow, ToolCallRow};
use crate::tui::theme::Theme;
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap,
};
use ratatui::Terminal;
use std::io;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum View {
    SessionList,
    SessionDetail,
    HistorySearch,
    ActionMenu,
}

pub struct App {
    pub db: Database,
    pub theme: Theme,
    pub view: View,
    pub should_quit: bool,

    // Session list
    pub sessions: Vec<SessionRow>,
    pub filtered_indices: Vec<usize>,
    pub list_state: ListState,
    pub search_input: String,
    pub search_active: bool,

    // Filters
    pub agent_filter: Option<String>,
    pub agent_filter_idx: usize,
    pub period_filter: Option<String>,

    // Session detail
    pub detail_session_id: Option<String>,
    pub detail_messages: Vec<MessageRow>,
    pub detail_tool_calls: Vec<ToolCallRow>,
    pub detail_scroll: u16,

    // History search
    pub history_input: String,
    pub history_results: Vec<crate::core::db::SearchResult>,
    pub history_state: ListState,

    // Action menu
    pub action_items: Vec<String>,
    pub action_state: ListState,
}

const AGENTS: &[&str] = &["All", "claude-code", "codex", "cursor"];

impl App {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            theme: Theme::dark(),
            view: View::SessionList,
            should_quit: false,
            sessions: Vec::new(),
            filtered_indices: Vec::new(),
            list_state: ListState::default(),
            search_input: String::new(),
            search_active: false,
            agent_filter: None,
            agent_filter_idx: 0,
            period_filter: None,
            detail_session_id: None,
            detail_messages: Vec::new(),
            detail_tool_calls: Vec::new(),
            detail_scroll: 0,
            history_input: String::new(),
            history_results: Vec::new(),
            history_state: ListState::default(),
            action_items: vec![
                "Resume session".to_string(),
                "Export context".to_string(),
                "Open project directory".to_string(),
                "Search in session".to_string(),
                "Add tags".to_string(),
                "Delete session".to_string(),
            ],
            action_state: ListState::default(),
        }
    }

    pub fn load_sessions(&mut self) -> Result<()> {
        self.sessions = self.db.list_sessions(
            self.agent_filter.as_deref(),
            None,
            None,
            None,
            500,
        )?;
        self.apply_filter();
        if !self.filtered_indices.is_empty() {
            self.list_state.select(Some(0));
        }
        Ok(())
    }

    fn apply_filter(&mut self) {
        if self.search_input.is_empty() {
            self.filtered_indices = (0..self.sessions.len()).collect();
        } else {
            let matcher = SkimMatcherV2::default();
            let mut scored: Vec<(usize, i64)> = self
                .sessions
                .iter()
                .enumerate()
                .filter_map(|(i, s)| {
                    let text = format!(
                        "{} {} {} {}",
                        s.project_name.as_deref().unwrap_or(""),
                        s.summary.as_deref().unwrap_or(""),
                        s.agent,
                        s.tags
                    );
                    matcher
                        .fuzzy_match(&text, &self.search_input)
                        .map(|score| (i, score))
                })
                .collect();
            scored.sort_by(|a, b| b.1.cmp(&a.1));
            self.filtered_indices = scored.into_iter().map(|(i, _)| i).collect();
        }
    }

    fn selected_session(&self) -> Option<&SessionRow> {
        self.list_state
            .selected()
            .and_then(|i| self.filtered_indices.get(i))
            .and_then(|&idx| self.sessions.get(idx))
    }

    fn open_detail(&mut self) -> Result<()> {
        if let Some(session) = self.selected_session() {
            let sid = session.id.clone();
            self.detail_messages = self.db.get_messages(&sid)?;
            self.detail_tool_calls = self.db.get_tool_calls(&sid)?;
            self.detail_session_id = Some(sid);
            self.detail_scroll = 0;
            self.view = View::SessionDetail;
        }
        Ok(())
    }

    fn handle_key_session_list(&mut self, key: KeyEvent) -> Result<()> {
        if self.search_active {
            match key.code {
                KeyCode::Esc => {
                    self.search_active = false;
                }
                KeyCode::Enter => {
                    self.search_active = false;
                }
                KeyCode::Backspace => {
                    self.search_input.pop();
                    self.apply_filter();
                    if !self.filtered_indices.is_empty() {
                        self.list_state.select(Some(0));
                    }
                }
                KeyCode::Char(c) => {
                    self.search_input.push(c);
                    self.apply_filter();
                    if !self.filtered_indices.is_empty() {
                        self.list_state.select(Some(0));
                    }
                }
                _ => {}
            }
            return Ok(());
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
            }
            KeyCode::Char('/') => {
                self.search_active = true;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let len = self.filtered_indices.len();
                if len > 0 {
                    let i = self.list_state.selected().unwrap_or(0);
                    self.list_state.select(Some((i + 1).min(len - 1)));
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let i = self.list_state.selected().unwrap_or(0);
                self.list_state.select(Some(i.saturating_sub(1)));
            }
            KeyCode::Enter => {
                self.action_state.select(Some(0));
                self.view = View::ActionMenu;
            }
            KeyCode::Char('r') => {
                // Resume session
                if let Some(session) = self.selected_session() {
                    let cmd = format!("claude --resume {}", session.id);
                    self.should_quit = true;
                    // Store command for execution after TUI closes
                    std::env::set_var("AIL_RESUME_CMD", &cmd);
                }
            }
            KeyCode::Char('e') => {
                // Export context
                if let Some(session) = self.selected_session() {
                    let sid = session.id.clone();
                    if let Ok(ctx) = context::export_context(&self.db, &sid, DetailLevel::Summary) {
                        let path = ".ail-context.md";
                        let _ = std::fs::write(path, &ctx);
                    }
                }
            }
            KeyCode::Tab => {
                // Cycle agent filter
                self.agent_filter_idx = (self.agent_filter_idx + 1) % AGENTS.len();
                self.agent_filter = if self.agent_filter_idx == 0 {
                    None
                } else {
                    Some(AGENTS[self.agent_filter_idx].to_string())
                };
                self.load_sessions()?;
            }
            KeyCode::Char('d') => {
                self.open_detail()?;
            }
            KeyCode::Char('h') => {
                self.view = View::HistorySearch;
                self.history_input.clear();
                self.history_results.clear();
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_key_session_detail(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.view = View::SessionList;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.detail_scroll = self.detail_scroll.saturating_add(1);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.detail_scroll = self.detail_scroll.saturating_sub(1);
            }
            KeyCode::PageDown => {
                self.detail_scroll = self.detail_scroll.saturating_add(10);
            }
            KeyCode::PageUp => {
                self.detail_scroll = self.detail_scroll.saturating_sub(10);
            }
            KeyCode::Char('e') => {
                if let Some(ref sid) = self.detail_session_id {
                    if let Ok(ctx) = context::export_context(&self.db, sid, DetailLevel::Summary) {
                        let _ = std::fs::write(".ail-context.md", &ctx);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_key_history(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.view = View::SessionList;
            }
            KeyCode::Enter => {
                // Execute search
                if !self.history_input.is_empty() {
                    self.history_results = self.db.search_messages(
                        &self.history_input,
                        self.agent_filter.as_deref(),
                        None,
                        None,
                        None,
                        50,
                    )?;
                    if !self.history_results.is_empty() {
                        self.history_state.select(Some(0));
                    }
                }
            }
            KeyCode::Backspace => {
                self.history_input.pop();
            }
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let len = self.history_results.len();
                if len > 0 {
                    let i = self.history_state.selected().unwrap_or(0);
                    self.history_state.select(Some((i + 1).min(len - 1)));
                }
            }
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let i = self.history_state.selected().unwrap_or(0);
                self.history_state.select(Some(i.saturating_sub(1)));
            }
            KeyCode::Char(c) => {
                self.history_input.push(c);
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_key_action_menu(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.view = View::SessionList;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let len = self.action_items.len();
                let i = self.action_state.selected().unwrap_or(0);
                self.action_state.select(Some((i + 1).min(len - 1)));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let i = self.action_state.selected().unwrap_or(0);
                self.action_state.select(Some(i.saturating_sub(1)));
            }
            KeyCode::Enter => {
                let selected = self.action_state.selected().unwrap_or(0);
                match selected {
                    0 => {
                        // Resume
                        if let Some(session) = self.selected_session() {
                            std::env::set_var(
                                "AIL_RESUME_CMD",
                                format!("claude --resume {}", session.id),
                            );
                            self.should_quit = true;
                        }
                    }
                    1 => {
                        // Export
                        if let Some(session) = self.selected_session() {
                            let sid = session.id.clone();
                            if let Ok(ctx) =
                                context::export_context(&self.db, &sid, DetailLevel::Summary)
                            {
                                let _ = std::fs::write(".ail-context.md", &ctx);
                            }
                        }
                        self.view = View::SessionList;
                    }
                    2 => {
                        // Open project dir
                        if let Some(session) = self.selected_session() {
                            if let Some(ref p) = session.project_path {
                                std::env::set_var("AIL_CD_PATH", p);
                                self.should_quit = true;
                            }
                        }
                    }
                    3 => {
                        // Search in session
                        self.open_detail()?;
                    }
                    5 => {
                        // Delete
                        if let Some(session) = self.selected_session() {
                            let sid = session.id.clone();
                            self.db.delete_session(&sid)?;
                            self.load_sessions()?;
                        }
                        self.view = View::SessionList;
                    }
                    _ => {
                        self.view = View::SessionList;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub fn handle_event(&mut self) -> Result<()> {
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                // Global quit: Ctrl+C
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.should_quit = true;
                    return Ok(());
                }

                match self.view {
                    View::SessionList => self.handle_key_session_list(key)?,
                    View::SessionDetail => self.handle_key_session_detail(key)?,
                    View::HistorySearch => self.handle_key_history(key)?,
                    View::ActionMenu => self.handle_key_action_menu(key)?,
                }
            }
        }
        Ok(())
    }

    pub fn draw(&mut self, frame: &mut ratatui::Frame) {
        match self.view {
            View::SessionList => self.draw_session_list(frame),
            View::SessionDetail => self.draw_session_detail(frame),
            View::HistorySearch => self.draw_history_search(frame),
            View::ActionMenu => {
                self.draw_session_list(frame);
                self.draw_action_popup(frame);
            }
        }
    }

    fn draw_session_list(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // search bar
                Constraint::Min(5),   // main content
                Constraint::Length(2), // status bar
            ])
            .split(area);

        // Search bar
        let search_text = if self.search_active {
            format!(" Search: {}|", self.search_input)
        } else if self.search_input.is_empty() {
            " / to search".to_string()
        } else {
            format!(" Search: {}", self.search_input)
        };

        let agent_label = AGENTS[self.agent_filter_idx];
        let filter_line = format!(
            "{}    Agent: {}",
            search_text, agent_label
        );

        let search_bar = Paragraph::new(filter_line).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(self.theme.border_style())
                .title(Span::styled(" ail ", self.theme.title_style())),
        );
        frame.render_widget(search_bar, chunks[0]);

        // Main content: session list + preview
        let main_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(chunks[1]);

        // Session list
        let items: Vec<ListItem> = self
            .filtered_indices
            .iter()
            .map(|&idx| {
                let session = &self.sessions[idx];
                let agent_span = Span::styled(
                    format!(" {} ", session.agent),
                    self.theme.agent_style(&session.agent),
                );
                let project = session.project_name.as_deref().unwrap_or("?");
                let time_ago = session
                    .started_at
                    .as_ref()
                    .map(|t| format_time_ago(t))
                    .unwrap_or_default();
                let summary = session
                    .summary
                    .as_deref()
                    .unwrap_or("")
                    .chars()
                    .take(50)
                    .collect::<String>();
                let tags_str = if session.tags.is_empty() {
                    String::new()
                } else {
                    format!(
                        " {}",
                        session
                            .tags
                            .split(',')
                            .filter(|t| !t.is_empty())
                            .map(|t| format!("#{}", t))
                            .collect::<Vec<_>>()
                            .join(" ")
                    )
                };

                let line1 = Line::from(vec![
                    agent_span,
                    Span::raw(format!("  {}  ", project)),
                    Span::styled(time_ago, self.theme.muted_style()),
                ]);
                let line2 = Line::from(vec![
                    Span::raw(format!("  \"{}\"", summary)),
                    Span::styled(tags_str, self.theme.tag_style()),
                ]);

                ListItem::new(vec![line1, line2, Line::raw("")])
            })
            .collect();

        let count = self.filtered_indices.len();
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(self.theme.border_style())
                    .title(Span::styled(
                        format!(" SESSIONS  {}  ", count),
                        self.theme.title_style(),
                    )),
            )
            .highlight_style(self.theme.highlight_style());

        frame.render_stateful_widget(list, main_chunks[0], &mut self.list_state);

        // Preview panel
        self.draw_preview(frame, main_chunks[1]);

        // Status bar
        let help_text = if self.search_active {
            " Type to search | Enter: confirm | Esc: cancel"
        } else {
            " j/k: Navigate | Enter: Actions | /: Search | Tab: Agent | d: Detail | e: Export | r: Resume | h: History | q: Quit"
        };
        let status = Paragraph::new(help_text).style(self.theme.status_bar_style());
        frame.render_widget(status, chunks[2]);
    }

    fn draw_preview(&self, frame: &mut ratatui::Frame, area: Rect) {
        let session = self.selected_session();

        if let Some(session) = session {
            let mut lines: Vec<Line> = Vec::new();

            lines.push(Line::from(vec![
                Span::styled("Session: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(&session.id[..session.id.len().min(8)]),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Agent: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(
                    agent_display(&session.agent),
                    self.theme.agent_style(&session.agent),
                ),
            ]));
            if let Some(ref p) = session.project_path {
                lines.push(Line::from(vec![
                    Span::styled("Project: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(p.as_str()),
                ]));
            }
            if let Some(ref t) = session.started_at {
                let duration = format_duration_between(t, session.ended_at.as_deref());
                lines.push(Line::from(vec![
                    Span::styled("Time: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(format!("{} messages", session.message_count)),
                    Span::styled(format!("  {}", duration), self.theme.muted_style()),
                ]));
            }
            lines.push(Line::raw(""));

            // Files changed
            let tool_calls = self.db.get_tool_calls(&session.id).unwrap_or_default();
            if !tool_calls.is_empty() {
                lines.push(Line::styled(
                    "── Files Changed ──",
                    self.theme.muted_style(),
                ));
                let mut seen = std::collections::HashSet::new();
                for tc in &tool_calls {
                    if let Some(ref fp) = tc.file_path {
                        if seen.insert(fp.clone()) {
                            let (prefix, style) = match tc.tool_name.as_str() {
                                "Write" | "create_file" => {
                                    ("+ ", self.theme.file_created_style())
                                }
                                "Edit" | "edit_file" => {
                                    ("~ ", self.theme.file_modified_style())
                                }
                                "delete_file" => ("- ", self.theme.file_deleted_style()),
                                _ => ("  ", Style::default()),
                            };
                            let short = short_path(fp);
                            lines.push(Line::from(vec![Span::styled(
                                format!("{}{}", prefix, short),
                                style,
                            )]));
                        }
                    }
                }
                lines.push(Line::raw(""));
            }

            // Last exchange
            let messages = self.db.get_messages(&session.id).unwrap_or_default();
            let recent: Vec<&MessageRow> = messages
                .iter()
                .filter(|m| m.role == "user" || m.role == "assistant")
                .rev()
                .take(4)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();

            if !recent.is_empty() {
                lines.push(Line::styled(
                    "── Last Exchange ──",
                    self.theme.muted_style(),
                ));
                for msg in recent {
                    let (label, style) = if msg.role == "user" {
                        ("You: ", self.theme.user_role_style())
                    } else {
                        ("AI: ", self.theme.assistant_role_style())
                    };
                    let content: String = msg.content.chars().take(150).collect();
                    let content = content.replace('\n', " ");
                    lines.push(Line::from(vec![
                        Span::styled(label, style),
                        Span::raw(content),
                    ]));
                }
            }

            let preview = Paragraph::new(lines)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(self.theme.border_style())
                        .title(Span::styled(" PREVIEW ", self.theme.title_style())),
                )
                .wrap(Wrap { trim: true });
            frame.render_widget(preview, area);
        } else {
            let empty = Paragraph::new(" No session selected").block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(self.theme.border_style())
                    .title(Span::styled(" PREVIEW ", self.theme.title_style())),
            );
            frame.render_widget(empty, area);
        }
    }

    fn draw_session_detail(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(5),
                Constraint::Length(2),
            ])
            .split(area);

        // Header
        let sid = self.detail_session_id.as_deref().unwrap_or("?");
        let session = self.db.get_session(sid).ok().flatten();
        let header_text = if let Some(ref s) = session {
            format!(
                " {} | {} | {} messages",
                agent_display(&s.agent),
                s.project_name.as_deref().unwrap_or("?"),
                s.message_count
            )
        } else {
            format!(" Session: {}", sid)
        };

        let header = Paragraph::new(header_text).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(self.theme.border_style())
                .title(Span::styled(
                    format!(" Session: {} ", &sid[..sid.len().min(12)]),
                    self.theme.title_style(),
                )),
        );
        frame.render_widget(header, chunks[0]);

        // Messages
        let mut lines: Vec<Line> = Vec::new();
        for msg in &self.detail_messages {
            if msg.role == "tool" {
                continue;
            }
            let (icon, style) = if msg.role == "user" {
                ("You", self.theme.user_role_style())
            } else {
                ("AI", self.theme.assistant_role_style())
            };
            let ts = msg
                .timestamp
                .as_ref()
                .map(|t| {
                    chrono::DateTime::parse_from_rfc3339(t)
                        .map(|d| d.format("%H:%M").to_string())
                        .unwrap_or_default()
                })
                .unwrap_or_default();

            lines.push(Line::from(vec![
                Span::styled(format!("{} ", icon), style),
                Span::styled(ts, self.theme.muted_style()),
            ]));

            for text_line in msg.content.lines() {
                lines.push(Line::raw(format!("  {}", text_line)));
            }
            lines.push(Line::raw(""));
        }

        let content = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(self.theme.border_style()),
            )
            .scroll((self.detail_scroll, 0))
            .wrap(Wrap { trim: false });
        frame.render_widget(content, chunks[1]);

        // Status bar
        let status = Paragraph::new(
            " j/k: Scroll | PgUp/PgDn: Page | e: Export | Esc: Back",
        )
        .style(self.theme.status_bar_style());
        frame.render_widget(status, chunks[2]);
    }

    fn draw_history_search(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(5),
                Constraint::Length(2),
            ])
            .split(area);

        // Search input
        let input_text = format!(" Search: {}|", self.history_input);
        let search_bar = Paragraph::new(input_text).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(self.theme.border_style())
                .title(Span::styled(" History Search ", self.theme.title_style())),
        );
        frame.render_widget(search_bar, chunks[0]);

        // Results
        let count = self.history_results.len();
        let items: Vec<ListItem> = self
            .history_results
            .iter()
            .map(|r| {
                let role_icon = if r.role == "user" { "You" } else { "AI" };
                let content: String = r.content.chars().take(100).collect();
                let content = content.replace('\n', " ");

                let line1 = Line::from(vec![
                    Span::styled(
                        format!(" {} ", r.agent),
                        self.theme.agent_style(&r.agent),
                    ),
                    Span::raw(format!(
                        " {} ",
                        r.project_name.as_deref().unwrap_or("?")
                    )),
                    Span::styled(
                        r.session_id[..r.session_id.len().min(8)].to_string(),
                        self.theme.muted_style(),
                    ),
                ]);
                let line2 = Line::from(vec![
                    Span::styled(
                        format!("  {}: ", role_icon),
                        if r.role == "user" {
                            self.theme.user_role_style()
                        } else {
                            self.theme.assistant_role_style()
                        },
                    ),
                    Span::raw(content),
                ]);
                ListItem::new(vec![line1, line2, Line::raw("")])
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(self.theme.border_style())
                    .title(Span::styled(
                        format!(" Results ({}) ", count),
                        self.theme.title_style(),
                    )),
            )
            .highlight_style(self.theme.highlight_style());
        frame.render_stateful_widget(list, chunks[1], &mut self.history_state);

        let status = Paragraph::new(
            " Type query, Enter: Search | Ctrl+j/k: Navigate results | Esc: Back",
        )
        .style(self.theme.status_bar_style());
        frame.render_widget(status, chunks[2]);
    }

    fn draw_action_popup(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        // Center popup
        let popup_width = 30;
        let popup_height = (self.action_items.len() as u16) + 2;
        let x = area.width.saturating_sub(popup_width) / 2;
        let y = area.height.saturating_sub(popup_height) / 2;
        let popup_area = Rect::new(x, y, popup_width.min(area.width), popup_height.min(area.height));

        frame.render_widget(Clear, popup_area);

        let items: Vec<ListItem> = self
            .action_items
            .iter()
            .map(|a| ListItem::new(format!("  {}", a)))
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title(Span::styled(" Actions ", self.theme.title_style())),
            )
            .highlight_style(self.theme.highlight_style());

        frame.render_stateful_widget(list, popup_area, &mut self.action_state);
    }
}

pub fn run_tui() -> Result<()> {
    let db_path = config::db_path();

    // Auto-index on startup
    if db_path.exists() {
        // DB exists, just open it
    } else {
        // Create and index
        config::ensure_data_dir()?;
    }

    let db = Database::open(&db_path)?;

    // Quick auto-index
    let _ = crate::core::indexer::index_all(&db);

    let mut app = App::new(db);
    app.load_sessions()?;

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Main loop
    let result = (|| -> Result<()> {
        loop {
            terminal.draw(|f| app.draw(f))?;
            app.handle_event()?;
            if app.should_quit {
                break;
            }
        }
        Ok(())
    })();

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    result?;

    // Handle post-TUI actions
    if let Ok(cmd) = std::env::var("AIL_RESUME_CMD") {
        std::env::remove_var("AIL_RESUME_CMD");
        println!("Resuming session...");
        println!("Run: {}", cmd);
    }
    if let Ok(path) = std::env::var("AIL_CD_PATH") {
        std::env::remove_var("AIL_CD_PATH");
        println!("cd {}", path);
    }

    Ok(())
}

// Helper functions

fn agent_display(agent: &str) -> &str {
    match agent {
        "claude-code" => "Claude Code",
        "codex" => "Codex",
        "cursor" => "Cursor",
        _ => agent,
    }
}

fn short_path(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() <= 2 {
        path.to_string()
    } else {
        parts[parts.len() - 2..].join("/")
    }
}

fn format_time_ago(ts_str: &str) -> String {
    let ts = match chrono::DateTime::parse_from_rfc3339(ts_str) {
        Ok(t) => t.with_timezone(&chrono::Utc),
        Err(_) => return String::new(),
    };
    let now = chrono::Utc::now();
    let diff = now.signed_duration_since(ts);

    if diff.num_minutes() < 1 {
        "just now".to_string()
    } else if diff.num_hours() < 1 {
        format!("{}m", diff.num_minutes())
    } else if diff.num_days() < 1 {
        format!("{}h", diff.num_hours())
    } else if diff.num_weeks() < 1 {
        format!("{}d", diff.num_days())
    } else {
        format!("{}w", diff.num_weeks())
    }
}

fn format_duration_between(start_str: &str, end_str: Option<&str>) -> String {
    let start = match chrono::DateTime::parse_from_rfc3339(start_str) {
        Ok(t) => t,
        Err(_) => return String::new(),
    };

    let end = if let Some(e) = end_str {
        match chrono::DateTime::parse_from_rfc3339(e) {
            Ok(t) => t,
            Err(_) => return String::new(),
        }
    } else {
        return String::new();
    };

    let diff = end.signed_duration_since(start);
    if diff.num_hours() > 0 {
        format!("{}h{}m", diff.num_hours(), diff.num_minutes() % 60)
    } else {
        format!("{}min", diff.num_minutes())
    }
}
