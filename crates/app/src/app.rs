//! アプリ本体: 状態管理と画面全体のレイアウト。

use crate::config::{Config, ViewMode};
use crate::edit;
use crate::editor::{self, Colors};
use crate::find;
use crate::fonts;
use crate::graph::Graph;
use crate::history_view::{self, HistoryView};
use crate::hover;
use crate::preview;
use crate::settings::SettingsView;
use crate::workspace::{self, Workspace};
use eframe::egui::text::{CCursor, CCursorRange};
use eframe::egui::{
    self, Align, Align2, Key, KeyboardShortcut, Layout, Modifiers, RichText, ScrollArea, TextEdit,
};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use notify::Watcher as _;
use robsidian_core::vault::{file_name, is_note_path, parent_dir};
use robsidian_core::{parse, search, Heading, Mention, NoteHit, Vault};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

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
    ToggleTheme,
    Rescan,
}

const CTRL: Modifiers = Modifiers::COMMAND;
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
            Command::OpenVault => "open vault folder",
            Command::ToggleTheme => "theme dark light",
            Command::Rescan => "rescan reload refresh index",
        }
    }
}

type CommandEntry = (Command, &'static str, Option<(Modifiers, Key)>);
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
    (Command::OpenVault, "別のvaultを開く", Some((CTRL, Key::O))),
    (Command::ToggleTheme, "ダーク/ライト切替", None),
    (Command::Rescan, "vaultを再読み込み", None),
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
    PasteImage(std::sync::Arc<egui::ColorImage>),
    /// 未リンクの言及をリンクにする（言及, リンク先のノート）
    LinkMentions(Vec<Mention>, String),
    ToggleBookmark(String),
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
        }
    }

    fn title(&self) -> &str {
        let name = file_name(&self.path);
        name.strip_suffix(".md").unwrap_or(name)
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
    Bookmarks,
}

enum PaletteItem {
    Note(String),
    Template(String),
    Create(String),
    Cmd(usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PaletteMode {
    Notes,
    Commands,
    /// 選んだテンプレートを現在のノートのカーソル位置に挿入する
    InsertTemplate,
    /// 選んだテンプレートで新規ノートを作る
    NewFromTemplate,
}

struct Palette {
    mode: PaletteMode,
    query: String,
    computed_for: Option<String>,
    items: Vec<PaletteItem>,
    selected: usize,
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
    /// 未リンクの言及: (索引の世代, ノートのパス, 結果)
    mentions: Option<(u64, String, Vec<Mention>)>,
    graph: Graph,
    show_graph: bool,
    watcher: Option<(notify::RecommendedWatcher, mpsc::Receiver<Vec<PathBuf>>)>,
    rescan_at: Option<Instant>,
    toast: Option<(String, Instant)>,
    applied_style: Option<(bool, f32)>,
    /// 最後に設定ファイルへ書いたタブ構成（変化したら書き直す）
    saved_session: (Vec<String>, usize),
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, vault_arg: Option<PathBuf>) -> Self {
        let cfg = Config::load();
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
            mentions: None,
            graph: Graph::new(true),
            show_graph: false,
            watcher: None,
            rescan_at: None,
            toast: None,
            applied_style: None,
            saved_session: (Vec::new(), 0),
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
            }
        }
        app
    }

    /// 開いているタブを設定ファイルに記録する（異常終了しても次回復元できるように）。
    fn persist_session(&mut self) {
        let session = (
            self.tabs.iter().map(|t| t.path.clone()).collect(),
            self.active,
        );
        if session != self.saved_session {
            self.cfg.open_tabs.clone_from(&session.0);
            self.cfg.active_tab = session.1;
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
                    let Some(rel) = vault.rel(&abs) else { continue };
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
            self.open_external(&file, ctx);
        } else if !target.is_empty() {
            // Obsidianと同じく、存在しないリンクを開くとノートを作る
            self.create_and_open(target);
        }
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
            }
        }
    }

    fn open_history(&mut self, path: &str) {
        if let Some(vault) = &self.vault {
            self.history = Some(HistoryView::new(vault.root(), path));
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
        let colors = Colors::new(self.cfg.dark);
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
        let colors = Colors::new(self.cfg.dark);
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
            self.find = None;
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

    fn close_tab(&mut self, idx: usize) {
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
            Action::OpenExternal(path) => self.open_external(&path, ctx),
            Action::FollowLink { target, from } => self.follow_link(&target, &from, ctx),
            Action::Activate(i) => {
                self.active = i;
                self.show_graph = false;
                self.completion = None;
            }
            Action::Close(i) => self.close_tab(i),
            Action::Search(query) => {
                self.search_query = query;
                self.cfg.show_left = true;
                self.left_tab = LeftTab::Search;
            }
            Action::Dialog(d) => self.dialog = Some(d),
            Action::OpenVault(path) => self.open_vault(path, ctx),
            Action::Run(cmd) => self.run(cmd, ctx),
            Action::History(path) => self.open_history(&path),
            Action::PasteImage(image) => self.paste_image(&image),
            Action::LinkMentions(mentions, target) => self.link_mentions(&mentions, &target),
            Action::ToggleBookmark(path) => self.toggle_bookmark(&path),
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        if self.dialog.is_some() || self.history.is_some() || self.settings.is_some() {
            return;
        }
        ctx.input_mut(|input| {
            for &(cmd, _, shortcut) in COMMANDS {
                if let Some((mods, key)) = shortcut {
                    if input.consume_shortcut(&KeyboardShortcut::new(mods, key)) {
                        actions.push(Action::Run(cmd));
                    }
                }
            }
        });
    }

    fn apply_style(&mut self, ctx: &egui::Context) {
        let key = (self.cfg.dark, self.cfg.font_size);
        if self.applied_style == Some(key) {
            return;
        }
        self.applied_style = Some(key);
        ctx.set_theme(if self.cfg.dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
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
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(tab) = self.tabs.get(self.active) {
                    ui.weak(if tab.dirty {
                        "● 未保存"
                    } else {
                        "保存済み"
                    });
                    ui.weak(format!("{}文字", tab.text.chars().count()));
                    if let Some(vault) = &self.vault {
                        ui.weak(format!("被リンク {}", vault.backlinks(&tab.path).len()));
                    }
                    ui.weak(&tab.path);
                }
            });
        });
    }

    fn left_panel(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.left_tab, LeftTab::Files, "ファイル");
            ui.selectable_value(&mut self.left_tab, LeftTab::Search, "検索");
            ui.selectable_value(&mut self.left_tab, LeftTab::Tags, "タグ");
            ui.selectable_value(&mut self.left_tab, LeftTab::Bookmarks, "★");
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
                    if ui
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
                            .on_hover_ui(|ui| {
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
                            .on_hover_ui(|ui| {
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
        let generation = self.generation;
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
        ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.add_space(4.0);
            let backlinks = vault.backlinks(&tab.path);
            ui.strong(format!("バックリンク ({})", backlinks.len()));
            ui.add_space(2.0);
            if backlinks.is_empty() {
                ui.weak("このノートへのリンクはありません");
            }
            let mut last_source = "";
            for link in backlinks {
                if link.source != last_source {
                    last_source = &link.source;
                    let name = file_name(&link.source);
                    let title = name.strip_suffix(".md").unwrap_or(name);
                    if ui
                        .selectable_label(false, title)
                        .on_hover_ui(|ui| hover::note_preview(ui, vault, cache, &link.source, None))
                        .clicked()
                    {
                        actions.push(Action::Open(link.source.clone(), Some(link.line)));
                    }
                }
                let text = RichText::new(vault.line(&link.source, link.line))
                    .small()
                    .weak();
                if ui.add(egui::Button::new(text).frame(false)).clicked() {
                    actions.push(Action::Open(link.source.clone(), Some(link.line)));
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
                                .on_hover_ui(|ui| hover::note_preview(ui, vault, cache, &m.source, None));
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
                        if ui.button("ブックマークに追加 / 解除").clicked() {
                            actions.push(Action::ToggleBookmark(tab.path.clone()));
                        }
                        if ui.button("履歴…").clicked() {
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
            find: find_bar,
            ..
        } = self;
        let (Some(tab), Some(vault)) = (tabs.get_mut(*active), vault.as_ref()) else {
            return;
        };
        let ctx = ui.ctx().clone();
        let id = editor_id(&tab.path);
        let size = cfg.font_size;
        let colors = Colors::new(cfg.dark);

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

        // ---- 編集補助（リストの継続、インデント、チェックボックス、強調、括弧、貼り付け） ----
        let popup_open = completion.as_ref().is_some_and(|c| !c.items.is_empty());
        if ctx.memory(|m| m.has_focus(id)) && !popup_open && !*ime_composing {
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

        if std::mem::take(&mut tab.focus) {
            ctx.memory_mut(|m| m.request_focus(id));
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
        let mut scroll_to = None;
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
            (marks, current_mark).hash(&mut h);
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
                let options = editor::Options {
                    size,
                    colors: &colors,
                    live,
                    cursor,
                    image: &image_size,
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
                                let background = ui.painter().add(egui::Shape::Noop);
                                let output = TextEdit::multiline(text)
                                    .id(id)
                                    .frame(egui::Frame::NONE)
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(30)
                                    .lock_focus(true)
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
                editor::DecoKind::Checkbox(checked) => {
                    let side = size * 0.95;
                    let rect = egui::Rect::from_center_size(span.center(), egui::vec2(side, side));
                    if *checked {
                        painter.rect_filled(rect, 3.0, colors.accent);
                        let (c, u) = (rect.center(), side * 0.5);
                        let points = vec![
                            c + egui::vec2(-0.5 * u, 0.0),
                            c + egui::vec2(-0.12 * u, 0.4 * u),
                            c + egui::vec2(0.55 * u, -0.4 * u),
                        ];
                        let stroke = egui::Stroke::new(1.8, colors.code_bg);
                        painter.add(egui::Shape::line(points, stroke));
                    } else {
                        let stroke = egui::Stroke::new(1.3, colors.dim);
                        painter.rect_stroke(rect, 3.0, stroke, egui::StrokeKind::Inside);
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
                        if hit.clicked() {
                            toggle = Some((deco.chars.start + 1, !*checked));
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
        if let Some((ch, checked)) = toggle {
            let byte = editor::byte_of_char(&tab.text, ch);
            if tab.text.is_char_boundary(byte) && tab.text.is_char_boundary(byte + 1) {
                // Ctrl+Zで戻せるよう、変更前の状態をTextEditの履歴に積む
                let mut state = TextEdit::load_state(&ctx, id).unwrap_or_default();
                let range = state.cursor.char_range().unwrap_or_default();
                let mut undoer = state.undoer();
                undoer.add_undo(&(range, tab.text.clone()));
                state.set_undoer(undoer);
                state.store(&ctx, id);
                tab.text
                    .replace_range(byte..byte + 1, if checked { "x" } else { " " });
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
        }
        let cursor_char = output.cursor_range.map(|r| r.primary.index.0);
        if let Some(c) = cursor_char {
            tab.cursor = editor::byte_of_char(&tab.text, c);
        }
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
                if let Some((path, heading)) = resolved {
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
            tab.preview = Some((tab.version, *generation, src));
        }
        let Some((_, _, src)) = &tab.preview else {
            return;
        };
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
                            .max_image_width(Some(width))
                            .show(ui, md_cache, src);
                    });
                });
                ui.add_space(12.0);
            });
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
            } else if p.mode == PaletteMode::Commands {
                let q = q.to_lowercase();
                let hits = COMMANDS.iter().enumerate().filter(|(_, c)| {
                    // IMEをオフにしたまま英語でも引けるようにする
                    c.1.to_lowercase().contains(&q) || c.0.keywords().contains(q.as_str())
                });
                p.items.extend(hits.map(|(i, _)| PaletteItem::Cmd(i)));
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
                p.items.extend(notes.into_iter().map(PaletteItem::Note));
                if !q.is_empty() && !exact {
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
                PaletteItem::Note(path) => actions.push(Action::Open(path.clone(), None)),
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
                PaletteItem::Cmd(ci) => actions.push(Action::Run(COMMANDS[*ci].0)),
            }
        } else if close {
            self.palette = None;
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
            row = row.on_hover_ui(|ui| hover::note_preview(ui, vault, cache, path, None));
        }
        if row.clicked() {
            actions.push(if *note {
                Action::Open(path.clone(), None)
            } else {
                Action::OpenExternal(path.clone())
            });
        }
        row.context_menu(|ui| {
            if *note && ui.button("ブックマークに追加 / 解除").clicked() {
                actions.push(Action::ToggleBookmark(path.clone()));
            }
            if *note && ui.button("履歴…").clicked() {
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
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.apply_style(&ctx);
        self.poll_watcher(&ctx);
        self.autosave(&ctx);
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
        if self.vault.is_some() && self.cfg.show_right && !self.tabs.is_empty() && !self.show_graph
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
