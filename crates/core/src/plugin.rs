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

/// `plugin.toml` の中身
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub name: String,
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
}
