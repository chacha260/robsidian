import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { marked } from "marked";
import "./styles.css";

interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
}

// 初回表示用の操作マニュアル (Markdown)
const WELCOME_MD = `
# Welcome to Robsidian 💎

Robsidian は、**Rust製の爆速バックエンド** と **Web技術のモダンなUI**、そして **Helix エディタ** を融合させた次世代のナレッジベースツールです。

## 🚀 基本操作

### 📂 ファイル管理
- **Vaultを開く**: 左上の **📂** ボタンで作業フォルダを選択します。
- **新規作成**: **📄+** (ファイル) / **📁+** (フォルダ) ボタンで作れます。
- **右クリック**: ファイル名変更 (Rename) や削除 (Delete) が可能です。

### 📝 編集モード
- **✏ Edit**: アプリ内でサクッと編集・保存できます。
- **Hx Button**: ターミナルエディタ **Helix** を起動してガッツリ編集できます。

### 🎨 カスタマイズ
- **⚙️ Settings**: フォント変更、ダーク/ライトモード切替が可能です。
- **サイドバー**: ドラッグして好きな幅に調整できます（次回も記憶されます）。

---
Happy Hacking!
`;

// ★修正: ここに 'async' を追加しました！
window.addEventListener("DOMContentLoaded", async () => {
  const app = document.getElementById("app");
  if (!app) return;

  // --- 設定値ロード ---
  let currentPath = localStorage.getItem("robsidian-last-path") || ".";
  let selectedEntry: FileEntry | null = null;
  let isEditing = false;
  
  // テーマ
  const savedTheme = localStorage.getItem("robsidian-theme");
  let isLightMode = savedTheme === "light";
  if (isLightMode) document.body.classList.add("light-theme");

  // フォント
  const defaultFont = "'HackGen', 'HackGen Console', sans-serif";
  let currentFont = localStorage.getItem("robsidian-font") || defaultFont;
  document.documentElement.style.setProperty("--font-family-ui", currentFont);
  document.documentElement.style.setProperty("--font-family-code", currentFont);

  // サイドバー幅
  const savedSidebarWidth = localStorage.getItem("robsidian-sidebar-width");
  if (savedSidebarWidth) document.documentElement.style.setProperty("--sidebar-width", savedSidebarWidth);

  // Welcome画面を表示するかどうか
  let showWelcome = localStorage.getItem("robsidian-hide-welcome") !== "true";


  // --- レイアウト構築 ---
  app.innerHTML = `
    <div style="display: grid; grid-template-columns: var(--sidebar-width) 5px 1fr; height: 100vh; width: 100vw; position: relative;">
      
      <div style="background-color: var(--bg-sidebar); border-right: 1px solid var(--border-color); display: flex; flex-direction: column; overflow: hidden;">
        <div style="padding: 10px; border-bottom: 1px solid var(--border-color); display: flex; justify-content: space-between; align-items: center;">
          <div style="display: flex; gap: 5px; align-items: center;">
            <button id="btn-open-vault" title="Open Folder (Vault)" style="border:none; background:transparent; font-size:1.1em; cursor: pointer;">📂</button>
            <button id="btn-up" title="Up directory">⬆</button>
            <span id="path-display" style="font-size: 0.85em; font-weight:bold; color: var(--text-main); max-width: 80px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;">ROOT</span>
          </div>
          <div style="display: flex; gap: 5px;">
            <button id="btn-new-file" title="New File" style="border:none; background:transparent; font-size:1.2em; cursor: pointer;">📄+</button>
            <button id="btn-new-dir" title="New Folder" style="border:none; background:transparent; font-size:1.2em; cursor: pointer;">📁+</button>
            <button id="btn-settings" title="Settings" style="border:none; background:transparent; font-size:1.2em; cursor: pointer;">⚙️</button>
          </div>
        </div>
        <ul id="file-list" style="list-style: none; padding: 0; margin: 0; overflow-y: auto; flex: 1;"></ul>
      </div>

      <div id="resizer"></div>

      <div id="preview-area" style="padding: 0 40px 40px 40px; overflow-y: auto; background-color: var(--bg-main); display: flex; flex-direction: column;">
        </div>

      <div id="context-menu">
        <button id="menu-rename">✏ Rename</button>
        <button id="menu-delete" class="danger">🗑 Delete</button>
      </div>

      <div id="settings-modal">
        <div class="modal-content">
          <div class="modal-header">Settings</div>
          
          <div class="form-group">
            <label>Font Family</label>
            <div id="display-current-font" class="current-value"></div>
            <input type="text" id="input-font" list="font-list" placeholder="🔍 Search or type new font name..." autocomplete="off">
            <datalist id="font-list"></datalist>
            <small style="color: var(--text-muted); display:block; margin-top:5px;">
              ※ Selecting a font will replace the current setting.
            </small>
          </div>

          <div class="form-group">
            <label>Theme</label>
            <button id="btn-toggle-theme" style="width: 100%; text-align: center; padding: 10px;">
              ${isLightMode ? "Switch to Dark Mode 🌙" : "Switch to Light Mode ☀"}
            </button>
          </div>

          <div class="modal-footer">
            <button id="btn-close-settings">Close</button>
            <button id="btn-save-settings" class="primary">Save</button>
          </div>
        </div>
      </div>
    </div>
  `;

  // 要素取得
  const listEl = document.getElementById("file-list")!;
  const pathDisplay = document.getElementById("path-display")!;
  const btnOpenVault = document.getElementById("btn-open-vault")!;
  const btnUp = document.getElementById("btn-up") as HTMLButtonElement;
  const previewArea = document.getElementById("preview-area")!;
  const resizer = document.getElementById("resizer")!;
  const btnNewFile = document.getElementById("btn-new-file")!;
  const btnNewDir = document.getElementById("btn-new-dir")!;
  
  const contextMenu = document.getElementById("context-menu")!;
  const btnRename = document.getElementById("menu-rename")!;
  const btnDelete = document.getElementById("menu-delete")!;
  
  const btnSettings = document.getElementById("btn-settings")!;
  const modalSettings = document.getElementById("settings-modal")!;
  const inputFont = document.getElementById("input-font") as HTMLInputElement;
  const displayCurrentFont = document.getElementById("display-current-font")!;
  const btnToggleTheme = document.getElementById("btn-toggle-theme")!;
  const btnSaveSettings = document.getElementById("btn-save-settings")!;
  const btnCloseSettings = document.getElementById("btn-close-settings")!;


  // --- Welcome画面描画関数 ---
  async function renderWelcome() {
    if (!showWelcome) {
      previewArea.innerHTML = `
        <div style="color: var(--text-muted); text-align: center; margin-top: 20vh;">
          <h2 style="margin-bottom: 10px; color: var(--text-main);">Welcome to Robsidian</h2>
          <p>Select a file to preview.</p>
        </div>`;
      return;
    }

    const htmlContent = await marked(WELCOME_MD);
    previewArea.innerHTML = `
      <div style="max-width: 800px; margin: 0 auto; padding-top: 20px;">
        <div class="markdown-body" style="color: var(--text-main);">
          ${htmlContent}
        </div>
        <div style="margin-top: 40px; padding-top: 20px; border-top: 1px solid var(--border-color); display: flex; align-items: center; gap: 10px;">
          <input type="checkbox" id="cb-dont-show">
          <label for="cb-dont-show" style="color: var(--text-muted); cursor: pointer; user-select: none;">次回から表示しない (Don't show again)</label>
        </div>
      </div>
    `;

    const cb = document.getElementById("cb-dont-show") as HTMLInputElement;
    cb.onchange = () => {
      if (cb.checked) {
        localStorage.setItem("robsidian-hide-welcome", "true");
        showWelcome = false;
      } else {
        localStorage.removeItem("robsidian-hide-welcome");
        showWelcome = true;
      }
    };
  }


  // --- 各種イベントハンドラ ---

  // Vault選択
  btnOpenVault.onclick = async () => {
    try {
      const selected = await open({ directory: true, multiple: false, title: "Select Vault Folder" });
      if (selected && typeof selected === "string") {
        currentPath = selected;
        localStorage.setItem("robsidian-last-path", currentPath);
        await loadFiles(currentPath);
        renderWelcome(); 
      }
    } catch (e) { alert("Failed to open folder: " + e); }
  };

  // 設定系
  btnSettings.onclick = async () => {
    displayCurrentFont.innerText = `Current: ${currentFont}`;
    inputFont.value = "";
    modalSettings.style.display = "flex";
    
    const dataList = document.getElementById("font-list")!;
    if (dataList.children.length === 0) {
      inputFont.placeholder = "Loading fonts...";
      try {
        const fonts = await invoke("get_available_fonts") as string[];
        fonts.forEach(fontName => {
          const option = document.createElement("option");
          option.value = fontName;
          dataList.appendChild(option);
        });
        inputFont.placeholder = "🔍 Search or type new font name...";
      } catch (e) {
        console.error("Failed to load fonts:", e);
        inputFont.placeholder = "Failed to load fonts.";
      }
    }
    inputFont.focus();
  };
  btnCloseSettings.onclick = () => { modalSettings.style.display = "none"; };
  
  btnToggleTheme.onclick = () => {
    isLightMode = !isLightMode;
    if (isLightMode) { document.body.classList.add("light-theme"); btnToggleTheme.innerText = "Switch to Dark Mode 🌙"; localStorage.setItem("robsidian-theme", "light"); }
    else { document.body.classList.remove("light-theme"); btnToggleTheme.innerText = "Switch to Light Mode ☀"; localStorage.setItem("robsidian-theme", "dark"); }
  };
  
  btnSaveSettings.onclick = () => {
    const newFont = inputFont.value.trim();
    if (newFont) { currentFont = newFont; localStorage.setItem("robsidian-font", currentFont); document.documentElement.style.setProperty("--font-family-ui", currentFont); document.documentElement.style.setProperty("--font-family-code", currentFont); }
    modalSettings.style.display = "none";
  };

  // ファイル操作系
  btnNewFile.onclick = async () => {
    const fileName = prompt("Enter new file name:");
    if (!fileName) return;
    const separator = currentPath.includes("/") ? "/" : "\\";
    const fullPath = (currentPath === "." ? "" : currentPath + separator) + fileName;
    try {
      await invoke("create_file", { path: fullPath });
      await loadFiles(currentPath);
      const entries = await invoke("list_files", { path: currentPath }) as FileEntry[];
      const newEntry = entries.find(e => e.name === fileName);
      if(newEntry) showPreview(newEntry);
    } catch (e) { alert("Failed: " + e); }
  };

  btnNewDir.onclick = async () => {
    const dirName = prompt("Enter new folder name:");
    if (!dirName) return;
    const separator = currentPath.includes("/") ? "/" : "\\";
    const fullPath = (currentPath === "." ? "" : currentPath + separator) + dirName;
    try {
      await invoke("create_directory", { path: fullPath });
      await loadFiles(currentPath);
    } catch (e) { alert("Failed to create folder: " + e); }
  };

  // リサイザー
  let isResizing = false;
  resizer.addEventListener("mousedown", () => { isResizing = true; document.body.style.cursor = "col-resize"; resizer.classList.add("resizing"); });
  document.addEventListener("mousemove", (e) => {
    if (!isResizing) return;
    const newWidth = Math.max(150, Math.min(e.clientX, 600));
    document.documentElement.style.setProperty("--sidebar-width", `${newWidth}px`);
  });
  document.addEventListener("mouseup", () => {
    if (isResizing) {
      isResizing = false;
      document.body.style.cursor = "default";
      resizer.classList.remove("resizing");
      const currentWidth = getComputedStyle(document.documentElement).getPropertyValue("--sidebar-width");
      localStorage.setItem("robsidian-sidebar-width", currentWidth);
    }
  });

  document.addEventListener("click", (e) => { if (e.target !== contextMenu && !contextMenu.contains(e.target as Node)) contextMenu.style.display = "none"; });

  btnRename.onclick = async () => {
    if (!selectedEntry) return;
    const newName = prompt("Rename to:", selectedEntry.name);
    if (!newName || newName === selectedEntry.name) return;
    const separator = selectedEntry.path.includes("/") ? "/" : "\\";
    const parentPath = selectedEntry.path.substring(0, selectedEntry.path.lastIndexOf(separator));
    try { await invoke("rename_item", { oldPath: selectedEntry.path, newPath: `${parentPath}${separator}${newName}` }); await loadFiles(currentPath); } catch (e) { alert("Error: " + e); }
  };
  
  btnDelete.onclick = async () => {
    if (!selectedEntry || !confirm(`Delete "${selectedEntry.name}"?`)) return;
    try { await invoke("delete_item", { path: selectedEntry.path }); await loadFiles(currentPath); previewArea.innerHTML = ""; } catch (e) { alert("Error: " + e); }
  };

  async function loadFiles(path: string) {
    try {
      const files = await invoke("list_files", { path }) as FileEntry[];
      currentPath = path;
      pathDisplay.innerText = path === "." ? "ROOT" : path.split("\\").pop() || path;
      btnUp.disabled = (path === "." || path.length < 3);
      btnUp.style.opacity = btnUp.disabled ? "0.3" : "1";
      listEl.innerHTML = "";

      files.forEach((entry) => {
        const li = document.createElement("li");
        li.style.padding = "6px 12px"; li.style.cursor = "pointer"; li.style.fontSize = "0.9em"; li.style.display = "flex"; li.style.alignItems = "center"; li.style.color = "var(--text-main)"; li.style.userSelect = "none";
        const icon = entry.is_dir ? "📂" : "📄";
        const fontWeight = entry.is_dir ? "bold" : "normal";
        li.innerHTML = `<span style="margin-right:8px; opacity:0.8;">${icon}</span> <span style="font-weight:${fontWeight}; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;">${entry.name}</span>`;
        li.onmouseover = () => { li.style.backgroundColor = "var(--bg-hover)"; };
        li.onmouseout = () => { li.style.backgroundColor = "transparent"; };
        li.oncontextmenu = (e) => { e.preventDefault(); e.stopPropagation(); selectedEntry = entry; contextMenu.style.display = "flex"; contextMenu.style.left = `${e.pageX}px`; contextMenu.style.top = `${e.pageY}px`; };
        li.onclick = () => { if (entry.is_dir) loadFiles(entry.path); else showPreview(entry); };
        listEl.appendChild(li);
      });
    } catch (e) { console.error(e); }
  }

  async function showPreview(entry: FileEntry) {
    isEditing = false;
    selectedEntry = entry;
    
    previewArea.innerHTML = "<p style='margin-top:20px; color:var(--text-muted);'>Loading...</p>";
    let content = "";
    try {
      content = await invoke("read_file_content", { path: entry.path }) as string;
    } catch (e) {
      previewArea.innerHTML = `<div style="margin-top:20px; color: #ff6b6b;">Cannot read this file.</div>`;
      return;
    }

    const render = async () => {
      if (isEditing) {
        previewArea.innerHTML = `
          <div style="position: sticky; top: 0; background: var(--bg-main); z-index: 10; padding: 10px 0; border-bottom: 1px solid var(--border-color); margin-bottom: 10px; display: flex; justify-content: space-between; align-items: center;">
            <h2 style="margin: 0; font-size: 1.2em; color: var(--text-main);">${entry.name} (Editing)</h2>
            <div style="display:flex; gap:10px;">
              <button id="btn-cancel">Cancel</button>
              <button id="btn-save" class="primary">Save</button>
            </div>
          </div>
          <textarea id="editor-textarea" spellcheck="false"></textarea>
        `;
        const textarea = document.getElementById("editor-textarea") as HTMLTextAreaElement;
        textarea.value = content;
        textarea.focus();
        document.getElementById("btn-save")!.onclick = async () => {
          try {
            const newContent = textarea.value;
            await invoke("save_file_content", { path: entry.path, content: newContent });
            content = newContent; isEditing = false; render();
          } catch (e) { alert("Save failed: " + e); }
        };
        document.getElementById("btn-cancel")!.onclick = () => { isEditing = false; render(); };

      } else {
        let displayHtml = "";
        if (entry.name.endsWith(".md")) {
          displayHtml = await marked(content);
        } else {
          const escaped = content.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
          displayHtml = `<pre><code>${escaped}</code></pre>`;
        }
        previewArea.innerHTML = `
          <div style="position: sticky; top: 0; background: var(--bg-main); z-index: 10; padding: 20px 0; border-bottom: 1px solid var(--border-color); margin-bottom: 20px; display: flex; justify-content: space-between; align-items: center;">
            <h2 style="margin: 0; font-size: 1.2em; color: var(--text-main); overflow: hidden; text-overflow: ellipsis;">${entry.name}</h2>
            <div style="display:flex; gap:10px;">
              <button id="btn-edit-internal" title="Edit in App">✏ Edit</button>
              <button id="btn-edit-helix" class="primary" title="Edit in Helix">Hx</button>
            </div>
          </div>
          <div class="markdown-body" style="padding-bottom: 50px; color: var(--text-main);">
            ${displayHtml}
          </div>
        `;
        document.getElementById("btn-edit-internal")!.onclick = () => { isEditing = true; render(); };
        document.getElementById("btn-edit-helix")!.onclick = () => { invoke("open_in_helix", { path: entry.path }); };
      }
    };
    render();
  }

  btnUp.onclick = () => {
    if (currentPath === "." || currentPath.length < 3) return;
    const parentPath = currentPath.substring(0, Math.max(currentPath.lastIndexOf("\\"), currentPath.lastIndexOf("/")));
    loadFiles(parentPath || ".");
  };

  // 初回実行: 
  await loadFiles(currentPath);
  renderWelcome();
});