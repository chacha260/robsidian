//! 開発用の時間の計測。環境変数 `ROBSIDIAN_PROFILE=1` のときだけ、時間の掛かった描画の内訳を標準エラーに出す。
//! `ROBSIDIAN_PROFILE=0.5` のように数を渡すと、それ（ミリ秒）を超えた描画を出す（すべて見たいときは `0.01` など）。
//!
//! ```ignore
//! let _t = profile::span("editor.layout");   // この行からブロックの終わりまでを測る
//! ```

use std::cell::RefCell;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// 計測するか。するなら、出す描画の時間の下限（環境変数で数を渡したとき）
fn setting() -> Option<Option<Duration>> {
    static ON: OnceLock<Option<Option<Duration>>> = OnceLock::new();
    *ON.get_or_init(|| {
        let v = std::env::var("ROBSIDIAN_PROFILE").ok()?;
        if v == "0" {
            return None;
        }
        // 「1」は有効にするだけ（下限は呼ぶ側の既定）
        let ms = v.parse::<f64>().ok().filter(|&ms| ms > 0.0 && v != "1");
        Some(ms.map(|ms| Duration::from_secs_f64(ms / 1000.0)))
    })
}

fn enabled() -> bool {
    setting().is_some()
}

thread_local! {
    /// この描画で測った区間（名前, 時間, 回数）
    static SPANS: RefCell<Vec<(&'static str, Duration, u32)>> = const { RefCell::new(Vec::new()) };
}

pub struct Span {
    name: &'static str,
    start: Option<Instant>,
}

/// 区間を測り始める（戻り値を落としたところまで）
pub fn span(name: &'static str) -> Span {
    Span {
        name,
        start: enabled().then(Instant::now),
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        let Some(start) = self.start else {
            return;
        };
        let d = start.elapsed();
        SPANS.with(|s| {
            let mut s = s.borrow_mut();
            match s.iter_mut().find(|(n, _, _)| *n == self.name) {
                Some(e) => {
                    e.1 += d;
                    e.2 += 1;
                }
                None => s.push((self.name, d, 1)),
            }
        });
    }
}

/// 1回の描画の終わりに呼ぶ。全体が `threshold` を超えたら内訳を出す
pub fn frame_end(total: Duration, threshold: Duration) {
    let Some(limit) = setting() else {
        return;
    };
    let threshold = limit.unwrap_or(threshold);
    let spans = SPANS.with(|s| std::mem::take(&mut *s.borrow_mut()));
    if total < threshold {
        return;
    }
    let parts: Vec<String> = spans
        .iter()
        .map(|(n, d, c)| {
            if *c > 1 {
                format!("{n}={:.1}ms×{c}", d.as_secs_f64() * 1000.0)
            } else {
                format!("{n}={:.1}ms", d.as_secs_f64() * 1000.0)
            }
        })
        .collect();
    eprintln!(
        "[profile] frame {:.1}ms: {}",
        total.as_secs_f64() * 1000.0,
        parts.join(" ")
    );
}
