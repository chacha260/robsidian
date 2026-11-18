//! プラグイン（フェーズE1）: `.robsidian/plugins/<id>/plugin.toml` に定義した外部プロセス。
//!
//! 呼ぶたびに `run` を起動し、標準入力に JSON のリクエストを1つ渡し、標準出力から JSON のレスポンスを1つ受け取る。
//! プラグインは中身（Markdown・してほしいこと）を返すだけで、描くのと書き換えるのは Robsidian が行う
//! （書き換えは必ず `Vault` を通るので、アトミックな書き込みとローカル履歴が効く）。
//! 詳しい形は `docs/ROADMAP.md` のフェーズE。

use crate::usercmd::Input;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

/// やりとりの形の版
pub const API: u32 = 1;
/// これより長く掛かるプラグインは打ち切る
pub const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub input: Input,
    /// 例: `ctrl+alt+w`
    #[serde(default)]
    pub key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodeBlockDef {
    /// ` ```lang ` の言語名
    pub lang: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EventKind {
    /// ノートを保存したとき
    Save,
    /// ノートを開いたとき
    Open,
}

impl EventKind {
    pub fn name(self) -> &'static str {
        match self {
            EventKind::Save => "save",
            EventKind::Open => "open",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventDef {
    pub on: EventKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelDef {
    pub id: String,
    pub title: String,
}

/// 起動のしかた
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// 呼ぶたびに起動する（段階1）
    #[default]
    Once,
    /// 起動したままにして、JSON-RPC で話す（段階2）
    Server,
}

/// `plugin.toml` の中身
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub name: String,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub run: String,
    #[serde(default)]
    pub command: Vec<CommandDef>,
    #[serde(default)]
    pub codeblock: Vec<CodeBlockDef>,
    #[serde(default)]
    pub event: Vec<EventDef>,
    #[serde(default)]
    pub panel: Vec<PanelDef>,
}

/// 見つけたプラグイン
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plugin {
    /// フォルダ名
    pub id: String,
    pub dir: PathBuf,
    pub manifest: Manifest,
    /// `plugin.toml` の本文（許可したときの内容と比べる）
    pub text: String,
}

impl Plugin {
    pub fn listens(&self, kind: EventKind) -> bool {
        self.manifest.event.iter().any(|e| e.on == kind)
    }
}

/// プラグインを置くフォルダ
pub fn dir(root: &Path) -> PathBuf {
    root.join(".robsidian").join("plugins")
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn parse_manifest(text: &str) -> Result<Manifest> {
    let m: Manifest = toml::from_str(text)?;
    if m.name.trim().is_empty() {
        bail!("name が空です");
    }
    if m.run.trim().is_empty() {
        bail!("run が空です");
    }
    for c in &m.command {
        if !valid_id(&c.id) {
            bail!("コマンドの id「{}」には英数字・-・_ だけが使えます", c.id);
        }
    }
    for p in &m.panel {
        if !valid_id(&p.id) {
            bail!("パネルの id「{}」には英数字・-・_ だけが使えます", p.id);
        }
    }
    for b in &m.codeblock {
        if b.lang.trim().is_empty() || b.lang.contains(char::is_whitespace) {
            bail!("コードブロックの lang「{}」が正しくありません", b.lang);
        }
    }
    Ok(m)
}

/// プラグインのフォルダを見て、読めたものと、読めなかったもの（id, 理由）を返す（id の順）
pub fn discover(root: &Path) -> (Vec<Plugin>, Vec<(String, String)>) {
    let mut ok = Vec::new();
    let mut bad = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir(root)) else {
        return (ok, bad);
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for d in dirs {
        let id = d
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if id.starts_with('.') {
            continue;
        }
        if !valid_id(&id) {
            bad.push((id, "フォルダ名には英数字・-・_ だけが使えます".into()));
            continue;
        }
        let path = d.join("plugin.toml");
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                bad.push((id, format!("plugin.toml を読めません: {e}")));
                continue;
            }
        };
        match parse_manifest(&text) {
            Ok(manifest) => ok.push(Plugin {
                id,
                dir: d,
                manifest,
                text,
            }),
            Err(e) => bad.push((id, format!("plugin.toml の書き方が正しくありません: {e:#}"))),
        }
    }
    (ok, bad)
}

// ---------------------------------------------------------------- やりとり

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Command,
    Codeblock,
    Event,
    Panel,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "command" => Kind::Command,
            "codeblock" => Kind::Codeblock,
            "event" => Kind::Event,
            "panel" => Kind::Panel,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct NoteInfo {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Request {
    pub api: u32,
    pub kind: Kind,
    /// command・panel の id、codeblock の lang、event の on
    pub id: String,
    pub vault: String,
    pub note: Option<NoteInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl Request {
    pub fn new(kind: Kind, id: &str, vault: &Path) -> Self {
        Self {
            api: API,
            kind,
            id: id.to_string(),
            vault: vault.display().to_string(),
            note: None,
            code: None,
        }
    }
}

/// プラグインにしてほしいと頼まれたこと
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    Notice {
        text: String,
    },
    /// 選択範囲を置き換える（選択が無ければカーソル位置に挿入する）
    ReplaceSelection {
        text: String,
    },
    Insert {
        text: String,
    },
    /// 開いているノートの本文を置き換える
    ReplaceNote {
        text: String,
    },
    Open {
        path: String,
    },
    /// ほかのノートを書く・作る
    WriteNote {
        path: String,
        text: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    #[serde(default)]
    pub markdown: Option<String>,
    #[serde(default)]
    pub actions: Vec<Action>,
}

/// 標準出力をレスポンスにする。JSON でなければ、出力そのものを notice（コマンド・イベント）か
/// markdown（コードブロック・パネル）として扱う
pub fn parse_response(kind: Kind, stdout: &str) -> Result<Response> {
    let t = stdout.trim();
    if t.starts_with('{') {
        return serde_json::from_str(t).context("プラグインの出力（JSON）の形が正しくありません");
    }
    Ok(match kind {
        Kind::Codeblock | Kind::Panel => Response {
            markdown: Some(stdout.to_string()),
            actions: Vec::new(),
        },
        Kind::Command | Kind::Event if t.is_empty() => Response::default(),
        Kind::Command | Kind::Event => Response {
            markdown: None,
            actions: vec![Action::Notice {
                text: t.to_string(),
            }],
        },
    })
}

/// プラグインを呼んで、終わるまで待つ（呼び出し側で別スレッドに載せる）。`TIMEOUT` で打ち切る
pub fn execute(plugin: &Plugin, req: &Request) -> Result<Response> {
    let mut c = crate::usercmd::shell(&plugin.manifest.run);
    c.current_dir(&plugin.dir)
        .env("ROB_VAULT", &req.vault)
        .env("ROB_PLUGIN_DIR", &plugin.dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(note) = &req.note {
        c.env("ROB_NOTE", &note.path);
    }
    let mut child = c
        .spawn()
        .with_context(|| format!("プラグイン「{}」を起動できません", plugin.manifest.name))?;
    let input = serde_json::to_string(req)?;
    let mut stdin = child.stdin.take();
    let writer = std::thread::spawn(move || {
        if let Some(s) = stdin.as_mut() {
            let _ = s.write_all(input.as_bytes());
        }
        // 書き終えたら閉じる（プラグインが標準入力の終わりを待てるように）
        drop(stdin);
    });
    let read = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            String::from_utf8_lossy(&buf).into_owned()
        })
    };
    let out = read(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let err = read(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let started = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break s;
        }
        if started.elapsed() > TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            bail!(
                "プラグイン「{}」が{}秒で終わらなかったので止めました",
                plugin.manifest.name,
                TIMEOUT.as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let _ = writer.join();
    let stdout = out.join().map_err(|_| anyhow!("出力を読めません"))?;
    let stderr = err.join().map_err(|_| anyhow!("出力を読めません"))?;
    if !status.success() {
        let line = stderr
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("終了コードが0ではありません")
            .trim()
            .to_string();
        bail!(
            "プラグイン「{}」が失敗しました: {line}",
            plugin.manifest.name
        );
    }
    parse_response(req.kind, &stdout)
        .with_context(|| format!("プラグイン「{}」", plugin.manifest.name))
}

// ---------------------------------------------------------------- 常駐するプラグイン（段階2）

/// プラグインから Robsidian への問い合わせ・通知
#[derive(Debug, Clone, PartialEq)]
pub struct HostCall {
    /// 問い合わせなら返事に使う id、通知なら None
    pub id: Option<serde_json::Value>,
    pub method: String,
    pub params: serde_json::Value,
}

type Pending = std::collections::HashMap<u64, std::sync::mpsc::Sender<Result<Response>>>;

/// 起動したままのプラグイン。行区切りの JSON-RPC 2.0 で話す。
/// 落とすと止める
pub struct Server {
    child: std::process::Child,
    stdin: std::sync::Arc<std::sync::Mutex<std::process::ChildStdin>>,
    pending: std::sync::Arc<std::sync::Mutex<Pending>>,
    next_id: std::sync::atomic::AtomicU64,
    calls: std::sync::mpsc::Receiver<HostCall>,
    dead: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Server {
    /// 起動して `initialize` を送る
    pub fn start(plugin: &Plugin, vault: &Path) -> Result<Self> {
        use std::io::BufRead;
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        use std::sync::{mpsc, Arc, Mutex};
        let mut c = crate::usercmd::shell(&plugin.manifest.run);
        c.current_dir(&plugin.dir)
            .env("ROB_VAULT", vault)
            .env("ROB_PLUGIN_DIR", &plugin.dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = c
            .spawn()
            .with_context(|| format!("プラグイン「{}」を起動できません", plugin.manifest.name))?;
        let stdin = Arc::new(Mutex::new(
            child
                .stdin
                .take()
                .ok_or_else(|| anyhow!("標準入力がありません"))?,
        ));
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("標準出力がありません"))?;
        let stderr = child.stderr.take();
        let pending: Arc<Mutex<Pending>> = Arc::default();
        let dead = Arc::new(AtomicBool::new(false));
        let tail = Arc::new(Mutex::new(String::new()));
        // 標準エラーは最後の数行だけ覚えておく（落ちたときに見せる）
        if let Some(err) = stderr {
            let tail = tail.clone();
            std::thread::spawn(move || {
                for line in std::io::BufReader::new(err).lines().map_while(Result::ok) {
                    if let Ok(mut t) = tail.lock() {
                        if !line.trim().is_empty() {
                            *t = line;
                        }
                    }
                }
            });
        }
        let (tx, calls) = mpsc::channel();
        {
            let (pending, dead, name) =
                (pending.clone(), dead.clone(), plugin.manifest.name.clone());
            std::thread::spawn(move || {
                for line in std::io::BufReader::new(stdout)
                    .lines()
                    .map_while(Result::ok)
                {
                    let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else {
                        continue;
                    };
                    if let Some(method) = msg.get("method").and_then(|m| m.as_str()) {
                        let _ = tx.send(HostCall {
                            id: msg.get("id").cloned(),
                            method: method.to_string(),
                            params: msg
                                .get("params")
                                .cloned()
                                .unwrap_or(serde_json::Value::Null),
                        });
                        continue;
                    }
                    let Some(id) = msg.get("id").and_then(|i| i.as_u64()) else {
                        continue;
                    };
                    let Some(reply) = pending.lock().ok().and_then(|mut p| p.remove(&id)) else {
                        continue;
                    };
                    let result = match (msg.get("result"), msg.get("error")) {
                        (_, Some(e)) => Err(anyhow!(
                            "プラグイン「{name}」: {}",
                            e.get("message")
                                .and_then(|m| m.as_str())
                                .unwrap_or("失敗しました")
                        )),
                        (Some(r), None) => serde_json::from_value::<Response>(r.clone())
                            .with_context(|| {
                                format!("プラグイン「{name}」の返事の形が正しくありません")
                            }),
                        (None, None) => Ok(Response::default()),
                    };
                    let _ = reply.send(result);
                }
                // 終わった（落ちた）: 待っている依頼はすべて失敗にする
                dead.store(true, Ordering::Relaxed);
                let why = tail.lock().map(|t| t.clone()).unwrap_or_default();
                if let Ok(mut p) = pending.lock() {
                    for (_, reply) in p.drain() {
                        let _ =
                            reply.send(Err(anyhow!("プラグイン「{name}」が終了しました。{why}")));
                    }
                }
            });
        }
        let server = Self {
            child,
            stdin,
            pending,
            next_id: AtomicU64::new(1),
            calls,
            dead,
        };
        server.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "api": API,
                "vault": vault.display().to_string(),
                "plugin_dir": plugin.dir.display().to_string(),
            }
        }))?;
        Ok(server)
    }

    fn send(&self, msg: &serde_json::Value) -> Result<()> {
        let mut line = serde_json::to_string(msg)?;
        line.push('\n');
        let mut stdin = self
            .stdin
            .lock()
            .map_err(|_| anyhow!("プラグインに書けません"))?;
        stdin.write_all(line.as_bytes())?;
        stdin.flush()?;
        Ok(())
    }

    /// 依頼を送る。返事は戻り値の受け口に届く
    pub fn request(&self, req: &Request) -> std::sync::mpsc::Receiver<Result<Response>> {
        use std::sync::atomic::Ordering;
        let (tx, rx) = std::sync::mpsc::channel();
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut p) = self.pending.lock() {
            p.insert(id, tx.clone());
        }
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "plugin/request",
            "params": req,
        });
        if let Err(e) = self.send(&msg) {
            if let Ok(mut p) = self.pending.lock() {
                p.remove(&id);
            }
            let _ = tx.send(Err(e));
        }
        rx
    }

    /// プラグインからの問い合わせ・通知（届いた分）
    pub fn calls(&self) -> Vec<HostCall> {
        self.calls.try_iter().collect()
    }

    /// 問い合わせに返事をする
    pub fn reply(&self, id: serde_json::Value, result: Result<serde_json::Value>) {
        let msg = match result {
            Ok(r) => serde_json::json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err(e) => serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32000, "message": format!("{e:#}")},
            }),
        };
        let _ = self.send(&msg);
    }

    pub fn alive(&self) -> bool {
        !self.dead.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.send(&serde_json::json!({"jsonrpc": "2.0", "method": "shutdown"}));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// 常駐するプラグインに依頼して、返事が来るまで待つ（CLI とテスト用）。
/// 待つあいだに来た問い合わせには `host_call` で答え、`ui/notice` の通知は返事の actions に足す
pub fn request_blocking(
    server: &Server,
    req: &Request,
    vault: &mut crate::Vault,
) -> Result<Response> {
    let rx = server.request(req);
    let started = Instant::now();
    let mut notices = Vec::new();
    let handle = |vault: &mut crate::Vault, notices: &mut Vec<Action>| {
        for call in server.calls() {
            match call.id {
                Some(id) => {
                    let r = host_call(vault, &call.method, &call.params);
                    server.reply(id, r);
                }
                None if call.method == "ui/notice" => {
                    if let Some(t) = call.params.get("text").and_then(|t| t.as_str()) {
                        notices.push(Action::Notice {
                            text: t.to_string(),
                        });
                    }
                }
                None => {}
            }
        }
    };
    loop {
        handle(vault, &mut notices);
        match rx.recv_timeout(Duration::from_millis(10)) {
            Ok(r) => {
                // 返事より前に送られた通知を拾う（読み取りのスレッドは届いた順に渡すので、
                // 返事を受け取った時点で、それより前の通知はもう届いている）
                handle(vault, &mut notices);
                let mut r = r?;
                notices.append(&mut r.actions);
                r.actions = notices;
                return Ok(r);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) if started.elapsed() > TIMEOUT => {
                bail!("プラグインが{}秒で返事をしませんでした", TIMEOUT.as_secs())
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                bail!("プラグインが終了しました")
            }
        }
    }
}

/// プラグインからの問い合わせに答える（GUI と CLI で共有する）。
///
/// - `vault/read` `{path}` → `{path, text}`（名前だけでも、リンクと同じ規則で探す）
/// - `vault/list` `{folder?}` → `[path]`（ノートだけ）
/// - `vault/search` `{query, limit?}` → 検索条件（`rob query` と同じ）でノートかタスク
/// - `vault/backlinks` `{path}` → `[{source, line}]`
/// - `vault/write` `{path, text}` → `{}`（`Vault::write_note` を通す）
pub fn host_call(
    vault: &mut crate::Vault,
    method: &str,
    params: &serde_json::Value,
) -> Result<serde_json::Value> {
    use serde_json::json;
    let text_param = |key: &str| -> Result<String> {
        params
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| anyhow!("{method}: {key} がありません"))
    };
    let note_path = |vault: &crate::Vault, p: &str| -> Option<String> {
        vault
            .note(p)
            .map(|n| n.path.clone())
            .or_else(|| vault.resolve(p, "").map(str::to_string))
    };
    Ok(match method {
        "vault/read" => {
            let p = text_param("path")?;
            let path =
                note_path(vault, &p).ok_or_else(|| anyhow!("ノートが見つかりません: {p}"))?;
            let text = vault
                .note(&path)
                .map(|n| n.text.clone())
                .unwrap_or_default();
            json!({"path": path, "text": text})
        }
        "vault/list" => {
            let folder = params.get("folder").and_then(|v| v.as_str()).unwrap_or("");
            let prefix = if folder.is_empty() {
                String::new()
            } else {
                format!("{}/", folder.trim_matches('/'))
            };
            let mut paths: Vec<&str> = vault
                .notes()
                .map(|n| n.path.as_str())
                .filter(|p| p.starts_with(&prefix))
                .collect();
            paths.sort_unstable();
            json!(paths)
        }
        "vault/search" => {
            let q = text_param("query")?;
            let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
            let today = chrono::Local::now().date_naive();
            match crate::search::run_query(vault, &q, today) {
                crate::search::QueryResult::Notes(hits) => json!({
                    "notes": hits.iter().take(limit).map(|h| json!({"path": h.path})).collect::<Vec<_>>()
                }),
                crate::search::QueryResult::Tasks(hits) => json!({
                    "tasks": hits.iter().take(limit).map(|h| json!({
                        "path": h.path,
                        "line": h.task.line + 1,
                        "status": h.task.status.to_string(),
                        "text": h.task.text,
                        "due": h.task.due.map(|d| d.to_string()),
                    })).collect::<Vec<_>>()
                }),
            }
        }
        "vault/backlinks" => {
            let p = text_param("path")?;
            let path =
                note_path(vault, &p).ok_or_else(|| anyhow!("ノートが見つかりません: {p}"))?;
            json!(vault
                .backlinks(&path)
                .iter()
                .map(|b| json!({"source": &*b.source, "line": b.line + 1}))
                .collect::<Vec<_>>())
        }
        "vault/write" => {
            let p = text_param("path")?;
            let text = text_param("text")?;
            let path = if p.ends_with(".md") {
                p
            } else {
                format!("{p}.md")
            };
            vault.write_note(&path, &text)?;
            json!({"path": path})
        }
        other => bail!("未対応の問い合わせです: {other}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn reads_manifests() {
        let m = parse_manifest(
            r#"
name = "例"
run = "python3 main.py"
[[command]]
id = "count"
name = "数える"
input = "selection"
key = "ctrl+alt+w"
[[codeblock]]
lang = "gantt"
[[event]]
on = "save"
[[panel]]
id = "summary"
title = "要約"
"#,
        )
        .unwrap();
        assert_eq!(m.command[0].input, Input::Selection);
        assert_eq!(m.codeblock[0].lang, "gantt");
        assert_eq!(m.event[0].on, EventKind::Save);
        assert!(parse_manifest("name = \"x\"\nrun = \"\"").is_err());
        assert!(parse_manifest("name = \"x\"\nrun = \"y\"\ntypo = 1").is_err());
        assert!(parse_manifest(
            "name = \"x\"\nrun = \"y\"\n[[command]]\nid = \"a b\"\nname = \"z\""
        )
        .is_err());

        let dir = tempfile::tempdir().unwrap();
        let w = |p: &str, t: &str| {
            let abs = dir.path().join(".robsidian/plugins").join(p);
            fs::create_dir_all(abs.parent().unwrap()).unwrap();
            fs::write(abs, t).unwrap();
        };
        w("good/plugin.toml", "name = \"良い\"\nrun = \"cat\"\n");
        w("broken/plugin.toml", "name = \n");
        fs::create_dir_all(dir.path().join(".robsidian/plugins/empty")).unwrap();
        let (ok, bad) = discover(dir.path());
        assert_eq!(ok.len(), 1);
        assert_eq!(ok[0].id, "good");
        let bad_ids: Vec<&str> = bad.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(bad_ids, ["broken", "empty"]);
    }

    #[test]
    fn parses_responses() {
        let r = parse_response(
            Kind::Command,
            r#"{"actions":[{"type":"notice","text":"hi"},{"type":"write_note","path":"a.md","text":"x"}]}"#,
        )
        .unwrap();
        assert_eq!(r.actions[0], Action::Notice { text: "hi".into() });
        assert_eq!(
            r.actions[1],
            Action::WriteNote {
                path: "a.md".into(),
                text: "x".into()
            }
        );
        // JSON でなければ、平文として扱う
        assert_eq!(
            parse_response(Kind::Command, "3語\n").unwrap().actions,
            [Action::Notice {
                text: "3語".into()
            }]
        );
        assert_eq!(
            parse_response(Kind::Codeblock, "| a |\n")
                .unwrap()
                .markdown
                .as_deref(),
            Some("| a |\n")
        );
        assert!(parse_response(Kind::Command, "")
            .unwrap()
            .actions
            .is_empty());
        assert!(parse_response(Kind::Command, r#"{"actions":[{"type":"explode"}]}"#).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn runs_plugins() {
        let dir = tempfile::tempdir().unwrap();
        let pdir = dir.path().join("p");
        fs::create_dir_all(&pdir).unwrap();
        // 標準入力の JSON から id を取り出して返す（jq を使わずシェルだけで）
        fs::write(
            pdir.join("main.sh"),
            "input=$(cat)\ncase \"$input\" in *'\"kind\":\"codeblock\"'*) echo \"| $ROB_NOTE |\";; *) echo '{\"actions\":[{\"type\":\"notice\",\"text\":\"ok\"}]}';; esac\n",
        )
        .unwrap();
        let plugin = Plugin {
            id: "p".into(),
            dir: pdir,
            manifest: parse_manifest("name = \"p\"\nrun = \"sh main.sh\"").unwrap(),
            text: String::new(),
        };
        let mut req = Request::new(Kind::Command, "x", dir.path());
        let r = execute(&plugin, &req).unwrap();
        assert_eq!(r.actions, [Action::Notice { text: "ok".into() }]);
        req.kind = Kind::Codeblock;
        req.note = Some(NoteInfo {
            path: "a.md".into(),
            ..NoteInfo::default()
        });
        let r = execute(&plugin, &req).unwrap();
        assert_eq!(r.markdown.as_deref(), Some("| a.md |\n"));

        let failing = Plugin {
            manifest: parse_manifest("name = \"f\"\nrun = \"echo oops >&2; exit 3\"").unwrap(),
            ..plugin
        };
        let e = execute(&failing, &req).unwrap_err();
        assert!(format!("{e:#}").contains("oops"), "{e:#}");
    }

    #[cfg(unix)]
    #[test]
    fn server_plugins_can_ask_the_vault() {
        // python3 が無い環境では確かめない
        if std::process::Command::new("python3")
            .arg("-c")
            .arg("1")
            .status()
            .is_err()
        {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.md"), "# A\n[[b]]").unwrap();
        fs::write(dir.path().join("b.md"), "# B").unwrap();
        let pdir = dir.path().join(".robsidian/plugins/srv");
        fs::create_dir_all(&pdir).unwrap();
        // 依頼が来たら、vault/backlinks で問い合わせてから返事をする。呼ばれた回数を覚えている（常駐の確かめ）
        fs::write(
            pdir.join("main.py"),
            r#"
import json, sys
count = 0
def send(m):
    sys.stdout.write(json.dumps(m) + "\n"); sys.stdout.flush()
def recv():
    return json.loads(sys.stdin.readline())
while True:
    m = recv()
    if m.get("method") == "plugin/request":
        count += 1
        path = m["params"]["note"]["path"]
        send({"jsonrpc": "2.0", "id": "q", "method": "vault/backlinks", "params": {"path": path}})
        links = recv()["result"]
        send({"jsonrpc": "2.0", "method": "ui/notice", "params": {"text": "呼ばれました"}})
        send({"jsonrpc": "2.0", "id": m["id"], "result": {"markdown": f"{count}:{len(links)}"}})
    elif m.get("method") == "shutdown":
        break
"#,
        )
        .unwrap();
        fs::write(
            pdir.join("plugin.toml"),
            "name = \"常駐\"\nmode = \"server\"\nrun = \"python3 main.py\"\n[[panel]]\nid = \"p\"\ntitle = \"p\"\n",
        )
        .unwrap();
        let mut vault = crate::Vault::open(dir.path()).unwrap();
        let (plugins, bad) = discover(dir.path());
        assert!(bad.is_empty(), "{bad:?}");
        assert_eq!(plugins[0].manifest.mode, Mode::Server);
        let server = Server::start(&plugins[0], dir.path()).unwrap();
        let mut req = Request::new(Kind::Panel, "p", dir.path());
        req.note = Some(NoteInfo {
            path: "b.md".into(),
            ..NoteInfo::default()
        });
        let r = request_blocking(&server, &req, &mut vault).unwrap();
        assert_eq!(r.markdown.as_deref(), Some("1:1"));
        assert_eq!(
            r.actions,
            [Action::Notice {
                text: "呼ばれました".into()
            }]
        );
        // 2回目は同じプロセスが答える
        let r = request_blocking(&server, &req, &mut vault).unwrap();
        assert_eq!(r.markdown.as_deref(), Some("2:1"));
        assert!(server.alive());

        // 問い合わせ
        let read = host_call(&mut vault, "vault/read", &serde_json::json!({"path": "a"})).unwrap();
        assert_eq!(read["text"], "# A\n[[b]]");
        let list = host_call(&mut vault, "vault/list", &serde_json::json!({})).unwrap();
        assert_eq!(list, serde_json::json!(["a.md", "b.md"]));
        host_call(
            &mut vault,
            "vault/write",
            &serde_json::json!({"path": "c", "text": "x"}),
        )
        .unwrap();
        assert_eq!(fs::read_to_string(dir.path().join("c.md")).unwrap(), "x");
        assert!(host_call(&mut vault, "vault/explode", &serde_json::json!({})).is_err());
    }
}
