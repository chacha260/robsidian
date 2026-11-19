//! 開発用の時間の計測。環境変数 `ROBSIDIAN_PROFILE=1` のときだけ、時間の掛かった描画の内訳を標準エラーに出す。
//!
//! ```ignore
//! let _t = profile::span("editor.layout");   // この行からブロックの終わりまでを測る
//! ```

use std::cell::RefCell;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("ROBSIDIAN_PROFILE").is_some_and(|v| v != "0"))
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
    if !enabled() {
        return;
    }
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
