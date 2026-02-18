# Robsidian 💎

**Robsidian** は、Obsidianの拡張性と、Helix / NuShell のパワーを融合させた、Rust + Tauri 製の次世代ノートテイキングアプリです。
「見た目はObsidian、中身はハッカー仕様」をコンセプトに、軽量かつ強力なプラグインシステムを備えています。

## 🚀 開発環境のセットアップ (Development Setup)

Robsidian自体のビルドや開発を行うための手順です。

### 前提条件 (Prerequisites)

* **Node.js**
* **Rust / Cargo** (最新版)
* **Tauri CLI** (`npm install -g @tauri-apps/cli`)
* **WebView2** (Windowsの場合)

### インストール (Installation)

リポジトリをクローンした後、依存関係をインストールします。

```bash
npm install
```

### 開発モードでの実行 (Run in Development Mode)

ホットリロード対応の開発サーバーを立ち上げます。

```bash
npm run tauri dev

```

### ビルド (Build for Production)

配布用の実行ファイル（`.exe` や `.dmg`）を生成します。

```bash
cargo tauri build
# または
npm run tauri build
```

---

## 🧩 拡張機能の作成ガイド (Extension Guide)

Robsidianは **「機能プラグイン」** と **「外観テーマ」** の2種類の拡張に対応しています。
これらは現在開いているVault（フォルダ）の中にある `.robsidian` フォルダで管理されます。

### 📂 フォルダ構成

Vaultのルートに以下のフォルダを作成してください。

```text
[Vault Root]
  └─ .robsidian
       ├─ plugins/       <-- 機能プラグイン用
       │    └─ my-plugin/
       │         ├─ manifest.json
       │         └─ main.js
       │
       └─ themes/        <-- テーマ用
            └─ my-theme/
                 ├─ manifest.json
                 └─ styles.css

```

---

### 1. テーマの作成 (Creating Themes)

テーマはCSS変数（Custom Properties）を上書きすることで適用されます。

**manifest.json:**

```json
{
    "id": "ocean-dark",
    "name": "Ocean Dark Theme",
    "version": "1.0.0",
    "type": "theme",
    "description": "Deep blue theme inspired by the ocean.",
    "author": "Your Name"
}

```

**styles.css:**

```css
:root {
    /* 必須のオーバーライド変数 */
    --bg-main: #0f172a;       /* メイン背景色 */
    --bg-sidebar: #1e293b;    /* サイドバー背景色 */
    --text-main: #f1f5f9;     /* メイン文字色 */
    --text-muted: #94a3b8;    /* 薄い文字色 */
    --border-color: #334155;  /* ボーダー色 */
    --accent-color: #38bdf8;  /* アクセントカラー */
    --active-item-bg: #1e293b;/* 選択中のアイテム背景 */
}

```

---

### 2. プラグインの作成 (Creating Plugins)

JavaScriptを使ってアプリの機能を拡張できます。

**manifest.json:**

```json
{
    "id": "daily-calendar",
    "name": "Daily Calendar",
    "version": "1.0.0",
    "type": "plugin",
    "description": "Adds a calendar for managing daily notes.",
    "main": "main.js"
}

```

**main.js の基本構造:**

```javascript
return {
    // ロード時に呼ばれる
    async onload(app) {
        console.log("Plugin loaded!");
        
        // 設定の読み込み
        const settings = await app.plugin.loadData();

        // 設定画面の追加
        app.plugin.addSettingTab("My Plugin", (container) => {
            container.innerHTML = "<h3>Settings</h3>";
            // ...設定UIの構築
        });
    },

    // アンロード時（無効化時）に呼ばれる
    onunload() {
        console.log("Plugin unloaded!");
        // イベントリスナーの解除やDOMの削除を行う
    }
}

```

### 🛠️ Robsidian API (`window.app`)

プラグインからは `app` 引数を通じて以下の機能にアクセスできます。

#### `app.vault` (ファイル操作)

* `read(path)`: ファイルの内容をテキストとして読み込む。
* `write(path, content)`: ファイルにテキストを書き込む（上書き）。
* `create(path)`: 空のファイルを作成する。
* `createDir(path)`: フォルダを作成する。
* `delete(path)`: ファイルまたはフォルダを削除する。
* `rename(oldPath, newPath)`: ファイル名変更または移動。
* `list(folderPath)`: 指定フォルダ内のファイル一覧を取得。
* `exists(path)`: ファイルの存在確認（簡易実装）。
* `getResourcePath(path)`: 画像表示用のローカルパスを取得。

#### `app.workspace` (画面操作)

* `openFile(path)`: 指定したパスのファイルをエディタで開く。
* `openTerminal(title, command)`: 新しいターミナルタブを開く。
* `split()`: ペインを分割する。

#### `app.plugin` (設定管理)

* `loadData()`: プラグイン固有の保存された設定データを取得。
* `saveData(data)`: 設定データを保存（永続化）。
* `addSettingTab(name, handler)`: 設定画面に専用タブを追加する。

#### `app.utils` (ユーティリティ)

* `notice(message)`: 画面右上にトースト通知を表示する。
* `clipboardWrite(text)`: クリップボードにコピー。

---

### 📝 Tips

* **開発中のデバッグ**: アプリ上で `F12` キーを押すと DevTools が開き、`console.log` の内容やDOM構造を確認できます。
* **パスの区切り文字**: WindowsとMac/Linuxの互換性のため、パス結合時は `app.vault.root.includes("/") ? "/" : "\\"` のようにセパレータを判定することを推奨します。