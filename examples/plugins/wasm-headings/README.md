# wasm-headings（WASM のプラグインの例）

Rust で書く Robsidian の WASM のプラグインです。

```bash
rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/wasm_headings.wasm plugin.wasm
```

`plugin.toml` と `plugin.wasm` を、vault の `.robsidian/plugins/wasm-headings/` に置くと使えます
（ほかのファイルは要りません）。使う前に、設定の「プラグイン」で中身を確かめて許可してください。

- パネル「見出し（WASM）」: 開いているノートの見出しの一覧
- コマンド「このノートへのつながりを数える」: `vault/backlinks` で問い合わせる（`permissions = ["read"]` が要る）

`src/lib.rs` の下半分（`alloc`・`handle`・`call`・`notify`・`log`）は、ほかの言語でも同じ約束で書けます。
約束（ABI）は [docs/ROADMAP.md](../../../docs/ROADMAP.md) のフェーズE3 にあります。
