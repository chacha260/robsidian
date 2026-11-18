//! `rob mcp`: vault を MCP（Model Context Protocol）サーバーとして公開する。
//!
//! 標準入出力で1行1メッセージの JSON-RPC 2.0 を受け答えする（MCP の stdio トランスポート）。
//! AIアシスタント（Claude Desktop / Claude Code など）から、vault の検索・読み書きができる。
//! 処理は `rob serve` と共通（[`crate::serve::handle`]）。
//!
//! 書き込みは `.md` のノートだけで、必ず `Vault` を通す（アトミック書き込みとローカル履歴が効く）。

use crate::serve;
use anyhow::{bail, Context, Result};
use robsidian_core::Vault;
use serde_json::{json, Value};
use std::io::{BufRead, Write};

/// 対応している MCP の版（新しい順）。クライアントが求めた版が無ければ先頭を返す
const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "Obsidian互換のMarkdownノートの集まり（vault）を扱います。\
ノートは vault 内のパス（例: `projects/plan.md`）か、ノート名（例: `plan`）で指定できます。\
リンクは [[ノート名]]、タグは #タグ の書き方です。\
書き込むツールは、上書きの直前の内容をローカル履歴に残します。";

struct Tool {
    name: &'static str,
    description: &'static str,
    /// 引数: (名前, 型, 説明, 必須か)
    args: &'static [(&'static str, &'static str, &'static str, bool)],
    read_only: bool,
}

const TOOLS: &[Tool] = &[
    Tool {
        name: "search",
        description: "ノートを全文検索する。空白区切りはAND、\"…\" でフレーズ、tag:名前 でタグ、path:文字列 でパスを絞り込む。一致した行を返す",
        args: &[
            ("query", "string", "検索条件", true),
            ("limit", "integer", "最大件数（既定50）", false),
        ],
        read_only: true,
    },
    Tool {
        name: "find_notes",
        description: "ノート名をあいまい検索して、パスを返す",
        args: &[
            ("query", "string", "ノート名の一部", true),
            ("limit", "integer", "最大件数（既定50）", false),
        ],
        read_only: true,
    },
    Tool {
        name: "list_notes",
        description: "vault のすべてのノートのパスを返す",
        args: &[],
        read_only: true,
    },
    Tool {
        name: "read_note",
        description: "ノートの本文（Markdown）を返す",
        args: &[("note", "string", "ノートのパスか名前", true)],
        read_only: true,
    },
    Tool {
        name: "backlinks",
        description: "このノートにリンクしているノートと、その行を返す",
        args: &[("note", "string", "ノートのパスか名前", true)],
        read_only: true,
    },
    Tool {
        name: "outgoing_links",
        description: "このノートから出ているリンクと、その解決先を返す",
        args: &[("note", "string", "ノートのパスか名前", true)],
        read_only: true,
    },
    Tool {
        name: "related_notes",
        description: "直接はリンクしていないが、共通のリンク先・リンク元・タグを持つ関係の深いノートを返す",
        args: &[
            ("note", "string", "ノートのパスか名前", true),
            ("limit", "integer", "最大件数（既定10）", false),
        ],
        read_only: true,
    },
    Tool {
        name: "query",
        description: "条件でノートやタスクを探す（例: \"tag:project sort:mtime limit:10\"、\"task:todo due:overdue\"）",
        args: &[("query", "string", "条件", true)],
        read_only: true,
    },
    Tool {
        name: "tasks",
        description: "未完了のタスクを返す。条件で絞り込める（例: \"due:today\"、\"tag:work\"）",
        args: &[("query", "string", "絞り込みの条件（省略可）", false)],
        read_only: true,
    },
    Tool {
        name: "write_note",
        description: "ノートを作るか、本文全体を置き換える。上書き前の内容はローカル履歴に残る",
        args: &[
            ("note", "string", "vault 内のパス（.md）", true),
            ("text", "string", "新しい本文", true),
        ],
        read_only: false,
    },
    Tool {
        name: "append_to_note",
        description: "ノートの末尾に文章を追記する（無ければ作る）",
        args: &[
            ("note", "string", "ノートのパスか名前", true),
            ("text", "string", "追記する文章", true),
        ],
        read_only: false,
    },
    Tool {
        name: "daily_note",
        description: "今日のデイリーノートのパスを返す（無ければ作る）。text を渡すと末尾に追記する",
        args: &[("text", "string", "追記する文章（省略可）", false)],
        read_only: false,
    },
];

fn tool_list() -> Value {
    let tools: Vec<Value> = TOOLS
        .iter()
        .map(|t| {
            let mut props = serde_json::Map::new();
            let mut required = Vec::new();
            for (name, ty, desc, req) in t.args {
                props.insert((*name).into(), json!({ "type": ty, "description": desc }));
                if *req {
                    required.push(*name);
                }
            }
            json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": { "type": "object", "properties": props, "required": required },
                "annotations": {
                    "readOnlyHint": t.read_only,
                    "destructiveHint": t.name == "write_note",
                    "idempotentHint": t.read_only || t.name == "write_note",
                },
            })
        })
        .collect();
    json!({ "tools": tools })
}

fn note_path(vault: &Vault, arg: &str) -> String {
    if vault.note(arg).is_some() {
        return arg.to_string();
    }
    if let Some(found) = vault.resolve(arg, "") {
        return found.to_string();
    }
    if arg.ends_with(".md") {
        arg.to_string()
    } else {
        format!("{arg}.md")
    }
}

fn arg<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .with_context(|| format!("引数 {key} が必要です"))
}

/// ツールを1回実行し、結果の文字列を返す。
fn call(vault: &mut Vault, name: &str, args: &Value) -> Result<String> {
    let pretty = |v: Value| serde_json::to_string_pretty(&v).unwrap_or_default();
    Ok(match name {
        "search" => pretty(serve::handle(vault, "search", args)?),
        "find_notes" => pretty(serve::handle(vault, "find", args)?),
        "list_notes" => pretty(serve::handle(vault, "list", args)?),
        "read_note" => {
            let rel = note_path(vault, arg(args, "note")?);
            let note = vault
                .note(&rel)
                .with_context(|| format!("ノートが見つかりません: {rel}"))?;
            format!("# {rel}\n\n{}", note.text)
        }
        "backlinks" => pretty(serve::handle(vault, "backlinks", args)?),
        "outgoing_links" => pretty(serve::handle(vault, "links", args)?),
        "related_notes" => {
            let mut args = args.clone();
            if args.get("limit").is_none() {
                args["limit"] = json!(10);
            }
            pretty(serve::handle(vault, "related", &args)?)
        }
        "query" => pretty(serve::handle(vault, "query", args)?),
        "tasks" => {
            let args = if args.get("query").is_some() {
                args.clone()
            } else {
                json!({ "query": "" })
            };
            pretty(serve::handle(vault, "tasks", &args)?)
        }
        "write_note" => {
            let rel = arg(args, "note")?;
            if !rel.ends_with(".md") {
                bail!("書き込めるのは .md のノートだけです: {rel}");
            }
            let existed = vault.note(rel).is_some();
            vault.write_note(rel, arg(args, "text")?)?;
            format!("{}しました: {rel}", if existed { "上書き" } else { "作成" })
        }
        "append_to_note" => {
            let rel = note_path(vault, arg(args, "note")?);
            append(vault, &rel, arg(args, "text")?)?;
            format!("追記しました: {rel}")
        }
        "daily_note" => {
            let rel = vault.daily()?;
            if let Some(text) = args.get("text").and_then(Value::as_str) {
                append(vault, &rel, text)?;
                format!("追記しました: {rel}")
            } else {
                rel
            }
        }
        _ => bail!("知らないツールです: {name}"),
    })
}

fn append(vault: &mut Vault, rel: &str, text: &str) -> Result<()> {
    if !rel.ends_with(".md") {
        bail!("書き込めるのは .md のノートだけです: {rel}");
    }
    let mut body = vault.note(rel).map(|n| n.text.clone()).unwrap_or_default();
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    body.push_str(text);
    if !body.ends_with('\n') {
        body.push('\n');
    }
    vault.write_note(rel, &body)
}

/// 1つのメッセージに答える。通知（id の無いメッセージ）には `None`。
pub fn respond(vault: &mut Vault, msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let id = id?;
    let result = match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str);
            let version = asked
                .filter(|v| PROTOCOL_VERSIONS.contains(v))
                .unwrap_or(PROTOCOL_VERSIONS[0]);
            Ok(json!({
                "protocolVersion": version,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "robsidian", "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS,
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(tool_list()),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            if !TOOLS.iter().any(|t| t.name == name) {
                Err((-32602, format!("知らないツールです: {name}")))
            } else {
                // 他のアプリでの変更を取り込んでから答える
                vault.refresh();
                // ツールの失敗は、プロトコルのエラーではなく結果（isError）として返す
                Ok(match call(vault, name, &args) {
                    Ok(text) => json!({ "content": [{ "type": "text", "text": text }] }),
                    Err(e) => json!({
                        "content": [{ "type": "text", "text": format!("{e:#}") }],
                        "isError": true,
                    }),
                })
            }
        }
        _ => Err((-32601, format!("Method not found: {method}"))),
    };
    Some(match result {
        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
        Err((code, message)) => {
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
        }
    })
}

pub fn run(mut vault: Vault) -> Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(Value::Array(batch)) => {
                let replies: Vec<Value> = batch
                    .iter()
                    .filter_map(|m| respond(&mut vault, m))
                    .collect();
                (!replies.is_empty()).then_some(Value::Array(replies))
            }
            Ok(msg) => respond(&mut vault, &msg),
            Err(e) => Some(json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": { "code": -32700, "message": format!("Parse error: {e}") },
            })),
        };
        if let Some(reply) = reply {
            serde_json::to_writer(&mut stdout, &reply)?;
            writeln!(stdout)?;
            stdout.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("a.md"),
            "# A\n[[b]] #tag rust\n- [ ] やること\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("b.md"), "B note").unwrap();
        let v = Vault::open(dir.path()).unwrap();
        (dir, v)
    }

    fn rpc(v: &mut Vault, id: u64, method: &str, params: Value) -> Value {
        respond(
            v,
            &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
        )
        .unwrap()
    }

    fn text(reply: &Value) -> &str {
        reply["result"]["content"][0]["text"].as_str().unwrap()
    }

    #[test]
    fn speaks_mcp() {
        let (dir, mut v) = vault();
        let init = rpc(
            &mut v,
            1,
            "initialize",
            json!({"protocolVersion": "2024-11-05", "capabilities": {}}),
        );
        assert_eq!(init["result"]["protocolVersion"], "2024-11-05");
        assert!(init["result"]["capabilities"]["tools"].is_object());
        let init = rpc(
            &mut v,
            1,
            "initialize",
            json!({"protocolVersion": "1999-01-01"}),
        );
        assert_eq!(init["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);
        // 通知には答えない
        assert!(respond(
            &mut v,
            &json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
        )
        .is_none());

        let list = rpc(&mut v, 2, "tools/list", json!({}));
        let names: Vec<&str> = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"search") && names.contains(&"write_note"));
        assert_eq!(
            list["result"]["tools"][0]["inputSchema"]["required"][0],
            "query"
        );

        let r = rpc(
            &mut v,
            3,
            "tools/call",
            json!({"name": "read_note", "arguments": {"note": "a"}}),
        );
        assert!(text(&r).starts_with("# a.md\n\n# A"), "{r}");
        let r = rpc(
            &mut v,
            4,
            "tools/call",
            json!({"name": "backlinks", "arguments": {"note": "b"}}),
        );
        assert!(text(&r).contains("\"source\": \"a.md\""), "{r}");
        let r = rpc(
            &mut v,
            5,
            "tools/call",
            json!({"name": "search", "arguments": {"query": "rust"}}),
        );
        assert!(text(&r).contains("a.md"), "{r}");
        let r = rpc(
            &mut v,
            6,
            "tools/call",
            json!({"name": "tasks", "arguments": {}}),
        );
        assert!(text(&r).contains("やること"), "{r}");

        // 書き込みは Vault を通り、ディスクに反映される
        let r = rpc(
            &mut v,
            7,
            "tools/call",
            json!({"name": "write_note", "arguments": {"note": "new/c.md", "text": "C"}}),
        );
        assert_eq!(text(&r), "作成しました: new/c.md");
        let r = rpc(
            &mut v,
            8,
            "tools/call",
            json!({"name": "append_to_note", "arguments": {"note": "b", "text": "more"}}),
        );
        assert_eq!(text(&r), "追記しました: b.md");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("b.md")).unwrap(),
            "B note\nmore\n"
        );

        // ツールの失敗は isError、プロトコルの誤りは error
        let r = rpc(
            &mut v,
            9,
            "tools/call",
            json!({"name": "write_note", "arguments": {"note": "../x.md", "text": "x"}}),
        );
        assert_eq!(r["result"]["isError"], true);
        let r = rpc(
            &mut v,
            10,
            "tools/call",
            json!({"name": "write_note", "arguments": {"note": "x.png", "text": "x"}}),
        );
        assert_eq!(r["result"]["isError"], true);
        let r = rpc(
            &mut v,
            11,
            "tools/call",
            json!({"name": "read_note", "arguments": {}}),
        );
        assert_eq!(r["result"]["isError"], true);
        let r = rpc(
            &mut v,
            12,
            "tools/call",
            json!({"name": "nope", "arguments": {}}),
        );
        assert_eq!(r["error"]["code"], -32602);
        let r = rpc(&mut v, 13, "no/such", json!({}));
        assert_eq!(r["error"]["code"], -32601);
        assert_eq!(rpc(&mut v, 14, "ping", json!({}))["result"], json!({}));
    }
}
