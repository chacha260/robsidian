//! 外部のプロセスとのやりとり: ユーザーコマンド（C4）とgit連携（C3）。
//!
//! どちらも別スレッドで実行し、結果を `mpsc` で受け取って次の描画で反映する（画面を止めない）。

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, RichText, ScrollArea};
use robsidian_core::git;
use robsidian_core::usercmd::{self, Finished, Output, UserCommand};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

/// 設定ファイルの更新を確かめる間隔
const RECHECK: Duration = Duration::from_secs(1);
/// gitの変更件数を数え直すまで、最低限あける間隔
const GIT_INTERVAL: Duration = Duration::from_secs(2);

// ---- ユーザーコマンド ----

/// `.robsidian/commands.toml` の内容。更新時刻が変わったら読み直す。
#[derive(Default)]
pub struct UserCommands {
    /// ファイルの本文（無ければ空）。許可したときの内容と比べるのに使う
    pub text: String,
    pub list: Vec<UserCommand>,
    pub keys: Vec<Option<KeyboardShortcut>>,
    stamp: Option<SystemTime>,
    checked: Option<Instant>,
}

impl UserCommands {
    /// 必要なら読み直す。読み直して誤りがあれば、その説明を返す。
    pub fn refresh(&mut self, root: &Path) -> Option<String> {
        if self.checked.is_some_and(|t| t.elapsed() < RECHECK) {
            return None;
        }
        self.checked = Some(Instant::now());
        let stamp = std::fs::metadata(usercmd::path(root))
            .and_then(|m| m.modified())
            .ok();
        if stamp == self.stamp {
            return None;
        }
        self.stamp = stamp;
        match usercmd::load(root) {
            Ok(Some((text, list))) => {
                self.keys = list
                    .iter()
                    .map(|c| c.key.as_deref().and_then(parse_key))
                    .collect();
                self.text = text;
                self.list = list;
                let bad = self
                    .list
                    .iter()
                    .zip(&self.keys)
                    .find(|(c, k)| c.key.is_some() && k.is_none());
                bad.map(|(c, _)| {
                    format!(
                        "ユーザーコマンド「{}」のキー「{}」を解釈できません",
                        c.name,
                        c.key.as_deref().unwrap_or_default()
                    )
                })
            }
            Ok(None) => {
                *self = Self {
                    stamp,
                    checked: self.checked,
                    ..Self::default()
                };
                None
            }
            Err(e) => {
                // 前の内容は捨てる（壊れた設定で古いコマンドが動き続けないように）
                *self = Self {
                    stamp,
                    checked: self.checked,
                    ..Self::default()
                };
                Some(format!("{e:#}"))
            }
        }
    }

    /// 次の `refresh` で必ずファイルを確かめる
    pub fn recheck(&mut self) {
        self.checked = None;
    }
}

/// `ctrl+alt+t` / `Ctrl+Shift+F5` のような書き方を解釈する。
/// 組み込みのショートカットと同じく、Ctrl はmacOSでは Cmd になる。
pub fn parse_key(spec: &str) -> Option<KeyboardShortcut> {
    let mut mods = Modifiers::NONE;
    let mut key = None;
    for part in spec.split('+').map(str::trim) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "cmd" | "command" | "mod" => mods = mods.plus(Modifiers::COMMAND),
            "alt" | "option" => mods = mods.plus(Modifiers::ALT),
            "shift" => mods = mods.plus(Modifiers::SHIFT),
            _ if key.is_none() => {
                let found = Key::ALL
                    .iter()
                    .copied()
                    .find(|k| k.name().eq_ignore_ascii_case(part))
                    .or_else(|| Key::from_name(part));
                key = Some(found?);
            }
            _ => return None,
        }
    }
    Some(KeyboardShortcut::new(mods, key?))
}

/// 修飾キーまで完全に一致したときだけキー入力を消費する。
/// egui の `consume_shortcut` は余分な Alt/Shift を無視するので、Ctrl+Alt+D が Ctrl+D にも一致してしまう。
pub fn consume_exact(input: &mut egui::InputState, shortcut: KeyboardShortcut) -> bool {
    let mut hit = false;
    input.events.retain(|e| {
        let is = matches!(
            e,
            egui::Event::Key { key, pressed: true, modifiers, .. }
                if *key == shortcut.logical_key && modifiers.matches_exact(shortcut.modifiers)
        );
        hit |= is;
        !is
    });
    hit
}

/// 実行を頼まれたユーザーコマンド。選択範囲などは頼まれた時点で決める
/// （許可を求めるダイアログを出すと、エディタの選択が外れるため）。
pub struct Request {
    pub cmd: UserCommand,
    pub env: usercmd::Env,
    /// 頼まれたときのノートと本文の版
    pub path: Option<String>,
    pub version: u64,
    /// 出力で置き換える範囲（バイト位置）。入力が無いときは選択範囲（またはカーソル位置）
    pub range: Range<usize>,
    pub input: String,
}

/// 実行中のユーザーコマンド
pub struct Job {
    pub name: String,
    pub path: Option<String>,
    pub version: u64,
    pub range: Range<usize>,
    pub input: String,
    pub output: Output,
    rx: mpsc::Receiver<anyhow::Result<Finished>>,
}

impl Job {
    pub fn spawn(req: Request, root: PathBuf, ctx: &egui::Context) -> Self {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let (cmd, env, stdin) = (req.cmd.clone(), req.env, req.input.clone());
        std::thread::spawn(move || {
            let _ = tx.send(usercmd::execute(&cmd, &root, &env, &stdin));
            ctx.request_repaint();
        });
        Self {
            name: req.cmd.name,
            path: req.path,
            version: req.version,
            range: req.range,
            input: req.input,
            output: req.cmd.output,
            rx,
        }
    }

    /// 終わっていれば結果を返す。
    pub fn poll(&self) -> Option<anyhow::Result<Finished>> {
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err(anyhow::anyhow!("実行が中断されました")))
            }
        }
    }
}

pub enum TrustOutcome {
    Allow,
    Cancel,
}

/// 初めて（または内容が変わってから初めて）実行するときに、設定ファイルの中身を見せて許可を取る。
pub fn trust_ui(ctx: &egui::Context, root: &Path, text: &str) -> Option<TrustOutcome> {
    let mut outcome = None;
    let modal = egui::Modal::new(egui::Id::unique("trust-commands")).show(ctx, |ui| {
        let size = ctx.content_rect().size();
        ui.set_width((size.x * 0.7).min(760.0));
        ui.heading("ユーザーコマンドの実行を許可しますか？");
        ui.add_space(4.0);
        ui.label(
            "このvaultの設定ファイルに書かれたシェルコマンドを実行します。\
             他人から受け取ったvaultなら、中身を確かめてから許可してください。",
        );
        ui.weak(usercmd::path(root).display().to_string());
        ui.add_space(6.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ScrollArea::both()
                .max_height((size.y * 0.5).min(420.0))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    ui.label(RichText::new(text).monospace());
                });
        });
        ui.weak("許可はこの内容に対して記録します。ファイルが書き換わったら、もう一度確認します。");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("許可して実行").clicked() {
                outcome = Some(TrustOutcome::Allow);
            }
            if ui.button("キャンセル").clicked() {
                outcome = Some(TrustOutcome::Cancel);
            }
        });
    });
    if outcome.is_none() && modal.should_close() {
        outcome = Some(TrustOutcome::Cancel);
    }
    outcome
}

// ---- git ----

/// 数え直した結果: (未コミットの変更の件数, リモートとの差)
type Counts = (usize, Option<git::Upstream>);

/// gitの状態。vaultがリポジトリの中に無ければ何もしない。
#[derive(Default)]
pub struct GitState {
    pub repo: bool,
    /// コミットされていない変更の件数（数え終わるまで `None`）
    pub changes: Option<usize>,
    /// 追跡しているリモートとの差（追跡先が無ければ `None`）
    pub upstream: Option<git::Upstream>,
    /// 変更があったので数え直す
    stale: bool,
    last: Option<Instant>,
    counting: Option<mpsc::Receiver<anyhow::Result<Counts>>>,
    /// 実行中の操作: (ステータスバーに出す名前, 結果)
    busy: Option<(&'static str, mpsc::Receiver<anyhow::Result<String>>)>,
}

pub enum GitEvent {
    Done(String),
    Failed(String),
}

impl GitState {
    pub fn detect(root: &Path) -> Self {
        Self {
            repo: git::is_repo(root),
            stale: true,
            ..Self::default()
        }
    }

    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    /// 実行中の操作の名前（「コミット中」など）
    pub fn busy(&self) -> Option<&'static str> {
        self.busy.as_ref().map(|(label, _)| *label)
    }

    /// 数え直しの開始と、終わった処理の受け取り。
    pub fn poll(&mut self, root: &Path, ctx: &egui::Context) -> Option<GitEvent> {
        if !self.repo {
            return None;
        }
        if let Some(rx) = &self.counting {
            match rx.try_recv() {
                Ok(r) => {
                    match r {
                        Ok((changes, upstream)) => {
                            self.changes = Some(changes);
                            self.upstream = upstream;
                        }
                        Err(_) => self.changes = None,
                    }
                    self.counting = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.counting = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let mut event = None;
        if let Some((_, rx)) = &self.busy {
            let done = match rx.try_recv() {
                Ok(r) => Some(r),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err(anyhow::anyhow!("gitの操作が中断されました")))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some(r) = done {
                self.busy = None;
                self.stale = true;
                self.last = None;
                event = Some(match r {
                    Ok(msg) => GitEvent::Done(msg),
                    Err(e) => GitEvent::Failed(format!("{e:#}")),
                });
            }
        }
        if self.stale && self.counting.is_none() && self.busy.is_none() {
            let wait = self
                .last
                .map_or(Duration::ZERO, |t| GIT_INTERVAL.saturating_sub(t.elapsed()));
            if wait.is_zero() {
                self.stale = false;
                self.last = Some(Instant::now());
                self.counting = Some(spawn(root, ctx, |root| {
                    let changes = git::changes(&root)?.len();
                    Ok((changes, git::upstream(&root)?))
                }));
            } else {
                ctx.request_repaint_after(wait);
            }
        }
        event
    }

    fn start(
        &mut self,
        label: &'static str,
        root: &Path,
        ctx: &egui::Context,
        f: impl FnOnce(PathBuf) -> anyhow::Result<String> + Send + 'static,
    ) {
        if self.busy.is_none() {
            self.busy = Some((label, spawn(root, ctx, f)));
        }
    }

    pub fn commit(&mut self, root: &Path, message: String, ctx: &egui::Context) {
        self.start("コミット中…", root, ctx, move |root| {
            Ok(match git::snapshot(&root, &message)? {
                Some(hash) => format!("コミットしました: {hash}"),
                None => "コミットする変更はありません".into(),
            })
        });
    }

    pub fn pull(&mut self, root: &Path, ctx: &egui::Context) {
        self.start("取り込み中…", root, ctx, |root| git::pull(&root));
    }

    pub fn push(&mut self, root: &Path, ctx: &egui::Context) {
        self.start("送信中…", root, ctx, |root| git::push(&root));
    }
}

fn spawn<T: Send + 'static>(
    root: &Path,
    ctx: &egui::Context,
    f: impl FnOnce(PathBuf) -> T + Send + 'static,
) -> mpsc::Receiver<T> {
    let (tx, rx) = mpsc::channel();
    let root = root.to_path_buf();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let _ = tx.send(f(root));
        ctx.request_repaint();
    });
    rx
}

/// 既定のコミットメッセージ
pub fn snapshot_message() -> String {
    format!(
        "スナップショット {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_key_specs() {
        let k = parse_key("ctrl+alt+t").unwrap();
        assert_eq!(k.logical_key, Key::T);
        assert!(k.modifiers.command && k.modifiers.alt && !k.modifiers.shift);
        let k = parse_key("Shift + F5").unwrap();
        assert_eq!(k.logical_key, Key::F5);
        assert!(k.modifiers.shift);
        assert_eq!(parse_key("alt+enter").unwrap().logical_key, Key::Enter);
        assert_eq!(
            parse_key("ctrl+pagedown").unwrap().logical_key,
            Key::PageDown
        );
        assert!(parse_key("ctrl+nosuchkey").is_none());
        assert!(parse_key("ctrl+a+b").is_none());
        assert!(parse_key("ctrl").is_none());
    }
}
