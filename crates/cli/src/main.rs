//! `rob` — vaultをシェルから照会・操作するCLI。
//!
//! 出力は1行1件のプレーンテキスト（`--json` でJSON）なので、
//! fzf / xargs / jq / nushell などにそのままパイプできる。

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use robsidian_core::{search, Vault};
use serde_json::json;
use std::io::{Read, Write};
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
    /// このノートの名前がリンクにならずに書かれている箇所（未リンクの言及）
    Mentions {
        note: String,
        /// 見つかった言及をすべて `[[リンク]]` に書き換える
        #[arg(long)]
        link: bool,
    },
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
