# まずは以下を読んで、セットアップを行ってから、本リポジトリのファイルで上書きをしていってください!!

---

# Tauri + Vanilla TS

This template should help get you started developing with Tauri in vanilla HTML, CSS and Typescript.

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
---

# 🛠️ Robsidian 開発環境セットアップガイド (Windows / GCC版)

このガイドでは、Visual Studio (MSVC) を使用せず、**GCC (MinGW-w64)** を用いて Robsidian (Tauri v2 + Rust) を開発するための環境構築手順を説明します。

## 1. GCC (MinGW-w64) のインストール

Rust の GNU ツールチェーンを動作させるために、GCC コンパイラを含む MinGW-w64 をインストールします。

### 推奨: パッケージマネージャー (Scoop) を使う場合

管理者権限が不要で、環境汚染が少ない **Scoop** を使うと管理が楽です。

1. PowerShell で Scoop をインストール（未導入の場合）:
```powershell
Set-ExecutionPolicy RemoteSigned -Scope CurrentUser
irm get.scoop.sh | iex
```


2. GCC をインストール:
```powershell
scoop install gcc
```


3. インストール確認:
```powershell
gcc --version
```



### 代替案: 手動でインストールする場合 (Zip解凍)

インターネット接続制限などで Scoop が使えない場合は、手動で配置します。

1. [WinLibs for MinGW-w64](https://winlibs.com/) などの配布サイトから、**UCRT runtime** 版の Zip ファイル（例: `GCC 13.x.x + MinGW-w64 ...`）をダウンロードします。
2. 任意の場所（例: `C:\mingw64`）に解凍します。
3. `bin` フォルダ（例: `C:\mingw64\bin`）をユーザー環境変数の **PATH** に追加します。
4. ターミナルを再起動し、`gcc --version` で確認します。

## 2. Rust のインストール (GNU Toolchain)

デフォルトの MSVC ではなく、**GNU (GCC)** 向けの Rust 環境を構築します。

1. [rustup.rs](https://rustup.rs/) から `rustup-init.exe` をダウンロードして実行します。
2. 画面に選択肢が表示されたら、`2` (Customize installation) を入力して Enter を押します。
3. **Default host triple?** と聞かれるので、以下を入力して Enter を押します（ここが最重要です）。
```text
x86_64-pc-windows-gnu
```

4. 残りの質問（toolchain, profile, modify PATH）はデフォルトのままで Enter を押して進めます。
5. インストール完了後、ターミナルを再起動して以下を確認します。

```powershell
rustc --version
# 出力に (xxxx-xx-xx) だけでなく windows-gnu が含まれている、
# または `rustup show` で `stable-x86_64-pc-windows-gnu` が active になっていることを確認
```

> **既にRustをインストール済みの場合:**
> 以下のコマンドで GNU 版に切り替えられます。
> ```powershell
> rustup toolchain install stable-x86_64-pc-windows-gnu
> rustup default stable-x86_64-pc-windows-gnu
> ```


## 3. Node.js のインストール

フロントエンドのビルドに必要です。

1. [Node.js 公式サイト](https://nodejs.org/) から **LTS版** をダウンロードしてインストールします。
2. 確認:
```powershell
node -v
npm -v
```

## 4. WebView2 ランタイムの確認

Tauri は Windows の UI 描画に Edge (WebView2) を使用します。
Windows 10/11 であれば通常は標準搭載されていますが、サーバーOSや軽量化版OSを使用している場合は、[WebView2 Runtime](https://developer.microsoft.com/en-us/microsoft-edge/webview2/) のインストールが必要になる場合があります。

## 5. エディタの設定 (VS Code)

Visual Studio IDE は不要ですが、コードエディタとして VS Code は推奨されます。

* 必須拡張機能:
* **rust-analyzer**: GCC環境でも問題なく動作し、強力な補完を提供します。



## 6. プロジェクトの動作確認

### 依存関係のインストール

プロジェクトフォルダで実行します。

```powershell
npm install
```

### 開発サーバーの起動

GCC 環境でコンパイルと起動を行います。

```powershell
cargo tauri dev
```

> **注意点:**
> GCC (GNU) ビルドは、MSVC ビルドに比べてコンパイル後のバイナリサイズが若干大きくなる傾向がありますが、機能的な差はありません。また、初回ビルド時に `cc` クレートなどが `gcc.exe` を使用して C 言語依存ライブラリをコンパイルします。

### アイコンの適用
- `app-icon.png`をアイコンにしたい場合、以下のコマンドを実行してからビルドを行ってください。
```powershell
cargo tauri icon ./app-icon.png
```

---

### 💡 GCC環境でのトラブルシューティング

* **`CreateProcess error=2, The system cannot find the file specified`**:
* `gcc` コマンドへのパスが通っていません。環境変数 PATH を確認してください。


* **リンクエラー (`ld: cannot find -lxxxx`)**:
* システムライブラリが見つからない場合に発生します。通常の Tauri アプリ開発では稀ですが、MinGW-w64 のバージョンや種類（Mingw-builds, WinLibs, MSYS2等）を変えると解決することがあります。


* **実行時に `libgcc_s_seh-1.dll` 等が見つからないと言われる**:
* ビルドされた `.exe` を配布する際、MinGW の DLL が必要になる場合があります。これを避けるには、`Cargo.toml` や `.cargo/config` に静的リンクの設定を追加する必要があります（開発中はPATHが通っていれば問題ありません）。



---

これで、Visual Studio レスな「完全 GCC 環境」での開発準備は完了です！
