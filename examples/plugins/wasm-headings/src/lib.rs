//! Robsidian の WASM のプラグインの例。
//!
//! 下半分の「助け」（`alloc`・`handle`・`call`・`notify`）は、そのまま写して使える。
//! 自分のプラグインは `on_request` だけを書く。約束（ABI）は docs/ROADMAP.md のフェーズE3。

use serde_json::{json, Value};

/// Robsidian からの依頼（段階1のリクエストと同じ JSON）を受けて、レスポンスの JSON を返す
fn on_request(req: &Value) -> Value {
    let kind = req["kind"].as_str().unwrap_or("");
    let id = req["id"].as_str().unwrap_or("");
    match (kind, id) {
        // パネル: ノートの本文（依頼に入っている）から見出しを拾う。問い合わせはしないので権限は要らない
        ("panel", "headings") => {
            let text = req["note"]["text"].as_str().unwrap_or("");
            let mut md = String::new();
            for line in text.lines() {
                let level = line.chars().take_while(|c| *c == '#').count();
                if (1..=6).contains(&level) && line[level..].starts_with(' ') {
                    let indent = "  ".repeat(level - 1);
                    md.push_str(&format!("{indent}- {}\n", line[level..].trim()));
                }
            }
            if md.is_empty() {
                md = "*見出しはありません*".into();
            }
            json!({ "markdown": md })
        }
        // コマンド: vault/backlinks で問い合わせる（permissions に read が要る）
        ("command", "links") => {
            let Some(path) = req["note"]["path"].as_str() else {
                return json!({ "actions": [{"type": "notice", "text": "ノートを開いてから実行してください"}] });
            };
            let text = match call("vault/backlinks", &json!({ "path": path })) {
                Ok(links) => {
                    let mut sources: Vec<&str> = links
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|l| l["source"].as_str())
                        .collect();
                    sources.sort_unstable();
                    sources.dedup();
                    format!("{}件のノートからリンクされています", sources.len())
                }
                Err(e) => format!("問い合わせできませんでした: {e}"),
            };
            json!({ "actions": [{"type": "notice", "text": text}] })
        }
        _ => json!({}),
    }
}

// ---------------------------------------------------------------- 助け（そのまま写して使える）

#[link(wasm_import_module = "robsidian")]
extern "C" {
    #[link_name = "call"]
    fn host_call(mptr: *const u8, mlen: usize, pptr: *const u8, plen: usize) -> u64;
    #[link_name = "notify"]
    fn host_notify(mptr: *const u8, mlen: usize, pptr: *const u8, plen: usize);
    #[link_name = "log"]
    fn host_log(ptr: *const u8, len: usize);
}

/// Robsidian が書き込む場所を用意する（`handle` の入力と、`call` の返事に使われる）
#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len.max(1));
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// 返す文字列は、次に呼ばれるまで置いておく（Robsidian が読むので）
static mut OUT: Vec<u8> = Vec::new();

/// (ポインタ << 32) | 長さ
fn pack(ptr: *const u8, len: usize) -> u64 {
    ((ptr as u64) << 32) | len as u64
}

/// 依頼を受ける
#[no_mangle]
pub extern "C" fn handle(ptr: *mut u8, len: usize) -> u64 {
    // alloc で用意した場所なので、ここで受け取って片付ける
    let input = unsafe { Vec::from_raw_parts(ptr, len, len.max(1)) };
    let req: Value = serde_json::from_slice(&input).unwrap_or(Value::Null);
    let out = on_request(&req).to_string().into_bytes();
    unsafe {
        let out_ref = &raw mut OUT;
        *out_ref = out;
        pack((*out_ref).as_ptr(), (*out_ref).len())
    }
}

/// Robsidian に問い合わせる（vault/read・list・search・backlinks・write）。権限が無ければ Err
pub fn call(method: &str, params: &Value) -> Result<Value, String> {
    let p = params.to_string();
    let r = unsafe { host_call(method.as_ptr(), method.len(), p.as_ptr(), p.len()) };
    let (ptr, len) = ((r >> 32) as usize as *mut u8, (r & 0xffff_ffff) as usize);
    let bytes = unsafe { Vec::from_raw_parts(ptr, len, len.max(1)) };
    let v: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    match v.get("ok") {
        Some(ok) => Ok(ok.clone()),
        None => Err(v["error"].as_str().unwrap_or("失敗しました").to_string()),
    }
}

/// Robsidian に知らせる（ui/notice・panel/refresh）
pub fn notify(method: &str, params: &Value) {
    let p = params.to_string();
    unsafe { host_notify(method.as_ptr(), method.len(), p.as_ptr(), p.len()) }
}

/// 開発用の出力（Robsidian の標準エラーに出る）
pub fn log(text: &str) {
    unsafe { host_log(text.as_ptr(), text.len()) }
}
