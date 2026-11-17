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

    pub const ALL: [ViewMode; 4] = [Self::Live, Self::Edit, Self::Split, Self::Preview];

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
    /// 日本語フォントのファイル（空なら自動で探す）
    pub font_path: String,
    /// 本文を読みやすい幅に収め、中央に置く
    pub readable_width: bool,
    pub max_width: f32,
    /// Enterでのリスト継続とTabでの字下げ
    pub smart_lists: bool,
    /// 括弧・記号の自動補完と、選択範囲を囲む入力
    pub auto_pair: bool,
    /// 入力が止まってから自動保存するまでの秒数
    pub autosave_secs: f32,
    /// ローカル履歴に残す件数（0で残さない）と日数
    pub history_versions: usize,
    pub history_days: u64,
    /// Vimキーバインド（モーダル編集）
    pub vim: bool,
    /// 分割したもう一方のペインのノート
    pub split: Option<String>,
    pub split_focus_right: bool,
    /// 実行を許可したユーザーコマンドの設定ファイル（vaultのパス → 許可したときの内容）。
    /// vaultの外に置くことで、vaultを受け取っただけでは許可されない
    pub trusted_commands: std::collections::BTreeMap<String, String>,
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
            font_path: String::new(),
            readable_width: true,
            max_width: 760.0,
            smart_lists: true,
            auto_pair: true,
            autosave_secs: 1.2,
            history_versions: 50,
            history_days: 30,
            vim: false,
            split: None,
            split_focus_right: false,
            trusted_commands: Default::default(),
        }
    }
}

fn config_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("robsidian").join("config.json"))
}

impl Config {
    pub fn history_limits(&self) -> robsidian_core::history::Limits {
        robsidian_core::history::Limits {
            versions: self.history_versions,
            days: self.history_days.max(1),
        }
    }

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
