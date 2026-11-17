# Robsidian 開発メモ

Obsidian互換のvaultを扱う、純Rust（egui）製のノートアプリ。

## 構成

- `crates/core` — vault索引・リンク解決・検索・リネーム・ローカル履歴（GUI非依存）
- `crates/cli` — `rob` コマンド
- `crates/app` — eguiのGUI（`robsidian`）
- `docs/ROADMAP.md` — 機能追加案と実装方針。機能を実装したら該当項目を更新する

## ルール

- **機能追加・修正をするときは、ルートの `Cargo.toml` の `[workspace.package] version` を上げる。**
  `main` にマージされると、そのバージョンで GitHub Releases に自動でリリースされる
  （`.github/workflows/release.yml`）。同じバージョンのままではリリースされない。
  - 機能追加・修正はパッチ番号を上げる（例: 0.2.1 → 0.2.2）
  - 1つのプルリクエストでは1回だけ上げる。作業前に `main` のバージョンを確認し、それより1つ上にする
- コミット前に `cargo fmt --all`、`cargo clippy --workspace --all-targets`（警告ゼロ）、`cargo test --workspace` を通す
- 整形だけの変更は、機能の変更とは別のコミットにする
- UIの文言とコメントは日本語
- ファイルを書き換える処理は `Vault` を通す（アトミック書き込みとローカル履歴が効くように）

## 動作確認（Linuxのヘッドレス環境）

`Xvfb :99` を起動し、`DISPLAY=:99 LIBGL_ALWAYS_SOFTWARE=1` で `target/debug/robsidian <vault>` を起動、
`xdotool` で操作して `import -window root` でスクリーンショットを撮る。
X11では `libxkbcommon-x11` が必要。
