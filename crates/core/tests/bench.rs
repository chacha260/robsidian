//! 大きなvaultでの計測（普段は何もしない）。
//!
//! `ROB_BENCH_VAULT=/path/to/vault cargo test -p robsidian-core --release --test bench -- --nocapture`
//! 計測ではノートを書き換えて元に戻すので、コピーしたvaultで実行すること。

use robsidian_core::Vault;
use std::time::Instant;

#[test]
fn bench_big_vault() {
    let Some(path) = std::env::var_os("ROB_BENCH_VAULT") else {
        return;
    };
    let t = Instant::now();
    let mut v = Vault::open(&path).unwrap();
    let open = t.elapsed();
    let rel = v.notes().nth(v.note_count() / 2).unwrap().path.clone();
    let text = v.note(&rel).unwrap().text.clone();
    let t = Instant::now();
    for i in 0..10 {
        v.write_note(&rel, &format!("{text}\n{i}")).unwrap();
    }
    let save = t.elapsed() / 10;
    v.write_note(&rel, &text).unwrap();
    let t = Instant::now();
    let orphans = v.orphans().len();
    let orphan_time = t.elapsed();
    eprintln!(
        "notes {} / open {open:?} / save 1 note {save:?} / orphans {orphans} in {orphan_time:?}",
        v.note_count()
    );
}
