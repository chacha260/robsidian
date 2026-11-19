# プラグインの一覧（catalog.json）

Robsidian は、`catalog.json` に並んだプラグインを検索して入れられます（設定の「プラグイン」、または `rob plugin search` / `rob plugin install`）。
一覧は、ファイルを置けるところ（GitHub のリポジトリの raw の URL、Web サーバー、共有フォルダ）ならどこにでも置けます。

```json
{
  "plugins": [
    {
      "id": "wasm-headings",
      "name": "見出しとつながり（WASM）",
      "version": "0.1.0",
      "description": "…",
      "author": "…",
      "homepage": "https://…",
      "files": {
        "plugin.toml": {"url": "wasm-headings/plugin.toml", "sha256": "…"},
        "plugin.wasm": {"url": "wasm-headings/plugin.wasm", "sha256": "…"}
      }
    }
  ]
}
```

- `files` のキーは、プラグインのフォルダの中のパス。`url` は一覧からの相対でも、絶対でもよい
- Robsidian は、すべてのファイルの sha256 が一致したときだけ入れる。入れても、使う前の許可は別に求める
- 配るなら、権限を絞れる WASM のプラグイン（`mode = "wasm"`）を勧めます。外部プロセスのプラグインは、使う人の権限で何でもできます

## 作り方

公開するフォルダに、プラグインのフォルダ（`plugin.toml` と、WASM なら `plugin.wasm`）を並べて、

```bash
python3 make_catalog.py 公開するフォルダ > 公開するフォルダ/catalog.json
```

で一覧を作ります（Python 3.11 以上）。
