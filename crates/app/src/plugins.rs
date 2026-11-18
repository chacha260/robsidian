//! プラグイン（フェーズE1）のアプリ側: 一覧の読み直し、許可、実行（別スレッド）、
//! コードブロックの結果とパネルの中身の覚え書き。
//!
//! 実行の結果（してほしいこと）を本文に反映するのは `App`（`app.rs` の `finish_plugin`）。

use crate::config::Config;
use eframe::egui::{self, KeyboardShortcut, RichText, ScrollArea};
use robsidian_core::plugin::{self, Plugin, Request, Response};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

/// `plugin.toml` の更新を確かめる間隔
const RECHECK: Duration = Duration::from_secs(1);

/// コードブロックやパネルの結果
#[derive(Clone, Debug, PartialEq)]
pub enum Output {
    Running,
    Done(String),
    Failed(String),
}

/// パネルの中身（どのノートの、何回目の保存のときのものか）
pub struct Panel {
    pub note: String,
    pub saves: u64,
    pub output: Output,
}

/// 実行を頼んだ場面（結果をどこに反映するか）
pub enum Origin {
    /// コマンド。頼んだときのノートと本文の版、選択範囲（バイト位置）
    Command {
        path: Option<String>,
        version: u64,
        range: Range<usize>,
    },
    Block(u64),
    Panel {
        key: (String, String),
        note: String,
        saves: u64,
    },
    /// イベント。頼んだときのノートと本文の版
    Event {
        path: String,
        version: u64,
    },
}

pub struct Job {
    pub name: String,
    pub origin: Origin,
    rx: mpsc::Receiver<anyhow::Result<Response>>,
    /// 常駐するプラグインは返事が来ないこともあるので、頼んだ時刻を覚えて打ち切る
    started: Instant,
}

#[derive(Default)]
pub struct Plugins {
    pub list: Vec<Plugin>,
    /// 読めなかったプラグイン（id, 理由）
    pub broken: Vec<(String, String)>,
    /// コマンドのショートカット（プラグインの番号, コマンドの番号, キー）
    pub keys: Vec<(usize, usize, KeyboardShortcut)>,
    signature: Vec<(String, Option<SystemTime>)>,
    checked: Option<Instant>,
    jobs: Vec<Job>,
    /// コードブロックの結果（プラグイン・言語・中身のハッシュ → 結果）
    blocks: HashMap<u64, Output>,
    /// パネルの中身（(プラグイン, パネル) → 中身）
    pub panels: HashMap<(String, String), Panel>,
    /// 起動したままのプラグイン（段階2。id → プロセス）
    servers: HashMap<String, plugin::Server>,
}

/// 許可を覚えておくキー（vault とプラグインごと）
pub fn trust_key(root: &Path, id: &str) -> String {
    format!("plugin:{}#{id}", root.display())
}

/// 許可されていて、オフにもされていないか
pub fn usable(cfg: &Config, root: &Path, p: &Plugin) -> Usable {
    let key = trust_key(root, &p.id);
    if cfg.disabled_plugins.contains(&key) {
        Usable::Disabled
    } else if cfg.trusted_commands.get(&key) == Some(&p.text) {
        Usable::Yes
    } else {
        Usable::Untrusted
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Usable {
    Yes,
    Disabled,
    Untrusted,
}

fn block_key(plugin: &str, lang: &str, code: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (plugin, lang, code).hash(&mut h);
    h.finish()
}

impl Plugins {
    /// 必要なら一覧を読み直す（`plugin.toml` が増えた・減った・書き換わった）。読み直したら true
    pub fn refresh(&mut self, root: &Path) -> bool {
        if self.checked.is_some_and(|t| t.elapsed() < RECHECK) {
            return false;
        }
        self.checked = Some(Instant::now());
        let mut signature: Vec<(String, Option<SystemTime>)> = std::fs::read_dir(plugin::dir(root))
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| {
                        let toml = e.path().join("plugin.toml");
                        let stamp = std::fs::metadata(&toml).and_then(|m| m.modified()).ok();
                        (e.file_name().to_string_lossy().into_owned(), stamp)
                    })
                    .collect()
            })
            .unwrap_or_default();
        signature.sort();
        if signature == self.signature {
            return false;
        }
        self.signature = signature;
        let (list, broken) = plugin::discover(root);
        self.keys = list
            .iter()
            .enumerate()
            .flat_map(|(pi, p)| {
                p.manifest
                    .command
                    .iter()
                    .enumerate()
                    .filter_map(move |(ci, c)| {
                        let key = c.key.as_deref().and_then(crate::external::parse_key)?;
                        Some((pi, ci, key))
                    })
            })
            .collect();
        self.list = list;
        self.broken = broken;
        // 定義が変わったので、結果は作り直す（起動したままのプラグインも止めて、次に使うときに起動し直す）
        self.blocks.clear();
        self.panels.clear();
        self.servers.clear();
        true
    }

    pub fn find(&self, id: &str) -> Option<&Plugin> {
        self.list.iter().find(|p| p.id == id)
    }

    /// プラグインを別スレッドで呼ぶ
    pub fn spawn(&mut self, plugin: &Plugin, req: Request, origin: Origin, ctx: &egui::Context) {
        let rx = match plugin.manifest.mode {
            plugin::Mode::Once => {
                let (tx, rx) = mpsc::channel();
                let ctx = ctx.clone();
                let p = plugin.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(plugin::execute(&p, &req));
                    ctx.request_repaint();
                });
                rx
            }
            // 常駐するプラグイン: 起動していなければ起動し、依頼を送る（返事は poll で受け取る）
            plugin::Mode::Server => match self.server(plugin, Path::new(&req.vault)) {
                Ok(server) => server.request(&req),
                Err(e) => {
                    let (tx, rx) = mpsc::channel();
                    let _ = tx.send(Err(e));
                    rx
                }
            },
        };
        self.jobs.push(Job {
            name: plugin.manifest.name.clone(),
            origin,
            rx,
            started: Instant::now(),
        });
    }

    /// 起動したままのプラグイン（落ちていたら起動し直す）
    fn server(&mut self, plugin: &Plugin, vault: &Path) -> anyhow::Result<&plugin::Server> {
        if self.servers.get(&plugin.id).is_some_and(|s| !s.alive()) {
            self.servers.remove(&plugin.id);
        }
        if !self.servers.contains_key(&plugin.id) {
            let server = plugin::Server::start(plugin, vault)?;
            self.servers.insert(plugin.id.clone(), server);
        }
        Ok(&self.servers[&plugin.id])
    }

    /// 起動したままのプラグインを止める（オフにした・許可を取り消したとき）
    pub fn stop(&mut self, id: &str) {
        self.servers.remove(id);
    }

    /// 起動しているプラグインの id
    pub fn server_ids(&self) -> Vec<String> {
        self.servers.keys().cloned().collect()
    }

    /// 起動したままのプラグインからの問い合わせ・通知（プラグインの id ごと）
    pub fn host_calls(&mut self) -> Vec<(String, plugin::HostCall)> {
        self.servers
            .iter()
            .flat_map(|(id, s)| s.calls().into_iter().map(move |c| (id.clone(), c)))
            .collect()
    }

    /// 問い合わせに返事をする
    pub fn reply(
        &self,
        plugin: &str,
        id: serde_json::Value,
        result: anyhow::Result<serde_json::Value>,
    ) {
        if let Some(s) = self.servers.get(plugin) {
            s.reply(id, result);
        }
    }

    /// プラグインがパネルの作り直しを頼んだ（`panel/refresh`。id が無ければそのプラグインのパネルすべて）
    pub fn invalidate_panels(&mut self, plugin: &str, panel: Option<&str>) {
        self.panels
            .retain(|(p, id), _| p != plugin || panel.is_some_and(|x| x != id));
    }

    /// 終わった実行を取り出す。コードブロックとパネルの結果はここで覚える
    pub fn poll(&mut self) -> Vec<(Job, anyhow::Result<Response>)> {
        let mut done = Vec::new();
        let mut i = 0;
        while i < self.jobs.len() {
            let r = match self.jobs[i].rx.try_recv() {
                Ok(r) => r,
                Err(mpsc::TryRecvError::Empty)
                    if self.jobs[i].started.elapsed() > plugin::TIMEOUT =>
                {
                    Err(anyhow::anyhow!(
                        "プラグイン「{}」が{}秒で返事をしませんでした",
                        self.jobs[i].name,
                        plugin::TIMEOUT.as_secs()
                    ))
                }
                Err(mpsc::TryRecvError::Empty) => {
                    i += 1;
                    continue;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    Err(anyhow::anyhow!("実行が中断されました"))
                }
            };
            let job = self.jobs.remove(i);
            let output = match &r {
                Ok(res) => Output::Done(res.markdown.clone().unwrap_or_default()),
                Err(e) => Output::Failed(format!("{e:#}")),
            };
            match &job.origin {
                Origin::Block(key) => {
                    self.blocks.insert(*key, output);
                }
                Origin::Panel { key, note, saves } => {
                    // 新しいノートで頼み直していたら、古い結果は捨てる
                    if self
                        .panels
                        .get(key)
                        .is_some_and(|p| p.note == *note && p.saves == *saves)
                    {
                        self.panels.insert(
                            key.clone(),
                            Panel {
                                note: note.clone(),
                                saves: *saves,
                                output,
                            },
                        );
                    }
                }
                _ => {}
            }
            done.push((job, r));
        }
        done
    }

    /// 実行中のプラグインの名前（ステータスバーに出す）
    pub fn running(&self) -> impl Iterator<Item = &str> {
        self.jobs.iter().map(|j| j.name.as_str())
    }

    /// プレビューの ```lang のうち、プラグインが描くものを結果の Markdown に置き換える。
    /// まだ結果が無ければ実行を頼む（`run` を呼ぶ）
    pub fn render_blocks(
        &mut self,
        src: &str,
        cfg: &Config,
        root: &Path,
        note: &str,
        ctx: &egui::Context,
    ) -> String {
        let langs: HashMap<String, usize> = self
            .list
            .iter()
            .enumerate()
            .flat_map(|(pi, p)| {
                p.manifest
                    .codeblock
                    .iter()
                    .map(move |b| (b.lang.clone(), pi))
            })
            .collect();
        if langs.is_empty() || !src.contains("```") {
            return src.to_string();
        }
        let mut out = String::with_capacity(src.len());
        let mut lines = src.split_inclusive('\n').peekable();
        while let Some(line) = lines.next() {
            let t = line.trim_start();
            let lang = t.strip_prefix("```").map(str::trim);
            let Some(pi) = lang.and_then(|l| langs.get(l)).copied() else {
                out.push_str(line);
                // 描かないコードブロックの中身は、そのまま写す（中の ``` を開始と読まないように）
                if lang.is_some() {
                    for inner in lines.by_ref() {
                        out.push_str(inner);
                        if inner.trim_start().starts_with("```") {
                            break;
                        }
                    }
                }
                continue;
            };
            let lang = lang.unwrap_or_default().to_string();
            let mut code = String::new();
            let mut closed = false;
            for inner in lines.by_ref() {
                if inner.trim_start().starts_with("```") {
                    closed = true;
                    break;
                }
                code.push_str(inner);
            }
            if !closed {
                // 閉じていないブロックは書きかけとみなし、そのまま見せる
                out.push_str(line);
                out.push_str(&code);
                continue;
            }
            let p = self.list[pi].clone();
            let rendered = match usable(cfg, root, &p) {
                Usable::Disabled => format!("```{lang}\n{code}```\n"),
                Usable::Untrusted => format!(
                    "> [!INFO]\n> **プラグイン「{}」はまだ許可されていません**\n> 設定（Ctrl+,）の「プラグイン」で、中身を確かめてから許可してください。\n",
                    p.manifest.name
                ),
                Usable::Yes => {
                    let key = block_key(&p.id, &lang, &code);
                    match self.blocks.get(&key) {
                        Some(Output::Done(md)) => md.clone(),
                        Some(Output::Running) => format!("*（プラグイン「{}」で描いています…）*\n", p.manifest.name),
                        Some(Output::Failed(e)) => format!(
                            "> [!WARNING]\n> **プラグイン「{}」**\n> {}\n",
                            p.manifest.name,
                            e.replace('\n', " ")
                        ),
                        None => {
                            self.blocks.insert(key, Output::Running);
                            let mut req = Request::new(plugin::Kind::Codeblock, &lang, root);
                            req.note = Some(plugin::NoteInfo {
                                path: note.to_string(),
                                ..Default::default()
                            });
                            req.code = Some(code.clone());
                            self.spawn(&p, req, Origin::Block(key), ctx);
                            format!("*（プラグイン「{}」で描いています…）*\n", p.manifest.name)
                        }
                    }
                }
            };
            out.push_str(&rendered);
            if !rendered.ends_with('\n') {
                out.push('\n');
            }
        }
        out
    }
}

pub enum TrustOutcome {
    Allow,
    Cancel,
}

/// プラグインの実行を許可するか尋ねる（`plugin.toml` の中身を見せる）
pub fn trust_ui(ctx: &egui::Context, p: &Plugin) -> Option<TrustOutcome> {
    let mut outcome = None;
    let modal = egui::Modal::new(egui::Id::unique(("trust-plugin", &p.id))).show(ctx, |ui| {
        let size = ctx.content_rect().size();
        ui.set_width((size.x * 0.7).min(760.0));
        ui.heading(format!("プラグイン「{}」を許可しますか？", p.manifest.name));
        ui.add_space(4.0);
        ui.label(
            "このプラグインは、下の run のコマンドをあなたの権限で実行します。\
             他人から受け取った vault なら、プラグインのフォルダの中身（スクリプト）を確かめてから許可してください。",
        );
        ui.weak(p.dir.join("plugin.toml").display().to_string());
        ui.add_space(6.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ScrollArea::both()
                .max_height((size.y * 0.5).min(420.0))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    ui.label(RichText::new(&p.text).monospace());
                });
        });
        ui.weak("許可はこの内容に対して記録します。plugin.toml が書き換わったら、もう一度確認します。");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("許可する").clicked() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_known_blocks_only() {
        let dir = tempfile::tempdir().unwrap();
        let pdir = dir.path().join(".robsidian/plugins/tbl");
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(
            pdir.join("plugin.toml"),
            "name = \"表\"\nrun = \"cat\"\n[[codeblock]]\nlang = \"csv\"\n",
        )
        .unwrap();
        let mut p = Plugins::default();
        assert!(p.refresh(dir.path()));
        let mut cfg = Config::default();
        let ctx = egui::Context::default();
        let src = "前\n```csv\na,b\n```\n```rust\n```csv\n```\n後\n";
        // 許可していなければ、案内を出す（実行しない）
        let out = p.render_blocks(src, &cfg, dir.path(), "n.md", &ctx);
        assert!(out.contains("まだ許可されていません"), "{out}");
        assert!(out.contains("```rust\n```csv\n```\n"), "{out}");
        assert!(out.starts_with("前\n") && out.ends_with("後\n"));
        // 許可すれば実行を頼み、結果が来るまでは「描いています」
        cfg.trusted_commands
            .insert(trust_key(dir.path(), "tbl"), p.list[0].text.clone());
        let out = p.render_blocks(src, &cfg, dir.path(), "n.md", &ctx);
        assert!(out.contains("描いています"), "{out}");
        // オフにしたら、ふつうのコードブロックのまま
        cfg.disabled_plugins.insert(trust_key(dir.path(), "tbl"));
        let out = p.render_blocks(src, &cfg, dir.path(), "n.md", &ctx);
        assert!(out.contains("```csv\na,b\n```\n"), "{out}");
    }
}
