//! 起動をまたいで保持する設定（最後に開いたvault、開いていたタブ、表示設定）。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ViewMode {
    /// Live Preview: 編集しながら、カーソルの無い行は整形して見せる
    #[default]
    Live,
    /// ソース表示: 記法をすべてそのまま見せる
    Edit,
    Split,
    Preview,
}

impl ViewMode {
    pub fn next(self) -> Self {
        match self {
            Self::Live => Self::Edit,
            Self::Edit => Self::Split,
            Self::Split => Self::Preview,
            Self::Preview => Self::Live,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Live => "ライブ",
            Self::Edit => "ソース",
            Self::Split => "分割",
            Self::Preview => "プレビュー",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub vault: Option<PathBuf>,
    pub recent_vaults: Vec<PathBuf>,
    pub open_tabs: Vec<String>,
    pub active_tab: usize,
    pub dark: bool,
    pub view_mode: ViewMode,
    pub show_left: bool,
    pub show_right: bool,
    pub font_size: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            vault: None,
            recent_vaults: Vec::new(),
            open_tabs: Vec::new(),
            active_tab: 0,
            dark: true,
            view_mode: ViewMode::Live,
            show_left: true,
            show_right: true,
            font_size: 15.0,
        }
    }
}

fn config_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("robsidian").join("config.json"))
}

impl Config {
    pub fn load() -> Self {
        config_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = config_path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }

    pub fn remember_vault(&mut self, path: PathBuf) {
        self.recent_vaults.retain(|p| p != &path);
        self.recent_vaults.insert(0, path.clone());
        self.recent_vaults.truncate(8);
        self.vault = Some(path);
    }
}
