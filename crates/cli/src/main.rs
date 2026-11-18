//! `rob` — vaultをシェルから照会・操作するCLI。
//!
//! 出力は1行1件のプレーンテキスト（`--json` でJSON）なので、
//! fzf / xargs / jq / nushell などにそのままパイプできる。

mod export;
mod lint;
mod mcp;
mod serve;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use robsidian_core::{git, related, search, usercmd, Vault};
use serde_json::json;
use std::io::{IsTerminal, Read, Write};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "rob", version, about = "Robsidian vault CLI")]
struct Cli {
    /// vaultのパス（省略時: $ROBSIDIAN_VAULT → 親を辿って .obsidian/.robsidian のあるフォルダ → カレント）
    #[arg(short, long, global = true, env = "ROBSIDIAN_VAULT")]
    vault: Option<PathBuf>,
    /// JSONで出力する
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// ノートを一覧する
    List,
    /// ノート名をあいまい検索する
    Find {
        query: String,
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: usize,
    },
    /// 全文検索（空白区切りAND、"フレーズ"、tag:名前、path:文字列）
    Search {
        query: Vec<String>,
        #[arg(short = 'n', long, default_value_t = 50)]
        limit: usize,
    },
    /// ノートの本文を出力する
    Cat { note: String },
    /// このノートにリンクしているノート
    Backlinks { note: String },
    /// 直接はリンクしていないが関係の深いノート（共通のリンク先・リンク元・タグの重なりで順位付け）
    Related {
        note: String,
        #[arg(short = 'n', long, default_value_t = 10)]
        limit: usize,
    },
    /// このノートの名前がリンクにならずに書かれている箇所（未リンクの言及）
    Mentions {
        note: String,
        /// 見つかった言及をすべて `[[リンク]]` に書き換える
        #[arg(long)]
        link: bool,
    },
    /// タスクの一覧（既定は未完了。条件は search と同じ書き方で、due:overdue なども使える）
    Tasks {
        query: Vec<String>,
        /// 完了したタスクも含める
        #[arg(short, long)]
        all: bool,
        /// 完了したタスクだけ
        #[arg(long)]
        done: bool,
    },
    /// 条件でノートやタスクを探す（rob-query ブロックと同じ書き方。例: "tag:project sort:mtime limit:10"）
    Query { query: Vec<String> },
    /// このノートから出ているリンク
    Links { note: String },
    /// タグ一覧。タグを指定するとそのタグを持つノート
    Tags { tag: Option<String> },
    /// 孤立ノート（リンクの出入りが無いノート）
    Orphans,
    /// リンク先が存在しないリンク
    Unresolved,
    /// ノートを作成する（本文は --content か標準入力 `-`）
    New {
        name: String,
        #[arg(short, long, allow_hyphen_values = true)]
        content: Option<String>,
        /// テンプレート名（テンプレートフォルダ内のノート）。本文の前に展開する
        #[arg(short, long)]
        template: Option<String>,
    },
    /// テンプレートの一覧
    Templates,
    /// 今日のデイリーノートを（無ければ作って）パスを表示する
    Daily {
        /// 追記するテキスト
        #[arg(short, long, allow_hyphen_values = true)]
        append: Option<String>,
    },
    /// ノートやフォルダを移動/改名し、vault内のリンクを追従させる
    Mv { from: String, to: String },
    /// リンクグラフを出力する
    Graph {
        #[arg(short, long, value_enum, default_value_t = GraphFormat::Dot)]
        format: GraphFormat,
    },
    /// vaultの統計
    Stats,
    /// リンク切れ・埋め込み先なし・曖昧なリンクなどを報告する（エラーがあれば終了コード1）
    Lint {
        /// 孤立ノートも警告する
        #[arg(long)]
        orphans: bool,
        /// 警告もエラーとして扱う（終了コード1にする）
        #[arg(long)]
        strict: bool,
    },
    /// vaultを静的なHTMLサイトとして書き出す
    Export {
        /// 書き出し先のフォルダ
        out: PathBuf,
        /// 空でないフォルダにも書き出す
        #[arg(long)]
        force: bool,
    },
    /// タグを改名する（本文の #タグ と子タグ、frontmatterの tags）
    TagRename { old: String, new: String },
    /// 標準入出力で、1行1リクエストのJSONに答え続ける（エディタ連携用）
    Serve,
    /// MCPサーバーとして動く（AIアシスタントから vault を検索・読み書きできるようにする）
    Mcp,
    /// ユーザーコマンド（.robsidian/commands.toml）を実行する。名前を省くと一覧
    Run {
        name: Option<String>,
        /// このノートについて実行する（ROB_NOTE などに入る。input = "note" なら本文を標準入力に渡す）
        #[arg(short, long)]
        note: Option<String>,
    },
    /// リモートの変更を取り込む（git pull --rebase。競合したら取り消して元に戻す）
    Pull,
    /// こちらのコミットをリモートへ送る（git push）
    Push,
    /// vault以下の変更をgitにコミットする（スナップショット）
    Snapshot {
        /// コミットメッセージ（省略時は日時）
        #[arg(short, long)]
        message: Option<String>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum GraphFormat {
    Dot,
    Json,
}

fn find_vault(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(p) = explicit {
        return Ok(p);
    }
    let cwd = std::env::current_dir()?;
    let mut dir = cwd.as_path();
    loop {
        if dir.join(".obsidian").is_dir() || dir.join(".robsidian").is_dir() {
            return Ok(dir.to_path_buf());
        }
        match dir.parent() {
            Some(p) => dir = p,
            None => return Ok(cwd),
        }
    }
}

/// パスでも名前でも指定できるようにする。
fn note_arg(vault: &Vault, arg: &str) -> Result<String> {
    if vault.note(arg).is_some() {
        return Ok(arg.to_string());
    }
    vault
        .resolve(arg, "")
        .map(str::to_string)
        .with_context(|| format!("note not found: {arg}"))
}

struct Out {
    json: bool,
    w: std::io::BufWriter<std::io::StdoutLock<'static>>,
}

impl Out {
    fn lines<S: AsRef<str>>(&mut self, items: impl IntoIterator<Item = S>) -> Result<()> {
        if self.json {
            let v: Vec<String> = items.into_iter().map(|s| s.as_ref().to_string()).collect();
            return self.value(&json!(v));
        }
        for s in items {
            writeln!(self.w, "{}", s.as_ref())?;
        }
        Ok(())
    }

    fn value(&mut self, v: &serde_json::Value) -> Result<()> {
        serde_json::to_writer_pretty(&mut self.w, v)?;
        writeln!(self.w)?;
        Ok(())
    }
}

fn print_query(vault: &Vault, query: &str, out: &mut Out) -> Result<()> {
    for e in search::query_errors(query) {
        eprintln!("rob: 解釈できない条件を無視しました: {e}");
    }
    let today = chrono::Local::now().date_naive();
    match search::run_query(vault, query, today) {
        search::QueryResult::Tasks(tasks) if out.json => {
            out.value(&serde_json::to_value(&tasks)?)?
        }
        search::QueryResult::Tasks(tasks) => {
            for t in tasks {
                let due = t.task.due.map(|d| format!("  📅 {d}")).unwrap_or_default();
                writeln!(
                    out.w,
                    "{}:{}: [{}] {}{due}",
                    t.path,
                    t.task.line + 1,
                    t.task.status,
                    t.task.text
                )?;
            }
        }
        search::QueryResult::Notes(hits) if out.json => out.value(&serde_json::to_value(&hits)?)?,
        search::QueryResult::Notes(hits) => out.lines(hits.iter().map(|h| &h.path))?,
    }
    Ok(())
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        // `rob list | head` のようにパイプ先が先に閉じたのはエラー扱いしない
        if e.downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::BrokenPipe)
        {
            return;
        }
        eprintln!("rob: {e:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let mut vault = Vault::open(find_vault(cli.vault)?)?;
    let mut out = Out {
        json: cli.json,
        w: std::io::BufWriter::new(std::io::stdout().lock()),
    };

    match cli.cmd {
        Cmd::List => out.lines(vault.notes().map(|n| &n.path))?,
        Cmd::Find { query, limit } => out.lines(search::fuzzy(&vault, &query, limit))?,
        Cmd::Search { query, limit } => {
            let hits = search::search(&vault, &query.join(" "), limit);
            if out.json {
                out.value(&serde_json::to_value(&hits)?)?;
            } else {
                // grep互換の `path:line:text`
                for h in &hits {
                    if h.lines.is_empty() {
                        writeln!(out.w, "{}", h.path)?;
                    }
                    for l in &h.lines {
                        writeln!(out.w, "{}:{}:{}", h.path, l.line + 1, l.text)?;
                    }
                }
            }
        }
        Cmd::Cat { note } => {
            let rel = note_arg(&vault, &note)?;
            let text = &vault.note(&rel).context("note vanished")?.text;
            out.w.write_all(text.as_bytes())?;
        }
        Cmd::Related { note, limit } => {
            let rel = note_arg(&vault, &note)?;
            let found = related::related(&vault, &rel, limit);
            if out.json {
                out.value(&serde_json::to_value(&found)?)?;
            } else {
                out.lines(found.iter().map(|r| format!("{:.2}\t{}", r.score, r.path)))?;
            }
        }
        Cmd::Backlinks { note } => {
            let rel = note_arg(&vault, &note)?;
            let links = vault.backlinks(&rel);
            if out.json {
                let v: Vec<_> = links
                    .iter()
                    .map(|b| json!({"source": b.source, "line": b.line + 1, "context": vault.line(&b.source, b.line)}))
                    .collect();
                out.value(&json!(v))?;
            } else {
                for b in links {
                    let context = vault.line(&b.source, b.line);
                    writeln!(out.w, "{}:{}:{context}", b.source, b.line + 1)?;
                }
            }
        }
        Cmd::Mentions { note, link } => {
            let rel = note_arg(&vault, &note)?;
            let mentions = vault.unlinked_mentions(&rel);
            if link {
                // 後ろから書き換えると、同じノート内の前の範囲がずれない
                let mut changed: Vec<String> = Vec::new();
                for m in mentions.iter().rev() {
                    let Some(text) = vault.note(&m.source).map(|n| n.text.clone()) else {
                        continue;
                    };
                    if let Some(new) = vault.link_mention_text(&text, m, &rel) {
                        vault.write_note(&m.source, &new)?;
                        if !changed.contains(&m.source) {
                            changed.push(m.source.clone());
                        }
                    }
                }
                eprintln!(
                    "linked {} mentions in {} notes",
                    mentions.len(),
                    changed.len()
                );
                out.lines(changed)?;
            } else if out.json {
                out.value(&serde_json::to_value(&mentions)?)?;
            } else {
                for m in &mentions {
                    writeln!(out.w, "{}:{}:{}", m.source, m.line + 1, m.context)?;
                }
            }
        }
        Cmd::Tasks { query, all, done } => {
            let status = if done {
                "task:done"
            } else if all {
                "task:any"
            } else {
                "task:todo"
            };
            let q = format!("{status} {}", query.join(" "));
            print_query(&vault, &q, &mut out)?;
        }
        Cmd::Query { query } => print_query(&vault, &query.join(" "), &mut out)?,
        Cmd::Links { note } => {
            let rel = note_arg(&vault, &note)?;
            let links = vault.outgoing(&rel);
            if out.json {
                let v: Vec<_> = links
                    .iter()
                    .map(|(l, dst)| json!({"target": l.target, "resolved": dst, "line": l.line + 1, "embed": l.embed}))
                    .collect();
                out.value(&json!(v))?;
            } else {
                for (l, dst) in links {
                    match dst {
                        Some(d) => writeln!(out.w, "{d}")?,
                        None => writeln!(out.w, "{}\t(unresolved)", l.target)?,
                    }
                }
            }
        }
        Cmd::Tags { tag: None } => {
            let tags = vault.tags();
            if out.json {
                out.value(&serde_json::to_value(&tags)?)?;
            } else {
                for (tag, notes) in tags {
                    writeln!(out.w, "{}\t{tag}", notes.len())?;
                }
            }
        }
        Cmd::Tags { tag: Some(tag) } => {
            let hits = search::search(&vault, &format!("tag:{tag}"), usize::MAX);
            out.lines(hits.iter().map(|h| &h.path))?;
        }
        Cmd::Orphans => out.lines(vault.orphans())?,
        Cmd::Unresolved => {
            if out.json {
                out.value(&serde_json::to_value(vault.unresolved())?)?;
            } else {
                for (target, sources) in vault.unresolved() {
                    writeln!(out.w, "{target}\t{}", sources.join(", "))?;
                }
            }
        }
        Cmd::New {
            name,
            content,
            template,
        } => {
            let content = match content.as_deref() {
                Some("-") => {
                    let mut s = String::new();
                    std::io::stdin().read_to_string(&mut s)?;
                    s
                }
                Some(c) => c.to_string(),
                None => String::new(),
            };
            let content = match template {
                Some(t) => {
                    let tpl = vault
                        .find_template(&t)
                        .with_context(|| format!("template not found: {t}"))?
                        .to_string();
                    let title = name
                        .rsplit('/')
                        .next()
                        .unwrap_or(&name)
                        .trim_end_matches(".md");
                    let body = vault.render_template(&tpl, title).unwrap_or_default();
                    body + &content
                }
                None => content,
            };
            let rel = vault.create_note(&name, &content)?;
            out.lines([vault.abs(&rel).display().to_string()])?;
        }
        Cmd::Templates => out.lines(vault.templates())?,
        Cmd::Daily { append } => {
            let rel = vault.daily()?;
            if let Some(text) = append {
                let mut body = vault.note(&rel).map(|n| n.text.clone()).unwrap_or_default();
                if !body.is_empty() && !body.ends_with('\n') {
                    body.push('\n');
                }
                body.push_str(&text);
                body.push('\n');
                vault.write_note(&rel, &body)?;
            }
            out.lines([vault.abs(&rel).display().to_string()])?;
        }
        Cmd::Mv { from, to } => {
            let from = if vault.abs(&from).exists() {
                from
            } else {
                note_arg(&vault, &from)?
            };
            let updated = vault.rename(&from, &to)?;
            if out.json {
                out.value(&json!({ "updated": updated }))?;
            } else {
                eprintln!("moved {from} -> {to} ({} notes updated)", updated.len());
                out.lines(updated)?;
            }
        }
        Cmd::Graph { format } => {
            let edges = vault.edges();
            match format {
                GraphFormat::Json => {
                    let nodes: Vec<_> = vault.notes().map(|n| &n.path).collect();
                    out.value(&json!({ "nodes": nodes, "edges": edges }))?;
                }
                GraphFormat::Dot => {
                    let q =
                        |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
                    writeln!(out.w, "digraph vault {{")?;
                    for n in vault.notes() {
                        writeln!(out.w, "  {} [label={}];", q(&n.path), q(&n.name))?;
                    }
                    for (a, b) in edges {
                        writeln!(out.w, "  {} -> {};", q(a), q(b))?;
                    }
                    writeln!(out.w, "}}")?;
                }
            }
        }
        Cmd::Lint { orphans, strict } => {
            let issues = lint::run(&vault, orphans);
            let errors = issues
                .iter()
                .filter(|i| i.level == lint::Level::Error)
                .count();
            let warnings = issues.len() - errors;
            if out.json {
                out.value(&serde_json::to_value(&issues)?)?;
            } else {
                for i in &issues {
                    let level = match i.level {
                        lint::Level::Error => "error",
                        lint::Level::Warning => "warning",
                    };
                    writeln!(
                        out.w,
                        "{}:{}: {level}[{}]: {}",
                        i.path, i.line, i.code, i.message
                    )?;
                }
            }
            out.w.flush()?;
            eprintln!("{errors}件のエラー、{warnings}件の警告");
            if errors > 0 || (strict && warnings > 0) {
                std::process::exit(1);
            }
        }
        Cmd::Export { out: dest, force } => {
            let n = export::run(&vault, &dest, force)?;
            eprintln!("{n}ページを {} に書き出しました", dest.display());
        }
        Cmd::TagRename { old, new } => {
            let changed = vault.rename_tag(&old, &new)?;
            eprintln!("{}件のノートを書き換えました", changed.len());
            out.lines(changed)?;
        }
        Cmd::Serve => {
            out.w.flush()?;
            return serve::run(vault);
        }
        Cmd::Mcp => {
            out.w.flush()?;
            return mcp::run(vault);
        }
        Cmd::Run { name, note } => {
            let list = usercmd::load(vault.root())?
                .map(|(_, l)| l)
                .unwrap_or_default();
            let Some(name) = name else {
                if out.json {
                    let v: Vec<_> = list
                        .iter()
                        .map(|c| json!({"name": c.name, "run": c.run, "key": c.key}))
                        .collect();
                    out.value(&json!(v))?;
                } else {
                    out.lines(list.iter().map(|c| format!("{}\t{}", c.name, c.run)))?;
                }
                return Ok(());
            };
            let Some(cmd) = list.iter().find(|c| c.name == name) else {
                bail!("ユーザーコマンドが見つかりません: {name}");
            };
            let note = note.map(|n| note_arg(&vault, &n)).transpose()?;
            let input = match (cmd.input, &note) {
                (usercmd::Input::Note, Some(n)) => {
                    vault.note(n).map_or_else(String::new, |n| n.text.clone())
                }
                (usercmd::Input::None, _) => String::new(),
                _ if std::io::stdin().is_terminal() => String::new(),
                _ => std::io::read_to_string(std::io::stdin())?,
            };
            let env = usercmd::Env { note, line: None };
            let done = usercmd::execute(cmd, vault.root(), &env, &input)?;
            out.w.write_all(done.stdout.as_bytes())?;
            eprint!("{}", done.stderr);
            if !done.success {
                out.w.flush()?;
                std::process::exit(1);
            }
        }
        Cmd::Pull | Cmd::Push => {
            if !git::is_repo(vault.root()) {
                bail!("vaultはgitのリポジトリの中にありません");
            }
            let msg = if matches!(cli.cmd, Cmd::Pull) {
                git::pull(vault.root())?
            } else {
                git::push(vault.root())?
            };
            eprintln!("{msg}");
        }
        Cmd::Snapshot { message } => {
            if !git::is_repo(vault.root()) {
                bail!("vaultはgitのリポジトリの中にありません");
            }
            let message = message.unwrap_or_else(snapshot_message);
            match git::snapshot(vault.root(), &message)? {
                Some(hash) => eprintln!("コミットしました: {hash} {message}"),
                None => eprintln!("コミットする変更はありません"),
            }
        }
        Cmd::Stats => {
            let links: usize = vault.notes().map(|n| n.parsed.links.len()).sum();
            let words: usize = vault
                .notes()
                .map(|n| n.text.split_whitespace().count())
                .sum();
            let stats = json!({
                "vault": vault.root().display().to_string(),
                "notes": vault.note_count(),
                "attachments": vault.files().count(),
                "links": links,
                "unresolved": vault.unresolved().len(),
                "orphans": vault.orphans().len(),
                "tags": vault.tags().len(),
                "words": words,
            });
            if out.json {
                out.value(&stats)?;
            } else if let Some(map) = stats.as_object() {
                for (k, v) in map {
                    let v = v.as_str().map_or_else(|| v.to_string(), str::to_string);
                    writeln!(out.w, "{k:<12} {v}")?;
                }
            }
        }
    }
    out.w.flush()?;
    Ok(())
}

/// 既定のスナップショットのメッセージ
fn snapshot_message() -> String {
    format!(
        "スナップショット {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M")
    )
}
