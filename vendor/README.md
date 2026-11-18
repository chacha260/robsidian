# vendor

手を入れて使っている外部のクレート。ルートの `Cargo.toml` の `[patch.crates-io]` で差し替えている。
fmt と clippy の対象にはしない（`[workspace] exclude`）。

## egui_commonmark_backend 0.26.0

プレビュー（egui_commonmark）のリンクにマウスを乗せたときにノートの中身を見せる（ページプレビュー）ため、
どのリンクにマウスが乗っているかを知らせる口を足した。変更は `src/misc.rs` の「Robsidian:」と書いた箇所だけ。

- `CommonMarkCache` に `hovered_link` と `take_hovered_link()` を追加
- リンクを描いたとき、マウスが乗っていれば宛先を `hovered_link` に入れる

egui_commonmark を上げるときは、新しい版を取り込み直して同じ変更を当てる。
ライセンスは元のクレートと同じ（MIT または Apache-2.0。同じフォルダの LICENSE-*）。
