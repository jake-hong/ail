use ratatui::style::{Color, Modifier, Style};

pub struct Theme {
    pub bg: Color,
    pub fg: Color,
    pub accent: Color,
    pub highlight_bg: Color,
    pub highlight_fg: Color,
    pub muted: Color,
    pub border: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub agent_claude: Color,
    pub agent_codex: Color,
    pub agent_cursor: Color,
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            bg: Color::Reset,
            fg: Color::White,
            accent: Color::Cyan,
            highlight_bg: Color::DarkGray,
            highlight_fg: Color::White,
            muted: Color::Gray,
            border: Color::DarkGray,
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,
            agent_claude: Color::Rgb(204, 120, 50),
            agent_codex: Color::Green,
            agent_cursor: Color::Blue,
        }
    }

    pub fn title_style(&self) -> Style {
        Style::default().fg(self.accent).add_modifier(Modifier::BOLD)
    }

    pub fn highlight_style(&self) -> Style {
        Style::default()
            .bg(self.highlight_bg)
            .fg(self.highlight_fg)
            .add_modifier(Modifier::BOLD)
    }

    pub fn muted_style(&self) -> Style {
        Style::default().fg(self.muted)
    }

    pub fn border_style(&self) -> Style {
        Style::default().fg(self.border)
    }

    pub fn agent_style(&self, agent: &str) -> Style {
        let color = match agent {
            "claude-code" => self.agent_claude,
            "codex" => self.agent_codex,
            "cursor" => self.agent_cursor,
            _ => self.fg,
        };
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    }

    pub fn tag_style(&self) -> Style {
        Style::default().fg(Color::Magenta)
    }

    pub fn file_created_style(&self) -> Style {
        Style::default().fg(self.success)
    }

    pub fn file_modified_style(&self) -> Style {
        Style::default().fg(self.warning)
    }

    pub fn file_deleted_style(&self) -> Style {
        Style::default().fg(self.error)
    }

    pub fn user_role_style(&self) -> Style {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    }

    pub fn assistant_role_style(&self) -> Style {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    }

    pub fn status_bar_style(&self) -> Style {
        Style::default().bg(Color::DarkGray).fg(Color::White)
    }

    pub fn search_match_style(&self) -> Style {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    }
}
