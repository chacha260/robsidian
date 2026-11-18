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
    /// リボン（左端のアイコンの列）を出す
    pub show_ribbon: bool,
    pub font_size: f32,
    /// 日本語フォントのファイル（空なら自動で探す）
    pub font_path: String,
    /// 本文を読みやすい幅に収め、中央に置く
    pub readable_width: bool,
    /// マウスホイールのスクロールをなめらかにする（少し遅れて動く）。オフなら回した瞬間に動く
    pub smooth_scroll: bool,
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
    /// モーダル編集（Vim / Helix のキー操作）
    pub vim: bool,
    /// モーダル編集を Helix風（選択してから操作する）にする
    pub helix: bool,
    /// 分割したもう一方のペインのノート
    pub split: Option<String>,
    pub split_focus_right: bool,
    /// 実行を許可したユーザーコマンドの設定ファイル（vaultのパス → 許可したときの内容）。
    /// vaultの外に置くことで、vaultを受け取っただけでは許可されない
    pub trusted_commands: std::collections::BTreeMap<String, String>,
    /// Mermaid図を画像にするコマンド（空なら使わない）。`-i 入力 -o 出力 -b transparent -t テーマ` を後ろに付けて実行する
    pub mermaid_command: String,
    /// Obsidian のコアプラグインに当たる機能のオン・オフ
    pub features: Features,
}

/// Obsidian のコアプラグインに当たる機能のオン・オフ（設定画面の「コアプラグイン」）。
/// 新しく足した項目は、古い設定ファイルでもオンで始まる
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Features {
    pub backlinks: bool,
    pub bookmarks: bool,
    pub command_palette: bool,
    pub daily_notes: bool,
    pub file_explorer: bool,
    pub file_recovery: bool,
    pub graph: bool,
    pub outline: bool,
    pub page_preview: bool,
    pub properties: bool,
    pub quick_switcher: bool,
    pub search: bool,
    pub tags_view: bool,
    pub templates: bool,
    pub outgoing_links: bool,
    pub footnotes: bool,
    pub word_count: bool,
    pub related: bool,
    pub random_note: bool,
    pub unique_note: bool,
    pub slash_commands: bool,
    pub note_composer: bool,
    pub workspaces: bool,
    pub bases: bool,
    pub slides: bool,
    pub canvas: bool,
    pub audio: bool,
}

impl Default for Features {
    fn default() -> Self {
        Self {
            backlinks: true,
            bookmarks: true,
            command_palette: true,
            daily_notes: true,
            file_explorer: true,
            file_recovery: true,
            graph: true,
            outline: true,
            page_preview: true,
            properties: true,
            quick_switcher: true,
            search: true,
            tags_view: true,
            templates: true,
            outgoing_links: true,
            footnotes: true,
            word_count: true,
            related: true,
            random_note: true,
            unique_note: true,
            slash_commands: true,
            note_composer: true,
            workspaces: true,
            bases: true,
            slides: true,
            canvas: true,
            audio: true,
        }
    }
}

/// 設定画面に並べる項目（名前, 説明, 値）
pub struct FeatureInfo {
    pub name: &'static str,
    pub hint: &'static str,
    pub value: fn(&mut Features) -> &mut bool,
}

pub const FEATURES: &[FeatureInfo] = &[
    FeatureInfo {
        name: "音声録音",
        hint: "録音と、埋め込んだ WAV のその場での再生。オフなら WAV は既定のアプリで開く",
        value: |f| &mut f.audio,
    },
    FeatureInfo {
        name: "バックリンク",
        hint: "右サイドバーのバックリンクと未リンクの言及、ステータスバーの被リンク数",
        value: |f| &mut f.backlinks,
    },
    FeatureInfo {
        name: "Bases",
        hint: ".base を表やカードで表示する。オフなら既定のアプリで開く",
        value: |f| &mut f.bases,
    },
    FeatureInfo {
        name: "ブックマーク",
        hint: "左サイドバーの ★ と、ブックマークの追加",
        value: |f| &mut f.bookmarks,
    },
    FeatureInfo {
        name: "Canvas",
        hint: ".canvas を表示・編集する。オフなら既定のアプリで開く",
        value: |f| &mut f.canvas,
    },
    FeatureInfo {
        name: "コマンドパレット",
        hint: "Ctrl+Shift+P のコマンドパレット",
        value: |f| &mut f.command_palette,
    },
    FeatureInfo {
        name: "デイリーノート",
        hint: "Ctrl+D と「今日」ボタン",
        value: |f| &mut f.daily_notes,
    },
    FeatureInfo {
        name: "ファイルエクスプローラー",
        hint: "左サイドバーのファイル一覧",
        value: |f| &mut f.file_explorer,
    },
    FeatureInfo {
        name: "ファイル復元",
        hint: "ローカル履歴（保存の直前の内容を .robsidian/history に残す）と履歴の画面。オフなら残さない",
        value: |f| &mut f.file_recovery,
    },
    FeatureInfo {
        name: "脚注ビュー",
        hint: "右サイドバーに、脚注の一覧を出す",
        value: |f| &mut f.footnotes,
    },
    FeatureInfo {
        name: "グラフビュー",
        hint: "Ctrl+G とツールバーの「グラフ」",
        value: |f| &mut f.graph,
    },
    FeatureInfo {
        name: "ノートコンポーザー",
        hint: "選択範囲の切り出しと、ノートの結合",
        value: |f| &mut f.note_composer,
    },
    FeatureInfo {
        name: "発リンク",
        hint: "右サイドバーに、このノートからのリンクの一覧を出す",
        value: |f| &mut f.outgoing_links,
    },
    FeatureInfo {
        name: "アウトライン",
        hint: "右サイドバーの見出しの一覧",
        value: |f| &mut f.outline,
    },
    FeatureInfo {
        name: "ページプレビュー",
        hint: "リンクやノートの名前にマウスを乗せたときの中身のプレビュー",
        value: |f| &mut f.page_preview,
    },
    FeatureInfo {
        name: "プロパティ",
        hint: "ライブ表示で frontmatter を欄にして編集する。オフなら YAML のまま見せる",
        value: |f| &mut f.properties,
    },
    FeatureInfo {
        name: "クイックスイッチャー",
        hint: "Ctrl+P でノートを開く・作る",
        value: |f| &mut f.quick_switcher,
    },
    FeatureInfo {
        name: "ランダムノート",
        hint: "コマンド「ランダムなノートを開く」",
        value: |f| &mut f.random_note,
    },
    FeatureInfo {
        name: "検索",
        hint: "左サイドバーの全文検索（Ctrl+Shift+F）",
        value: |f| &mut f.search,
    },
    FeatureInfo {
        name: "スラッシュコマンド",
        hint: "行頭で / を入力すると、コマンドパレットを開く",
        value: |f| &mut f.slash_commands,
    },
    FeatureInfo {
        name: "スライド",
        hint: "コマンド「スライドとして表示」",
        value: |f| &mut f.slides,
    },
    FeatureInfo {
        name: "タグ一覧",
        hint: "左サイドバーのタグの一覧",
        value: |f| &mut f.tags_view,
    },
    FeatureInfo {
        name: "テンプレート",
        hint: "テンプレートの挿入と、テンプレートからの新規ノート",
        value: |f| &mut f.templates,
    },
    FeatureInfo {
        name: "ユニークノート",
        hint: "コマンド「ユニークノートを作成」",
        value: |f| &mut f.unique_note,
    },
    FeatureInfo {
        name: "ワークスペース",
        hint: "タブと分割の配置を保存して開く",
        value: |f| &mut f.workspaces,
    },
    FeatureInfo {
        name: "文字数カウント",
        hint: "ステータスバーに文字数と単語数を出す",
        value: |f| &mut f.word_count,
    },
    FeatureInfo {
        name: "関連ノート",
        hint: "右サイドバーに、リンクやタグが近いノートを出す（Robsidian 独自）",
        value: |f| &mut f.related,
    },
];

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
            show_ribbon: true,
            font_size: 15.0,
            font_path: String::new(),
            readable_width: true,
            smooth_scroll: false,
            max_width: 760.0,
            smart_lists: true,
            auto_pair: true,
            autosave_secs: 1.2,
            history_versions: 50,
            history_days: 30,
            vim: false,
            helix: false,
            split: None,
            split_focus_right: false,
            trusted_commands: Default::default(),
            mermaid_command: "mmdc".into(),
            features: Features::default(),
        }
    }
}

fn config_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("robsidian").join("config.json"))
}

impl Config {
    pub fn history_limits(&self) -> robsidian_core::history::Limits {
        robsidian_core::history::Limits {
            // ファイル復元をオフにしたら、履歴を残さない
            versions: if self.features.file_recovery {
                self.history_versions
            } else {
                0
            },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features_default_on_and_keep_unknown_off() {
        // 古い設定ファイル（features が無い）では、すべてオン
        let cfg: Config = serde_json::from_str(r#"{"dark": false}"#).unwrap();
        assert_eq!(cfg.features, Features::default());
        // 一部だけ書いてあれば、書いていない項目はオン
        let cfg: Config = serde_json::from_str(r#"{"features": {"canvas": false}}"#).unwrap();
        assert!(!cfg.features.canvas && cfg.features.bases);
        // 設定画面の一覧は、すべての項目を指している
        let mut f = Features::default();
        for info in FEATURES {
            *(info.value)(&mut f) = false;
        }
        assert_eq!(
            serde_json::to_string(&f).unwrap().matches("true").count(),
            0
        );
    }
}
