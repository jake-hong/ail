use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
pub struct AilConfig {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub agents: AgentsConfig,
    #[serde(default)]
    pub export: ExportConfig,
    #[serde(default)]
    pub report: ReportConfig,
    #[serde(default)]
    pub tui: TuiConfig,
    #[serde(default)]
    pub mcp: McpConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GeneralConfig {
    pub db_path: String,
    pub auto_index: bool,
    pub index_interval: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentsConfig {
    pub enabled: Vec<String>,
    #[serde(default)]
    pub claude_code: AgentPathConfig,
    #[serde(default)]
    pub codex: AgentPathConfig,
    #[serde(default)]
    pub cursor: AgentPathConfig,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AgentPathConfig {
    pub data_dir: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportConfig {
    pub default_detail: String,
    pub template: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ReportConfig {
    pub default_format: String,
    pub include_file_changes: bool,
    #[serde(default)]
    pub summarize: SummarizeConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SummarizeConfig {
    pub enabled: bool,
    pub api_key: Option<String>,
    pub model: String,
    pub max_input_chars: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TuiConfig {
    pub theme: String,
    pub max_results: usize,
    pub preview_lines: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct McpConfig {
    pub transport: String,
}

impl Default for AilConfig {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            agents: AgentsConfig::default(),
            export: ExportConfig::default(),
            report: ReportConfig::default(),
            tui: TuiConfig::default(),
            mcp: McpConfig::default(),
        }
    }
}

impl Default for GeneralConfig {
    fn default() -> Self {
        let db_path = data_dir().join("index.db");
        Self {
            db_path: db_path.to_string_lossy().to_string(),
            auto_index: true,
            index_interval: 300,
        }
    }
}

impl Default for AgentsConfig {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        Self {
            enabled: vec![
                "claude-code".to_string(),
                "codex".to_string(),
                "cursor".to_string(),
            ],
            claude_code: AgentPathConfig {
                data_dir: home.join(".claude").to_string_lossy().to_string(),
            },
            codex: AgentPathConfig {
                data_dir: home.join(".codex").to_string_lossy().to_string(),
            },
            cursor: AgentPathConfig {
                data_dir: home.join(".cursor").to_string_lossy().to_string(),
            },
        }
    }
}

impl Default for ExportConfig {
    fn default() -> Self {
        Self {
            default_detail: "summary".to_string(),
            template: "default".to_string(),
        }
    }
}

impl Default for ReportConfig {
    fn default() -> Self {
        Self {
            default_format: "markdown".to_string(),
            include_file_changes: true,
            summarize: SummarizeConfig::default(),
        }
    }
}

impl Default for SummarizeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key: None,
            model: "claude-haiku-4-5-20251001".to_string(),
            max_input_chars: 4000,
        }
    }
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            max_results: 200,
            preview_lines: 20,
        }
    }
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            transport: "stdio".to_string(),
        }
    }
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("ail")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("ail")
}

pub fn db_path() -> PathBuf {
    data_dir().join("index.db")
}

pub fn load_config() -> Result<AilConfig> {
    let path = config_path();
    if path.exists() {
        let content = fs::read_to_string(&path)?;
        let config: AilConfig = toml::from_str(&content)?;
        Ok(config)
    } else {
        Ok(AilConfig::default())
    }
}

pub fn save_config(config: &AilConfig) -> Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(config)?;
    fs::write(&path, content)?;
    Ok(())
}

pub fn ensure_data_dir() -> Result<PathBuf> {
    let dir = data_dir();
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn resolve_db_path(config: &AilConfig) -> PathBuf {
    let p = &config.general.db_path;
    if p.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(&p[2..]);
        }
    }
    PathBuf::from(p)
}

pub fn open_in_editor(path: &Path) -> Result<()> {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    std::process::Command::new(editor)
        .arg(path)
        .status()?;
    Ok(())
}
