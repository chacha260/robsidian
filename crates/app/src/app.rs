//! アプリ本体: 状態管理と画面全体のレイアウト。

use crate::config::{Config, ViewMode};
use crate::edit;
use crate::editor;
use crate::external::{self, GitEvent, TrustOutcome};
use crate::find;
use crate::fonts;
use crate::graph::Graph;
use crate::history_view::{self, HistoryView};
use crate::hover::{self, PreviewHover};
use crate::math_view;
use crate::preview;
use crate::props_ui::PropsPanel;
use crate::settings::SettingsView;
use crate::vim::{self, Vim};
use crate::workspace::{self, Workspace};
use eframe::egui::text::{CCursor, CCursorRange};
use eframe::egui::{
    self, Align, Align2, Key, KeyboardShortcut, Layout, Modifiers, RichText, ScrollArea, TextEdit,
};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use notify::Watcher as _;
use robsidian_core::usercmd::{self, Input, Output, UserCommand};
use robsidian_core::vault::{file_name, is_note_path, parent_dir};
use robsidian_core::{parse, search, Heading, Mention, NoteHit, Vault};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// 埋め込んだノートの、まだ測っていないときの高さと、描く高さの上限
const EMBED_ESTIMATE: f32 = 120.0;
const EMBED_MAX: f32 = 640.0;
const RESCAN_DELAY: Duration = Duration::from_millis(400);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Command {
    QuickSwitch,
    Palette,
    SearchAll,
    NewNote,
    Daily,
    Save,
    CloseTab,
    NextTab,
    PrevTab,
    ToggleView,
    ToggleGraph,
    ToggleLeft,
    ToggleRight,
    FollowLink,
    Rename,
    Delete,
    History,
    GoBack,
    GoForward,
    ToggleBookmark,
    InsertTemplate,
    NewFromTemplate,
    OpenVault,
    Find,
    Replace,
    FindNext,
    FindPrev,
    Settings,
    ToggleVim,
    ToggleSplit,
    SwapPane,
    AddProperty,
    ToggleTheme,
    Rescan,
    GitCommit,
    GitPull,
    GitPush,
    RandomNote,
    UniqueNote,
    ExtractNote,
    MergeNote,
    SaveLayout,
    OpenLayout,
    DeleteLayout,
    Slides,
    Record,
}

const CTRL: Modifiers = Modifiers::COMMAND;
/// 同じノートを開いている間、関連ノートを計算し直すまでの間隔
const RELATED_REFRESH: Duration = Duration::from_secs(5);
const CTRL_SHIFT: Modifiers = Modifiers::COMMAND.plus(Modifiers::SHIFT);

/// (コマンド, 表示名, ショートカット)。Shift付きを先に並べること（先に一致した方が勝つ）。
impl Command {
    /// コマンドパレットで英語でも検索できるようにするための語（小文字、空白区切り）。
    fn keywords(self) -> &'static str {
        match self {
            Command::QuickSwitch => "open note quick switcher",
            Command::Palette => "command palette",
            Command::SearchAll => "search find grep",
            Command::NewNote => "new note create",
            Command::Daily => "daily note today journal",
            Command::Save => "save",
            Command::CloseTab => "close tab",
            Command::NextTab => "next tab",
            Command::PrevTab => "previous prev tab",
            Command::ToggleView => "toggle view mode live source preview split",
            Command::ToggleGraph => "graph view",
            Command::ToggleLeft => "toggle left sidebar files",
            Command::ToggleRight => "toggle right sidebar backlinks outline",
            Command::FollowLink => "follow link open",
            Command::Rename => "rename move",
            Command::Delete => "delete trash remove",
            Command::History => "history restore version undo diff",
            Command::GoBack => "go back navigate previous",
            Command::GoForward => "go forward navigate next",
            Command::ToggleBookmark => "bookmark star favorite pin",
            Command::InsertTemplate => "insert template",
            Command::NewFromTemplate => "new note from template create",
            Command::Find => "find in note search",
            Command::Replace => "replace in note find",
            Command::FindNext => "find next match",
            Command::FindPrev => "find previous match",
            Command::Settings => "settings preferences options config",
            Command::ToggleVim => "vim modal keybindings normal mode",
            Command::ToggleSplit => "split pane side by side",
            Command::SwapPane => "switch pane focus other",
            Command::AddProperty => "add property frontmatter metadata yaml",
            Command::OpenVault => "open vault folder",
            Command::ToggleTheme => "theme dark light",
            Command::Rescan => "rescan reload refresh index",
            Command::RandomNote => "random note open shuffle",
            Command::UniqueNote => "unique note zettelkasten timestamp new create",
            Command::ExtractNote => "extract selection heading note composer split refactor",
            Command::MergeNote => "merge note composer combine join",
            Command::SaveLayout => "save workspace layout tabs",
            Command::OpenLayout => "open load workspace layout tabs",
            Command::DeleteLayout => "delete remove workspace layout",
            Command::Slides => "slides presentation present slideshow",
            Command::Record => "audio recorder record voice microphone",
            Command::GitCommit => "git commit snapshot save version",
            Command::GitPull => "git pull fetch sync download remote",
            Command::GitPush => "git push sync upload remote",
        }
    }
}

type CommandEntry = (Command, &'static str, Option<(Modifiers, Key)>);

impl Command {
    /// 設定の「コアプラグイン」でオフにされていないか
    fn enabled(self, f: &crate::config::Features) -> bool {
        match self {
            Command::RandomNote => f.random_note,
            Command::UniqueNote => f.unique_note,
            Command::ExtractNote | Command::MergeNote => f.note_composer,
            Command::SaveLayout | Command::OpenLayout | Command::DeleteLayout => f.workspaces,
            Command::Slides => f.slides,
            Command::Record => f.audio,
            Command::Palette => f.command_palette,
            Command::QuickSwitch => f.quick_switcher,
            Command::SearchAll => f.search,
            Command::Daily => f.daily_notes,
            Command::ToggleGraph => f.graph,
            Command::History => f.file_recovery,
            Command::ToggleBookmark => f.bookmarks,
            Command::InsertTemplate | Command::NewFromTemplate => f.templates,
            Command::AddProperty => f.properties,
            _ => true,
        }
    }
}
const COMMANDS: &[CommandEntry] = &[
    (
        Command::Palette,
        "コマンドパレット",
        Some((CTRL_SHIFT, Key::P)),
    ),
    (
        Command::SearchAll,
        "vault全体を検索",
        Some((CTRL_SHIFT, Key::F)),
    ),
    (Command::PrevTab, "前のタブ", Some((CTRL_SHIFT, Key::Tab))),
    (
        Command::QuickSwitch,
        "ノートを開く（クイックスイッチャー）",
        Some((CTRL, Key::P)),
    ),
    (Command::NewNote, "新規ノート", Some((CTRL, Key::N))),
    (
        Command::Daily,
        "今日のデイリーノートを開く",
        Some((CTRL, Key::D)),
    ),
    (Command::Save, "保存", Some((CTRL, Key::S))),
    (Command::CloseTab, "タブを閉じる", Some((CTRL, Key::W))),
    (Command::NextTab, "次のタブ", Some((CTRL, Key::Tab))),
    (
        Command::ToggleView,
        "表示切替（編集 → 分割 → プレビュー）",
        Some((CTRL, Key::E)),
    ),
    (Command::ToggleGraph, "グラフビュー", Some((CTRL, Key::G))),
    (
        Command::ToggleLeft,
        "左サイドバーの表示切替",
        Some((CTRL, Key::Backslash)),
    ),
    (
        Command::ToggleRight,
        "右サイドバーの表示切替",
        Some((CTRL, Key::J)),
    ),
    (
        Command::FollowLink,
        "カーソル位置のリンクを開く",
        Some((CTRL, Key::Enter)),
    ),
    (
        Command::Rename,
        "現在のノートを改名/移動",
        Some((Modifiers::NONE, Key::F2)),
    ),
    (Command::Delete, "現在のノートをゴミ箱へ", None),
    (Command::History, "現在のノートの履歴（復元）", None),
    (
        Command::GoBack,
        "戻る",
        Some((Modifiers::ALT, Key::ArrowLeft)),
    ),
    (
        Command::GoForward,
        "進む",
        Some((Modifiers::ALT, Key::ArrowRight)),
    ),
    (Command::ToggleBookmark, "ブックマークに追加 / 解除", None),
    (Command::InsertTemplate, "テンプレートを挿入", None),
    (Command::NewFromTemplate, "テンプレートから新規ノート", None),
    (
        Command::FindPrev,
        "前の一致へ",
        Some((Modifiers::SHIFT, Key::F3)),
    ),
    (Command::Find, "ノート内を検索", Some((CTRL, Key::F))),
    (Command::Replace, "ノート内で置換", Some((CTRL, Key::H))),
    (
        Command::FindNext,
        "次の一致へ",
        Some((Modifiers::NONE, Key::F3)),
    ),
    (Command::Settings, "設定", Some((CTRL, Key::Comma))),
    (Command::ToggleVim, "Vimキーバインドの切り替え", None),
    (
        Command::ToggleSplit,
        "ペインを左右に分割 / 分割を閉じる",
        None,
    ),
    (Command::SwapPane, "もう一方のペインへ移る", None),
    (Command::AddProperty, "プロパティを追加", None),
    (Command::OpenVault, "別のvaultを開く", Some((CTRL, Key::O))),
    (Command::ToggleTheme, "ダーク/ライト切替", None),
    (Command::Rescan, "vaultを再読み込み", None),
    (Command::RandomNote, "ランダムなノートを開く", None),
    (
        Command::UniqueNote,
        "ユニークノートを作成（日時の名前）",
        None,
    ),
    (
        Command::ExtractNote,
        "選択範囲（または見出しの節）を新しいノートに切り出す",
        None,
    ),
    (Command::MergeNote, "このノートを別のノートに結合", None),
    (
        Command::SaveLayout,
        "ワークスペース（タブと分割の配置）を保存",
        None,
    ),
    (Command::OpenLayout, "ワークスペースを開く", None),
    (Command::DeleteLayout, "ワークスペースを削除", None),
    (
        Command::Slides,
        "スライドとして表示（`---` で区切って全画面）",
        None,
    ),
    (Command::Record, "録音を開始 / 停止", None),
    (Command::GitCommit, "gitにスナップショットをコミット", None),
    (
        Command::GitPull,
        "gitでリモートの変更を取り込む（pull）",
        None,
    ),
    (Command::GitPush, "gitでリモートへ送る（push）", None),
];

enum Action {
    Open(String, Option<usize>),
    OpenExternal(String),
    FollowLink {
        target: String,
        from: String,
    },
    Activate(usize),
    Close(usize),
    Search(String),
    Dialog(Dialog),
    OpenVault(PathBuf),
    Run(Command),
    History(String),
    Vim(vim::Cmd),
    /// タスクの完了状態を切り替える（ノートのパス, 行, タスクの本文）
    ToggleTask(String, usize, String),
    Toast(String),
    PasteImage(std::sync::Arc<egui::ColorImage>),
    /// 未リンクの言及をリンクにする（言及, リンク先のノート）
    LinkMentions(Vec<Mention>, String),
    ToggleBookmark(String),
    /// ユーザーコマンドを実行する（`UserCommands::list` の番号）
    RunUser(usize),
    /// スラッシュコマンド: (ノート, 入力した `/` のバイト位置)
    Slash(String, usize),
    /// 音声を再生する / 止める（vault内のパス）
    PlayAudio(String),
}

enum Dialog {
    NewNote {
        dir: String,
        name: String,
        /// 新しいノートに展開するテンプレート
        template: Option<String>,
    },
    NewFolder {
        dir: String,
        name: String,
    },
    Rename {
        path: String,
        new: String,
    },
    Delete {
        path: String,
    },
    /// 選択範囲（または見出しの節）を新しいノートに切り出す
    Extract {
        path: String,
        /// 切り出す範囲（バイト位置）と、そのときの本文の版
        range: std::ops::Range<usize>,
        version: u64,
        /// 見出しの節を切り出すときは、見出しの行を残す
        heading: bool,
        name: String,
    },
    /// `source` を `target` の末尾に結合する
    Merge {
        source: String,
        target: String,
    },
    SaveLayout {
        name: String,
    },
}

/// 組版結果を使い回せる条件: (文字サイズ, ダーク, Live Preview, カーソルの文字範囲, 画像の最大幅)
type LayoutKey = (f32, bool, bool, Option<(usize, usize)>, u32, u64);

struct Tab {
    path: String,
    text: String,
    /// 編集の元になったディスク上の内容。保存前にディスクと比べて、外部での変更を検知する
    base: String,
    /// 未保存の編集中に外部で書き換えられたときの、ディスク側の内容
    conflict: Option<String>,
    dirty: bool,
    edited_at: Option<Instant>,
    version: u64,
    /// 次の描画でこの行（0始まり）へカーソルを移動する
    jump: Option<usize>,
    /// 次の描画でエディタにフォーカスを移す
    focus: bool,
    /// 次の描画でカーソルをこのバイト位置に置く
    place_cursor: Option<usize>,
    /// 次の描画でこの範囲（バイト位置）を選択し、見える位置までスクロールする（フォーカスは移さない）
    select: Option<std::ops::Range<usize>>,
    /// 最後に確認したカーソル位置（バイト）
    cursor: usize,
    layout: editor::Layout,
    layout_key: Option<LayoutKey>,
    /// 画像の読み込み待ちなどで、次の描画で組版をやり直す必要がある
    layout_stale: bool,
    preview: Option<(u64, u64, String)>,
    outline: Option<(u64, Vec<Heading>)>,
    /// 文字数・単語数（本文の版ごとに数え直す）
    counts: std::cell::Cell<Option<(u64, robsidian_core::text::Counts)>>,
}

impl Tab {
    fn new(path: String, text: String) -> Self {
        Self {
            path,
            base: text.clone(),
            text,
            conflict: None,
            dirty: false,
            edited_at: None,
            version: 0,
            jump: None,
            focus: false,
            place_cursor: None,
            select: None,
            cursor: 0,
            layout: editor::Layout::default(),
            layout_key: None,
            layout_stale: false,
            preview: None,
            outline: None,
            counts: std::cell::Cell::new(None),
        }
    }

    fn title(&self) -> &str {
        let name = file_name(&self.path);
        name.strip_suffix(".md").unwrap_or(name)
    }

    fn counts(&self) -> robsidian_core::text::Counts {
        match self.counts.get() {
            Some((v, c)) if v == self.version => c,
            _ => {
                let c = robsidian_core::text::count(&self.text);
                self.counts.set(Some((self.version, c)));
                c
            }
        }
    }

    fn set_text(&mut self, text: String) {
        self.text = text;
        self.version += 1;
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LeftTab {
    Files,
    Search,
    Tags,
    Tasks,
    Bookmarks,
}

enum PaletteItem {
    Note(String),
    Template(String),
    Create(String),
    Cmd(usize),
    /// ユーザーコマンド（`.robsidian/commands.toml`）
    User(usize),
    /// ワークスペースの名前
    Layout(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PaletteMode {
    Notes,
    Commands,
    /// 選んだテンプレートを現在のノートのカーソル位置に挿入する
    InsertTemplate,
    /// 選んだテンプレートで新規ノートを作る
    NewFromTemplate,
    /// 現在のノートを結合する先を選ぶ
    MergeTarget,
    /// 開くワークスペースを選ぶ
    Layouts,
    /// 削除するワークスペースを選ぶ
    DeleteLayout,
}

struct Palette {
    mode: PaletteMode,
    query: String,
    computed_for: Option<String>,
    items: Vec<PaletteItem>,
    selected: usize,
    /// スラッシュコマンドで開いた: (ノート, 入力した `/` のバイト位置)。コマンドを選んだら `/` を消す
    slash: Option<(String, usize)>,
}

/// ノート内の検索バー。
struct FindBar {
    query: find::Query,
    replace: String,
    show_replace: bool,
    /// 次の描画で検索欄にフォーカスする
    focus: bool,
    /// 次の描画で置換欄にフォーカスする
    focus_replace: bool,
    /// (ノートのパス, 本文の版, 検索条件) が変わったら一致を数え直す
    key: Option<(String, u64, find::Query)>,
    matches: Vec<std::ops::Range<usize>>,
    current: Option<usize>,
    error: Option<String>,
}

struct Completion {
    /// `[[` の直後のバイト位置
    start: usize,
    query: String,
    /// (挿入するリンク文字列, 表示用パス)
    items: Vec<(String, String)>,
    selected: usize,
}

struct TreeDir {
    name: String,
    path: String,
    dirs: Vec<usize>,
    files: Vec<(String, String, bool)>,
}

fn build_tree(vault: &Vault) -> Vec<TreeDir> {
    fn dir_index(tree: &mut Vec<TreeDir>, map: &mut HashMap<String, usize>, path: &str) -> usize {
        if let Some(&i) = map.get(path) {
            return i;
        }
        let parent = dir_index(tree, map, parent_dir(path));
        let i = tree.len();
        tree.push(TreeDir {
            name: file_name(path).to_string(),
            path: path.to_string(),
            dirs: Vec::new(),
            files: Vec::new(),
        });
        tree[parent].dirs.push(i);
        map.insert(path.to_string(), i);
        i
    }

    let mut tree = vec![TreeDir {
        name: String::new(),
        path: String::new(),
        dirs: Vec::new(),
        files: Vec::new(),
    }];
    let mut map = HashMap::from([(String::new(), 0)]);
    for dir in vault.dirs() {
        dir_index(&mut tree, &mut map, dir);
    }
    let notes = vault.notes().map(|n| (n.path.as_str(), true));
    let files = vault.files().map(|f| (f, false));
    for (path, note) in notes.chain(files) {
        let i = dir_index(&mut tree, &mut map, parent_dir(path));
        let name = file_name(path);
        let name = if note {
            name.strip_suffix(".md").unwrap_or(name)
        } else {
            name
        };
        tree[i]
            .files
            .push((name.to_string(), path.to_string(), note));
    }
    for dir in &mut tree {
        dir.files.sort_by_cached_key(|f| f.0.to_lowercase());
    }
    tree
}

pub struct App {
    cfg: Config,
    vault: Option<Vault>,
    /// 索引が変わるたびに増える（キャッシュの無効化用）
    generation: u64,
    tabs: Vec<Tab>,
    active: usize,
    left_tab: LeftTab,
    search_query: String,
    search_hits: Vec<NoteHit>,
    search_key: Option<(String, u64)>,
    focus_search: bool,
    tree: Option<(u64, Vec<TreeDir>)>,
    palette: Option<Palette>,
    completion: Option<Completion>,
    completion_dismissed: Option<usize>,
    dialog: Option<Dialog>,
    history: Option<HistoryView>,
    find: Option<FindBar>,
    /// 検索バーを閉じた後も `n` で続きを探せるよう、最後の検索条件を覚えておく
    last_find: Option<find::Query>,
    vim: Vim,
    task_filter: String,
    task_show_done: bool,
    /// タスク一覧: ((索引の世代, 条件, 今日), 結果)
    task_cache: Option<((u64, String, chrono::NaiveDate), Vec<search::TaskHit>)>,
    settings: Option<SettingsView>,
    /// 読み込んだ日本語フォントのパス
    font_in_use: Option<String>,
    /// 順序を保つために次のフレームへ回した入力イベント
    deferred_events: Vec<egui::Event>,
    /// IMEで変換中か（変換中のEnterやTabは編集補助の対象にしない）
    ime_composing: bool,
    workspace: Workspace,
    /// 戻る/進むの履歴: (ノートのパス, カーソルのあった行)
    nav_back: Vec<(String, usize)>,
    nav_forward: Vec<(String, usize)>,
    /// 戻る/進むで移動中（このときは履歴に積まない）
    navigating: bool,
    file_dialog: egui_file_dialog::FileDialog,
    md_cache: CommonMarkCache,
    /// ホバープレビュー用（本文のプレビューとはスクロールなどの状態を分ける）
    hover_cache: CommonMarkCache,
    /// エディタ内に埋め込んだノートの描画用
    embed_cache: CommonMarkCache,
    /// 埋め込んだノートを描いた高さ（次の組版で場所を確保するために覚えておく）
    embed_heights: HashMap<String, f32>,
    /// 未リンクの言及: (索引の世代, ノートのパス, 結果)
    mentions: Option<(u64, String, Vec<Mention>)>,
    graph: Graph,
    show_graph: bool,
    watcher: Option<(notify::RecommendedWatcher, mpsc::Receiver<Vec<PathBuf>>)>,
    rescan_at: Option<Instant>,
    toast: Option<(String, Instant)>,
    /// 最後に当てたスタイル: (ダーク, 文字サイズ, テーマの世代)
    applied_style: Option<(bool, f32, u64)>,
    /// 最後に設定ファイルへ書いたタブ構成（変化したら書き直す）
    saved_session: (Vec<String>, usize, Option<String>),
    props_panel: PropsPanel,
    /// 分割したもう一方のペインに表示しているノート
    other: Option<String>,
    /// フォーカスのあるペインが右側か（`active` が右のペインに出ている）
    focus_right: bool,
    /// このフレームでクリックされた、フォーカスの無いペイン
    pane_clicked: bool,
    user_cmds: external::UserCommands,
    jobs: Vec<external::Job>,
    /// 実行の許可を待っているユーザーコマンド
    /// (実行を頼まれたコマンド, 許可を求めている設定ファイルの本文)
    trust_prompt: Option<(external::Request, String)>,
    git: external::GitState,
    /// コミットメッセージの入力中
    commit_message: Option<String>,
    theme: crate::theme::ThemeFile,
    mermaid: crate::mermaid::Mermaid,
    /// 開いている `.base` の表
    base_view: Option<crate::base_view::BaseView>,
    /// 開いている `.canvas`
    canvas_view: Option<crate::canvas_view::CanvasView>,
    /// 録音中
    recorder: Option<crate::recorder::Recorder>,
    /// 再生中の音声
    player: Option<crate::player::Player>,
    /// スライド表示中: (ノート, 何枚目)
    slideshow: Option<(String, usize)>,
    /// 折りたためるコールアウトを開いた / 閉じた: (ノート, 最初の行) → 開いているか
    callout_folds: HashMap<(String, usize), bool>,
    /// 関連ノート: (索引の世代, ノートのパス, 計算した時刻, 結果)
    related: Option<(u64, String, Instant, Vec<robsidian_core::related::Related>)>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, vault_arg: Option<PathBuf>) -> Self {
        let cfg = Config::load();
        preview::set_features(&cfg.features);
        let font = fonts::install(
            &cc.egui_ctx,
            (!cfg.font_path.is_empty()).then(|| std::path::Path::new(&cfg.font_path)),
        );
        let mut app = Self {
            cfg,
            vault: None,
            generation: 0,
            tabs: Vec::new(),
            active: 0,
            left_tab: LeftTab::Files,
            search_query: String::new(),
            search_hits: Vec::new(),
            search_key: None,
            focus_search: false,
            tree: None,
            palette: None,
            completion: None,
            completion_dismissed: None,
            dialog: None,
            history: None,
            find: None,
            last_find: None,
            vim: Vim::default(),
            task_filter: String::new(),
            task_show_done: false,
            task_cache: None,
            settings: None,
            font_in_use: font.map(|p| p.display().to_string()),
            deferred_events: Vec::new(),
            ime_composing: false,
            workspace: Workspace::default(),
            nav_back: Vec::new(),
            nav_forward: Vec::new(),
            navigating: false,
            file_dialog: egui_file_dialog::FileDialog::new().title("vaultフォルダを選択"),
            md_cache: CommonMarkCache::default(),
            hover_cache: CommonMarkCache::default(),
            embed_cache: CommonMarkCache::default(),
            embed_heights: HashMap::new(),
            mentions: None,
            graph: Graph::new(true),
            show_graph: false,
            watcher: None,
            rescan_at: None,
            toast: None,
            applied_style: None,
            saved_session: (Vec::new(), 0, None),
            props_panel: PropsPanel::default(),
            other: None,
            focus_right: false,
            pane_clicked: false,
            user_cmds: Default::default(),
            jobs: Vec::new(),
            trust_prompt: None,
            git: Default::default(),
            commit_message: None,
            theme: Default::default(),
            related: None,
            mermaid: Default::default(),
            callout_folds: HashMap::new(),
            base_view: None,
            canvas_view: None,
            recorder: None,
            player: None,
            slideshow: None,
        };
        // 前回と同じvaultを開くときは、前回のタブを復元する（引数で指定した場合も）
        let same = |p: &PathBuf| {
            let last = app.cfg.vault.as_ref().and_then(|v| v.canonicalize().ok());
            last.is_some() && p.canonicalize().ok() == last
        };
        let restore = vault_arg.as_ref().is_none_or(same);
        if let Some(path) = vault_arg.or_else(|| app.cfg.vault.clone()) {
            let (tabs, active) = (std::mem::take(&mut app.cfg.open_tabs), app.cfg.active_tab);
            app.open_vault(path, &cc.egui_ctx);
            if restore {
                for path in tabs {
                    app.open_note(&path, None);
                }
                app.active = active.min(app.tabs.len().saturating_sub(1));
                // 分割の状態を戻す
                let split = app.cfg.split.clone();
                app.other = split.filter(|p| app.tabs.iter().any(|t| t.path == *p));
                app.focus_right = app.cfg.split_focus_right && app.other.is_some();
            }
        }
        app
    }

    /// 開いているタブを設定ファイルに記録する（異常終了しても次回復元できるように）。
    fn persist_session(&mut self) {
        let session = (
            self.tabs.iter().map(|t| t.path.clone()).collect(),
            self.active,
            self.other.clone(),
        );
        if session != self.saved_session {
            self.cfg.open_tabs.clone_from(&session.0);
            self.cfg.active_tab = session.1;
            self.cfg.split.clone_from(&session.2);
            self.cfg.split_focus_right = self.focus_right;
            self.cfg.save();
            self.saved_session = session;
        }
    }

    fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    fn current(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    // ---- vault ----

    fn open_vault(&mut self, path: PathBuf, ctx: &egui::Context) {
        self.save_all();
        match Vault::open(&path) {
            Ok(vault) => {
                self.cfg.remember_vault(vault.root().to_path_buf());
                self.tabs.clear();
                self.active = 0;
                self.watcher = None;
                self.start_watcher(&vault, ctx);
                self.workspace = Workspace::load(vault.root());
                let mut vault = vault;
                vault.history_limits = self.cfg.history_limits();
                self.nav_back.clear();
                self.nav_forward.clear();
                self.user_cmds = Default::default();
                self.jobs.clear();
                self.trust_prompt = None;
                self.git = external::GitState::detect(vault.root());
                self.commit_message = None;
                self.theme.reset();
                self.related = None;
                self.vault = Some(vault);
                self.generation += 1;
                self.cfg.save();
            }
            Err(e) => {
                self.cfg.vault = None;
                self.toast(format!("vaultを開けません: {e:#}"));
            }
        }
    }

    fn start_watcher(&mut self, vault: &Vault, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let handler = move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                if !matches!(event.kind, notify::EventKind::Access(_))
                    && tx.send(event.paths).is_ok()
                {
                    ctx.request_repaint();
                }
            }
        };
        let watcher = notify::recommended_watcher(handler).and_then(|mut w| {
            w.watch(vault.root(), notify::RecursiveMode::Recursive)?;
            Ok(w)
        });
        match watcher {
            Ok(w) => self.watcher = Some((w, rx)),
            Err(e) => self.toast(format!("ファイル監視を開始できません: {e}")),
        }
    }

    /// 外部での変更（別エディタ、git pull、Obsidian本体など）を索引に取り込む。
    fn poll_watcher(&mut self, ctx: &egui::Context) {
        let Some(vault) = self.vault.as_mut() else {
            return;
        };
        let mut changed = false;
        if let Some((_, rx)) = &self.watcher {
            for paths in rx.try_iter() {
                for abs in paths {
                    let Some(rel) = vault.rel(&abs) else {
                        // 設定ファイル（.robsidian/commands.toml や theme.toml）は待たずに読み直す
                        if abs.starts_with(vault.root().join(".robsidian")) {
                            self.user_cmds.recheck();
                            self.theme.recheck();
                        }
                        continue;
                    };
                    self.git.mark_stale();
                    if is_note_path(&rel) {
                        changed |= vault.reload(&rel);
                    } else {
                        self.rescan_at = Some(Instant::now() + RESCAN_DELAY);
                    }
                }
            }
        }
        if let Some(at) = self.rescan_at {
            if Instant::now() >= at {
                vault.rescan();
                self.rescan_at = None;
                changed = true;
            } else {
                ctx.request_repaint_after(RESCAN_DELAY);
            }
        }
        if changed {
            self.generation += 1;
            self.sync_tabs();
        }
    }

    /// 未編集のタブを索引の内容に合わせる。消えたノートのタブは閉じる。
    fn sync_tabs(&mut self) {
        let Some(vault) = &self.vault else { return };
        let before = self.tabs.len();
        self.tabs.retain_mut(|tab| match vault.note(&tab.path) {
            Some(note) => {
                if note.text == tab.text {
                    // 外部で同じ内容になった（または自分の保存が反映された）
                    tab.base.clone_from(&note.text);
                    tab.conflict = None;
                } else if !tab.dirty {
                    tab.set_text(note.text.clone());
                    tab.base.clone_from(&note.text);
                } else if note.text != tab.base {
                    // 未保存の編集中に外部で書き換えられた。黙って上書きしない
                    tab.conflict = Some(note.text.clone());
                }
                true
            }
            None => tab.dirty,
        });
        if self.tabs.len() != before {
            self.active = self.active.min(self.tabs.len().saturating_sub(1));
        }
    }

    fn save_tab(&mut self, idx: usize) {
        let (Some(vault), Some(tab)) = (self.vault.as_mut(), self.tabs.get_mut(idx)) else {
            return;
        };
        if !tab.dirty {
            return;
        }
        if tab.conflict.is_some() {
            // 解決前にタブを閉じたり終了したりしても編集が消えないよう、履歴に退避しておく
            let _ = robsidian_core::history::snapshot_with(
                vault.root(),
                &tab.path,
                &tab.text,
                vault.history_limits,
            );
            return;
        }
        // 索引の更新（ファイル監視）を待たず、実ファイルで外部の変更を確かめる
        if let Some(disk) = vault.disk_text(&tab.path) {
            if disk != tab.base && disk != tab.text {
                tab.conflict = Some(disk);
                tab.edited_at = None;
                return;
            }
        }
        match vault.write_note(&tab.path, &tab.text) {
            Ok(()) => {
                tab.dirty = false;
                tab.edited_at = None;
                tab.base.clone_from(&tab.text);
                self.generation += 1;
                self.git.mark_stale();
            }
            Err(e) => {
                // 失敗し続けて毎フレーム再試行しないよう、次の編集まで自動保存を止める
                tab.edited_at = None;
                self.toast(format!("保存に失敗しました: {e:#}"));
            }
        }
    }

    fn save_all(&mut self) {
        for i in 0..self.tabs.len() {
            self.save_tab(i);
        }
    }

    fn autosave(&mut self, ctx: &egui::Context) {
        for i in 0..self.tabs.len() {
            if let Some(at) = self.tabs[i].edited_at {
                let delay = Duration::from_secs_f32(self.cfg.autosave_secs.clamp(0.3, 60.0));
                if at.elapsed() >= delay {
                    self.save_tab(i);
                } else {
                    ctx.request_repaint_after(delay);
                }
            }
        }
    }

    fn open_note(&mut self, path: &str, line: Option<usize>) {
        if self.other.as_deref() == Some(path) {
            // もう一方のペインに出ているノートなら、そのペインに移る
            self.swap_panes();
            if let Some(tab) = self.tabs.get_mut(self.active) {
                tab.jump = line.or(tab.jump);
                tab.focus = true;
            }
            return;
        }
        if self.vault.as_ref().is_some_and(|v| v.note(path).is_some()) {
            self.record_navigation(path);
            self.workspace.touch(path);
            self.save_workspace();
        }
        let idx = match self.tabs.iter().position(|t| t.path == path) {
            Some(i) => i,
            None => {
                let Some(note) = self.vault.as_ref().and_then(|v| v.note(path)) else {
                    self.toast(format!("ノートが見つかりません: {path}"));
                    return;
                };
                self.tabs
                    .push(Tab::new(path.to_string(), note.text.clone()));
                self.tabs.len() - 1
            }
        };
        self.active = idx;
        self.tabs[idx].jump = line.or(self.tabs[idx].jump);
        self.tabs[idx].focus = true;
        self.show_graph = false;
        self.completion = None;
    }

    /// 別のノートへ移る前に、いまの場所を「戻る」の履歴に積む。
    fn record_navigation(&mut self, to: &str) {
        if self.navigating || self.show_graph {
            return;
        }
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        if tab.path == to {
            return;
        }
        let line = tab.text[..tab.cursor.min(tab.text.len())]
            .matches('\n')
            .count();
        self.nav_back.push((tab.path.clone(), line));
        if self.nav_back.len() > 100 {
            self.nav_back.remove(0);
        }
        self.nav_forward.clear();
    }

    fn navigate(&mut self, back: bool) {
        let (from, to) = if back {
            (&mut self.nav_back, &mut self.nav_forward)
        } else {
            (&mut self.nav_forward, &mut self.nav_back)
        };
        let Some(vault) = &self.vault else {
            return;
        };
        // 消えたノートは飛ばす
        let target = loop {
            match from.pop() {
                Some((path, line)) if vault.note(&path).is_some() => break Some((path, line)),
                Some(_) => continue,
                None => break None,
            }
        };
        let Some((path, line)) = target else {
            return;
        };
        if let Some(tab) = self.tabs.get(self.active) {
            let here = tab.text[..tab.cursor.min(tab.text.len())]
                .matches('\n')
                .count();
            to.push((tab.path.clone(), here));
        }
        self.navigating = true;
        self.open_note(&path, Some(line));
        self.navigating = false;
    }

    fn toggle_bookmark(&mut self, path: &str) {
        let added = self.workspace.toggle_bookmark(path);
        self.save_workspace();
        let name = file_name(path).trim_end_matches(".md");
        self.toast(if added {
            format!("ブックマークに追加しました: {name}")
        } else {
            format!("ブックマークから外しました: {name}")
        });
    }

    fn save_workspace(&self) {
        if let Some(vault) = &self.vault {
            self.workspace.save(vault.root());
        }
    }

    /// 現在のノートのカーソル位置に文字列を挿入する。
    fn insert_at_cursor(&mut self, text: &str) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let at = tab.cursor.min(tab.text.len());
        let mut body = tab.text.clone();
        body.insert_str(at, text);
        tab.set_text(body);
        tab.place_cursor = Some(at + text.len());
        tab.focus = true;
        tab.dirty = true;
        tab.edited_at = Some(Instant::now());
    }

    fn create_and_open(&mut self, name: &str) {
        self.create_from(name, None);
    }

    /// ノートを作って開く。`template` があれば、ノート名を `{{title}}` として展開した本文で作る。
    fn create_from(&mut self, name: &str, template: Option<&str>) {
        let Some(vault) = self.vault.as_mut() else {
            return;
        };
        let title = file_name(name.trim().trim_end_matches('/')).trim_end_matches(".md");
        let body = template
            .and_then(|t| vault.render_template(t, title))
            .unwrap_or_default();
        match vault.create_note(name, &body) {
            Ok(path) => {
                self.generation += 1;
                self.open_note(&path, None);
            }
            Err(e) => self.toast(format!("作成できません: {e:#}")),
        }
    }

    fn follow_link(&mut self, target: &str, from: &str, ctx: &egui::Context) {
        let (target, heading) = match target.split_once('#') {
            Some((t, h)) => (t.trim(), Some(h)),
            None => (target.trim(), None),
        };
        let Some(vault) = &self.vault else { return };
        // `[[#見出し]]` は同じノート内のリンク
        let same_note = target.is_empty().then(|| from.to_string());
        if let Some(path) = same_note.or_else(|| vault.resolve(target, from).map(str::to_string)) {
            self.open_note(&path, None);
            if let Some(heading) = heading {
                self.jump_to_heading(&path, heading);
            }
        } else if let Some(file) = vault.resolve_file(target, from).map(str::to_string) {
            if self.cfg.features.audio && crate::player::is_audio(&file) {
                // 録音などの音声は、その場で鳴らす（もう一度で止める）
                self.toggle_audio(&file);
            } else {
                self.open_external(&file, ctx);
            }
        } else if !target.is_empty() {
            // Obsidianと同じく、存在しないリンクを開くとノートを作る
            self.create_and_open(target);
        }
    }

    /// 音声を鳴らす。同じファイルを鳴らしていれば止める。
    fn toggle_audio(&mut self, path: &str) {
        let same = self.player.as_ref().is_some_and(|p| p.path == path);
        self.player = None;
        if same {
            return;
        }
        let Some(vault) = &self.vault else { return };
        self.player = Some(crate::player::Player::start(
            vault.abs(path),
            path.to_string(),
        ));
    }

    /// 開いているノートの見出しへ移動する。`a#b` のような入れ子の指定は最後の見出しで探す。
    fn jump_to_heading(&mut self, path: &str, heading: &str) {
        let want = heading
            .rsplit('#')
            .next()
            .unwrap_or(heading)
            .trim()
            .to_lowercase();
        let Some(tab) = self.tabs.iter_mut().find(|t| t.path == path) else {
            return;
        };
        let line = parse::parse(&tab.text)
            .headings
            .into_iter()
            .find(|h| h.text.trim().to_lowercase() == want)
            .map(|h| h.line);
        match line {
            Some(line) => tab.jump = Some(line),
            None => self.toast(format!("見出しが見つかりません: {heading}")),
        }
    }

    fn open_external(&mut self, rel: &str, ctx: &egui::Context) {
        if let Some(vault) = &self.vault {
            let abs = vault.abs(rel).display().to_string().replace('\\', "/");
            let slash = if abs.starts_with('/') { "" } else { "/" };
            ctx.open_url(egui::OpenUrl::new_tab(format!("file://{slash}{abs}")));
        }
    }

    fn rename(&mut self, old: &str, new: &str) {
        self.save_all();
        let Some(vault) = self.vault.as_mut() else {
            return;
        };
        match vault.rename(old, new) {
            Ok(updated) => {
                self.generation += 1;
                let new = new.trim().trim_matches('/');
                for tab in &mut self.tabs {
                    if tab.path == old {
                        tab.path = [new.to_string(), format!("{new}.md")]
                            .into_iter()
                            .find(|p| vault.note(p).is_some())
                            .unwrap_or_else(|| new.to_string());
                    } else if let Some(rest) =
                        tab.path.strip_prefix(old).filter(|r| r.starts_with('/'))
                    {
                        tab.path = format!("{new}{rest}");
                    }
                }
                let new_path = [new.to_string(), format!("{new}.md")]
                    .into_iter()
                    .find(|p| vault.note(p).is_some() || vault.abs(p).is_dir())
                    .unwrap_or_else(|| new.to_string());
                self.workspace.renamed(old, &new_path);
                if let Some(other) = self.other.as_mut() {
                    if let Some(moved) = workspace::moved_path(other, old, &new_path) {
                        *other = moved;
                    }
                }
                for (path, _) in self.nav_back.iter_mut().chain(self.nav_forward.iter_mut()) {
                    if let Some(moved) = workspace::moved_path(path, old, &new_path) {
                        *path = moved;
                    }
                }
                self.save_workspace();
                self.sync_tabs();
                if !updated.is_empty() {
                    self.toast(format!("{}件のノートのリンクを更新しました", updated.len()));
                }
            }
            Err(e) => self.toast(format!("改名できません: {e:#}")),
        }
    }

    fn delete(&mut self, path: &str) {
        let Some(vault) = self.vault.as_mut() else {
            return;
        };
        match trash::delete(vault.abs(path)) {
            Ok(()) => {
                vault.rescan();
                self.generation += 1;
                let prefix = format!("{path}/");
                self.tabs
                    .retain(|t| t.path != path && !t.path.starts_with(&prefix));
                self.active = self.active.min(self.tabs.len().saturating_sub(1));
                self.workspace.removed(path);
                self.save_workspace();
                self.toast(format!("ゴミ箱へ移動しました: {path}"));
            }
            Err(e) => self.toast(format!("ゴミ箱へ移動できません: {e}")),
        }
    }

    // ---- コマンド ----

    fn run(&mut self, cmd: Command, ctx: &egui::Context) {
        // オフにした機能は動かさない（録音中にオフにしたときは、止めることだけはできる）
        if !cmd.enabled(&self.cfg.features) && !(cmd == Command::Record && self.recorder.is_some())
        {
            self.toast("この機能は設定（コアプラグイン）でオフになっています");
            return;
        }
        let current = self.current().map(|t| t.path.clone());
        match cmd {
            Command::QuickSwitch | Command::Palette => {
                let mode = if cmd == Command::Palette {
                    PaletteMode::Commands
                } else {
                    PaletteMode::Notes
                };
                self.palette = Some(Palette {
                    mode,
                    query: String::new(),
                    computed_for: None,
                    items: Vec::new(),
                    selected: 0,
                    slash: None,
                });
            }
            Command::SearchAll => {
                self.cfg.show_left = true;
                self.left_tab = LeftTab::Search;
                self.focus_search = true;
            }
            Command::NewNote => {
                let dir = current.as_deref().map(parent_dir).unwrap_or("").to_string();
                self.dialog = Some(Dialog::NewNote {
                    dir,
                    name: String::new(),
                    template: None,
                });
            }
            Command::Daily => {
                if let Some(vault) = self.vault.as_mut() {
                    match vault.daily() {
                        Ok(path) => {
                            self.generation += 1;
                            self.open_note(&path, None);
                        }
                        Err(e) => self.toast(format!("デイリーノートを作成できません: {e:#}")),
                    }
                }
            }
            Command::Save => self.save_all(),
            Command::CloseTab => self.close_tab(self.active),
            Command::NextTab | Command::PrevTab => {
                let n = self.tabs.len();
                if n > 0 {
                    let step = if cmd == Command::NextTab { 1 } else { n - 1 };
                    self.active = (self.active + step) % n;
                    self.show_graph = false;
                }
            }
            Command::ToggleView => self.cfg.view_mode = self.cfg.view_mode.next(),
            Command::ToggleGraph => self.show_graph = !self.show_graph,
            Command::ToggleLeft => self.cfg.show_left = !self.cfg.show_left,
            Command::ToggleRight => self.cfg.show_right = !self.cfg.show_right,
            Command::FollowLink => {
                if let Some(tab) = self.current() {
                    if let Some(target) = editor::link_at(&tab.text, tab.cursor.min(tab.text.len()))
                    {
                        let from = tab.path.clone();
                        self.follow_link(&target, &from, ctx);
                    }
                }
            }
            Command::Rename => {
                if let Some(path) = current {
                    self.dialog = Some(Dialog::Rename {
                        new: path.clone(),
                        path,
                    });
                }
            }
            Command::Delete => {
                if let Some(path) = current {
                    self.dialog = Some(Dialog::Delete { path });
                }
            }
            Command::History => {
                if let Some(path) = current {
                    self.open_history(&path);
                }
            }
            Command::Find | Command::Replace => {
                if self.cfg.view_mode == ViewMode::Preview {
                    self.cfg.view_mode = ViewMode::Live;
                }
                // 1行だけ選択していれば、それを検索語にする
                let selected = self.current().and_then(|tab| {
                    let state = TextEdit::load_state(ctx, editor_id(&tab.path))?;
                    let r = state.cursor.char_range()?.as_sorted_char_range();
                    let (a, b) = (
                        editor::byte_of_char(&tab.text, r.start.0),
                        editor::byte_of_char(&tab.text, r.end.0),
                    );
                    let t = &tab.text[a..b];
                    (!t.is_empty() && !t.contains('\n')).then(|| t.to_string())
                });
                let bar = self.find.get_or_insert_with(|| FindBar {
                    query: find::Query::default(),
                    replace: String::new(),
                    show_replace: false,
                    focus: true,
                    focus_replace: false,
                    key: None,
                    matches: Vec::new(),
                    current: None,
                    error: None,
                });
                if let Some(t) = selected {
                    bar.query.text = t;
                }
                // 置換で開いたときは、検索語が入っていれば置換欄から打てるようにする
                let to_replace = cmd == Command::Replace && !bar.query.text.is_empty();
                bar.focus = !to_replace;
                bar.focus_replace = to_replace;
                bar.show_replace |= cmd == Command::Replace;
            }
            Command::FindNext | Command::FindPrev => self.find_step(cmd == Command::FindNext),
            Command::Settings => self.settings = Some(SettingsView::new(&self.cfg)),
            Command::ToggleSplit => self.toggle_split(),
            Command::SwapPane => {
                self.swap_panes();
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    tab.focus = true;
                }
            }
            Command::AddProperty => {
                if current.is_some() {
                    self.cfg.view_mode = ViewMode::Live;
                    self.props_panel.start_adding();
                }
            }
            Command::ToggleVim => {
                self.cfg.vim = !self.cfg.vim;
                self.vim = Vim::default();
                self.cfg.save();
                self.toast(if self.cfg.vim {
                    "Vimキーバインドを有効にしました（Escでノーマルモード）"
                } else {
                    "Vimキーバインドを無効にしました"
                });
            }
            Command::GoBack => self.navigate(true),
            Command::GoForward => self.navigate(false),
            Command::ToggleBookmark => {
                if let Some(path) = current {
                    self.toggle_bookmark(&path);
                }
            }
            Command::InsertTemplate | Command::NewFromTemplate => {
                let Some(vault) = &self.vault else { return };
                if vault.templates().is_empty() {
                    let folder = vault.template_folder();
                    self.toast(format!(
                        "テンプレートがありません。「{folder}」フォルダにノートを置いてください"
                    ));
                    return;
                }
                if cmd == Command::InsertTemplate && current.is_none() {
                    self.toast("テンプレートを挿入するノートを開いてください");
                    return;
                }
                let mode = if cmd == Command::InsertTemplate {
                    PaletteMode::InsertTemplate
                } else {
                    PaletteMode::NewFromTemplate
                };
                self.palette = Some(Palette {
                    mode,
                    query: String::new(),
                    computed_for: None,
                    items: Vec::new(),
                    selected: 0,
                    slash: None,
                });
            }
            Command::OpenVault => self.file_dialog.pick_directory(),
            Command::ToggleTheme => self.cfg.dark = !self.cfg.dark,
            Command::Rescan => {
                if let Some(vault) = self.vault.as_mut() {
                    vault.rescan();
                    self.generation += 1;
                    self.sync_tabs();
                }
                self.user_cmds.recheck();
                self.git.mark_stale();
                self.mermaid.forget_failures();
                for tab in &mut self.tabs {
                    tab.preview = None;
                }
            }
            Command::RandomNote => {
                let Some(vault) = &self.vault else { return };
                let paths: Vec<&str> = vault
                    .notes()
                    .map(|n| n.path.as_str())
                    .filter(|p| Some(*p) != current.as_deref())
                    .collect();
                if paths.is_empty() {
                    self.toast("ほかのノートがありません");
                    return;
                }
                // 乱数の代わりに時刻のナノ秒を使う（毎回違うノートが出れば十分）
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.subsec_nanos() as usize ^ d.as_secs() as usize);
                let path = paths[nanos % paths.len()].to_string();
                self.open_note(&path, None);
            }
            Command::UniqueNote => {
                // Obsidian と同じく「年月日時分」の名前で作る。同じ名前があれば秒を足す
                let Some(vault) = &self.vault else { return };
                let dir = current.as_deref().map(parent_dir).unwrap_or("").to_string();
                let now = chrono::Local::now();
                let join = |name: String| {
                    if dir.is_empty() {
                        name
                    } else {
                        format!("{dir}/{name}")
                    }
                };
                let mut name = join(now.format("%Y%m%d%H%M").to_string());
                if vault.note(&format!("{name}.md")).is_some() {
                    name = join(now.format("%Y%m%d%H%M%S").to_string());
                    let mut n = 2;
                    while vault.note(&format!("{name}.md")).is_some() {
                        name = join(format!("{}-{n}", now.format("%Y%m%d%H%M%S")));
                        n += 1;
                    }
                }
                self.create_and_open(&name);
            }
            Command::ExtractNote => self.start_extract(ctx),
            Command::Record => match self.recorder.take() {
                Some(rec) => self.finish_recording(rec),
                None => match current {
                    Some(path) => match crate::recorder::Recorder::start(path) {
                        Ok(rec) => self.recorder = Some(rec),
                        Err(e) => self.toast(format!("{e:#}")),
                    },
                    None => self.toast("録音を埋め込むノートを開いてください"),
                },
            },
            Command::Slides => match current {
                Some(path) => {
                    self.save_all();
                    self.slideshow = Some((path, 0));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
                }
                None => self.toast("スライドにするノートを開いてください"),
            },
            Command::MergeNote | Command::OpenLayout | Command::DeleteLayout => {
                let mode = match cmd {
                    Command::MergeNote if current.is_none() => {
                        self.toast("結合するノートを開いてください");
                        return;
                    }
                    Command::MergeNote => PaletteMode::MergeTarget,
                    _ if self.workspace.layouts.is_empty() => {
                        self.toast("保存したワークスペースはありません");
                        return;
                    }
                    Command::OpenLayout => PaletteMode::Layouts,
                    _ => PaletteMode::DeleteLayout,
                };
                self.palette = Some(Palette {
                    mode,
                    query: String::new(),
                    computed_for: None,
                    items: Vec::new(),
                    selected: 0,
                    slash: None,
                });
            }
            Command::SaveLayout => {
                self.dialog = Some(Dialog::SaveLayout {
                    name: String::new(),
                });
            }
            Command::GitCommit | Command::GitPull | Command::GitPush => {
                let root = self.vault.as_ref().map(|v| v.root().to_path_buf());
                if !self.git.repo {
                    self.toast("このvaultはgitのリポジトリの中にありません");
                } else if let Some(busy) = self.git.busy() {
                    self.toast(format!("git: {busy}"));
                } else if let Some(root) = root {
                    // 取り込む・送る前に、編集中の内容を保存しておく
                    self.save_all();
                    match cmd {
                        Command::GitPull => self.git.pull(&root, ctx),
                        Command::GitPush => self.git.push(&root, ctx),
                        _ => self.commit_message = Some(external::snapshot_message()),
                    }
                }
            }
        }
    }

    fn open_history(&mut self, path: &str) {
        if let Some(vault) = &self.vault {
            self.history = Some(HistoryView::new(vault.root(), path, self.git.repo));
        }
    }

    // ---- ユーザーコマンドとgit ----

    /// 設定ファイルの更新の確認と、実行中のコマンド・gitの結果の受け取り。
    fn poll_external(&mut self, ctx: &egui::Context) {
        let Some(root) = self.vault.as_ref().map(|v| v.root().to_path_buf()) else {
            return;
        };
        if let Some(err) = self.user_cmds.refresh(&root) {
            self.toast(err);
        }
        if let Some(err) = self.theme.refresh(&root) {
            self.toast(err);
        }
        // Mermaid図ができたら、プレビューを作り直す
        if self.mermaid.poll() {
            for tab in &mut self.tabs {
                tab.preview = None;
            }
        }
        let mut i = 0;
        while i < self.jobs.len() {
            match self.jobs[i].poll() {
                Some(result) => {
                    let job = self.jobs.remove(i);
                    self.finish_job(job, result, ctx);
                }
                None => i += 1,
            }
        }
        match self.git.poll(&root, ctx) {
            Some(GitEvent::Done(msg)) => self.toast(msg),
            Some(GitEvent::Failed(e)) => self.toast(format!("git: {e}")),
            None => {}
        }
    }

    /// ユーザーコマンドを実行する。設定ファイルが未許可なら、先に許可を求める。
    fn run_user(&mut self, i: usize, ctx: &egui::Context) {
        let Some(root) = self.vault.as_ref().map(|v| v.root().to_path_buf()) else {
            return;
        };
        // 許可した内容と比べるので、最新の内容を読んでおく
        self.user_cmds.recheck();
        if let Some(err) = self.user_cmds.refresh(&root) {
            self.toast(err);
            return;
        }
        let Some(cmd) = self.user_cmds.list.get(i).cloned() else {
            return;
        };
        let Some(req) = self.prepare_user(cmd, ctx) else {
            return;
        };
        let trusted = self.cfg.trusted_commands.get(&root.display().to_string());
        if trusted != Some(&self.user_cmds.text) {
            self.trust_prompt = Some((req, self.user_cmds.text.clone()));
            return;
        }
        self.jobs.push(external::Job::spawn(req, root, ctx));
    }

    /// 実行に使う入力（選択範囲・本文）と環境を、いまのエディタの状態から決める。
    fn prepare_user(&mut self, cmd: UserCommand, ctx: &egui::Context) -> Option<external::Request> {
        let uses_note =
            cmd.input != Input::None || matches!(cmd.output, Output::Replace | Output::Insert);
        if uses_note && self.tabs.get(self.active).is_none() {
            self.toast("ノートを開いてから実行してください");
            return None;
        }
        // $ROB_FILE を読むコマンドが編集中の内容を見られるよう、先に保存する
        self.save_tab(self.active);
        let Some(tab) = self.tabs.get(self.active) else {
            return Some(external::Request {
                cmd,
                env: usercmd::Env::default(),
                path: None,
                version: 0,
                range: 0..0,
                input: String::new(),
            });
        };
        let sel = selection_bytes(ctx, tab);
        let (range, input) = match cmd.input {
            Input::Note => (0..tab.text.len(), tab.text.clone()),
            Input::Selection => (sel.clone(), tab.text[sel].to_string()),
            Input::None => (sel, String::new()),
        };
        let line = tab.text[..range.start].matches('\n').count() + 1;
        Some(external::Request {
            cmd,
            env: usercmd::Env {
                note: Some(tab.path.clone()),
                line: Some(line),
            },
            path: Some(tab.path.clone()),
            version: tab.version,
            range,
            input,
        })
    }

    /// 終わったユーザーコマンドの出力を反映する。
    fn finish_job(
        &mut self,
        job: external::Job,
        result: anyhow::Result<usercmd::Finished>,
        ctx: &egui::Context,
    ) {
        let name = &job.name;
        let done = match result {
            Ok(done) => done,
            Err(e) => return self.toast(format!("「{name}」を実行できません: {e:#}")),
        };
        if !done.success {
            return self.toast(format!("「{name}」が失敗しました: {}", done.error_line()));
        }
        match job.output {
            Output::None => self.toast(format!("「{name}」を実行しました")),
            Output::Notice => {
                let msg = done.stdout.trim();
                let msg = if msg.is_empty() {
                    format!("「{name}」を実行しました")
                } else {
                    let mut short: String = msg.chars().take(500).collect();
                    if short.len() < msg.len() {
                        short.push('…');
                    }
                    short
                };
                self.toast(msg);
            }
            Output::Replace | Output::Insert => {
                let Some(path) = job.path.as_deref() else {
                    return;
                };
                let Some(tab) = self.tabs.iter_mut().find(|t| t.path == path) else {
                    return self.toast(format!(
                        "ノートが閉じられたので「{name}」の結果を反映しませんでした"
                    ));
                };
                if tab.version != job.version {
                    return self.toast(format!(
                        "実行中にノートが変わったので「{name}」の結果を反映しませんでした"
                    ));
                }
                let (range, out) = if job.output == Output::Replace {
                    (
                        job.range.clone(),
                        usercmd::fit_output(&job.input, &done.stdout),
                    )
                } else {
                    let end = job.range.end;
                    (end..end, usercmd::fit_output("", &done.stdout))
                };
                let mut text = tab.text.clone();
                text.replace_range(range.clone(), &out);
                let end = range.start + out.len();
                replace_text_undoable(ctx, tab, text, range.start..end);
                // すぐ Ctrl+Z で戻せるよう、エディタにフォーカスを戻す
                tab.focus = true;
            }
        }
    }

    /// ユーザーコマンドの許可と、gitのコミットメッセージの入力。
    fn external_ui(&mut self, ctx: &egui::Context) {
        let Some(root) = self.vault.as_ref().map(|v| v.root().to_path_buf()) else {
            return;
        };
        if let Some((_, text)) = &self.trust_prompt {
            match external::trust_ui(ctx, &root, text) {
                Some(TrustOutcome::Allow) => {
                    // 見せた内容を許可し、その内容から読んだコマンドを実行する
                    if let Some((req, text)) = self.trust_prompt.take() {
                        let key = root.display().to_string();
                        self.cfg.trusted_commands.insert(key, text);
                        self.cfg.save();
                        self.jobs.push(external::Job::spawn(req, root.clone(), ctx));
                    }
                }
                Some(TrustOutcome::Cancel) => self.trust_prompt = None,
                None => {}
            }
        }
        let Some(message) = self.commit_message.as_mut() else {
            return;
        };
        let changes = self.git.changes;
        let mut commit = None;
        let modal = egui::Modal::new(egui::Id::unique("git-commit")).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.heading("gitにスナップショットをコミット");
            ui.label(match changes {
                Some(n) => format!("vault以下の{n}件の変更をコミットします。"),
                None => "vault以下の変更をすべてコミットします。".to_string(),
            });
            ui.add_space(4.0);
            let edit = TextEdit::singleline(message)
                .hint_text("コミットメッセージ")
                .desired_width(f32::INFINITY);
            let r = ui.add(edit);
            if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                commit = Some(true);
            } else {
                r.request_focus();
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("コミット").clicked() {
                    commit = Some(true);
                }
                if ui.button("キャンセル").clicked() {
                    commit = Some(false);
                }
            });
        });
        if commit.is_none() && modal.should_close() {
            commit = Some(false);
        }
        match commit {
            Some(true) => {
                let mut message = self.commit_message.take().unwrap_or_default();
                if message.trim().is_empty() {
                    message = external::snapshot_message();
                }
                self.git.commit(&root, message.trim().to_string(), ctx);
            }
            Some(false) => self.commit_message = None,
            None => {}
        }
    }

    fn history_ui(&mut self, ctx: &egui::Context) {
        let Some(view) = self.history.as_mut() else {
            return;
        };
        let current = match self.tabs.iter().find(|t| t.path == view.path) {
            Some(tab) => tab.text.clone(),
            None => self
                .vault
                .as_ref()
                .and_then(|v| v.note(&view.path))
                .map_or_else(String::new, |n| n.text.clone()),
        };
        let colors = self.theme.colors(self.cfg.dark);
        let Some(outcome) = view.show(ctx, &current, &colors) else {
            return;
        };
        let path = view.path.clone();
        self.history = None;
        if let history_view::Outcome::Restore(text) = outcome {
            self.open_note(&path, None);
            if let Some(tab) = self.tabs.iter_mut().find(|t| t.path == path) {
                // 通常の編集と同じ扱いにする（自動保存され、上書き前の内容は履歴に残る）
                tab.set_text(text);
                tab.dirty = true;
                tab.edited_at = Some(Instant::now());
                self.toast("履歴の版に戻しました");
            }
        }
    }

    /// 未保存の編集中に外部でノートが書き換えられたときの選択肢を出す。
    fn conflict_banner(&mut self, ui: &mut egui::Ui) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let Some(disk) = tab.conflict.clone() else {
            return;
        };
        let colors = self.theme.colors(self.cfg.dark);
        let mut choice = None;
        egui::Frame::NONE
            .fill(colors.warn_bg)
            .inner_margin(egui::Margin::symmetric(12, 8))
            .corner_radius(4.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("このノートは、編集中に外部（別のアプリや同期）で変更されました。").strong());
                ui.weak("保存を止めています。どちらの内容を残すか選んでください。どれを選んでも、消える側の内容は履歴に残ります。");
                ui.horizontal_wrapped(|ui| {
                    if ui.button("外部の変更を読み込む").on_hover_text("自分の編集を破棄（履歴に残す）").clicked() {
                        choice = Some(0);
                    }
                    if ui.button("自分の編集で上書き").clicked() {
                        choice = Some(1);
                    }
                    if ui.button("両方残す").on_hover_text("自分の編集を別名のノートとして保存し、外部の変更を読み込む").clicked() {
                        choice = Some(2);
                    }
                    if ui.button("差分を見る").on_hover_text("履歴から比べる").clicked() {
                        choice = Some(3);
                    }
                });
            });
        ui.add_space(4.0);
        let Some(choice) = choice else { return };
        let idx = self.active;
        let Some(vault) = self.vault.as_mut() else {
            return;
        };
        let tab = &mut self.tabs[idx];
        match choice {
            0 => {
                let _ = robsidian_core::history::snapshot_with(
                    vault.root(),
                    &tab.path,
                    &tab.text,
                    vault.history_limits,
                );
                tab.set_text(disk.clone());
                tab.base = disk;
                tab.dirty = false;
                tab.edited_at = None;
                tab.conflict = None;
            }
            1 => {
                tab.base = disk;
                tab.conflict = None;
                self.save_tab(idx);
            }
            2 => {
                let stem = tab.path.strip_suffix(".md").unwrap_or(&tab.path);
                let stamp = chrono::Local::now().format("%Y-%m-%d %H%M%S");
                let copy = format!("{stem} (競合 {stamp})");
                match vault.create_note(&copy, &tab.text) {
                    Ok(created) => {
                        tab.set_text(disk.clone());
                        tab.base = disk;
                        tab.dirty = false;
                        tab.edited_at = None;
                        tab.conflict = None;
                        self.generation += 1;
                        self.toast(format!("自分の編集を「{created}」に保存しました"));
                    }
                    Err(e) => self.toast(format!("別名で保存できません: {e:#}")),
                }
            }
            _ => {
                // 外部の内容を履歴に入れておくと、履歴ダイアログで自分の編集と比べられる
                let path = tab.path.clone();
                let _ = robsidian_core::history::snapshot_with(
                    vault.root(),
                    &path,
                    &disk,
                    vault.history_limits,
                );
                self.open_history(&path);
            }
        }
    }

    /// クリップボードの画像を添付フォルダにPNGで保存し、カーソル位置に埋め込む。
    fn paste_image(&mut self, image: &egui::ColorImage) {
        let Some(path) = self.current().map(|t| t.path.clone()) else {
            return;
        };
        let Some(vault) = self.vault.as_mut() else {
            return;
        };
        let [w, h] = image.size;
        let rgba: Vec<u8> = image
            .pixels
            .iter()
            .flat_map(|c| c.to_srgba_unmultiplied())
            .collect();
        let mut png = Vec::new();
        let encoded = image::RgbaImage::from_raw(w as u32, h as u32, rgba)
            .map(|img| img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png));
        if !matches!(encoded, Some(Ok(()))) {
            self.toast("画像を変換できませんでした");
            return;
        }
        let name = format!(
            "Pasted image {}.png",
            chrono::Local::now().format("%Y%m%d%H%M%S")
        );
        let rel = vault.attachment_path(&path, &name);
        if let Err(e) = vault.write_file(&rel, &png) {
            self.toast(format!("画像を保存できません: {e:#}"));
            return;
        }
        self.generation += 1;
        // ファイル名だけで一意に辿れるならObsidianと同じく名前だけで書く
        let shown = if vault.resolve_file(file_name(&rel), &path) == Some(rel.as_str()) {
            file_name(&rel).to_string()
        } else {
            rel.clone()
        };
        self.insert_at_cursor(&format!("![[{shown}]]"));
    }

    /// 録音を止めて添付フォルダに WAV で保存し、録音を始めたノートに埋め込む。
    fn finish_recording(&mut self, rec: crate::recorder::Recorder) {
        let note = rec.note.clone();
        let bytes = match rec.finish() {
            Ok(b) => b,
            Err(e) => {
                self.toast(format!("録音を保存できません: {e:#}"));
                return;
            }
        };
        let Some(vault) = self.vault.as_mut() else {
            return;
        };
        let name = format!(
            "Recording {}.wav",
            chrono::Local::now().format("%Y%m%d%H%M%S")
        );
        let rel = vault.attachment_path(&note, &name);
        if let Err(e) = vault.write_file(&rel, &bytes) {
            self.toast(format!("録音を保存できません: {e:#}"));
            return;
        }
        self.generation += 1;
        let shown = if vault.resolve_file(file_name(&rel), &note) == Some(rel.as_str()) {
            file_name(&rel).to_string()
        } else {
            rel.clone()
        };
        // 別のノートに移っていたら、録音を始めたノートに戻ってから埋め込む
        if self.current().map(|t| t.path.as_str()) != Some(note.as_str()) {
            self.open_note(&note, None);
        }
        if self.current().map(|t| t.path.as_str()) == Some(note.as_str()) {
            self.insert_at_cursor(&format!("![[{shown}]]"));
        } else {
            self.toast(format!("録音を保存しました: {rel}"));
        }
    }

    /// 未リンクの言及を `[[リンク]]` に書き換える。開いているタブはタブの本文を、
    /// そうでなければファイルを書き換える。後ろの言及から順に処理するので範囲はずれない。
    fn link_mentions(&mut self, mentions: &[Mention], target: &str) {
        let Some(vault) = self.vault.as_mut() else {
            return;
        };
        let mut sorted: Vec<&Mention> = mentions.iter().collect();
        sorted.sort_by(|a, b| {
            a.source
                .cmp(&b.source)
                .then(b.range.start.cmp(&a.range.start))
        });
        let (mut done, mut skipped) = (0, 0);
        for m in sorted {
            if let Some(tab) = self.tabs.iter_mut().find(|t| t.path == m.source) {
                match vault.link_mention_text(&tab.text, m, target) {
                    Some(new) => {
                        tab.set_text(new);
                        tab.dirty = true;
                        tab.edited_at = Some(Instant::now());
                        done += 1;
                    }
                    None => skipped += 1,
                }
                continue;
            }
            let Some(text) = vault.note(&m.source).map(|n| n.text.clone()) else {
                skipped += 1;
                continue;
            };
            match vault.link_mention_text(&text, m, target) {
                Some(new) => match vault.write_note(&m.source, &new) {
                    Ok(()) => done += 1,
                    Err(_) => skipped += 1,
                },
                None => skipped += 1,
            }
        }
        // 開いているタブの変更もすぐ保存して、索引（バックリンク）に反映する
        self.save_all();
        self.generation += 1;
        self.toast(if skipped == 0 {
            format!("{done}件をリンクにしました")
        } else {
            format!("{done}件をリンクにしました（本文が変わっていた{skipped}件は飛ばしました）")
        });
    }

    /// 検索の一致を、必要なら数え直す。
    fn refresh_find(&mut self) {
        let (Some(bar), Some(tab)) = (self.find.as_mut(), self.tabs.get(self.active)) else {
            return;
        };
        let key = (tab.path.clone(), tab.version, bar.query.clone());
        if bar.key.as_ref() == Some(&key) {
            return;
        }
        let query_changed = bar
            .key
            .as_ref()
            .is_none_or(|k| k.0 != key.0 || k.2 != key.2);
        match bar.query.find_all(&tab.text) {
            Ok(m) => {
                bar.matches = m;
                bar.error = None;
            }
            Err(e) => {
                bar.matches.clear();
                bar.error = Some(e);
            }
        }
        bar.current = if query_changed {
            find::next_from(
                &bar.matches,
                tab.cursor.saturating_sub(bar.query.text.len()),
            )
        } else {
            bar.current
                .filter(|&i| i < bar.matches.len())
                .or(find::next_from(&bar.matches, tab.cursor))
        };
        bar.key = Some(key);
    }

    /// 次（または前）の一致を選ぶ。
    fn find_step(&mut self, forward: bool) {
        self.refresh_find();
        let Some(bar) = self.find.as_mut() else {
            return;
        };
        let n = bar.matches.len();
        if n == 0 {
            return;
        }
        let i = match bar.current {
            Some(i) if forward => (i + 1) % n,
            Some(i) => (i + n - 1) % n,
            None => 0,
        };
        bar.current = Some(i);
        let range = bar.matches[i].clone();
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.select = Some(range);
        }
    }

    fn replace_current(&mut self, ctx: &egui::Context) {
        self.refresh_find();
        let (Some(bar), Some(tab)) = (self.find.as_mut(), self.tabs.get_mut(self.active)) else {
            return;
        };
        let Some(range) = bar.current.and_then(|i| bar.matches.get(i).cloned()) else {
            return;
        };
        let with = bar
            .query
            .replacement_for(&tab.text, range.clone(), &bar.replace);
        let mut text = tab.text.clone();
        text.replace_range(range.clone(), &with);
        let end = range.start + with.len();
        replace_text_undoable(ctx, tab, text, range.start..end);
        // 置き換えた直後から次の一致を探す
        bar.key = None;
        tab.cursor = end;
        let after = end;
        self.refresh_find();
        if let Some(bar) = self.find.as_mut() {
            bar.current = find::next_from(&bar.matches, after);
            if let (Some(i), Some(tab)) = (bar.current, self.tabs.get_mut(self.active)) {
                tab.select = Some(bar.matches[i].clone());
            }
        }
    }

    fn replace_all(&mut self, ctx: &egui::Context) {
        let (Some(bar), Some(tab)) = (self.find.as_mut(), self.tabs.get_mut(self.active)) else {
            return;
        };
        match bar.query.replace_all(&tab.text, &bar.replace) {
            Ok((_, 0)) => {}
            Ok((text, n)) => {
                let cursor = tab.cursor.min(text.len());
                let cursor = (0..=cursor)
                    .rev()
                    .find(|&c| text.is_char_boundary(c))
                    .unwrap_or(0);
                replace_text_undoable(ctx, tab, text, cursor..cursor);
                bar.key = None;
                self.toast(format!("{n}件置き換えました（Ctrl+Zで戻せます）"));
            }
            Err(e) => bar.error = Some(e),
        }
    }

    fn find_bar(&mut self, ui: &mut egui::Ui) {
        if self.find.is_none() {
            return;
        }
        self.refresh_find();
        let ctx = ui.ctx().clone();
        let Some(bar) = self.find.as_mut() else {
            return;
        };
        let mut step = None;
        let (mut close, mut replace_one, mut replace_all) = (false, false, false);
        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let query = ui.add(
                        TextEdit::singleline(&mut bar.query.text)
                            .id(egui::Id::unique("find-query"))
                            .hint_text("ノート内を検索")
                            .desired_width(240.0),
                    );
                    if std::mem::take(&mut bar.focus) {
                        query.request_focus();
                        // 入力済みの語は全選択して、そのまま打ち直せるようにする
                        if let Some(mut state) = TextEdit::load_state(&ctx, query.id) {
                            state
                                .cursor
                                .set_char_range(Some(CCursorRange::select_all_str(
                                    &bar.query.text,
                                )));
                            state.store(&ctx, query.id);
                        }
                    }
                    // Enterで次、Shift+Enterで前。検索欄からフォーカスは外さない
                    if query.lost_focus() {
                        let (enter, shift, esc) = ui.input(|i| {
                            (
                                i.key_pressed(Key::Enter),
                                i.modifiers.shift,
                                i.key_pressed(Key::Escape),
                            )
                        });
                        if enter {
                            step = Some(!shift);
                            query.request_focus();
                        }
                        if esc {
                            close = true;
                        }
                    }
                    ui.toggle_value(&mut bar.query.case, "Aa")
                        .on_hover_text("大文字と小文字を区別");
                    ui.toggle_value(&mut bar.query.regex, ".*")
                        .on_hover_text("正規表現（置換で $1 などが使える）");
                    match (&bar.error, bar.matches.len(), bar.current) {
                        (Some(_), _, _) => {
                            ui.colored_label(ui.visuals().error_fg_color, "正規表現の誤り")
                                .on_hover_text(bar.error.clone().unwrap_or_default());
                        }
                        (None, 0, _) if !bar.query.text.is_empty() => {
                            ui.weak("見つかりません");
                        }
                        (None, 0, _) => {}
                        (None, n, Some(i)) => {
                            ui.weak(format!("{}/{n}", i + 1));
                        }
                        (None, n, None) => {
                            ui.weak(format!("{n}件"));
                        }
                    }
                    if ui
                        .small_button("↑")
                        .on_hover_text("前 (Shift+Enter / Shift+F3)")
                        .clicked()
                    {
                        step = Some(false);
                    }
                    if ui
                        .small_button("↓")
                        .on_hover_text("次 (Enter / F3)")
                        .clicked()
                    {
                        step = Some(true);
                    }
                    ui.toggle_value(&mut bar.show_replace, "置換")
                        .on_hover_text("Ctrl+H");
                    if ui.small_button("×").on_hover_text("閉じる (Esc)").clicked() {
                        close = true;
                    }
                });
                if bar.show_replace {
                    ui.horizontal(|ui| {
                        let field = ui.add(
                            TextEdit::singleline(&mut bar.replace)
                                .hint_text("置換後の文字列")
                                .desired_width(240.0),
                        );
                        if std::mem::take(&mut bar.focus_replace) {
                            field.request_focus();
                        }
                        if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            replace_one = true;
                            field.request_focus();
                        }
                        replace_one |= ui.button("置換").clicked();
                        replace_all |= ui.button("すべて置換").clicked();
                    });
                }
            });
        ui.separator();
        // ボタンを押した後など、どこにもフォーカスが無いときのEscでも閉じる
        if ctx.input(|i| i.key_pressed(Key::Escape)) && ctx.memory(|m| m.focused().is_none()) {
            close = true;
        }
        if close {
            self.last_find = self
                .find
                .take()
                .map(|f| f.query)
                .filter(|q| !q.text.is_empty());
            if let Some(tab) = self.tabs.get_mut(self.active) {
                tab.focus = true;
            }
            return;
        }
        if replace_one {
            self.replace_current(&ctx);
        }
        if replace_all {
            self.replace_all(&ctx);
        }
        if let Some(forward) = step {
            self.find_step(forward);
        }
    }

    fn settings_ui(&mut self, ctx: &egui::Context) {
        let Some(view) = self.settings.as_mut() else {
            return;
        };
        let note = self.tabs.get(self.active).map(|t| t.path.as_str());
        let before = self.cfg.history_limits();
        let features_before = self.cfg.features;
        let out = view.show(
            ctx,
            &mut self.cfg,
            self.vault.as_ref(),
            note,
            self.font_in_use.as_deref(),
        );
        if out.font {
            let path =
                (!self.cfg.font_path.is_empty()).then(|| std::path::Path::new(&self.cfg.font_path));
            self.font_in_use = fonts::install(ctx, path).map(|p| p.display().to_string());
            if !self.cfg.font_path.is_empty()
                && self.font_in_use.as_deref() != Some(self.cfg.font_path.as_str())
            {
                self.toast(
                    "指定したフォントを読み込めませんでした（自動で探したフォントを使います）",
                );
            }
        }
        if self.cfg.features != features_before {
            self.apply_features();
        }
        if self.cfg.history_limits() != before {
            if let Some(vault) = self.vault.as_mut() {
                vault.history_limits = self.cfg.history_limits();
            }
        }
        if out.close {
            self.settings = None;
            self.cfg.save();
        }
    }

    /// 「コアプラグイン」のオン・オフを反映する
    fn apply_features(&mut self) {
        let f = self.cfg.features;
        preview::set_features(&f);
        // プレビューとエディタの組版をやり直す
        self.generation += 1;
        for tab in &mut self.tabs {
            tab.layout_stale = true;
            tab.preview = None;
        }
        if !f.bases {
            self.base_view = None;
        }
        if !f.canvas {
            self.canvas_view = None;
        }
        if !f.audio {
            self.player = None;
        }
        if !f.graph {
            self.show_graph = false;
        }
    }

    /// タスクの完了状態を切り替える。開いているタブは未保存の編集ごと書き換え、
    /// そうでなければファイルを書き換える。行がずれていたら、同じ本文のタスクを探す。
    fn toggle_task(&mut self, path: &str, line: usize, text: &str, ctx: &egui::Context) {
        use robsidian_core::task;
        let find_line = |body: &str| -> Option<usize> {
            let same = |n: usize| {
                body.lines()
                    .nth(n)
                    .and_then(task::task_in_line)
                    .is_some_and(|(_, _, b)| task::split_due(b).0 == text)
            };
            if same(line) {
                return Some(line);
            }
            (0..body.lines().count()).find(|&n| same(n))
        };
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.path == path) {
            let toggled = find_line(&tab.text).and_then(|n| task::toggle(&tab.text, n));
            match toggled {
                Some(new) => {
                    let cursor = tab.cursor.min(new.len());
                    let cursor = (0..=cursor)
                        .rev()
                        .find(|&c| new.is_char_boundary(c))
                        .unwrap_or(0);
                    replace_text_undoable(ctx, tab, new, cursor..cursor);
                    self.save_all();
                }
                None => {
                    self.toast("タスクが見つかりませんでした（内容が変わった可能性があります）")
                }
            }
            return;
        }
        let Some(vault) = self.vault.as_mut() else {
            return;
        };
        let Some(body) = vault.note(path).map(|n| n.text.clone()) else {
            return;
        };
        match find_line(&body).and_then(|n| task::toggle(&body, n)) {
            Some(new) => match vault.write_note(path, &new) {
                Ok(()) => self.generation += 1,
                Err(e) => self.toast(format!("保存できません: {e:#}")),
            },
            None => self.toast("タスクが見つかりませんでした（内容が変わった可能性があります）"),
        }
    }

    fn vim_command(&mut self, cmd: vim::Cmd, ctx: &egui::Context) {
        match cmd {
            vim::Cmd::Save => self.save_tab(self.active),
            vim::Cmd::Close => self.close_tab(self.active),
            vim::Cmd::SaveClose => {
                self.save_tab(self.active);
                self.close_tab(self.active);
            }
            vim::Cmd::OpenFind(_) => self.run(Command::Find, ctx),
            vim::Cmd::FindNext | vim::Cmd::FindPrev => {
                if self.find.is_none() {
                    let Some(query) = self.last_find.clone() else {
                        return;
                    };
                    self.open_find_with(query, false);
                }
                self.find_step(cmd == vim::Cmd::FindNext);
            }
            vim::Cmd::SearchWord(word, forward) => {
                let query = find::Query {
                    text: word,
                    case: true,
                    regex: false,
                };
                self.open_find_with(query, false);
                self.find_step(forward);
            }
            vim::Cmd::ClearSearch => {
                self.last_find = self.find.take().map(|f| f.query);
            }
            vim::Cmd::Undo(_) | vim::Cmd::Redo(_) => {}
            vim::Cmd::Unknown(c) => self.toast(format!("未対応のコマンド: :{c}")),
        }
    }

    /// 検索バーを、指定した検索条件で開く（`focus` が false ならエディタにフォーカスを残す）。
    fn open_find_with(&mut self, query: find::Query, focus: bool) {
        self.find = Some(FindBar {
            query,
            replace: String::new(),
            show_replace: false,
            focus,
            focus_replace: false,
            key: None,
            matches: Vec::new(),
            current: None,
            error: None,
        });
    }

    /// フォーカスのあるペインと、もう一方のペインを入れ替える。
    fn swap_panes(&mut self) {
        let Some(other) = self.other.clone() else {
            return;
        };
        let Some(idx) = self.tabs.iter().position(|t| t.path == other) else {
            self.other = None;
            return;
        };
        self.other = self.tabs.get(self.active).map(|t| t.path.clone());
        self.active = idx;
        self.focus_right = !self.focus_right;
        self.completion = None;
    }

    /// ペインを分割する（分割中なら閉じる）。もう一方には、最近開いた別のノートを出す。
    fn toggle_split(&mut self) {
        if self.other.take().is_some() {
            self.focus_right = false;
            return;
        }
        let Some(current) = self.current().map(|t| t.path.clone()) else {
            self.toast("ノートを開いてから分割してください");
            return;
        };
        let candidate = self
            .workspace
            .recent
            .iter()
            .chain(self.tabs.iter().map(|t| &t.path))
            .find(|p| **p != current && self.vault.as_ref().is_some_and(|v| v.note(p).is_some()))
            .cloned();
        let Some(path) = candidate else {
            self.toast("分割して並べる、もう1つのノートを開いてください");
            return;
        };
        if !self.tabs.iter().any(|t| t.path == path) {
            if let Some(note) = self.vault.as_ref().and_then(|v| v.note(&path)) {
                self.tabs.push(Tab::new(path.clone(), note.text.clone()));
            }
        }
        self.other = Some(path);
        self.focus_right = false;
    }

    /// 1つのペイン（見出しと本文）を描く。`show` が Some なら、フォーカスの無いペインとしてそのタブを描く。
    fn pane_ui(&mut self, ui: &mut egui::Ui, show: Option<usize>, actions: &mut Vec<Action>) {
        let idx = show.unwrap_or(self.active);
        let Some(tab) = self.tabs.get(idx) else {
            return;
        };
        let focused = show.is_none();
        let title = tab.title().to_string();
        ui.horizontal(|ui| {
            let text = if focused {
                RichText::new(&title).strong()
            } else {
                RichText::new(&title).weak()
            };
            if ui.add(egui::Button::new(text).frame(false)).clicked() && !focused {
                self.pane_clicked = true;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("×").on_hover_text("分割を閉じる").clicked() {
                    actions.push(Action::Run(Command::ToggleSplit));
                }
            });
        });
        if !focused {
            // フォーカスの無いペインのエディタが、フォーカスを取らないようにする
            if let Some(tab) = self.tabs.get_mut(idx) {
                tab.focus = false;
            }
        }
        let saved = self.active;
        self.active = idx;
        self.view_ui(ui, actions);
        self.active = saved;
    }

    /// 現在のタブ（`active`）を、表示モードに合わせて描く。
    fn view_ui(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        match self.cfg.view_mode {
            ViewMode::Live => self.editor_ui(ui, actions, true),
            ViewMode::Edit => self.editor_ui(ui, actions, false),
            ViewMode::Preview => self.preview_ui(ui, actions),
            ViewMode::Split => {
                ui.columns(2, |cols| {
                    self.editor_ui(&mut cols[0], actions, false);
                    self.preview_ui(&mut cols[1], actions);
                });
            }
        }
    }

    fn close_tab(&mut self, idx: usize) {
        if self.tabs.get(idx).map(|t| &t.path) == self.other.as_ref() {
            self.other = None;
            self.focus_right = false;
        }
        if idx < self.tabs.len() {
            self.save_tab(idx);
            self.tabs.remove(idx);
            if self.active > idx || self.active >= self.tabs.len() {
                self.active = self.active.saturating_sub(1);
            }
            self.completion = None;
        }
    }

    fn apply(&mut self, action: Action, ctx: &egui::Context) {
        match action {
            Action::Open(path, line) => self.open_note(&path, line),
            Action::OpenExternal(path) if path.ends_with(".base") && self.cfg.features.bases => {
                self.base_view = Some(crate::base_view::BaseView::new(path));
                self.canvas_view = None;
                self.show_graph = false;
            }
            Action::OpenExternal(path) if path.ends_with(".canvas") && self.cfg.features.canvas => {
                self.canvas_view = Some(crate::canvas_view::CanvasView::new(path));
                self.base_view = None;
                self.show_graph = false;
            }
            Action::OpenExternal(path) => self.open_external(&path, ctx),
            Action::FollowLink { target, from } => self.follow_link(&target, &from, ctx),
            Action::PlayAudio(path) => self.toggle_audio(&path),
            Action::Activate(i) => {
                self.active = i;
                self.show_graph = false;
                self.completion = None;
            }
            Action::Close(i) => self.close_tab(i),
            Action::Search(_) if !self.cfg.features.search => {
                self.toast("検索は設定（コアプラグイン）でオフになっています");
            }
            Action::History(_) if !self.cfg.features.file_recovery => {
                self.toast("ファイル復元は設定（コアプラグイン）でオフになっています");
            }
            Action::ToggleBookmark(_) if !self.cfg.features.bookmarks => {
                self.toast("ブックマークは設定（コアプラグイン）でオフになっています");
            }
            Action::Search(query) => {
                self.search_query = query;
                self.cfg.show_left = true;
                self.left_tab = LeftTab::Search;
            }
            Action::Dialog(d) => self.dialog = Some(d),
            Action::OpenVault(path) => self.open_vault(path, ctx),
            Action::Run(cmd) => self.run(cmd, ctx),
            Action::History(path) => self.open_history(&path),
            Action::Vim(cmd) => self.vim_command(cmd, ctx),
            Action::ToggleTask(path, line, text) => self.toggle_task(&path, line, &text, ctx),
            Action::Toast(msg) => self.toast(msg),
            Action::PasteImage(image) => self.paste_image(&image),
            Action::LinkMentions(mentions, target) => self.link_mentions(&mentions, &target),
            Action::ToggleBookmark(path) => self.toggle_bookmark(&path),
            Action::RunUser(i) => self.run_user(i, ctx),
            Action::Slash(path, pos) => {
                self.palette = Some(Palette {
                    mode: PaletteMode::Commands,
                    query: String::new(),
                    computed_for: None,
                    items: Vec::new(),
                    selected: 0,
                    slash: Some((path, pos)),
                });
            }
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        if self.dialog.is_some()
            || self.history.is_some()
            || self.settings.is_some()
            || self.trust_prompt.is_some()
            || self.commit_message.is_some()
        {
            return;
        }
        // Vimのノーマルモードで編集中は、Ctrl+D（半画面移動）をエディタに渡す
        let vim_normal = self.cfg.vim
            && self.vim.mode != vim::Mode::Insert
            && self
                .current()
                .is_some_and(|t| ctx.memory(|m| m.has_focus(editor_id(&t.path))));
        ctx.input_mut(|input| {
            // ユーザーコマンドを先に見る（修飾キーまで完全一致。組み込みより具体的な指定なので優先する）
            for (i, key) in self.user_cmds.keys.iter().enumerate() {
                if key.is_some_and(|k| external::consume_exact(input, k)) {
                    actions.push(Action::RunUser(i));
                }
            }
            for &(cmd, _, shortcut) in COMMANDS {
                if !cmd.enabled(&self.cfg.features) {
                    continue;
                }
                if vim_normal && cmd == Command::Daily {
                    continue;
                }
                if let Some((mods, key)) = shortcut {
                    if input.consume_shortcut(&KeyboardShortcut::new(mods, key)) {
                        actions.push(Action::Run(cmd));
                    }
                }
            }
        });
    }

    fn apply_style(&mut self, ctx: &egui::Context) {
        let key = (self.cfg.dark, self.cfg.font_size, self.theme.generation);
        if self.applied_style == Some(key) {
            return;
        }
        self.applied_style = Some(key);
        ctx.set_theme(if self.cfg.dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        });
        // テーマ設定ファイルの色（既定の色から作り直すので、指定を消せば元に戻る）
        ctx.style_mut_of(egui::Theme::Dark, |s| s.visuals = self.theme.visuals(true));
        ctx.style_mut_of(egui::Theme::Light, |s| {
            s.visuals = self.theme.visuals(false)
        });
        let size = self.cfg.font_size;
        ctx.all_styles_mut(|style| {
            use egui::{FontFamily::Proportional, FontId, TextStyle};
            style
                .text_styles
                .insert(TextStyle::Body, FontId::new(size - 1.0, Proportional));
            style
                .text_styles
                .insert(TextStyle::Button, FontId::new(size - 1.0, Proportional));
            style
                .text_styles
                .insert(TextStyle::Small, FontId::new(size - 3.5, Proportional));
            style
                .text_styles
                .insert(TextStyle::Heading, FontId::new(size + 6.0, Proportional));
            style.spacing.item_spacing = egui::vec2(6.0, 4.0);
            style.spacing.button_padding = egui::vec2(6.0, 3.0);
        });
    }

    // ---- 画面 ----

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let mut git_cmd = None;
        if let Some(p) = self.player.take_if(|p| p.finished()) {
            if let Some(e) = p.error() {
                self.toast(format!("再生できません: {e}"));
            }
        }
        let mut stop_audio = false;
        ui.horizontal(|ui| {
            if let Some(vault) = &self.vault {
                let name = vault
                    .root()
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned());
                ui.weak(format!(
                    "{} ・ {}ノート",
                    name.unwrap_or_default(),
                    vault.note_count()
                ));
            }
            if self.cfg.vim {
                let label = match &self.vim.cmdline {
                    Some(line) => format!(":{line}"),
                    None => format!("-- {} -- {}", self.vim.mode_label(), self.vim.pending()),
                };
                ui.label(RichText::new(label).monospace().strong());
            }
            if self.git.repo {
                let label = match self.git.changes {
                    _ if self.git.busy().is_some() => {
                        format!("git: {}", self.git.busy().unwrap_or_default())
                    }
                    Some(0) => "git: 変更なし".to_string(),
                    Some(n) => format!("git: {n}件の変更"),
                    None => "git".to_string(),
                };
                let button = egui::Button::new(RichText::new(label).weak()).frame(false);
                if ui
                    .add(button)
                    .on_hover_text("クリックでスナップショットをコミット")
                    .clicked()
                {
                    git_cmd = Some(Command::GitCommit);
                }
                // リモートとの差。クリックで送る / 取り込む
                if let Some(up) = self.git.upstream {
                    let ahead = egui::Button::new(RichText::new(format!("↑{}", up.ahead)).weak())
                        .frame(false);
                    if ui
                        .add(ahead)
                        .on_hover_text("リモートに無いコミットの数。クリックで送る（push）")
                        .clicked()
                    {
                        git_cmd = Some(Command::GitPush);
                    }
                    let behind = egui::Button::new(RichText::new(format!("↓{}", up.behind)).weak())
                        .frame(false);
                    if ui
                        .add(behind)
                        .on_hover_text("リモートにあって手元に無いコミットの数（最後に取り込んだ時点）。クリックで取り込む（pull）")
                        .clicked()
                    {
                        git_cmd = Some(Command::GitPull);
                    }
                }
            }
            for job in &self.jobs {
                ui.weak(format!("「{}」を実行中…", job.name));
            }
            if let Some(p) = &self.player {
                let label = format!("▶ {}  {}", file_name(&p.path), p.progress_label());
                if ui
                    .add(egui::Button::new(RichText::new(label).weak()).frame(false))
                    .on_hover_text("クリックで止める")
                    .clicked()
                {
                    stop_audio = true;
                }
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(250));
            }
            if let Some(err) = self.recorder.as_ref().and_then(|r| r.error()) {
                // マイクが外されたなど。そこまでの録音を保存する
                git_cmd = Some(Command::Record);
                self.toast(format!("録音が中断されました: {err}"));
            }
            if let Some(rec) = &self.recorder {
                let secs = rec.elapsed().as_secs();
                let label = RichText::new(format!("● 録音中 {}:{:02}", secs / 60, secs % 60))
                    .color(egui::Color32::from_rgb(0xe9, 0x3e, 0x3e));
                if ui
                    .add(egui::Button::new(label).frame(false))
                    .on_hover_text("クリックで録音を止めて、ノートに埋め込む")
                    .clicked()
                {
                    git_cmd = Some(Command::Record);
                }
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(tab) = self.tabs.get(self.active) {
                    ui.weak(if tab.dirty {
                        "● 未保存"
                    } else {
                        "保存済み"
                    });
                    if self.cfg.features.word_count {
                    let counts = tab.counts();
                    ui.weak(format!("{}文字 ・ {}語", counts.chars, counts.words))
                        .on_hover_text("文字数は空白を除く。単語数は、欧文は語、日本語などは1文字を1語と数える");
                    }
                    let sel = selection_bytes(ui.ctx(), tab);
                    if self.cfg.features.word_count && !sel.is_empty() {
                        let c = robsidian_core::text::count(&tab.text[sel]);
                        ui.weak(format!("選択 {}文字 ・ {}語", c.chars, c.words));
                    }
                    if let Some(vault) = self.vault.as_ref().filter(|_| self.cfg.features.backlinks) {
                        ui.weak(format!("被リンク {}", vault.backlinks(&tab.path).len()));
                    }
                    ui.weak(&tab.path);
                }
            });
        });
        if stop_audio {
            self.player = None;
        }
        if let Some(cmd) = git_cmd {
            let ctx = ui.ctx().clone();
            self.run(cmd, &ctx);
        }
    }

    fn left_panel(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        ui.add_space(4.0);
        let f = self.cfg.features;
        let tabs = [
            (LeftTab::Files, "ファイル", f.file_explorer),
            (LeftTab::Search, "検索", f.search),
            (LeftTab::Tags, "タグ", f.tags_view),
            (LeftTab::Tasks, "タスク", true),
            (LeftTab::Bookmarks, "★", f.bookmarks),
        ];
        // オフにしたタブを開いていたら、出ているタブに移る
        if !tabs.iter().any(|(t, _, on)| *on && *t == self.left_tab) {
            self.left_tab = LeftTab::Tasks;
            if let Some((t, _, _)) = tabs.iter().find(|(_, _, on)| *on) {
                self.left_tab = *t;
            }
        }
        ui.horizontal(|ui| {
            for (t, name, on) in tabs {
                if on {
                    ui.selectable_value(&mut self.left_tab, t, name);
                }
            }
        });
        ui.separator();
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
        let Some(vault) = &self.vault else { return };
        let active = self.tabs.get(self.active).map(|t| t.path.as_str());

        match self.left_tab {
            LeftTab::Files => {
                ui.horizontal(|ui| {
                    if ui
                        .button("＋ノート")
                        .on_hover_text("新規ノート (Ctrl+N)")
                        .clicked()
                    {
                        actions.push(Action::Run(Command::NewNote));
                    }
                    if ui.button("＋フォルダ").clicked() {
                        let d = Dialog::NewFolder {
                            dir: String::new(),
                            name: String::new(),
                        };
                        actions.push(Action::Dialog(d));
                    }
                    if self.cfg.features.daily_notes
                        && ui
                            .button("今日")
                            .on_hover_text("デイリーノート (Ctrl+D)")
                            .clicked()
                    {
                        actions.push(Action::Run(Command::Daily));
                    }
                });
                ui.add_space(2.0);
                if self
                    .tree
                    .as_ref()
                    .is_none_or(|(g, _)| *g != self.generation)
                {
                    self.tree = Some((self.generation, build_tree(vault)));
                }
                let Some((_, tree)) = &self.tree else { return };
                ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    tree_ui(ui, tree, 0, active, actions, vault, &mut self.hover_cache);
                });
            }
            LeftTab::Search => {
                let edit = TextEdit::singleline(&mut self.search_query)
                    .hint_text("検索  例: rust tag:memo \"完全一致\"")
                    .desired_width(f32::INFINITY);
                let response = ui.add(edit);
                if std::mem::take(&mut self.focus_search) {
                    response.request_focus();
                }
                let key = (self.search_query.clone(), self.generation);
                if self.search_key.as_ref() != Some(&key) {
                    self.search_hits = search::search(vault, &self.search_query, 200);
                    self.search_key = Some(key);
                }
                if !self.search_query.trim().is_empty() {
                    ui.weak(format!("{}件のノート", self.search_hits.len()));
                }
                ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    for hit in &self.search_hits {
                        let name = file_name(&hit.path);
                        let title = name.strip_suffix(".md").unwrap_or(name);
                        let label = ui
                            .selectable_label(
                                active == Some(hit.path.as_str()),
                                RichText::new(title).strong(),
                            )
                            .on_hover_preview(|ui| {
                                hover::note_preview(
                                    ui,
                                    vault,
                                    &mut self.hover_cache,
                                    &hit.path,
                                    None,
                                )
                            });
                        if label.clicked() {
                            let line = hit.lines.first().map(|l| l.line);
                            actions.push(Action::Open(hit.path.clone(), line));
                        }
                        for line in hit.lines.iter().take(5) {
                            let text = RichText::new(&line.text).small().weak();
                            let row = ui.add(egui::Button::new(text).frame(false));
                            if row.clicked() {
                                actions.push(Action::Open(hit.path.clone(), Some(line.line)));
                            }
                        }
                        ui.add_space(4.0);
                    }
                });
            }
            LeftTab::Tasks => {
                ui.horizontal(|ui| {
                    ui.add(
                        TextEdit::singleline(&mut self.task_filter)
                            .hint_text("絞り込み 例: tag:work 予算")
                            .desired_width(ui.available_width() - 60.0),
                    );
                    ui.toggle_value(&mut self.task_show_done, "完了");
                });
                let today = chrono::Local::now().date_naive();
                let status = if self.task_show_done {
                    "task:any"
                } else {
                    "task:todo"
                };
                let query = format!("{status} sort:due {}", self.task_filter);
                let key = (self.generation, query.clone(), today);
                if self.task_cache.as_ref().is_none_or(|(k, _)| *k != key) {
                    let tasks = match search::run_query(vault, &query, today) {
                        search::QueryResult::Tasks(t) => t,
                        search::QueryResult::Notes(_) => Vec::new(),
                    };
                    self.task_cache = Some((key, tasks));
                }
                let Some((_, tasks)) = &self.task_cache else {
                    return;
                };
                let group = |t: &search::TaskHit| -> usize {
                    match t.task.due {
                        _ if t.task.done() => 4,
                        Some(d) if d < today => 0,
                        Some(d) if d == today => 1,
                        Some(_) => 2,
                        None => 3,
                    }
                };
                const TITLES: [&str; 5] = ["期限切れ", "今日", "今後", "期日なし", "完了"];
                ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    if tasks.is_empty() {
                        ui.weak("タスクはありません");
                        ui.weak("ノートに `- [ ] やること` と書くと、ここに集まります。期日は `📅 2026-01-31` か `due:2026-01-31`");
                    }
                    for (g, title) in TITLES.iter().enumerate() {
                        let items: Vec<&search::TaskHit> = tasks.iter().filter(|t| group(t) == g).collect();
                        if items.is_empty() {
                            continue;
                        }
                        ui.add_space(4.0);
                        let heading = RichText::new(format!("{title}（{}）", items.len())).strong();
                        ui.label(if g == 0 { heading.color(ui.visuals().warn_fg_color) } else { heading });
                        for t in items {
                            ui.horizontal(|ui| {
                                let mut done = t.task.done();
                                if ui.checkbox(&mut done, "").changed() {
                                    actions.push(Action::ToggleTask(t.path.clone(), t.task.line, t.task.text.clone()));
                                }
                                let text = if t.task.text.is_empty() { "（本文なし）" } else { t.task.text.as_str() };
                                let label = if t.task.done() { RichText::new(text).weak().strikethrough() } else { RichText::new(text) };
                                let row = ui.add(egui::Button::new(label).frame(false));
                                if row.clicked() {
                                    actions.push(Action::Open(t.path.clone(), Some(t.task.line)));
                                }
                                row.on_hover_preview(|ui| hover::note_preview(ui, vault, &mut self.hover_cache, &t.path, None));
                            });
                            let note = file_name(&t.path).trim_end_matches(".md");
                            let meta = match t.task.due {
                                Some(d) => format!("{note} ・ {d}"),
                                None => note.to_string(),
                            };
                            ui.horizontal(|ui| {
                                ui.add_space(26.0);
                                ui.weak(RichText::new(meta).small());
                            });
                        }
                    }
                });
            }
            LeftTab::Bookmarks => {
                ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    if self.workspace.bookmarks.is_empty() {
                        ui.weak("ブックマークはまだありません。");
                        ui.weak("ノートの右クリックメニューやコマンドパレットから追加できます。");
                    }
                    for path in &self.workspace.bookmarks {
                        let exists = vault.note(path).is_some();
                        let title = file_name(path).trim_end_matches(".md");
                        let text = if exists {
                            RichText::new(title)
                        } else {
                            RichText::new(format!("{title}（見つかりません）")).weak()
                        };
                        let row = ui
                            .selectable_label(active == Some(path.as_str()), text)
                            .on_hover_preview(|ui| {
                                hover::note_preview(ui, vault, &mut self.hover_cache, path, None)
                            });
                        if row.clicked() && exists {
                            actions.push(Action::Open(path.clone(), None));
                        }
                        row.context_menu(|ui| {
                            if ui.button("ブックマークから外す").clicked() {
                                actions.push(Action::ToggleBookmark(path.clone()));
                            }
                        });
                    }
                });
            }
            LeftTab::Tags => {
                ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    let tags = vault.tags();
                    if tags.is_empty() {
                        ui.weak("タグはまだありません");
                    }
                    for (tag, notes) in tags {
                        if ui
                            .selectable_label(false, format!("#{tag}  ({})", notes.len()))
                            .clicked()
                        {
                            actions.push(Action::Search(format!("tag:{tag}")));
                        }
                    }
                });
            }
        }
    }

    fn right_panel(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let (Some(vault), Some(tab)) = (&self.vault, self.tabs.get_mut(self.active)) else {
            return;
        };
        let cache = &mut self.hover_cache;
        let mentions_cache = &mut self.mentions;
        let related_cache = &mut self.related;
        let generation = self.generation;
        let features = self.cfg.features;
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
        ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            if features.backlinks {
            ui.add_space(4.0);
            let backlinks = vault.backlinks(&tab.path);
            ui.strong(format!("バックリンク ({})", backlinks.len()));
            ui.add_space(2.0);
            if backlinks.is_empty() {
                ui.weak("このノートへのリンクはありません");
            }
            let mut last_source = "";
            for link in backlinks {
                if *link.source != *last_source {
                    last_source = &link.source;
                    let name = file_name(&link.source);
                    let title = name.strip_suffix(".md").unwrap_or(name);
                    if ui
                        .selectable_label(false, title)
                        .on_hover_preview(|ui| hover::note_preview(ui, vault, cache, &link.source, None))
                        .clicked()
                    {
                        actions.push(Action::Open(link.source.to_string(), Some(link.line)));
                    }
                }
                let text = RichText::new(vault.line(&link.source, link.line))
                    .small()
                    .weak();
                if ui.add(egui::Button::new(text).frame(false)).clicked() {
                    actions.push(Action::Open(link.source.to_string(), Some(link.line)));
                }
            }

            ui.add_space(6.0);
            let header = egui::CollapsingHeader::new(RichText::new("未リンクの言及").strong())
                .id_salt("unlinked-mentions")
                .default_open(false)
                .show(ui, |ui| {
                    if mentions_cache
                        .as_ref()
                        .is_none_or(|(g, p, _)| *g != generation || *p != tab.path)
                    {
                        let found = vault.unlinked_mentions(&tab.path);
                        *mentions_cache = Some((generation, tab.path.clone(), found));
                    }
                    let Some((_, _, mentions)) = mentions_cache.as_ref() else {
                        return;
                    };
                    if mentions.is_empty() {
                        ui.weak("このノートの名前は、リンクにならずに書かれていません");
                        return;
                    }
                    ui.horizontal(|ui| {
                        ui.weak(format!("{}件", mentions.len()));
                        if ui
                            .small_button("すべてリンクにする")
                            .on_hover_text("言及をすべて [[リンク]] に書き換えます（各ノートの履歴から戻せます）")
                            .clicked()
                        {
                            actions.push(Action::LinkMentions(mentions.clone(), tab.path.clone()));
                        }
                    });
                    let mut last_source = "";
                    for m in mentions {
                        if m.source != last_source {
                            last_source = &m.source;
                            let title = file_name(&m.source).trim_end_matches(".md");
                            let row = ui
                                .selectable_label(false, title)
                                .on_hover_preview(|ui| hover::note_preview(ui, vault, cache, &m.source, None));
                            if row.clicked() {
                                actions.push(Action::Open(m.source.clone(), Some(m.line)));
                            }
                        }
                        ui.horizontal(|ui| {
                            if ui.small_button("リンク").clicked() {
                                actions.push(Action::LinkMentions(vec![m.clone()], tab.path.clone()));
                            }
                            let job = mention_job(ui, m);
                            if ui.add(egui::Button::new(job).frame(false)).clicked() {
                                actions.push(Action::Open(m.source.clone(), Some(m.line)));
                            }
                        });
                    }
                });
            // 閉じている間は結果を捨てる（大きなvaultで無駄に走査しないよう、開いているときだけ計算する）
            if header.body_returned.is_none() {
                *mentions_cache = None;
            }
            }

            if features.related {
            ui.add_space(6.0);
            let header = egui::CollapsingHeader::new(RichText::new("関連ノート").strong())
                .id_salt("related-notes")
                .default_open(true)
                .show(ui, |ui| {
                    // 別のノートに移ったら計算し直す。同じノートの編集中は、索引が変わっても数秒は使い回す
                    // （密な大きいvaultでは1回に数十ミリ秒かかるため、保存のたびには計算しない）
                    let stale = related_cache.as_ref().is_none_or(|(g, p, at, _)| {
                        *p != tab.path || (*g != generation && at.elapsed() > RELATED_REFRESH)
                    });
                    if stale {
                        let found = robsidian_core::related::related(vault, &tab.path, 8);
                        *related_cache = Some((generation, tab.path.clone(), Instant::now(), found));
                    }
                    let Some((_, _, _, found)) = related_cache.as_ref() else {
                        return;
                    };
                    if found.is_empty() {
                        ui.weak("リンク先・リンク元・タグが共通するノートはありません");
                        return;
                    }
                    for r in found {
                        let title = file_name(&r.path).trim_end_matches(".md");
                        let mut why = Vec::new();
                        if !r.links.is_empty() {
                            let names: Vec<&str> = r
                                .links
                                .iter()
                                .take(5)
                                .map(|l| file_name(l).trim_end_matches(".md"))
                                .collect();
                            let more = if r.links.len() > 5 { " ほか" } else { "" };
                            why.push(format!("共通のつながり: {}{more}", names.join("、")));
                        }
                        if !r.tags.is_empty() {
                            let tags: Vec<String> = r.tags.iter().map(|t| format!("#{t}")).collect();
                            why.push(format!("共通のタグ: {}", tags.join(" ")));
                        }
                        let row = ui
                            .selectable_label(false, title)
                            .on_hover_preview(|ui| {
                                ui.weak(why.join("\n"));
                                ui.separator();
                                hover::note_preview(ui, vault, cache, &r.path, None);
                            });
                        if row.clicked() {
                            actions.push(Action::Open(r.path.clone(), None));
                        }
                        ui.label(RichText::new(why.join(" ・ ")).small().weak());
                    }
                });
            if header.body_returned.is_none() {
                *related_cache = None;
            }
            }

            // 発リンク（このノートから出ているリンク。存在しないリンク先は薄く表示し、クリックで作る）
            if features.outgoing_links {
            ui.add_space(6.0);
            let outgoing = vault.outgoing(&tab.path);
            let mut seen = std::collections::HashSet::new();
            let targets: Vec<(String, Option<&str>)> = outgoing
                .iter()
                .filter(|(l, _)| !l.target.is_empty())
                .filter_map(|(l, dst)| {
                    let key = dst.map_or_else(|| l.target.to_lowercase(), str::to_string);
                    seen.insert(key).then(|| (l.target.clone(), *dst))
                })
                .collect();
            egui::CollapsingHeader::new(RichText::new(format!("発リンク ({})", targets.len())).strong())
                .id_salt("outgoing-links")
                .default_open(false)
                .show(ui, |ui| {
                    if targets.is_empty() {
                        ui.weak("このノートからのリンクはありません");
                    }
                    for (target, dst) in &targets {
                        match dst {
                            Some(path) => {
                                let title = file_name(path).trim_end_matches(".md");
                                let row = ui
                                    .selectable_label(false, title)
                                    .on_hover_preview(|ui| hover::note_preview(ui, vault, cache, path, None));
                                if row.clicked() {
                                    actions.push(Action::Open(path.to_string(), None));
                                }
                            }
                            None => {
                                let label = RichText::new(format!("{target}（未作成）")).weak();
                                if ui
                                    .selectable_label(false, label)
                                    .on_hover_text("クリックでノートを作ります")
                                    .clicked()
                                {
                                    actions.push(Action::FollowLink {
                                        target: target.clone(),
                                        from: tab.path.clone(),
                                    });
                                }
                            }
                        }
                    }
                });

            }

            // 脚注（あるときだけ）。定義の行へ移動する
            let notes = if features.footnotes {
                robsidian_core::text::footnotes(&tab.text)
            } else {
                Vec::new()
            };
            if !notes.is_empty() {
                ui.add_space(6.0);
                egui::CollapsingHeader::new(RichText::new(format!("脚注 ({})", notes.len())).strong())
                    .id_salt("footnotes")
                    .default_open(true)
                    .show(ui, |ui| {
                        for note in &notes {
                            let text = if note.text.is_empty() {
                                "（定義がありません）".to_string()
                            } else {
                                note.text.clone()
                            };
                            let label = format!("[^{}] {text}", note.id);
                            let row = ui.selectable_label(false, label).on_hover_text(format!(
                                "本文から{}か所で参照",
                                note.refs.len()
                            ));
                            if row.clicked() {
                                let line = note.line.or_else(|| note.refs.first().copied());
                                actions.push(Action::Open(tab.path.clone(), line));
                            }
                        }
                    });
            }

            if !features.outline {
                return;
            }
            ui.add_space(10.0);
            ui.separator();
            ui.strong("アウトライン");
            ui.add_space(2.0);
            if tab.outline.as_ref().is_none_or(|(v, _)| *v != tab.version) {
                tab.outline = Some((tab.version, parse::parse(&tab.text).headings));
            }
            let headings = tab.outline.as_ref().map_or(&[][..], |(_, h)| h.as_slice());
            if headings.is_empty() {
                ui.weak("見出しはありません");
            }
            for h in headings {
                ui.horizontal(|ui| {
                    ui.add_space(f32::from(h.level.saturating_sub(1)) * 10.0);
                    if ui.selectable_label(false, &h.text).clicked() {
                        actions.push(Action::Open(tab.path.clone(), Some(h.line)));
                    }
                });
            }
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        ui.horizontal(|ui| {
            if ui
                .selectable_label(self.cfg.show_left, "一覧")
                .on_hover_text("サイドバー (Ctrl+\\)")
                .clicked()
            {
                self.cfg.show_left = !self.cfg.show_left;
            }
            let mode = self.cfg.view_mode;
            if ui
                .button(mode.label())
                .on_hover_text("表示切替 (Ctrl+E)")
                .clicked()
            {
                self.cfg.view_mode = mode.next();
            }
            if ui
                .selectable_label(self.other.is_some(), "分割")
                .on_hover_text("ペインを左右に分割（もう一度押すと閉じる）")
                .clicked()
            {
                self.toggle_split();
            }
            if self.cfg.features.graph
                && ui
                    .selectable_label(self.show_graph, "グラフ")
                    .on_hover_text("Ctrl+G")
                    .clicked()
            {
                self.show_graph = !self.show_graph;
            }
            ui.separator();
            ScrollArea::horizontal().id_salt("tabs").show(ui, |ui| {
                for (i, tab) in self.tabs.iter().enumerate() {
                    let selected = i == self.active && !self.show_graph;
                    let mark = if tab.dirty { " ●" } else { "" };
                    let label = ui
                        .selectable_label(selected, format!("{}{mark}", tab.title()))
                        .on_hover_text(&tab.path);
                    if label.clicked() {
                        actions.push(Action::Activate(i));
                    }
                    if label.middle_clicked() {
                        actions.push(Action::Close(i));
                    }
                    label.context_menu(|ui| {
                        if ui.button("閉じる").clicked() {
                            actions.push(Action::Close(i));
                        }
                        if self.cfg.features.bookmarks
                            && ui.button("ブックマークに追加 / 解除").clicked()
                        {
                            actions.push(Action::ToggleBookmark(tab.path.clone()));
                        }
                        if self.cfg.features.file_recovery && ui.button("履歴…").clicked() {
                            actions.push(Action::History(tab.path.clone()));
                        }
                        if ui.button("改名/移動…").clicked() {
                            let d = Dialog::Rename {
                                path: tab.path.clone(),
                                new: tab.path.clone(),
                            };
                            actions.push(Action::Dialog(d));
                        }
                    });
                    if selected
                        && ui
                            .small_button("×")
                            .on_hover_text("閉じる (Ctrl+W)")
                            .clicked()
                    {
                        actions.push(Action::Close(i));
                    }
                }
            });
        });
    }

    fn central(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        if self.vault.is_none() {
            return self.welcome(ui, actions);
        }
        self.toolbar(ui, actions);
        ui.separator();

        if let (Some(view), Some(vault)) = (self.canvas_view.as_mut(), self.vault.as_ref()) {
            use crate::canvas_view::Outcome;
            let colors = self.theme.colors(self.cfg.dark);
            match view.show(ui, vault, &colors) {
                Some(Outcome::Open(path)) => {
                    self.canvas_view = None;
                    actions.push(Action::Open(path, None));
                }
                Some(Outcome::OpenUrl(url)) => ui.ctx().open_url(egui::OpenUrl::new_tab(url)),
                Some(Outcome::OpenExternal(path)) => {
                    let ctx = ui.ctx().clone();
                    self.open_external(&path, &ctx);
                }
                Some(Outcome::Close) => self.canvas_view = None,
                None => {}
            }
            // 編集した内容をファイルに書く
            if let (Some(view), Some(vault)) = (self.canvas_view.as_mut(), self.vault.as_mut()) {
                if let Some(json) = view.take_save() {
                    match vault.write_file(&view.path, json.as_bytes()) {
                        Ok(()) => view.saved(vault),
                        Err(e) => actions
                            .push(Action::Toast(format!("キャンバスを保存できません: {e:#}"))),
                    }
                }
            }
            return;
        }
        if let (Some(view), Some(vault)) = (self.base_view.as_mut(), self.vault.as_ref()) {
            use crate::base_view::Outcome;
            match view.show(ui, vault, self.generation) {
                Some(Outcome::Open(path)) => {
                    self.base_view = None;
                    actions.push(Action::Open(path, None));
                }
                Some(Outcome::OpenExternal(path)) => {
                    let ctx = ui.ctx().clone();
                    self.open_external(&path, &ctx);
                }
                Some(Outcome::Close) => self.base_view = None,
                None => {}
            }
            return;
        }

        if self.show_graph {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.graph.local, true, "ローカル（現在のノート周辺）");
                ui.selectable_value(&mut self.graph.local, false, "vault全体");
                ui.weak("ドラッグで移動・ホイールで拡大縮小・クリックで開く");
            });
            if let Some(vault) = &self.vault {
                let current = self.tabs.get(self.active).map(|t| t.path.as_str());
                if let Some(path) = self.graph.show(ui, vault, self.generation, current) {
                    actions.push(Action::Open(path, None));
                }
            }
            return;
        }

        if self.tabs.is_empty() {
            ui.add_space(ui.available_height() * 0.3);
            ui.vertical_centered(|ui| {
                ui.heading("ノートが開かれていません");
                ui.add_space(8.0);
                ui.weak("Ctrl+P  ノートを開く / 作る");
                ui.weak("Ctrl+N  新規ノート");
                ui.weak("Ctrl+D  今日のデイリーノート");
                ui.weak("Ctrl+Shift+F  全文検索");
                ui.weak("Ctrl+Shift+P  コマンドパレット");
            });
            return;
        }
        self.active = self.active.min(self.tabs.len() - 1);
        self.conflict_banner(ui);
        if self.cfg.view_mode != ViewMode::Preview {
            self.find_bar(ui);
        }
        let other = self
            .other
            .as_ref()
            .and_then(|p| self.tabs.iter().position(|t| &t.path == p))
            .filter(|&i| i != self.active);
        let Some(other) = other else {
            return self.view_ui(ui, actions);
        };
        // 左右に分けて、フォーカスのあるペインとフォーカスの無いペインを描く
        self.pane_clicked = false;
        let right_focused = self.focus_right;
        ui.columns(2, |cols| {
            let (left, right) = cols.split_at_mut(1);
            let (focused_ui, other_ui) = if right_focused {
                (&mut right[0], &mut left[0])
            } else {
                (&mut left[0], &mut right[0])
            };
            self.pane_ui(focused_ui, None, actions);
            self.pane_ui(other_ui, Some(other), actions);
        });
        // フォーカスの無いペインのエディタをクリックしたら、そちらに移る
        let other_path = self.tabs[other].path.clone();
        if self.pane_clicked || ui.ctx().memory(|m| m.has_focus(editor_id(&other_path))) {
            self.swap_panes();
        }
    }

    fn welcome(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        ui.add_space(ui.available_height() * 0.25);
        ui.vertical_centered(|ui| {
            ui.heading("Robsidian");
            ui.weak("Markdownフォルダ（Obsidianのvaultもそのまま）を開いてください");
            ui.add_space(12.0);
            if ui.button("vaultフォルダを開く…").clicked() {
                actions.push(Action::Run(Command::OpenVault));
            }
            if !self.cfg.recent_vaults.is_empty() {
                ui.add_space(16.0);
                ui.weak("最近開いたvault");
                for path in &self.cfg.recent_vaults {
                    if ui.link(path.display().to_string()).clicked() {
                        actions.push(Action::OpenVault(path.clone()));
                    }
                }
            }
        });
    }

    fn editor_ui(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>, live: bool) {
        let Self {
            tabs,
            vault,
            completion,
            completion_dismissed,
            cfg,
            active,
            deferred_events,
            ime_composing,
            hover_cache,
            embed_cache,
            embed_heights,
            find: find_bar,
            vim: vim_state,
            theme,
            callout_folds,
            props_panel,
            player,
            ..
        } = self;
        let (Some(tab), Some(vault)) = (tabs.get_mut(*active), vault.as_ref()) else {
            return;
        };
        let ctx = ui.ctx().clone();
        let id = editor_id(&tab.path);
        let size = cfg.font_size;
        let colors = theme.colors(cfg.dark);

        // 補完ポップアップが開いている間は、決定/移動キーをエディタより先に受け取る
        let mut accept = false;
        if let Some(c) = completion.as_mut().filter(|c| !c.items.is_empty()) {
            ctx.input_mut(|input| {
                if input.consume_key(Modifiers::NONE, Key::ArrowDown) {
                    c.selected = (c.selected + 1) % c.items.len();
                }
                if input.consume_key(Modifiers::NONE, Key::ArrowUp) {
                    c.selected = (c.selected + c.items.len() - 1) % c.items.len();
                }
                accept = input.consume_key(Modifiers::NONE, Key::Enter)
                    | input.consume_key(Modifiers::NONE, Key::Tab);
                if input.consume_key(Modifiers::NONE, Key::Escape) {
                    *completion_dismissed = Some(c.start);
                }
            });
        }

        let mut scroll_to_cursor: Option<CCursor> = None;
        // ---- Vimキーバインド ----
        let vim_on = cfg.vim;
        if vim_state.helix != cfg.helix {
            // 流派を切り替えたら、途中のキーや選択を捨てる
            *vim_state = Vim::default();
            vim_state.helix = cfg.helix;
        }
        if vim_on && ctx.memory(|m| m.has_focus(id)) {
            let normal = vim_state.mode != vim::Mode::Insert;
            let mut keys = Vec::new();
            ctx.input_mut(|input| {
                input.events.retain(|e| {
                    if !normal {
                        // 挿入モードでは Esc（と Ctrl+[）だけを受け取る
                        let esc = matches!(e, egui::Event::Key { key: Key::Escape, pressed: true, .. })
                            || matches!(e, egui::Event::Key { key: Key::OpenBracket, pressed: true, modifiers, .. } if modifiers.command_only());
                        if esc {
                            keys.push(vim::Key::Esc);
                        }
                        return !esc;
                    }
                    match vim_keys(e) {
                        Some(ks) => {
                            keys.extend(ks);
                            false
                        }
                        // ノーマルモードでは、本文を直接変える入力をエディタに渡さない
                        None => !changes_text(e) || is_navigation(e),
                    }
                });
            });
            if !keys.is_empty() {
                let mut state = TextEdit::load_state(&ctx, id).unwrap_or_default();
                let range = state.cursor.char_range().unwrap_or_default();
                let (primary, secondary) = (
                    editor::byte_of_char(&tab.text, range.primary.index.0),
                    editor::byte_of_char(&tab.text, range.secondary.index.0),
                );
                // エンジンが置いた選択のままなら、エンジンの頭の位置を使う（選択の表示では、
                // 主の端が頭の次の文字になっているため、そのまま読むと1文字ずつずれる）
                let mut cursor = match vim_state.synced {
                    Some((p, s, head)) if (p, s) == (primary, secondary) => {
                        head.min(tab.text.len())
                    }
                    _ => primary,
                };
                // Vim のノーマルモードではカーソルを改行の上に置かない（Helix は行末の改行も選べる）
                if vim_state.mode == vim::Mode::Normal && !vim_state.helix {
                    cursor = vim::clamp_normal(&tab.text, cursor);
                }
                let mut undoer = state.undoer();
                let mut changed = false;
                for key in keys {
                    let before = (
                        CCursorRange::one(CCursor::new(editor::char_of_byte(&tab.text, cursor))),
                        tab.text.clone(),
                    );
                    let fx = vim_state.handle(&mut tab.text, &mut cursor, key);
                    if fx.changed {
                        // 1キーの変更を1回の「元に戻す」にする
                        undoer.add_undo(&before);
                        changed = true;
                    }
                    if let Some(y) = fx.yank {
                        ctx.copy_text(y);
                    }
                    match fx.cmd {
                        Some(vim::Cmd::Undo(n) | vim::Cmd::Redo(n)) => {
                            let redo = matches!(fx.cmd, Some(vim::Cmd::Redo(_)));
                            for _ in 0..n {
                                let current = (
                                    CCursorRange::one(CCursor::new(editor::char_of_byte(
                                        &tab.text, cursor,
                                    ))),
                                    tab.text.clone(),
                                );
                                let restored = if redo {
                                    undoer.redo(&current).cloned()
                                } else {
                                    undoer.undo(&current).cloned()
                                };
                                let Some((range, text)) = restored else { break };
                                tab.text = text;
                                cursor = vim::clamp_normal(
                                    &tab.text,
                                    editor::byte_of_char(&tab.text, range.primary.index.0),
                                );
                                vim_state.anchor = cursor;
                                changed = true;
                            }
                        }
                        Some(cmd) => actions.push(Action::Vim(cmd)),
                        None => {}
                    }
                }
                state.set_undoer(undoer);
                let to_char = |b: usize| CCursor::new(editor::char_of_byte(&tab.text, b));
                let range = match vim_state.selection(&tab.text, cursor) {
                    Some((a, b)) if cursor < vim_state.anchor => {
                        CCursorRange::two(to_char(b), to_char(a))
                    }
                    Some((a, b)) => CCursorRange::two(to_char(a), to_char(b)),
                    None => CCursorRange::one(to_char(cursor)),
                };
                state.cursor.set_char_range(Some(range));
                state.store(&ctx, id);
                vim_state.synced = Some((
                    editor::byte_of_char(&tab.text, range.primary.index.0),
                    editor::byte_of_char(&tab.text, range.secondary.index.0),
                    cursor,
                ));
                scroll_to_cursor = Some(to_char(cursor));
                tab.cursor = cursor;
                if changed {
                    // 版を上げて、プレビューやアウトラインを作り直させる
                    tab.version += 1;
                    tab.dirty = true;
                    tab.edited_at = Some(Instant::now());
                }
            }
        }

        // ---- 編集補助（リストの継続、インデント、チェックボックス、強調、括弧、貼り付け） ----
        let popup_open = completion.as_ref().is_some_and(|c| !c.items.is_empty());
        let helpers = !vim_on || vim_state.mode == vim::Mode::Insert;
        if helpers && ctx.memory(|m| m.has_focus(id)) && !popup_open && !*ime_composing {
            let mut state = TextEdit::load_state(&ctx, id).unwrap_or_default();
            let range = state.cursor.char_range();
            let chars = range.map_or(0..0, |r| {
                let r = r.as_sorted_char_range();
                r.start.0..r.end.0
            });
            let sel = editor::byte_of_char(&tab.text, chars.start)
                ..editor::byte_of_char(&tab.text, chars.end);
            let text = &tab.text;
            let (smart_lists, auto_pair) = (cfg.smart_lists, cfg.auto_pair);
            let picked = ctx.input_mut(|input| {
                take_edit_event(&mut input.events, deferred_events, |key| match key {
                    EditKey::Enter if !smart_lists => None,
                    EditKey::Tab(_) if !smart_lists => None,
                    EditKey::Type(_) if !auto_pair => None,
                    EditKey::Enter => edit::on_enter(text, sel.clone()).map(Some),
                    EditKey::Tab(outdent) => edit::on_tab(text, sel.clone(), *outdent).map(Some),
                    EditKey::Checkbox => Some(Some(edit::toggle_checkbox(text, sel.clone()))),
                    EditKey::Wrap(marker) => {
                        Some(Some(edit::toggle_wrap(text, sel.clone(), marker)))
                    }
                    EditKey::Type(ch) => edit::on_type(text, sel.clone(), *ch).map(Some),
                    EditKey::Paste(pasted) => edit::paste_url(text, sel.clone(), pasted).map(Some),
                    EditKey::PasteImage(_) => Some(None),
                })
            });
            match picked {
                Some((_, Some(change))) => {
                    // 変更前の状態をTextEditの「元に戻す」履歴に積んでおく（Ctrl+Zで戻せるように）
                    let mut undoer = state.undoer();
                    undoer.add_undo(&(range.unwrap_or_default(), tab.text.clone()));
                    state.set_undoer(undoer);
                    change.apply(&mut tab.text);
                    let a = CCursor::new(editor::char_of_byte(&tab.text, change.select.start));
                    let b = CCursor::new(editor::char_of_byte(&tab.text, change.select.end));
                    let new_range = if a == b {
                        CCursorRange::one(a)
                    } else {
                        CCursorRange::two(a, b)
                    };
                    state.cursor.set_char_range(Some(new_range));
                    state.store(&ctx, id);
                    tab.cursor = change.select.end;
                    tab.dirty = true;
                    tab.edited_at = Some(Instant::now());
                    tab.version += 1;
                }
                Some((EditKey::PasteImage(image), None)) => {
                    tab.cursor = sel.end;
                    actions.push(Action::PasteImage(image));
                }
                _ => {}
            }
        }

        if tab.focus {
            // ダイアログを閉じた直後のフレームでは、egui が前のフレームのモーダルより下へのフォーカスを止める。
            // その間は要求を保留し、次のフレームで要求し直す
            if ctx.memory(|m| m.is_above_modal_layer(ui.layer_id())) {
                tab.focus = false;
                ctx.memory_mut(|m| m.request_focus(id));
            } else {
                ctx.request_repaint();
            }
        }
        // ライブ表示ではfrontmatterを畳むので、カーソルが入り込まないよう本文の先頭へ寄せる
        let fm_end = if live {
            robsidian_core::props::block(&tab.text).map(|r| r.end)
        } else {
            None
        };
        if let (Some(end), true) = (fm_end, ctx.memory(|m| m.has_focus(id))) {
            let mut state = TextEdit::load_state(&ctx, id).unwrap_or_default();
            let end_char = editor::char_of_byte(&tab.text, end);
            if state
                .cursor
                .char_range()
                .is_none_or(|r| r.primary.index.0 < end_char)
            {
                state
                    .cursor
                    .set_char_range(Some(CCursorRange::one(CCursor::new(end_char))));
                state.store(&ctx, id);
                tab.cursor = end;
            }
        }
        if let Some(byte) = tab.place_cursor.take() {
            let ccursor = CCursor::new(editor::char_of_byte(&tab.text, byte));
            let mut state = TextEdit::load_state(&ctx, id).unwrap_or_default();
            state
                .cursor
                .set_char_range(Some(CCursorRange::one(ccursor)));
            state.store(&ctx, id);
            tab.cursor = byte;
        }
        let mut scroll_to = scroll_to_cursor.take();
        if let Some(range) = tab.select.take() {
            let a = CCursor::new(editor::char_of_byte(&tab.text, range.start));
            let b = CCursor::new(editor::char_of_byte(&tab.text, range.end));
            let mut state = TextEdit::load_state(&ctx, id).unwrap_or_default();
            state.cursor.set_char_range(Some(if a == b {
                CCursorRange::one(a)
            } else {
                CCursorRange::two(a, b)
            }));
            state.store(&ctx, id);
            tab.cursor = range.end;
            scroll_to = Some(b);
        }
        if let Some(line) = tab.jump.take() {
            let byte = editor::line_start(&tab.text, line);
            let ccursor = CCursor::new(editor::char_of_byte(&tab.text, byte));
            let mut state = TextEdit::load_state(&ctx, id).unwrap_or_default();
            state
                .cursor
                .set_char_range(Some(CCursorRange::one(ccursor)));
            state.store(&ctx, id);
            ctx.memory_mut(|m| m.request_focus(id));
            scroll_to = Some(ccursor);
        }

        // Live Preview: カーソル（選択範囲）が触れている要素だけ記法を見せる。
        // 組版はTextEditが入力を処理する前の状態で決めるので、カーソルが動いた直後は1フレーム遅れる
        // （下で再描画を要求して追従させる）。
        let span = |range: Option<CCursorRange>| {
            range.map(|r| {
                let (a, b) = (r.primary.index.0, r.secondary.index.0);
                (a.min(b), a.max(b))
            })
        };
        let focused = ctx.memory(|m| m.has_focus(id));
        let cursor = if live && focused {
            span(TextEdit::load_state(&ctx, id).and_then(|s| s.cursor.char_range()))
        } else {
            None
        };

        // 画像の埋め込みは、読み込み済みならその場に表示する
        let max_width =
            (ui.available_width() - 2.0 * side_margin(ui.available_width(), cfg) - 24.0).max(80.0);
        let pending = std::cell::Cell::new(false);
        let note_path = tab.path.clone();
        let embed_key = |path: &str, heading: Option<&str>| {
            format!("{path}#{}@{}", heading.unwrap_or(""), max_width as u32)
        };
        let note_embed = |target: &str| -> Option<f32> {
            let (note, heading) = hover::split_target(target);
            let path = vault.resolve(note, &note_path)?;
            // 自分自身の埋め込みは無限に入れ子になるので描かない
            (path != note_path).then(|| {
                embed_heights
                    .get(&embed_key(path, heading))
                    .copied()
                    .unwrap_or(EMBED_ESTIMATE)
            })
        };
        let load_image = |target: &str| -> Option<(egui::TextureId, egui::Vec2)> {
            let rel = vault
                .resolve_file(target, &note_path)
                .filter(|p| preview::is_image(p))?;
            let uri = preview::file_uri(vault, rel);
            let options = egui::TextureOptions::LINEAR;
            match ctx.try_load_texture(&uri, options, egui::load::SizeHint::default()) {
                Ok(egui::load::TexturePoll::Ready { texture }) => {
                    let scale = (max_width / texture.size.x)
                        .min(420.0 / texture.size.y)
                        .min(1.0);
                    Some((texture.id, texture.size * scale))
                }
                Ok(egui::load::TexturePoll::Pending { .. }) => {
                    pending.set(true);
                    None
                }
                Err(_) => None,
            }
        };
        let image_size = |target: &str| load_image(target).map(|(_, size)| size);

        let (marks, current_mark) = find_bar
            .as_ref()
            .filter(|f| f.key.as_ref().is_some_and(|k| k.0 == tab.path))
            .map_or((&[][..], None), |f| (&f.matches[..], f.current));
        let marks_key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            // テーマの色が変わったら組版をやり直す
            (marks, current_mark, theme.generation).hash(&mut h);
            h.finish()
        };
        let key: LayoutKey = (size, cfg.dark, live, cursor, max_width as u32, marks_key);
        if tab.layout_key != Some(key) || tab.layout_stale {
            tab.layout_key = Some(key);
            tab.layout = editor::Layout::default();
        }
        let Tab { text, layout, .. } = tab;
        let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
            if layout.job.text != buf.as_str() || layout.job.sections.is_empty() {
                let math_size = |tex: &str, display: bool| {
                    let b = math_view::layout(
                        &ctx,
                        tex,
                        math_font(size, display),
                        display,
                        colors.text,
                    );
                    Some(b.size())
                };
                let fold_path = tab.path.clone();
                let callout_open =
                    |line: usize| callout_folds.get(&(fold_path.clone(), line)).copied();
                let options = editor::Options {
                    size,
                    colors: &colors,
                    live,
                    cursor,
                    image: &image_size,
                    note_embed: &note_embed,
                    math: &math_size,
                    callout_open: &callout_open,
                    hide_frontmatter: live && cfg.features.properties,
                    marks,
                    current_mark,
                };
                *layout = editor::layout(buf.as_str(), &options);
            }
            let mut job = layout.job.clone();
            job.wrap.max_width = wrap_width;
            ui.fonts_mut(|f| f.layout_job(job))
        };

        let side = side_margin(ui.available_width(), cfg);
        let block_cursor = vim_on && vim_state.mode != vim::Mode::Insert;
        let mut props_changed: Option<String> = None;
        let (output, background, painter) = ScrollArea::vertical()
            .id_salt(("editor-scroll", id))
            .auto_shrink(false)
            .show(ui, |ui| {
                let (output, background) = egui::Frame::NONE
                    .inner_margin(egui::Margin {
                        left: 0,
                        right: 0,
                        top: 12,
                        bottom: 12,
                    })
                    .show(ui, |ui| {
                        ui.horizontal_top(|ui| {
                            ui.add_space(side);
                            ui.vertical(|ui| {
                                ui.set_width((ui.available_width() - side).max(80.0));
                                // 文字より後ろに描く装飾（コードブロックの背景）の場所を先に確保しておく
                                if live && cfg.features.properties {
                                    if let Some(new) =
                                        props_panel.show(ui, &note_path, text, &colors)
                                    {
                                        props_changed = Some(new);
                                    }
                                }
                                let background = ui.painter().add(egui::Shape::Noop);
                                if block_cursor {
                                    // ノーマルモードは下でブロック型のカーソルを描く
                                    ui.visuals_mut().text_cursor.stroke.width = 0.0;
                                }
                                let output = TextEdit::multiline(text)
                                    .id(id)
                                    .frame(egui::Frame::NONE)
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(30)
                                    .event_filter(egui::EventFilter {
                                        tab: true,
                                        horizontal_arrows: true,
                                        vertical_arrows: true,
                                        // Vimでは Esc でノーマルモードに戻るので、フォーカスを手放さない
                                        escape: vim_on,
                                    })
                                    .layouter(&mut layouter)
                                    .show(ui);
                                (output, background)
                            })
                            .inner
                        })
                        .inner
                    })
                    .inner;
                if let Some(ccursor) = scroll_to {
                    let rect = output.galley.pos_from_cursor(ccursor);
                    let rect = rect
                        .translate(output.galley_pos.to_vec2())
                        .expand2(egui::vec2(0.0, 60.0));
                    ui.scroll_to_rect(rect, Some(Align::Center));
                }
                (output, background, ui.painter().clone())
            })
            .inner;
        if let Some(new) = props_changed {
            // プロパティ欄での変更。本文側のカーソルは、frontmatterの長さの変化だけずらす
            let delta = new.len() as isize - tab.text.len() as isize;
            let cursor = (tab.cursor as isize + delta).clamp(0, new.len() as isize) as usize;
            let cursor = (0..=cursor)
                .rev()
                .find(|&c| new.is_char_boundary(c))
                .unwrap_or(0);
            replace_text_undoable(&ctx, tab, new, cursor..cursor);
        }
        tab.layout_stale = pending.get();
        if tab.layout_stale {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        if live && response_focus(&output) && span(output.cursor_range) != cursor {
            ctx.request_repaint();
        }

        // ---- 装飾の上描き ----
        let clip = painter.clip_rect();
        let origin = output.galley_pos.to_vec2();
        let left = output.galley_pos.x;
        // 右端はエディタ自身の幅で決める（分割表示で隣の列にはみ出さないように）
        let right = output.response.response.rect.right().max(left + 40.0);
        let row = |ch: usize| {
            output
                .galley
                .pos_from_cursor(CCursor::new(ch))
                .translate(origin)
        };
        let mut behind = Vec::new();
        let mut toggle = None;
        let mut fold_toggle = None;
        for deco in &tab.layout.decos {
            let (a, b) = (row(deco.chars.start), row(deco.chars.end));
            if b.bottom() < clip.top() || a.top() > clip.bottom() {
                continue;
            }
            let span = egui::Rect::from_min_max(
                a.left_top(),
                egui::pos2(b.right().max(a.left()), a.bottom()),
            );
            match &deco.kind {
                editor::DecoKind::Code => {
                    let rect = egui::Rect::from_min_max(
                        egui::pos2(left - 10.0, a.top() - 3.0),
                        egui::pos2(right + 10.0, b.bottom() + 3.0),
                    );
                    behind.push(egui::Shape::rect_filled(rect, 0.0, colors.code_bg));
                }
                editor::DecoKind::Bullet => {
                    painter.circle_filled(span.center(), size * 0.17, colors.dim);
                }
                editor::DecoKind::Quote => {
                    let x = left + 2.0;
                    let stroke = egui::Stroke::new(3.0, colors.dim.gamma_multiply(0.7));
                    painter
                        .line_segment([egui::pos2(x, a.top()), egui::pos2(x, b.bottom())], stroke);
                }
                editor::DecoKind::Rule => {
                    let y = a.center().y;
                    let stroke = egui::Stroke::new(1.0, colors.dim.gamma_multiply(0.7));
                    painter.line_segment([egui::pos2(left, y), egui::pos2(right, y)], stroke);
                }
                editor::DecoKind::Checkbox(status) => {
                    let side = size * 0.95;
                    let rect = egui::Rect::from_center_size(span.center(), egui::vec2(side, side));
                    let (c, u) = (rect.center(), side * 0.5);
                    let mark = egui::Stroke::new(1.8, colors.code_bg);
                    match status {
                        ' ' => {
                            let stroke = egui::Stroke::new(1.3, colors.dim);
                            painter.rect_stroke(rect, 3.0, stroke, egui::StrokeKind::Inside);
                        }
                        'x' | 'X' => {
                            painter.rect_filled(rect, 3.0, colors.accent);
                            let points = vec![
                                c + egui::vec2(-0.5 * u, 0.0),
                                c + egui::vec2(-0.12 * u, 0.4 * u),
                                c + egui::vec2(0.55 * u, -0.4 * u),
                            ];
                            painter.add(egui::Shape::line(points, mark));
                        }
                        // 進行中: 左半分を塗る
                        '/' => {
                            let stroke = egui::Stroke::new(1.3, colors.accent);
                            painter.rect_stroke(rect, 3.0, stroke, egui::StrokeKind::Inside);
                            let half =
                                egui::Rect::from_min_max(rect.min, egui::pos2(c.x, rect.max.y));
                            painter.rect_filled(
                                half,
                                egui::CornerRadius {
                                    nw: 3,
                                    sw: 3,
                                    ne: 0,
                                    se: 0,
                                },
                                colors.accent,
                            );
                        }
                        // 取り消し: 横線
                        '-' => {
                            painter.rect_filled(rect, 3.0, colors.dim);
                            painter.line_segment(
                                [c + egui::vec2(-0.5 * u, 0.0), c + egui::vec2(0.5 * u, 0.0)],
                                mark,
                            );
                        }
                        // そのほか（`>` `!` `?` など）は、その文字を描く
                        other => {
                            painter.rect_filled(rect, 3.0, colors.accent.gamma_multiply(0.75));
                            painter.text(
                                c,
                                Align2::CENTER_CENTER,
                                other.to_string(),
                                egui::FontId::new(size * 0.75, fonts::bold()),
                                colors.code_bg,
                            );
                        }
                    }
                    // クリックで切り替える（カーソルは動かさない）
                    if clip.contains_rect(rect) {
                        let hit = ui.interact(
                            rect.expand(2.0),
                            id.with(("checkbox", deco.chars.start)),
                            egui::Sense::click(),
                        );
                        if hit.hovered() {
                            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        let label = match status {
                            ' ' => "クリックで完了にする",
                            _ => "クリックで未完了に戻す",
                        };
                        if hit.on_hover_text(label).clicked() {
                            // Obsidian と同じく、未完了なら完了に、それ以外の状態なら未完了に戻す
                            let next = if *status == ' ' { 'x' } else { ' ' };
                            toggle = Some((deco.chars.start + 1, next));
                        }
                    }
                }
                editor::DecoKind::Table(rows) => {
                    let pad = egui::vec2(10.0, 0.0);
                    let cells: Vec<Vec<_>> = rows
                        .iter()
                        .enumerate()
                        .map(|(r, table_row)| {
                            let color = if r == 0 { colors.heading } else { colors.text };
                            table_row
                                .cells
                                .iter()
                                .map(|c| {
                                    let job = editor::inline_job(c, size, &colors, color);
                                    ctx.fonts_mut(|f| f.layout_job(job))
                                })
                                .collect()
                        })
                        .collect();
                    let columns = cells.iter().map(Vec::len).max().unwrap_or(0);
                    let mut xs = vec![left];
                    for col in 0..columns {
                        let widest = cells
                            .iter()
                            .filter_map(|r| r.get(col))
                            .map(|g| g.size().x)
                            .fold(24.0, f32::max);
                        xs.push(xs[col] + widest + pad.x * 2.0);
                    }
                    let table_right = xs[columns];
                    let line = egui::Stroke::new(1.0, colors.dim.gamma_multiply(0.5));
                    for (r, (table_row, galleys)) in rows.iter().zip(&cells).enumerate() {
                        let top = row(table_row.chars.start).top();
                        // 見出し行は、畳んだ区切り行の高さも含めて次の行の上端までとする
                        let bottom = match rows.get(r + 1).filter(|_| r == 0) {
                            Some(next) => row(next.chars.start).top(),
                            None => row(table_row.chars.end).bottom(),
                        };
                        if r == 0 {
                            let header = egui::Rect::from_min_max(
                                egui::pos2(left, top),
                                egui::pos2(table_right, bottom),
                            );
                            painter.rect_filled(header, 0.0, colors.code_bg);
                            painter.hline(left..=table_right, top, line);
                        }
                        for (col, galley) in galleys.iter().enumerate() {
                            let y = (top + bottom - galley.size().y) * 0.5;
                            painter.galley(
                                egui::pos2(xs[col] + pad.x, y),
                                galley.clone(),
                                colors.text,
                            );
                        }
                        painter.hline(left..=table_right, bottom, line);
                    }
                    for x in &xs {
                        painter.vline(*x, a.top()..=b.bottom(), line);
                    }
                }
                editor::DecoKind::Callout {
                    color,
                    first,
                    label,
                    fold,
                } => {
                    let rect = egui::Rect::from_min_max(
                        egui::pos2(left - 8.0, a.top() - if *first { 3.0 } else { 0.0 }),
                        egui::pos2(right + 8.0, b.bottom()),
                    );
                    behind.push(egui::Shape::rect_filled(
                        rect,
                        0.0,
                        color.gamma_multiply(0.13),
                    ));
                    let bar =
                        egui::Rect::from_min_size(rect.left_top(), egui::vec2(3.0, rect.height()));
                    behind.push(egui::Shape::rect_filled(bar, 0.0, *color));
                    if let Some((text, at)) = label {
                        painter.text(
                            row(*at).left_center(),
                            Align2::LEFT_CENTER,
                            text,
                            egui::FontId::new(size, fonts::bold()),
                            *color,
                        );
                    }
                    // 折りたためるコールアウトは、右端の三角で開閉する（カーソルは動かさない）
                    if let Some((line, open)) = fold {
                        let c = egui::pos2(right - 6.0, a.center().y);
                        let r = size * 0.32;
                        let points = if *open {
                            vec![
                                c + egui::vec2(-r, -r * 0.5),
                                c + egui::vec2(r, -r * 0.5),
                                c + egui::vec2(0.0, r * 0.6),
                            ]
                        } else {
                            vec![
                                c + egui::vec2(-r * 0.5, -r),
                                c + egui::vec2(-r * 0.5, r),
                                c + egui::vec2(r * 0.6, 0.0),
                            ]
                        };
                        painter.add(egui::Shape::convex_polygon(
                            points,
                            *color,
                            egui::Stroke::NONE,
                        ));
                        let hit_rect =
                            egui::Rect::from_center_size(c, egui::vec2(size * 1.4, size * 1.4));
                        if clip.contains_rect(hit_rect) {
                            let hit = ui.interact(
                                hit_rect,
                                id.with(("fold", *line)),
                                egui::Sense::click(),
                            );
                            if hit.hovered() {
                                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            if hit
                                .on_hover_text(if *open { "折りたたむ" } else { "開く" })
                                .clicked()
                            {
                                fold_toggle = Some((*line, !*open));
                            }
                        }
                    }
                }
                editor::DecoKind::Audio(target) => {
                    let top = a.top() + editor::line_height(size) + 4.0;
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(left, top),
                        egui::vec2((right - left).clamp(80.0, 460.0), editor::AUDIO_HEIGHT),
                    );
                    behind.push(egui::Shape::rect_filled(rect, 16.0, colors.code_bg));
                    let file = vault.resolve_file(target, &tab.path).map(str::to_string);
                    let playing = player
                        .as_ref()
                        .filter(|p| Some(&p.path) == file.as_ref() && !p.finished());
                    // 再生 / 停止のボタン（文字ではなく図形で描く）
                    let c = egui::pos2(rect.left() + 16.0, rect.center().y);
                    let r = 6.0;
                    let mark = if file.is_none() {
                        colors.dim
                    } else {
                        colors.accent
                    };
                    if playing.is_some() {
                        painter.rect_filled(
                            egui::Rect::from_center_size(c, egui::vec2(r * 1.6, r * 1.6)),
                            1.0,
                            mark,
                        );
                    } else {
                        painter.add(egui::Shape::convex_polygon(
                            vec![
                                c + egui::vec2(-r * 0.7, -r),
                                c + egui::vec2(-r * 0.7, r),
                                c + egui::vec2(r, 0.0),
                            ],
                            mark,
                            egui::Stroke::NONE,
                        ));
                    }
                    let name = file_name(file.as_deref().unwrap_or(target)).to_string();
                    let label = match (&file, playing) {
                        (None, _) => format!("{name}（見つかりません）"),
                        (_, Some(p)) => format!("{name}   {}", p.progress_label()),
                        _ => name,
                    };
                    painter.with_clip_rect(rect.intersect(clip)).text(
                        egui::pos2(rect.left() + 32.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        label,
                        egui::FontId::proportional(size * 0.9),
                        colors.text,
                    );
                    if let Some(p) = playing {
                        // 進み具合の細い線
                        let frac = p.fraction();
                        let track = egui::Rect::from_min_max(
                            egui::pos2(rect.left() + 32.0, rect.bottom() - 5.0),
                            egui::pos2(rect.right() - 16.0, rect.bottom() - 3.0),
                        );
                        painter.rect_filled(track, 1.0, colors.dim.gamma_multiply(0.4));
                        let mut done = track;
                        done.set_right(track.left() + track.width() * frac);
                        painter.rect_filled(done, 1.0, colors.accent);
                        ctx.request_repaint_after(std::time::Duration::from_millis(200));
                    }
                    if let Some(file) = file.filter(|_| clip.intersects(rect)) {
                        let hit = ui.interact(
                            rect,
                            id.with(("audio", deco.chars.start)),
                            egui::Sense::click(),
                        );
                        if hit.hovered() {
                            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if hit
                            .on_hover_text(if playing.is_some() {
                                "止める"
                            } else {
                                "再生する"
                            })
                            .clicked()
                        {
                            actions.push(Action::PlayAudio(file));
                        }
                    }
                }
                editor::DecoKind::NoteEmbed(target) => {
                    let (note, heading) = hover::split_target(target);
                    let Some(path) = vault.resolve(note, &tab.path).map(str::to_string) else {
                        continue;
                    };
                    let key = embed_key(&path, heading);
                    let reserved = embed_heights.get(&key).copied().unwrap_or(EMBED_ESTIMATE);
                    let top = a.top() + editor::line_height(size) + 4.0;
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(left, top),
                        egui::vec2((right - left).max(80.0), reserved),
                    );
                    behind.push(egui::Shape::rect_filled(rect, 4.0, colors.code_bg));
                    let bar =
                        egui::Rect::from_min_size(rect.left_top(), egui::vec2(3.0, rect.height()));
                    behind.push(egui::Shape::rect_filled(bar, 0.0, colors.accent));
                    let inner = rect.shrink2(egui::vec2(14.0, 8.0));
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(egui::Rect::from_min_size(
                                inner.min,
                                egui::vec2(inner.width(), f32::INFINITY),
                            ))
                            .layout(Layout::top_down(Align::Min)),
                    );
                    child.set_clip_rect(rect.intersect(clip));
                    hover::embed_ui(&mut child, vault, embed_cache, &path, heading);
                    // 実際の高さを覚え、確保した場所と違えば組版をやり直す
                    let used = (child.min_rect().height() + 16.0).min(EMBED_MAX);
                    if (used - reserved).abs() > 1.0 {
                        embed_heights.insert(key, used);
                        tab.layout_stale = true;
                        ctx.request_repaint();
                    }
                }
                editor::DecoKind::Math { tex, display } => {
                    let m = math_view::layout(
                        &ctx,
                        tex,
                        math_font(size, *display),
                        *display,
                        colors.text,
                    );
                    let (x, center) = if *display {
                        (
                            (left + right) / 2.0 - m.w / 2.0,
                            (a.top() + b.bottom()) / 2.0,
                        )
                    } else {
                        // 確保した字間の右端から数式の幅だけ戻る（字間が文字の前後どちらに付いても合う）
                        (b.left() - m.w, a.center().y)
                    };
                    // 上下の中心を、確保した場所の中心にそろえる
                    let baseline = center + (m.asc - m.desc) / 2.0;
                    m.paint(&painter, egui::pos2(x, baseline), colors.text);
                }
                editor::DecoKind::InlineImage(target, shown) => {
                    if let Some((texture, _)) = load_image(target) {
                        // 確保した字間の右端から幅だけ戻り、行の下端にそろえる
                        let rect = egui::Rect::from_min_size(
                            egui::pos2(b.left() - shown.x - 2.0, a.bottom() - shown.y - 2.0),
                            *shown,
                        );
                        let uv =
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                        painter.image(texture, rect, uv, egui::Color32::WHITE);
                    }
                }
                editor::DecoKind::Image(target) => {
                    if let Some((texture, image_size)) = load_image(target) {
                        let top = a.top() + editor::line_height(size) + 4.0;
                        let rect = egui::Rect::from_min_size(egui::pos2(left, top), image_size);
                        let uv =
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                        painter.image(texture, rect, uv, egui::Color32::WHITE);
                    }
                }
            }
        }
        painter.set(background, egui::Shape::Vec(behind));
        if let Some((line, open)) = fold_toggle {
            callout_folds.insert((tab.path.clone(), line), open);
            tab.layout_stale = true;
            ctx.request_repaint();
        }

        if block_cursor && output.response.response.has_focus() {
            // ノーマルモードではIMEを止める（日本語入力のままでもキーがコマンドとして届くように）
            ctx.output_mut(|o| o.ime = None);
            if let Some(r) = output.cursor_range {
                let (p, s) = (r.primary.index.0, r.secondary.index.0);
                let (pb, sb) = (
                    editor::byte_of_char(&tab.text, p),
                    editor::byte_of_char(&tab.text, s),
                );
                if vim_state.helix {
                    // Helix風: エンジンが置いた選択から変わったとき（マウスで選んだ / クリックした）だけ取り込む
                    let unchanged = vim_state.synced.filter(|&(p, s, _)| (p, s) == (pb, sb));
                    if let Some((_, _, head)) = unchanged {
                        // ブロックカーソルは選択の頭に描く
                        tab.cursor = head;
                    } else if vim_state.mode != vim::Mode::Insert {
                        let (anchor, head) = if p == s {
                            let c = vim::clamp_normal(&tab.text, pb);
                            (c, c)
                        } else if pb > sb {
                            let head =
                                pb - tab.text[..pb].chars().next_back().map_or(0, char::len_utf8);
                            (sb, head)
                        } else {
                            let anchor =
                                sb - tab.text[..sb].chars().next_back().map_or(0, char::len_utf8);
                            (anchor, pb)
                        };
                        vim_state.anchor = anchor;
                        tab.cursor = head;
                        vim_state.synced = Some((pb, sb, head));
                    }
                } else if vim_state.mode == vim::Mode::Normal && p != s {
                    // マウスで範囲を選んだらビジュアルモードにする
                    vim_state.mode = vim::Mode::Visual;
                    vim_state.anchor = sb;
                    tab.cursor = if pb > sb {
                        pb - tab.text[..pb].chars().next_back().map_or(0, char::len_utf8)
                    } else {
                        pb
                    };
                } else if vim_state.mode == vim::Mode::Normal {
                    let clamped = vim::clamp_normal(&tab.text, pb);
                    if clamped != pb {
                        let mut state = TextEdit::load_state(&ctx, id).unwrap_or_default();
                        state
                            .cursor
                            .set_char_range(Some(CCursorRange::one(CCursor::new(
                                editor::char_of_byte(&tab.text, clamped),
                            ))));
                        state.store(&ctx, id);
                        tab.cursor = clamped;
                    }
                }
            }
            let c = editor::char_of_byte(&tab.text, tab.cursor.min(tab.text.len()));
            let here = row(c);
            let after = row(c + 1);
            let width = if tab.text[tab.cursor.min(tab.text.len())..].starts_with('\n')
                || tab.cursor >= tab.text.len()
                || (after.top() - here.top()).abs() > 1.0
            {
                size * 0.55
            } else {
                (after.left() - here.left()).max(2.0)
            };
            let rect = egui::Rect::from_min_size(here.left_top(), egui::vec2(width, here.height()));
            painter.rect_filled(rect, 1.0, colors.accent.gamma_multiply(0.45));
        }
        if let Some((ch, next)) = toggle {
            let byte = editor::byte_of_char(&tab.text, ch);
            let end = tab.text[byte..]
                .chars()
                .next()
                .map_or(byte, |c| byte + c.len_utf8());
            if end > byte {
                // Ctrl+Zで戻せるよう、変更前の状態をTextEditの履歴に積む
                let mut state = TextEdit::load_state(&ctx, id).unwrap_or_default();
                let range = state.cursor.char_range().unwrap_or_default();
                let mut undoer = state.undoer();
                undoer.add_undo(&(range, tab.text.clone()));
                state.set_undoer(undoer);
                state.store(&ctx, id);
                tab.text.replace_range(byte..end, &next.to_string());
                // チェックボックスのクリックでエディタのフォーカスが外れないようにする
                ctx.memory_mut(|m| m.request_focus(id));
                tab.dirty = true;
                tab.edited_at = Some(Instant::now());
                tab.version += 1;
            }
        }

        let response = &output.response.response;
        if response.changed() {
            tab.dirty = true;
            tab.edited_at = Some(Instant::now());
            tab.version += 1;
            // 行頭か空白の直後に `/` を入力したら、コマンドパレットを開く（スラッシュコマンド）
            let typed_slash = ctx.input(|i| {
                i.events
                    .iter()
                    .any(|e| matches!(e, egui::Event::Text(t) if t == "/"))
            });
            if typed_slash
                && cfg.features.slash_commands
                && !(vim_on && vim_state.mode != vim::Mode::Insert)
            {
                if let Some(c) = output.cursor_range.map(|r| r.primary.index.0) {
                    let at = editor::byte_of_char(&tab.text, c);
                    if at > 0 && tab.text[..at].ends_with('/') {
                        let before = tab.text[..at - 1].chars().next_back();
                        if before.is_none_or(char::is_whitespace) {
                            actions.push(Action::Slash(tab.path.clone(), at - 1));
                        }
                    }
                }
            }
        }
        let cursor_char = output.cursor_range.map(|r| r.primary.index.0);
        if let Some(c) = cursor_char {
            tab.cursor = editor::byte_of_char(&tab.text, c);
        }
        // 埋め込みの中のリンクのクリックを横取りしてノートを開く
        ctx.output_mut(|out| {
            out.commands.retain(|cmd| {
                if let egui::OutputCommand::OpenUrl(open) = cmd {
                    if let Some(target) = preview::decode_link(&open.url) {
                        actions.push(Action::FollowLink {
                            target,
                            from: tab.path.clone(),
                        });
                        return false;
                    }
                }
                true
            });
        });
        // Ctrl+ホバーでリンク先を覗く
        if response.hovered() && ctx.input(|i| i.modifiers.command) {
            if let Some(pointer) = ctx.pointer_hover_pos() {
                let at = output.galley.cursor_from_pos(pointer - output.galley_pos);
                let byte = editor::byte_of_char(&tab.text, at.index.0);
                let target = editor::link_at(&tab.text, byte);
                let resolved = target.as_deref().and_then(|t| {
                    let (note, heading) = hover::split_target(t);
                    let path = if note.is_empty() {
                        Some(tab.path.as_str())
                    } else {
                        vault.resolve(note, &tab.path)
                    };
                    path.map(|p| (p.to_string(), heading.map(str::to_string)))
                });
                // ページプレビュー（オフなら、リンクの上でも中身は見せない）
                if let Some((path, heading)) = resolved.filter(|_| cfg.features.page_preview) {
                    ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                    egui::Tooltip::always_open(
                        ctx.clone(),
                        ui.layer_id(),
                        id.with("hover"),
                        pointer,
                    )
                    .width(hover::WIDTH)
                    .show(|ui| {
                        hover::note_preview(ui, vault, hover_cache, &path, heading.as_deref())
                    });
                }
            }
        }
        // Ctrl+クリックでリンクを辿る
        if response.clicked() && ctx.input(|i| i.modifiers.command) {
            if let Some(target) = editor::link_at(&tab.text, tab.cursor.min(tab.text.len())) {
                actions.push(Action::FollowLink {
                    target,
                    from: tab.path.clone(),
                });
            }
        }

        // ---- `[[` 補完 ----
        let cursor = tab.cursor.min(tab.text.len());
        if accept {
            if let Some(c) = completion.take() {
                if let Some((insert, _)) = c.items.get(c.selected) {
                    let start = c.start.min(cursor);
                    let close = if tab.text[cursor..].starts_with("]]") {
                        ""
                    } else {
                        "]]"
                    };
                    tab.text
                        .replace_range(start..cursor, &format!("{insert}{close}"));
                    let new_cursor = start + insert.len() + 2;
                    let ccursor = CCursor::new(editor::char_of_byte(&tab.text, new_cursor));
                    let mut state = TextEdit::load_state(&ctx, id).unwrap_or_default();
                    state
                        .cursor
                        .set_char_range(Some(CCursorRange::one(ccursor)));
                    state.store(&ctx, id);
                    tab.cursor = new_cursor;
                    tab.dirty = true;
                    tab.edited_at = Some(Instant::now());
                    tab.version += 1;
                }
            }
            return;
        }
        let start = (response.has_focus() && cursor_char.is_some())
            .then(|| editor::completion_start(&tab.text, cursor))
            .flatten();
        let Some(start) = start.filter(|s| Some(*s) != *completion_dismissed) else {
            if start.is_none() {
                *completion_dismissed = None;
            }
            *completion = None;
            return;
        };
        let query = &tab.text[start..cursor];
        if completion
            .as_ref()
            .is_none_or(|c| c.start != start || c.query != query)
        {
            let items = match query.split_once('#') {
                // `[[ノート#見出し` → そのノートの見出しを候補にする（`[[#見出し` は同じノート）
                Some((note, heading)) => {
                    let text = if note.trim().is_empty() {
                        Some(tab.text.as_str())
                    } else {
                        vault
                            .resolve(note, &tab.path)
                            .and_then(|p| vault.note(p))
                            .map(|n| n.text.as_str())
                    };
                    let want = heading.trim().to_lowercase();
                    text.map(|t| parse::parse(t).headings)
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|h| h.text.to_lowercase().contains(&want))
                        .take(20)
                        .map(|h| {
                            let shown =
                                format!("{}{}", "  ".repeat(usize::from(h.level) - 1), h.text);
                            (format!("{note}#{}", h.text), shown)
                        })
                        .collect()
                }
                None => search::fuzzy(vault, query, 8)
                    .into_iter()
                    .filter(|p| *p != tab.path)
                    .map(|path| {
                        let full = path.strip_suffix(".md").unwrap_or(&path);
                        let name = file_name(full);
                        // 同名ノートがあって名前だけでは別のノートに解決される場合はパスで書く
                        let unique = vault.resolve(name, &tab.path) == Some(path.as_str());
                        (if unique { name } else { full }.to_string(), path.clone())
                    })
                    .collect(),
            };
            *completion = Some(Completion {
                start,
                query: query.to_string(),
                items,
                selected: 0,
            });
        }
        let Some(c) = completion.as_mut().filter(|c| !c.items.is_empty()) else {
            return;
        };
        let cursor_rect = output
            .galley
            .pos_from_cursor(CCursor::new(cursor_char.unwrap_or(0)))
            .translate(output.galley_pos.to_vec2());
        let mut clicked = None;
        egui::Area::new(id.with("completion"))
            .order(egui::Order::Foreground)
            .fixed_pos(cursor_rect.left_bottom() + egui::vec2(0.0, 4.0))
            .show(&ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(260.0);
                    for (i, (insert, path)) in c.items.iter().enumerate() {
                        let row = ui
                            .selectable_label(i == c.selected, insert)
                            .on_hover_text(path);
                        if row.clicked() {
                            clicked = Some(i);
                        }
                    }
                    ui.weak(RichText::new("↑↓ 選択  Enter 決定  Esc 閉じる").small());
                });
            });
        if let Some(i) = clicked {
            // クリックで選んだ場合は、次フレームでEnterと同じ経路を通す
            c.selected = i;
            ctx.memory_mut(|m| m.request_focus(id));
            ctx.input_mut(|input| {
                input.events.push(egui::Event::Key {
                    key: Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                });
            });
            ctx.request_repaint();
        }
    }

    fn preview_ui(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let Self {
            tabs,
            vault,
            md_cache,
            generation,
            active,
            cfg,
            mermaid,
            ..
        } = self;
        let (Some(tab), Some(vault)) = (tabs.get_mut(*active), vault.as_ref()) else {
            return;
        };
        if tab
            .preview
            .as_ref()
            .is_none_or(|(v, g, _)| (*v, *g) != (tab.version, *generation))
        {
            let src = preview::to_preview(&tab.text, &tab.path, vault);
            let src = mermaid_images(&src, vault.root(), mermaid, cfg, ui.ctx());
            tab.preview = Some((tab.version, *generation, src));
        }
        let Some((_, _, src)) = &tab.preview else {
            return;
        };
        // チェックボックスのクリックを受けるため、描画用の複製を渡す（変化したら元のノートに反映する）
        let mut rendered = src.clone();
        ScrollArea::vertical()
            .id_salt(("preview-scroll", &tab.path))
            .auto_shrink(false)
            .show(ui, |ui| {
                let side = side_margin(ui.available_width(), cfg);
                ui.add_space(12.0);
                ui.horizontal_top(|ui| {
                    ui.add_space(side);
                    ui.vertical(|ui| {
                        ui.set_width((ui.available_width() - side).max(80.0));
                        let width = ui.available_width().max(100.0) as usize;
                        CommonMarkViewer::new()
                            .alerts(preview::alerts())
                            .render_math_fn(Some(&crate::math_view::show))
                            .max_image_width(Some(width))
                            .show_mut(ui, md_cache, &mut rendered);
                    });
                });
                ui.add_space(12.0);
            });
        if rendered != *src {
            match preview::toggle_from_preview(&tab.text, src, &rendered) {
                Some(text) => {
                    let cursor = tab.cursor.min(text.len());
                    let cursor = (0..=cursor)
                        .rev()
                        .find(|&c| text.is_char_boundary(c))
                        .unwrap_or(0);
                    replace_text_undoable(ui.ctx(), tab, text, cursor..cursor);
                }
                None => actions.push(Action::Toast(
                    "このチェックボックスは元のノートと対応づけられませんでした".into(),
                )),
            }
        }
        // プレビュー内のwikilinkクリックを横取りしてノートを開く
        ui.ctx().output_mut(|out| {
            out.commands.retain(|cmd| {
                if let egui::OutputCommand::OpenUrl(open) = cmd {
                    if let Some(target) = preview::decode_link(&open.url) {
                        actions.push(Action::FollowLink {
                            target,
                            from: tab.path.clone(),
                        });
                        return false;
                    }
                }
                true
            });
        });
    }

    fn palette_ui(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let current_path = self.tabs.get(self.active).map(|t| t.path.clone());
        let Some(p) = self.palette.as_mut() else {
            return;
        };
        let mut close = false;
        let mut chosen = None;
        let (up, down, enter, shift_enter) = ctx.input_mut(|i| {
            close |= i.consume_key(Modifiers::NONE, Key::Escape);
            (
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::SHIFT, Key::Enter),
            )
        });

        if p.computed_for.as_deref() != Some(p.query.as_str()) {
            p.items.clear();
            let q = p.query.trim();
            if matches!(
                p.mode,
                PaletteMode::InsertTemplate | PaletteMode::NewFromTemplate
            ) {
                if let Some(vault) = &self.vault {
                    let q = q.to_lowercase();
                    let hits = vault
                        .templates()
                        .into_iter()
                        .filter(|t| t.to_lowercase().contains(&q));
                    p.items
                        .extend(hits.map(|t| PaletteItem::Template(t.to_string())));
                }
            } else if matches!(p.mode, PaletteMode::Layouts | PaletteMode::DeleteLayout) {
                let q = q.to_lowercase();
                p.items.extend(
                    self.workspace
                        .layouts
                        .keys()
                        .filter(|n| n.to_lowercase().contains(&q))
                        .map(|n| PaletteItem::Layout(n.clone())),
                );
            } else if p.mode == PaletteMode::Commands {
                let q = q.to_lowercase();
                let features = self.cfg.features;
                let hits = COMMANDS.iter().enumerate().filter(|(_, c)| {
                    if !c.0.enabled(&features) {
                        return false;
                    }
                    // IMEをオフにしたまま英語でも引けるようにする。空白で区切った語がすべて含まれれば一致
                    let label = c.1.to_lowercase();
                    let keywords = c.0.keywords();
                    q.split_whitespace()
                        .all(|w| label.contains(w) || keywords.contains(w))
                });
                p.items.extend(hits.map(|(i, _)| PaletteItem::Cmd(i)));
                let user = self.user_cmds.list.iter().enumerate();
                let user = user.filter(|(_, c)| c.name.to_lowercase().contains(&q));
                p.items.extend(user.map(|(i, _)| PaletteItem::User(i)));
            } else if let Some(vault) = &self.vault {
                let mut notes = search::fuzzy(vault, q, 30);
                if q.is_empty() {
                    // 入力前は「最近開いた順」を先に並べ、残りを更新日時順で続ける
                    let recent: Vec<String> = self
                        .workspace
                        .recent
                        .iter()
                        .filter(|r| vault.note(r).is_some())
                        .cloned()
                        .collect();
                    notes.retain(|n| !recent.contains(n));
                    notes.splice(0..0, recent);
                    notes.truncate(30);
                }
                let exact = notes.iter().any(|n| {
                    n.strip_suffix(".md")
                        .is_some_and(|s| s.eq_ignore_ascii_case(q))
                        || file_name(n)
                            .strip_suffix(".md")
                            .is_some_and(|s| s.eq_ignore_ascii_case(q))
                });
                if p.mode == PaletteMode::MergeTarget {
                    // 結合先の候補から、結合するノート自身は除く
                    notes.retain(|n| Some(n) != current_path.as_ref());
                }
                p.items.extend(notes.into_iter().map(PaletteItem::Note));
                if !q.is_empty() && !exact && p.mode == PaletteMode::Notes {
                    p.items.push(PaletteItem::Create(q.to_string()));
                }
            }
            p.selected = 0;
            p.computed_for = Some(p.query.clone());
        }
        let n = p.items.len();
        if n > 0 {
            if down {
                p.selected = (p.selected + 1) % n;
            }
            if up {
                p.selected = (p.selected + n - 1) % n;
            }
            if enter {
                chosen = Some(p.selected);
            }
        }
        // Shift+Enter: 候補があっても入力した名前で新規作成する
        let force_create =
            shift_enter && p.mode == PaletteMode::Notes && !p.query.trim().is_empty();

        let area = egui::Area::new(egui::Id::unique("palette"))
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_TOP, [0.0, 70.0])
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(560.0);
                    let hint = match p.mode {
                        PaletteMode::Commands => "コマンドを検索…（英語でも可）",
                        PaletteMode::Notes => "ノート名を入力…（無ければEnterで新規作成）",
                        PaletteMode::InsertTemplate => "挿入するテンプレートを選ぶ…",
                        PaletteMode::NewFromTemplate => "新規ノートに使うテンプレートを選ぶ…",
                        PaletteMode::MergeTarget => {
                            "結合先のノートを選ぶ…（このノートの本文を末尾に足します）"
                        }
                        PaletteMode::Layouts => "開くワークスペースを選ぶ…",
                        PaletteMode::DeleteLayout => "削除するワークスペースを選ぶ…",
                    };
                    let edit = TextEdit::singleline(&mut p.query)
                        .hint_text(hint)
                        .desired_width(f32::INFINITY);
                    ui.add(edit).request_focus();
                    ui.separator();
                    ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                        for (i, item) in p.items.iter().enumerate() {
                            let selected = i == p.selected;
                            let row = match item {
                                PaletteItem::Note(path) => {
                                    let full = path.strip_suffix(".md").unwrap_or(path);
                                    ui.horizontal(|ui| {
                                        let r = ui.selectable_label(selected, file_name(full));
                                        ui.weak(parent_dir(full));
                                        r
                                    })
                                    .inner
                                }
                                PaletteItem::Layout(name) => {
                                    let tabs = self
                                        .workspace
                                        .layouts
                                        .get(name)
                                        .map_or(0, |l| l.tabs.len());
                                    ui.horizontal(|ui| {
                                        let r = ui.selectable_label(selected, name.as_str());
                                        ui.weak(format!("{tabs}個のタブ"));
                                        r
                                    })
                                    .inner
                                }
                                PaletteItem::Template(path) => {
                                    let name = path.trim_end_matches(".md");
                                    ui.selectable_label(selected, file_name(name))
                                }
                                PaletteItem::Create(name) => ui
                                    .selectable_label(selected, format!("＋ 「{name}」を新規作成")),
                                PaletteItem::Cmd(ci) => {
                                    let (_, label, shortcut) = COMMANDS[*ci];
                                    ui.horizontal(|ui| {
                                        let r = ui.selectable_label(selected, label);
                                        if let Some((mods, key)) = shortcut {
                                            ui.weak(ui.ctx().format_shortcut(
                                                &KeyboardShortcut::new(mods, key),
                                            ));
                                        }
                                        r
                                    })
                                    .inner
                                }
                                PaletteItem::User(ui_idx) => {
                                    let cmd = &self.user_cmds.list[*ui_idx];
                                    let key = self.user_cmds.keys[*ui_idx];
                                    ui.horizontal(|ui| {
                                        let r = ui.selectable_label(selected, &cmd.name);
                                        if let Some(k) = key {
                                            ui.weak(ui.ctx().format_shortcut(&k));
                                        }
                                        ui.weak("ユーザーコマンド");
                                        r
                                    })
                                    .inner
                                }
                            };
                            if selected && (up || down) {
                                row.scroll_to_me(None);
                            }
                            if row.clicked() {
                                chosen = Some(i);
                            }
                        }
                        if p.items.is_empty() {
                            ui.weak("一致するものがありません");
                        }
                    });
                });
            });
        if area.response.clicked_elsewhere() {
            close = true;
        }

        if force_create {
            let name = p.query.trim().to_string();
            self.palette = None;
            self.create_and_open(&name);
        } else if let Some(i) = chosen {
            let Some(p) = self.palette.take() else { return };
            match &p.items[i] {
                PaletteItem::Note(path) if p.mode == PaletteMode::MergeTarget => {
                    if let Some(source) = current_path {
                        self.dialog = Some(Dialog::Merge {
                            source,
                            target: path.clone(),
                        });
                    }
                }
                PaletteItem::Note(path) => actions.push(Action::Open(path.clone(), None)),
                PaletteItem::Layout(name) if p.mode == PaletteMode::DeleteLayout => {
                    self.workspace.layouts.remove(name);
                    self.save_workspace();
                    self.toast(format!("ワークスペース「{name}」を削除しました"));
                }
                PaletteItem::Layout(name) => self.open_layout(name),
                PaletteItem::Template(path) if p.mode == PaletteMode::InsertTemplate => {
                    let title = self
                        .current()
                        .map(|t| t.title().to_string())
                        .unwrap_or_default();
                    let text = self
                        .vault
                        .as_ref()
                        .and_then(|v| v.render_template(path, &title));
                    if let Some(text) = text {
                        self.insert_at_cursor(&text);
                    }
                }
                PaletteItem::Template(path) => {
                    let dir = self
                        .current()
                        .map(|t| parent_dir(&t.path).to_string())
                        .unwrap_or_default();
                    self.dialog = Some(Dialog::NewNote {
                        dir,
                        name: String::new(),
                        template: Some(path.clone()),
                    });
                }
                PaletteItem::Create(name) => self.create_and_open(name),
                PaletteItem::Cmd(ci) => {
                    self.remove_slash(p.slash.as_ref());
                    actions.push(Action::Run(COMMANDS[*ci].0));
                }
                PaletteItem::User(i) => {
                    self.remove_slash(p.slash.as_ref());
                    actions.push(Action::RunUser(*i));
                }
            }
        } else if close {
            self.palette = None;
        }
    }

    /// スライド表示（全画面）。→ / Space / PageDown で次、← / PageUp で前、Home / End、Esc で終わる。
    fn slides_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some((path, index)) = self.slideshow.clone() else {
            return;
        };
        let text = match self.tabs.iter().find(|t| t.path == path) {
            Some(tab) => tab.text.clone(),
            None => self
                .vault
                .as_ref()
                .and_then(|v| v.note(&path))
                .map_or_else(String::new, |n| n.text.clone()),
        };
        let slides = robsidian_core::text::slides(&text);
        let mut exit = slides.is_empty();
        let last = slides.len().saturating_sub(1);
        let mut index = index.min(last);
        ctx.input_mut(|i| {
            let mut pressed = |k| i.consume_key(Modifiers::NONE, k);
            if pressed(Key::Escape) {
                exit = true;
            }
            if pressed(Key::ArrowRight)
                || pressed(Key::Space)
                || pressed(Key::PageDown)
                || pressed(Key::ArrowDown)
                || pressed(Key::Enter)
            {
                index = (index + 1).min(last);
            }
            if pressed(Key::ArrowLeft)
                || pressed(Key::PageUp)
                || pressed(Key::ArrowUp)
                || pressed(Key::Backspace)
            {
                index = index.saturating_sub(1);
            }
            if pressed(Key::Home) {
                index = 0;
            }
            if pressed(Key::End) {
                index = last;
            }
        });
        if exit {
            if slides.is_empty() {
                self.toast("スライドにする本文がありません");
            }
            self.slideshow = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            return;
        }
        self.slideshow = Some((path.clone(), index));
        let Some(vault) = self.vault.as_ref() else {
            return;
        };
        let md = preview::to_preview(&slides[index], &path, vault);
        let md_cache = &mut self.md_cache;
        let base = self.cfg.font_size;
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(48.0))
            .show(ui, |ui| {
                // 文字を大きくする
                ui.style_mut()
                    .text_styles
                    .iter_mut()
                    .for_each(|(style, font)| {
                        font.size = match style {
                            egui::TextStyle::Heading => base * 2.6,
                            egui::TextStyle::Small => base * 1.2,
                            _ => base * 1.7,
                        };
                    });
                let footer = 40.0;
                let height = (ui.available_height() - footer).max(100.0);
                ui.allocate_ui(egui::vec2(ui.available_width(), height), |ui| {
                    ScrollArea::vertical()
                        .id_salt(("slide", &path, index))
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            let side = ((ui.available_width() - 1100.0) / 2.0).max(0.0);
                            ui.horizontal_top(|ui| {
                                ui.add_space(side);
                                ui.vertical(|ui| {
                                    ui.set_width(ui.available_width() - side);
                                    let width = ui.available_width().max(100.0) as usize;
                                    CommonMarkViewer::new()
                                        .alerts(preview::alerts())
                                        .render_math_fn(Some(&crate::math_view::show))
                                        .max_image_width(Some(width))
                                        .show(ui, md_cache, &md);
                                });
                            });
                        });
                });
                ui.horizontal(|ui| {
                    if ui.button("◀").clicked() {
                        index = index.saturating_sub(1);
                    }
                    if ui.button("▶").clicked() {
                        index = (index + 1).min(last);
                    }
                    ui.weak(format!(
                        "{} / {}　←→ で移動・Esc で終了",
                        index + 1,
                        last + 1
                    ));
                });
            });
        if let Some(show) = self.slideshow.as_mut() {
            show.1 = index;
        }
    }

    /// 切り出しの準備: 選択範囲か、カーソルのある見出しの節を決めて、名前を聞くダイアログを出す。
    fn start_extract(&mut self, ctx: &egui::Context) {
        let Some(tab) = self.tabs.get(self.active) else {
            self.toast("切り出すノートを開いてください");
            return;
        };
        let sel = selection_bytes(ctx, tab);
        let (range, heading, name) = if !sel.is_empty() {
            let text = &tab.text[sel.clone()];
            (sel, false, suggested_name(text))
        } else {
            // カーソルのある見出しの節（見出しの次の行から、同じかより上の見出しの手前まで）
            let headings = parse::parse(&tab.text).headings;
            let line = tab.text[..tab.cursor.min(tab.text.len())]
                .matches('\n')
                .count();
            let Some(i) = headings.iter().rposition(|h| h.line <= line) else {
                self.toast("切り出す範囲を選ぶか、見出しの中で実行してください");
                return;
            };
            let h = &headings[i];
            let end_line = headings[i + 1..]
                .iter()
                .find(|n| n.level <= h.level)
                .map(|n| n.line);
            let start = editor::line_start(&tab.text, h.line + 1).min(tab.text.len());
            let end = end_line.map_or(tab.text.len(), |l| editor::line_start(&tab.text, l));
            if start >= end || tab.text[start..end].trim().is_empty() {
                self.toast("この見出しの下には切り出す本文がありません");
                return;
            }
            (start..end, true, sanitize_name(&h.text))
        };
        self.dialog = Some(Dialog::Extract {
            path: tab.path.clone(),
            range,
            version: tab.version,
            heading,
            name,
        });
    }

    /// 切り出しを行う: 新しいノートを作り、元の範囲をそのノートへのリンクに置き換える（Ctrl+Zで戻せる）。
    fn extract(
        &mut self,
        path: &str,
        range: std::ops::Range<usize>,
        version: u64,
        heading: bool,
        name: &str,
        ctx: &egui::Context,
    ) {
        let Some(tab) = self.tabs.iter().find(|t| t.path == path) else {
            return;
        };
        if tab.version != version || range.end > tab.text.len() {
            self.toast("ノートが変わったので切り出しを取りやめました。もう一度選んでください");
            return;
        }
        let body = tab.text[range.clone()].trim_matches('\n').to_string() + "\n";
        let body_starts_blank = tab.text[range.clone()].starts_with('\n');
        let dir = parent_dir(path);
        let full = if dir.is_empty() {
            name.to_string()
        } else {
            format!("{dir}/{name}")
        };
        let Some(vault) = self.vault.as_mut() else {
            return;
        };
        let created = match vault.create_note(&full, &body) {
            Ok(p) => p,
            Err(e) => return self.toast(format!("作れません: {e:#}")),
        };
        self.generation += 1;
        let title = file_name(&created).trim_end_matches(".md").to_string();
        let link = if heading {
            // 見出しの直後の空行は残す
            let blank = if body_starts_blank { "\n" } else { "" };
            format!("{blank}[[{title}]]\n\n")
        } else {
            format!("[[{title}]]")
        };
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.path == path) {
            let mut text = tab.text.clone();
            text.replace_range(range.clone(), &link);
            let end = range.start + link.trim_end().len();
            replace_text_undoable(ctx, tab, text, range.start..end);
            tab.focus = true;
        }
        self.toast(format!(
            "「{title}」に切り出しました（Ctrl+Zで元の本文に戻せます）"
        ));
    }

    /// 結合を行う: 結合先の末尾に本文を足し、リンクを付け替え、結合元をゴミ箱へ移す。
    fn merge(&mut self, source: &str, target: &str) {
        self.save_all();
        let Some(vault) = self.vault.as_mut() else {
            return;
        };
        let updated = match vault.merge(source, target) {
            Ok(u) => u,
            Err(e) => return self.toast(format!("結合できません: {e:#}")),
        };
        let trashed = trash::delete(vault.abs(source));
        vault.rescan();
        self.generation += 1;
        self.tabs.retain(|t| t.path != source);
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        self.workspace.renamed(source, target);
        self.save_workspace();
        self.sync_tabs();
        self.open_note(target, None);
        let links = updated.len().saturating_sub(1);
        if let Err(e) = trashed {
            return self.toast(format!("結合しましたが、結合元をゴミ箱へ移せません: {e}"));
        }
        self.toast(format!(
            "「{}」を「{}」に結合しました（リンクを{links}件のノートで付け替え、結合元はゴミ箱へ）",
            file_name(source).trim_end_matches(".md"),
            file_name(target).trim_end_matches(".md"),
        ));
    }

    /// 保存したワークスペースを開く（今のタブは閉じる）。
    fn open_layout(&mut self, name: &str) {
        let Some(layout) = self.workspace.layouts.get(name).cloned() else {
            return;
        };
        self.save_all();
        self.tabs.clear();
        self.other = None;
        for path in &layout.tabs {
            if self.vault.as_ref().is_some_and(|v| v.note(path).is_some()) {
                self.open_note(path, None);
            }
        }
        self.active = layout.active.min(self.tabs.len().saturating_sub(1));
        self.other = layout
            .split
            .filter(|p| self.tabs.iter().any(|t| t.path == *p));
        self.focus_right = layout.focus_right && self.other.is_some();
        self.show_graph = false;
        self.toast(format!("ワークスペース「{name}」を開きました"));
    }

    /// スラッシュコマンドで入力した `/` を消す（コマンドを選んだとき）。
    fn remove_slash(&mut self, slash: Option<&(String, usize)>) {
        let Some((path, pos)) = slash else { return };
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.path == *path) {
            if tab.text.get(*pos..pos + 1) == Some("/") {
                let mut text = tab.text.clone();
                text.replace_range(*pos..pos + 1, "");
                tab.set_text(text);
                tab.place_cursor = Some(*pos);
                tab.focus = true;
                tab.dirty = true;
                tab.edited_at = Some(Instant::now());
            }
        }
    }

    fn dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.as_mut() else {
            return;
        };
        let mut confirm = false;
        let modal = egui::Modal::new(egui::Id::unique("dialog")).show(ctx, |ui| {
            ui.set_width(420.0);
            let input = |ui: &mut egui::Ui, title: &str, hint: &str, value: &mut String| {
                ui.heading(title);
                ui.add_space(6.0);
                let edit = TextEdit::singleline(value)
                    .hint_text(hint)
                    .desired_width(f32::INFINITY);
                let response = ui.add(edit);
                response.request_focus();
            };
            match dialog {
                Dialog::NewNote {
                    dir,
                    name,
                    template,
                } => {
                    let mut title = if dir.is_empty() {
                        "新規ノート".to_string()
                    } else {
                        format!("新規ノート（{dir}/）")
                    };
                    if let Some(t) = template {
                        let t = file_name(t);
                        title.push_str(&format!(" ・テンプレート: {}", t.trim_end_matches(".md")));
                    }
                    input(ui, &title, "ノート名（フォルダ/名前 も可）", name);
                }
                Dialog::NewFolder { dir, name } => {
                    let title = if dir.is_empty() {
                        "新規フォルダ".to_string()
                    } else {
                        format!("新規フォルダ（{dir}/）")
                    };
                    input(ui, &title, "フォルダ名", name);
                }
                Dialog::Rename { path, new } => {
                    input(ui, "改名 / 移動", "新しいパス", new);
                    ui.weak(format!(
                        "元: {path}  ・このノートへのリンクは自動で書き換えます"
                    ));
                }
                Dialog::Delete { path } => {
                    ui.heading("ゴミ箱へ移動");
                    ui.add_space(6.0);
                    ui.label(format!("「{path}」をゴミ箱へ移動しますか？"));
                }
                Dialog::Extract {
                    heading, name, range, ..
                } => {
                    let what = if *heading { "見出しの節" } else { "選択範囲" };
                    input(ui, &format!("{what}を新しいノートに切り出す"), "新しいノートの名前", name);
                    ui.weak(format!(
                        "{}文字を同じフォルダの新しいノートに移し、元の場所はそのノートへのリンクにします",
                        range.len()
                    ));
                }
                Dialog::Merge { source, target } => {
                    ui.heading("ノートを結合");
                    ui.add_space(6.0);
                    ui.label(format!(
                        "「{}」の本文を「{}」の末尾に足します。",
                        file_name(source).trim_end_matches(".md"),
                        file_name(target).trim_end_matches(".md")
                    ));
                    ui.weak("結合元へのリンクは結合先に付け替え、結合元はゴミ箱へ移します。");
                }
                Dialog::SaveLayout { name } => {
                    input(ui, "ワークスペースを保存", "名前（同じ名前なら上書き）", name);
                    ui.weak("開いているタブと、左右の分割の配置を保存します");
                }
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                confirm |= ui.button("OK").clicked();
                ui.button("キャンセル").clicked()
            })
            .inner
        });
        confirm |= ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
        let cancel = modal.inner || modal.should_close();
        if !confirm {
            if cancel {
                self.dialog = None;
            }
            return;
        }
        let join = |dir: &str, name: &str| {
            let name = name.trim().trim_matches('/');
            if dir.is_empty() {
                name.to_string()
            } else {
                format!("{dir}/{name}")
            }
        };
        match self.dialog.take() {
            Some(Dialog::NewNote {
                dir,
                name,
                template,
            }) if !name.trim().is_empty() => {
                self.create_from(&join(&dir, &name), template.as_deref());
            }
            Some(Dialog::NewFolder { dir, name }) if !name.trim().is_empty() => {
                if let Some(vault) = self.vault.as_mut() {
                    match vault.create_dir(&join(&dir, &name)) {
                        Ok(()) => self.generation += 1,
                        Err(e) => self.toast(format!("フォルダを作成できません: {e:#}")),
                    }
                }
            }
            Some(Dialog::Rename { path, new }) if !new.trim().is_empty() => {
                self.rename(&path, &new)
            }
            Some(Dialog::Delete { path }) => self.delete(&path),
            Some(Dialog::Extract {
                path,
                range,
                version,
                heading,
                name,
            }) => {
                let name = sanitize_name(&name);
                if name.is_empty() {
                    self.toast("名前を入れてください");
                } else {
                    self.extract(&path, range, version, heading, &name, ctx);
                }
            }
            Some(Dialog::Merge { source, target }) => self.merge(&source, &target),
            Some(Dialog::SaveLayout { name }) if !name.trim().is_empty() => {
                let name = name.trim().to_string();
                let layout = workspace::Layout {
                    tabs: self.tabs.iter().map(|t| t.path.clone()).collect(),
                    active: self.active,
                    split: self.other.clone(),
                    focus_right: self.focus_right,
                };
                self.workspace.layouts.insert(name.clone(), layout);
                self.save_workspace();
                self.toast(format!("ワークスペース「{name}」を保存しました"));
            }
            _ => {}
        }
    }

    fn toast_ui(&mut self, ctx: &egui::Context) {
        let Some((msg, at)) = &self.toast else { return };
        let remaining = Duration::from_secs(4).saturating_sub(at.elapsed());
        if remaining.is_zero() {
            self.toast = None;
            return;
        }
        egui::Area::new(egui::Id::unique("toast"))
            .order(egui::Order::Tooltip)
            .anchor(Align2::RIGHT_BOTTOM, [-16.0, -36.0])
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(420.0);
                    ui.label(msg);
                });
            });
        ctx.request_repaint_after(remaining);
    }
}

/// ノートごとのエディタのID（TextEditの状態の保存先）。
/// 本文の左右の余白。「読みやすい行幅」が有効なら、本文が最大幅に収まるよう中央に寄せる。
fn side_margin(available: f32, cfg: &Config) -> f32 {
    const MIN: f32 = 24.0;
    if cfg.readable_width {
        ((available - cfg.max_width) / 2.0).max(MIN)
    } else {
        MIN
    }
}

/// プレビューの ```mermaid ブロックを、画像ができていれば画像に置き換える。
/// 作成中・失敗のときは、コードのまま見せて一言添える。
fn mermaid_images(
    src: &str,
    root: &std::path::Path,
    mermaid: &mut crate::mermaid::Mermaid,
    cfg: &Config,
    ctx: &egui::Context,
) -> String {
    use crate::mermaid::State;
    let blocks = crate::mermaid::blocks(src);
    if blocks.is_empty() {
        return src.to_string();
    }
    let mut out = String::with_capacity(src.len());
    let mut at = 0;
    for (range, body) in blocks {
        out.push_str(&src[at..range.start]);
        let block = &src[range.clone()];
        match mermaid.image(root, &body, cfg.dark, &cfg.mermaid_command, ctx) {
            Some(State::Ready(png)) => {
                let uri = png.display().to_string().replace('\\', "/");
                let uri = if uri.starts_with('/') {
                    format!("file://{uri}")
                } else {
                    format!("file:///{uri}")
                };
                // 空白を含むパスでも切れないよう <…> で囲む（エンコードはしない。読み込み側が解かないため）
                out.push_str(&format!("![Mermaid](<{uri}>)\n"));
            }
            Some(State::Pending) => {
                out.push_str("*Mermaid図を作成中…*\n\n");
                out.push_str(block);
            }
            Some(State::Failed(e)) => {
                out.push_str(&format!(
                    "*Mermaid図を作れませんでした: {e}（コマンドは設定で変えられます）*\n\n"
                ));
                out.push_str(block);
            }
            None => out.push_str(block),
        }
        at = range.end;
    }
    out.push_str(&src[at..]);
    out
}

/// ファイル名に使えない文字を除く
fn sanitize_name(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .filter(|c| {
            !matches!(
                c,
                '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '#' | '^' | '[' | ']'
            )
        })
        .collect();
    cleaned
        .trim()
        .chars()
        .take(50)
        .collect::<String>()
        .trim()
        .to_string()
}

/// 切り出す文字列から、新しいノートの名前の候補を作る（最初の空でない行。見出しや箇条書きの記号は除く）
fn suggested_name(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let line = line.trim_start_matches(['#', '-', '*', '+', '>', ' ']);
    sanitize_name(line)
}

/// 数式の文字の大きさ（表示数式は少し大きくする）
fn math_font(size: f32, display: bool) -> f32 {
    if display {
        size * 1.1
    } else {
        size
    }
}

/// エディタの選択範囲（バイト位置）。選択が無ければカーソル位置の空の範囲。
fn selection_bytes(ctx: &egui::Context, tab: &Tab) -> std::ops::Range<usize> {
    let state = TextEdit::load_state(ctx, editor_id(&tab.path));
    match state.and_then(|s| s.cursor.char_range()) {
        Some(r) => {
            let a = editor::byte_of_char(&tab.text, r.primary.index.0);
            let b = editor::byte_of_char(&tab.text, r.secondary.index.0);
            a.min(b)..a.max(b)
        }
        None => {
            let c = tab.cursor.min(tab.text.len());
            c..c
        }
    }
}

fn editor_id(path: &str) -> egui::Id {
    egui::Id::unique(("editor", path))
}

/// エディタの本文を書き換え、変更前の状態を「元に戻す」の履歴に積み、選択範囲を設定する。
fn replace_text_undoable(
    ctx: &egui::Context,
    tab: &mut Tab,
    text: String,
    select: std::ops::Range<usize>,
) {
    let id = editor_id(&tab.path);
    let mut state = TextEdit::load_state(ctx, id).unwrap_or_default();
    let range = state.cursor.char_range().unwrap_or_default();
    let mut undoer = state.undoer();
    undoer.add_undo(&(range, tab.text.clone()));
    state.set_undoer(undoer);
    state.store(ctx, id);
    tab.set_text(text);
    tab.select = Some(select);
    tab.dirty = true;
    tab.edited_at = Some(Instant::now());
}

/// 言及の文脈を、該当部分だけ目立たせて組む。
fn mention_job(ui: &egui::Ui, m: &Mention) -> egui::text::LayoutJob {
    use egui::text::{LayoutJob, TextFormat};
    let font = egui::TextStyle::Small.resolve(ui.style());
    let weak = TextFormat::simple(font.clone(), ui.visuals().weak_text_color());
    let strong = TextFormat::simple(font, ui.visuals().strong_text_color());
    let mut job = LayoutJob::default();
    let (a, b) = (m.column, m.column + m.text.len());
    match (m.context.get(..a), m.context.get(a..b), m.context.get(b..)) {
        (Some(before), Some(hit), Some(after)) => {
            job.append(before, 0.0, weak.clone());
            job.append(hit, 0.0, strong);
            job.append(after, 0.0, weak);
        }
        _ => job.append(&m.context, 0.0, weak),
    }
    job
}

/// ノーマルモードで、Vimのキーとして扱う入力。
fn vim_keys(event: &egui::Event) -> Option<Vec<vim::Key>> {
    use egui::Event;
    match event {
        Event::Text(t) => Some(
            t.chars()
                .map(|c| vim::Key::Char(vim::normalize_key(c)))
                .collect(),
        ),
        Event::Ime(egui::ImeEvent::Commit(t)) => Some(
            t.chars()
                .map(|c| vim::Key::Char(vim::normalize_key(c)))
                .collect(),
        ),
        Event::Ime(_) => Some(Vec::new()),
        Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } => match (key, *modifiers) {
            (Key::Escape, _) => Some(vec![vim::Key::Esc]),
            (Key::Enter, m) if m.is_none() => Some(vec![vim::Key::Enter]),
            (Key::Backspace, m) if m.is_none() => Some(vec![vim::Key::Backspace]),
            (Key::Delete, m) if m.is_none() => Some(vec![vim::Key::Char('x')]),
            (Key::R, m) if m.command_only() => Some(vec![vim::Key::CtrlR]),
            (Key::D, m) if m.command_only() => Some(vec![vim::Key::CtrlD]),
            (Key::U, m) if m.command_only() => Some(vec![vim::Key::CtrlU]),
            (Key::OpenBracket, m) if m.command_only() => Some(vec![vim::Key::Esc]),
            _ => None,
        },
        _ => None,
    }
}

/// カーソルの移動だけで本文を変えない入力（ノーマルモードでもエディタに渡す）。
fn is_navigation(event: &egui::Event) -> bool {
    matches!(
        event,
        egui::Event::Key {
            key: Key::ArrowLeft
                | Key::ArrowRight
                | Key::ArrowUp
                | Key::ArrowDown
                | Key::Home
                | Key::End
                | Key::PageUp
                | Key::PageDown,
            ..
        }
    ) || matches!(event, egui::Event::Key { modifiers, .. } if modifiers.command && !matches!(event, egui::Event::Key { key: Key::V | Key::X, .. }))
}

/// 編集補助が受け取る入力。
enum EditKey {
    Enter,
    /// trueならShift+Tab
    Tab(bool),
    Checkbox,
    Wrap(&'static str),
    Type(char),
    Paste(String),
    PasteImage(std::sync::Arc<egui::ColorImage>),
}

fn classify(event: &egui::Event) -> Option<EditKey> {
    use egui::Event;
    match event {
        Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } => match (key, *modifiers) {
            (Key::Enter, m) if m.is_none() => Some(EditKey::Enter),
            (Key::Tab, m) if m.is_none() => Some(EditKey::Tab(false)),
            (Key::Tab, m) if m == Modifiers::SHIFT => Some(EditKey::Tab(true)),
            (Key::L, m) if m.command_only() => Some(EditKey::Checkbox),
            (Key::B, m) if m.command_only() => Some(EditKey::Wrap("**")),
            (Key::I, m) if m.command_only() => Some(EditKey::Wrap("*")),
            _ => None,
        },
        Event::Text(t) => {
            let mut chars = t.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if "([{\"`*_=~)]".contains(c) => Some(EditKey::Type(c)),
                _ => None,
            }
        }
        Event::Paste(t) => Some(EditKey::Paste(t.clone())),
        Event::PasteImage(img) => Some(EditKey::PasteImage(img.clone())),
        _ => None,
    }
}

/// TextEditが本文を変える入力か（編集補助の入力より前にあると、順序が崩れる）。
fn changes_text(event: &egui::Event) -> bool {
    use egui::Event;
    match event {
        Event::Text(_) | Event::Paste(_) | Event::Cut | Event::Ime(_) => true,
        Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } => {
            modifiers.command
                || matches!(
                    key,
                    Key::Enter
                        | Key::Tab
                        | Key::Backspace
                        | Key::Delete
                        | Key::ArrowLeft
                        | Key::ArrowRight
                        | Key::ArrowUp
                        | Key::ArrowDown
                        | Key::Home
                        | Key::End
                        | Key::PageUp
                        | Key::PageDown
                )
        }
        _ => false,
    }
}

/// 入力の中から、編集補助で扱える最初のものを取り出す。`try_edit` が Some を返したものだけを取る
/// （Noneなら通常の入力としてTextEditに任せる）。それより前に本文を変える入力があれば、
/// 順序を保つためにその入力以降を次のフレームへ回す。
fn take_edit_event<T>(
    events: &mut Vec<egui::Event>,
    deferred: &mut Vec<egui::Event>,
    mut try_edit: impl FnMut(&EditKey) -> Option<T>,
) -> Option<(EditKey, T)> {
    let mut blocked = false;
    for i in 0..events.len() {
        if let Some(key) = classify(&events[i]) {
            if let Some(result) = try_edit(&key) {
                if blocked {
                    deferred.extend(events.drain(i..));
                    return None;
                }
                events.remove(i);
                return Some((key, result));
            }
        }
        blocked |= changes_text(&events[i]);
    }
    None
}

fn response_focus(output: &egui::text_edit::TextEditOutput) -> bool {
    output.response.response.has_focus()
}

fn tree_ui(
    ui: &mut egui::Ui,
    tree: &[TreeDir],
    idx: usize,
    active: Option<&str>,
    actions: &mut Vec<Action>,
    vault: &Vault,
    cache: &mut CommonMarkCache,
) {
    for &d in &tree[idx].dirs {
        let dir = &tree[d];
        let header = egui::CollapsingHeader::new(&dir.name)
            .id_salt(("dir", &dir.path))
            .show(ui, |ui| tree_ui(ui, tree, d, active, actions, vault, cache));
        header.header_response.context_menu(|ui| {
            let path = dir.path.clone();
            if ui.button("ここに新規ノート").clicked() {
                actions.push(Action::Dialog(Dialog::NewNote {
                    dir: path.clone(),
                    name: String::new(),
                    template: None,
                }));
            }
            if ui.button("ここに新規フォルダ").clicked() {
                actions.push(Action::Dialog(Dialog::NewFolder {
                    dir: path.clone(),
                    name: String::new(),
                }));
            }
            if ui.button("改名/移動…").clicked() {
                actions.push(Action::Dialog(Dialog::Rename {
                    new: path.clone(),
                    path: path.clone(),
                }));
            }
            if ui.button("ゴミ箱へ移動").clicked() {
                actions.push(Action::Dialog(Dialog::Delete { path }));
            }
        });
    }
    for (name, path, note) in &tree[idx].files {
        let text = if *note {
            RichText::new(name)
        } else {
            RichText::new(name).weak()
        };
        let mut row = ui.selectable_label(active == Some(path.as_str()), text);
        // ファイル一覧では、Ctrlを押しているときだけプレビューを出す（一覧を眺めるときに邪魔にならないように）
        if *note && ui.input(|i| i.modifiers.command) {
            row = row.on_hover_preview(|ui| hover::note_preview(ui, vault, cache, path, None));
        }
        if row.clicked() {
            actions.push(if *note {
                Action::Open(path.clone(), None)
            } else {
                Action::OpenExternal(path.clone())
            });
        }
        row.context_menu(|ui| {
            if *note && preview::bookmarks_on() && ui.button("ブックマークに追加 / 解除").clicked()
            {
                actions.push(Action::ToggleBookmark(path.clone()));
            }
            if *note && preview::file_recovery_on() && ui.button("履歴…").clicked() {
                actions.push(Action::History(path.clone()));
            }
            if ui.button("改名/移動…").clicked() {
                actions.push(Action::Dialog(Dialog::Rename {
                    path: path.clone(),
                    new: path.clone(),
                }));
            }
            if ui.button("ゴミ箱へ移動").clicked() {
                actions.push(Action::Dialog(Dialog::Delete { path: path.clone() }));
            }
        });
    }
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        // ホイールのスクロールを、egui がならして遅らせる前に、すぐ反映される形に直す
        if !self.cfg.smooth_scroll {
            crate::wheel::make_immediate(raw_input);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_watcher(&ctx);
        self.autosave(&ctx);
        self.poll_external(&ctx);
        self.apply_style(&ctx);
        if self.slideshow.is_some() {
            self.slides_ui(ui, &ctx);
            self.toast_ui(&ctx);
            return;
        }
        // ウィンドウからフォーカスが外れたら即保存する
        if ctx.input(|i| {
            i.events
                .iter()
                .any(|e| matches!(e, egui::Event::WindowFocused(false)))
        }) {
            self.save_all();
        }

        // 前のフレームから回した入力を先頭に戻し、IMEの変換状態を追う
        ctx.input_mut(|input| {
            if !self.deferred_events.is_empty() {
                let mut events = std::mem::take(&mut self.deferred_events);
                events.append(&mut input.events);
                input.events = events;
            }
            for event in &input.events {
                if let egui::Event::Ime(ime) = event {
                    self.ime_composing = match ime {
                        egui::ImeEvent::Preedit { text, .. } => !text.is_empty(),
                        _ => false,
                    };
                }
            }
        });
        let mut actions = Vec::new();
        if self.palette.is_none() {
            self.shortcuts(&ctx, &mut actions);
            // マウスの「戻る/進む」ボタン
            let (back, forward) = ctx.input(|i| {
                (
                    i.pointer.button_pressed(egui::PointerButton::Extra1),
                    i.pointer.button_pressed(egui::PointerButton::Extra2),
                )
            });
            if back {
                actions.push(Action::Run(Command::GoBack));
            }
            if forward {
                actions.push(Action::Run(Command::GoForward));
            }
        }

        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        if self.vault.is_some() && self.cfg.show_left {
            egui::Panel::left("left")
                .default_size(250.0)
                .size_range(160.0..=500.0)
                .show(ui, |ui| self.left_panel(ui, &mut actions));
        }
        // 右サイドバーに出すものが1つも無ければ（コアプラグインをオフにした）、出さない
        let f = self.cfg.features;
        let right_has_content =
            f.backlinks || f.related || f.outgoing_links || f.footnotes || f.outline;
        if self.vault.is_some()
            && self.cfg.show_right
            && right_has_content
            && !self.tabs.is_empty()
            && !self.show_graph
        {
            egui::Panel::right("right")
                .default_size(240.0)
                .size_range(160.0..=500.0)
                .show(ui, |ui| self.right_panel(ui, &mut actions));
        }
        egui::CentralPanel::default().show(ui, |ui| self.central(ui, &mut actions));

        self.palette_ui(&ctx, &mut actions);
        self.dialog_ui(&ctx);
        self.history_ui(&ctx);
        self.external_ui(&ctx);
        self.settings_ui(&ctx);
        self.file_dialog.update(&ctx);
        if let Some(path) = self.file_dialog.take_picked() {
            actions.push(Action::OpenVault(path));
        }
        self.toast_ui(&ctx);

        for action in actions {
            self.apply(action, &ctx);
        }
        self.persist_session();
        if !self.deferred_events.is_empty() {
            ctx.request_repaint();
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_all();
        self.cfg.open_tabs = self.tabs.iter().map(|t| t.path.clone()).collect();
        self.cfg.active_tab = self.active;
        self.cfg.save();
    }
}

#[cfg(test)]
mod composer_tests {
    use super::*;

    #[test]
    fn names_for_extracted_notes() {
        assert_eq!(suggested_name("\n\n## 設計の方針\n本文"), "設計の方針");
        assert_eq!(suggested_name("- 買い物: 牛乳/卵 [[x]]"), "買い物 牛乳卵 x");
        assert_eq!(sanitize_name("  a#b^c?  "), "abc");
        assert_eq!(sanitize_name(&"長".repeat(80)).chars().count(), 50);
    }
}
