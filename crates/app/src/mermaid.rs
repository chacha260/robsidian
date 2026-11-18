//! Mermaid図の表示（任意機能）。純Rustで描く手段が無いので、外部コマンド（既定は `mmdc`）が
//! 使えるときだけ、図を画像にしてプレビューに出す。
//!
//! 画像は `.robsidian/cache/mermaid/<内容のハッシュ>.png` に置き、同じ図は作り直さない。
//! 作成は別スレッドで行い、できたらプレビューを作り直す。

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;

/// 図の状態
pub enum State {
    Ready(PathBuf),
    Pending,
    Failed(String),
}

#[derive(Default)]
pub struct Mermaid {
    pending: HashSet<u64>,
    failed: HashMap<u64, String>,
    tx: Option<mpsc::Sender<(u64, Result<(), String>)>>,
    rx: Option<mpsc::Receiver<(u64, Result<(), String>)>>,
}

fn key(src: &str, dark: bool, command: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (src.trim(), dark, command).hash(&mut h);
    h.finish()
}

pub fn cache_dir(root: &Path) -> PathBuf {
    root.join(".robsidian").join("cache").join("mermaid")
}

/// シェルに渡すためにパスを引用符で囲む
fn quote(p: &Path) -> String {
    let s = p.display().to_string();
    if cfg!(windows) {
        format!("\"{s}\"")
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

fn render(command: &str, input: &Path, output: &Path, dark: bool) -> Result<(), String> {
    let line = format!(
        "{command} -i {} -o {} -b transparent -t {}",
        quote(input),
        quote(output),
        if dark { "dark" } else { "default" }
    );
    let mut c = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(&line);
        c
    } else {
        let mut c = Command::new("sh");
        c.arg("-c").arg(&line);
        c
    };
    let out = c.output().map_err(|e| e.to_string())?;
    if out.status.success() && output.exists() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("終了コードが0ではありません");
        Err(line.trim().chars().take(200).collect())
    }
}

impl Mermaid {
    /// 図の画像。無ければ作り始める（`command` が空なら何もしない）。
    pub fn image(
        &mut self,
        root: &Path,
        src: &str,
        dark: bool,
        command: &str,
        ctx: &eframe::egui::Context,
    ) -> Option<State> {
        let command = command.trim();
        if command.is_empty() {
            return None;
        }
        let k = key(src, dark, command);
        let png = cache_dir(root).join(format!("{k:016x}.png"));
        if png.exists() {
            return Some(State::Ready(png));
        }
        if let Some(e) = self.failed.get(&k) {
            return Some(State::Failed(e.clone()));
        }
        if self.pending.insert(k) {
            if self.tx.is_none() {
                let (tx, rx) = mpsc::channel();
                self.tx = Some(tx);
                self.rx = Some(rx);
            }
            let tx = self.tx.clone().expect("set above");
            let (dir, src, command, ctx) = (
                cache_dir(root),
                src.to_string(),
                command.to_string(),
                ctx.clone(),
            );
            std::thread::spawn(move || {
                let result = (|| {
                    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                    let input = dir.join(format!("{k:016x}.mmd"));
                    std::fs::write(&input, &src).map_err(|e| e.to_string())?;
                    // 途中で止まっても壊れた画像を残さないよう、別名で作ってから置き換える
                    let tmp = dir.join(format!("{k:016x}.tmp.png"));
                    let r = render(&command, &input, &tmp, dark)
                        .and_then(|()| std::fs::rename(&tmp, &png).map_err(|e| e.to_string()));
                    let _ = std::fs::remove_file(&input);
                    let _ = std::fs::remove_file(&tmp);
                    r
                })();
                let _ = tx.send((k, result));
                ctx.request_repaint();
            });
        }
        Some(State::Pending)
    }

    /// 作り終えた図があれば true（プレビューを作り直す）。失敗は理由を覚えておく。
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &self.rx else {
            return false;
        };
        let mut done = false;
        for (k, result) in rx.try_iter() {
            self.pending.remove(&k);
            if let Err(e) = result {
                self.failed.insert(k, e);
            }
            done = true;
        }
        done
    }

    /// 設定を変えたときなど、失敗の記録を消してやり直せるようにする
    pub fn forget_failures(&mut self) {
        self.failed.clear();
    }
}

/// ` ```mermaid ` ブロックの範囲（フェンスを含むバイト位置）と中身。
pub fn blocks(text: &str) -> Vec<(std::ops::Range<usize>, String)> {
    let mut out = Vec::new();
    let mut offset = 0;
    let mut open: Option<(usize, String, usize)> = None;
    for line in text.split_inclusive('\n') {
        let t = line.trim();
        match &mut open {
            None => {
                let ticks = t.chars().take_while(|&c| c == '`' || c == '~').count();
                if ticks >= 3 && t[ticks..].trim().eq_ignore_ascii_case("mermaid") {
                    open = Some((offset, String::new(), ticks));
                }
            }
            Some((start, body, ticks)) => {
                let closing = t.len() >= *ticks && t.chars().all(|c| c == '`' || c == '~');
                if closing {
                    out.push((*start..offset + line.len(), std::mem::take(body)));
                    open = None;
                } else {
                    body.push_str(line);
                }
            }
        }
        offset += line.len();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_mermaid_blocks() {
        let text = "a\n```mermaid\ngraph LR\n  A-->B\n```\nb\n```rust\nx\n```\n~~~Mermaid\nx\n~~~\n```mermaid\n未完";
        let found = blocks(text);
        assert_eq!(found.len(), 2);
        assert_eq!(
            &text[found[0].0.clone()],
            "```mermaid\ngraph LR\n  A-->B\n```\n"
        );
        assert_eq!(found[0].1, "graph LR\n  A-->B\n");
        assert_eq!(found[1].1, "x\n");
    }

    #[test]
    fn key_depends_on_content_theme_and_command() {
        assert_eq!(key("a", true, "mmdc"), key(" a\n", true, "mmdc"));
        assert_ne!(key("a", true, "mmdc"), key("a", false, "mmdc"));
        assert_ne!(key("a", true, "mmdc"), key("b", true, "mmdc"));
        assert_ne!(key("a", true, "mmdc"), key("a", true, "mmdc -s 2"));
    }

    #[cfg(unix)]
    #[test]
    fn renders_with_a_command_and_reports_failures() {
        let dir = tempfile::tempdir().unwrap();
        // 画像の代わりに入力を出力へ複製する偽のコマンド（引数 -i 入力 -o 出力 …）
        let fake = dir.path().join("fake.sh");
        std::fs::write(&fake, "#!/bin/sh\ncp \"$2\" \"$4\"\n").unwrap();
        let input = dir.path().join("in.mmd");
        std::fs::write(&input, "graph").unwrap();
        let out = dir.path().join("out dir's.png");
        render(&format!("sh {}", fake.display()), &input, &out, true).unwrap();
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "graph");
        let err = render("sh -c 'echo boom >&2; exit 1' x", &input, &out, true).unwrap_err();
        assert_eq!(err, "boom");
    }
}
