pub mod traits;
pub mod claude_code;
pub mod codex;
pub mod cursor;

pub use traits::*;

use claude_code::ClaudeCodeAdapter;
use codex::CodexAdapter;
use cursor::CursorAdapter;

/// Returns all available adapters
pub fn all_adapters() -> Vec<Box<dyn AgentAdapter>> {
    vec![
        Box::new(ClaudeCodeAdapter::new()),
        Box::new(CodexAdapter::new()),
        Box::new(CursorAdapter::new()),
    ]
}

/// Returns only installed adapters
pub fn installed_adapters() -> Vec<Box<dyn AgentAdapter>> {
    all_adapters()
        .into_iter()
        .filter(|a| a.is_installed())
        .collect()
}

/// Get adapter by agent type string
pub fn get_adapter(agent: &str) -> Option<Box<dyn AgentAdapter>> {
    match agent.to_lowercase().as_str() {
        "claude-code" | "claude" => Some(Box::new(ClaudeCodeAdapter::new())),
        "codex" => Some(Box::new(CodexAdapter::new())),
        "cursor" => Some(Box::new(CursorAdapter::new())),
        _ => None,
    }
}
