//! `rob serve`: 標準入出力で、1行1リクエストのJSONに答え続ける常駐モード。
//!
//! リクエスト: `{"id": 1, "method": "search", "params": {"query": "rust", "limit": 10}}`
//! 応答:       `{"id": 1, "result": ...}` または `{"id": 1, "error": "..."}`
//! 索引を持ち続けるので、問い合わせのたびにvaultを読み直さない（変わったファイルだけ読み直す）。

use anyhow::{bail, Context, Result};
use robsidian_core::{search, Vault};
use serde_json::{json, Value};
use std::io::{BufRead, Write};

fn str_param<'a>(params: &'a Value, key: &str) -> Result<&'a str> {
    params
        .get(key)
        .and_then(Value::as_str)
        .with_context(|| format!("params.{key} が必要です"))
}

fn note(vault: &Vault, params: &Value) -> Result<String> {
    let arg = str_param(params, "note")?;
    if vault.note(arg).is_some() {
        return Ok(arg.to_string());
    }
    vault
        .resolve(arg, "")
        .map(str::to_string)
        .with_context(|| format!("note not found: {arg}"))
}

/// 1つのリクエストを処理する。
pub fn handle(vault: &mut Vault, method: &str, params: &Value) -> Result<Value> {
    let limit = params.get("limit").and_then(Value::as_u64).unwrap_or(50) as usize;
    let today = chrono::Local::now().date_naive();
    Ok(match method {
        "stats" => json!({
            "vault": vault.root().display().to_string(),
            "notes": vault.note_count(),
            "unresolved": vault.unresolved().len(),
        }),
        "list" => json!(vault.notes().map(|n| &n.path).collect::<Vec<_>>()),
        "find" => json!(search::fuzzy(vault, str_param(params, "query")?, limit)),
        "search" => json!(search::search(vault, str_param(params, "query")?, limit)),
        "query" | "tasks" => {
            let mut q = str_param(params, "query").unwrap_or("").to_string();
            if method == "tasks" && !q.contains("task:") {
                q = format!("task:todo {q}");
            }
            match search::run_query(vault, &q, today) {
                search::QueryResult::Notes(n) => json!({ "notes": n }),
                search::QueryResult::Tasks(t) => json!({ "tasks": t }),
            }
        }
        "cat" => {
            let rel = note(vault, params)?;
            json!({ "path": rel, "text": vault.note(&rel).map(|n| n.text.clone()) })
        }
        "backlinks" => {
            let rel = note(vault, params)?;
            let links: Vec<Value> = vault
                .backlinks(&rel)
                .iter()
                .map(|b| json!({"source": &*b.source, "line": b.line + 1, "context": vault.line(&b.source, b.line)}))
                .collect();
            json!(links)
        }
        "links" => {
            let rel = note(vault, params)?;
            let links: Vec<Value> = vault
                .outgoing(&rel)
                .iter()
                .map(|(l, dst)| json!({"target": l.target, "resolved": dst, "line": l.line + 1}))
                .collect();
            json!(links)
        }
        "resolve" => {
            let target = str_param(params, "target")?;
            let from = params.get("from").and_then(Value::as_str).unwrap_or("");
            json!(vault
                .resolve(target, from)
                .or_else(|| vault.resolve_file(target, from)))
        }
        "mentions" => json!(vault.unlinked_mentions(&note(vault, params)?)),
        "write" => {
            let rel = str_param(params, "note")?.to_string();
            vault.write_note(&rel, str_param(params, "text")?)?;
            json!({ "path": rel })
        }
        "daily" => json!({ "path": vault.daily()? }),
        _ => bail!("unknown method: {method}"),
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
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(req) => {
                let id = req.get("id").cloned().unwrap_or(Value::Null);
                let method = req.get("method").and_then(Value::as_str).unwrap_or("");
                let params = req.get("params").cloned().unwrap_or(Value::Null);
                // 他のアプリでの変更を取り込んでから答える
                vault.refresh();
                match handle(&mut vault, method, &params) {
                    Ok(result) => json!({ "id": id, "result": result }),
                    Err(e) => json!({ "id": id, "error": format!("{e:#}") }),
                }
            }
            Err(e) => json!({ "id": null, "error": format!("invalid JSON: {e}") }),
        };
        serde_json::to_writer(&mut stdout, &response)?;
        writeln!(stdout)?;
        stdout.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_requests() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "[[b]] - [ ] task").unwrap();
        std::fs::write(dir.path().join("b.md"), "hello").unwrap();
        let mut v = Vault::open(dir.path()).unwrap();
        let r = handle(&mut v, "backlinks", &json!({"note": "b"})).unwrap();
        assert_eq!(r[0]["source"], "a.md");
        let r = handle(&mut v, "search", &json!({"query": "hello"})).unwrap();
        assert_eq!(r[0]["path"], "b.md");
        let r = handle(&mut v, "resolve", &json!({"target": "B"})).unwrap();
        assert_eq!(r, "b.md");
        handle(&mut v, "write", &json!({"note": "c.md", "text": "[[b]]"})).unwrap();
        assert_eq!(v.backlinks("b.md").len(), 2);
        assert!(handle(&mut v, "nope", &Value::Null).is_err());
        assert!(handle(&mut v, "cat", &json!({})).is_err());
    }
}
