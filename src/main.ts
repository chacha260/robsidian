import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { readFile } from "@tauri-apps/plugin-fs";
import { marked } from "marked";
import { RobsidianTerminal } from "./terminal";
import "./styles.css";

// --- CodeMirror Imports ---
import { EditorView, basicSetup } from "codemirror";
import { keymap } from "@codemirror/view";
import { defaultKeymap, historyKeymap } from "@codemirror/commands";
import { markdown } from "@codemirror/lang-markdown";
import { oneDark } from "@codemirror/theme-one-dark";
import { autocompletion, CompletionContext } from "@codemirror/autocomplete";

// --- Highlight.js Imports ---
import hljs from 'highlight.js';
import 'highlight.js/styles/atom-one-dark.css';

// --- グローバル変数 ---
let draggingTabState: { paneId: string; tabId: string } | null = null;
let saveSessionTimeout: number | null = null;
const expandedPaths = new Set<string>();
let currentZoom = parseFloat(localStorage.getItem("robsidian-zoom") || "1.0");
let currentFilesCache: FileEntry[] = []; 

// ★修正1: loadFilesをクラスから呼べるようにするためのグローバル参照
let loadFilesGlobal: (path: string) => Promise<void> = async () => {}; 

declare global {
  interface Window {
    robsidianActiveTerm: RobsidianTerminal | undefined;
    robsidianOpenHelix: (entry: FileEntry) => void;
  }
}

interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
}

interface SearchResult {
  path: string;
  name: string;
  is_dir: boolean;
  snippet: string;
}

interface EditorTab {
    id: string;
    file: FileEntry;
    contentCache: string;
    scrollTop: number;
    isEditing: boolean;
    isDirty: boolean;
}

interface SavedTabState { path: string; name: string; scrollTop: number; }
interface SavedPaneState { width: string; flex: string; tabs: SavedTabState[]; activeTabIndex: number; }
interface SavedSession {
    panes: SavedPaneState[];
    sidebarWidth: string;
    sidebarClosed: boolean;
}

// --- Helper Functions ---

function applyZoom(zoom: number) {
    currentZoom = Math.min(Math.max(zoom, 0.5), 3.0); 
    const app = document.getElementById("app");
    if (app) {
        document.documentElement.style.setProperty("--ui-zoom", currentZoom.toString());
        app.style.transform = `scale(${currentZoom})`;
        app.style.width = `calc(100vw / ${currentZoom})`;
        app.style.height = `calc(100vh / ${currentZoom})`;
    }
    localStorage.setItem("robsidian-zoom", currentZoom.toString());
}

window.addEventListener("resize", () => { applyZoom(currentZoom); });

const loadImagesInElement = async (element: HTMLElement) => {
    const images = element.querySelectorAll('img[data-local-path]');
    for (const img of images) {
        const target = img as HTMLImageElement;
        const path = target.getAttribute('data-local-path');
        if (!path) continue;
        try {
            const contents = await readFile(path);
            const blob = new Blob([contents]);
            target.src = URL.createObjectURL(blob);
            target.style.opacity = '1';
        } catch (e) { target.alt += " (Img Not Found)"; }
    }
};

// --- Marked Config ---
const renderer = new marked.Renderer();
renderer.image = ({ href, title, text }: { href: string, title: string | null, text: string }) => {
    if (!href) return "";
    const titleAttr = title ? ` title="${title}"` : "";
    if (href.startsWith("http") || href.startsWith("data:")) {
        return `<img src="${href}" alt="${text}"${titleAttr} style="max-width: 100%; border-radius: 4px;">`;
    }
    return `<img data-local-path="${href}" alt="${text}"${titleAttr} style="max-width: 100%; border-radius: 4px; opacity: 0.5;">`;
};
renderer.code = ({ text, lang }: { text: string, lang?: string }) => {
    const language = lang && hljs.getLanguage(lang) ? lang : 'plaintext';
    const highlighted = hljs.highlight(text, { language }).value;
    return `<pre><code class="hljs language-${language}">${highlighted}</code></pre>`;
};

const wikiLinkExtension = {
    name: 'wikiLink',
    level: 'inline',
    start(src: string) { return src.match(/\[\[/)?.index; },
    tokenizer(src: string) {
        const rule = /^\[\[([^\]]+)\]\]/;
        const match = rule.exec(src);
        if (match) {
            return {
                type: 'wikiLink',
                raw: match[0],
                text: match[1].trim(),
            };
        }
    },
    renderer(token: any) {
        return `<span class="wiki-link" data-target="${token.text}">${token.text}</span>`;
    }
};

marked.use({ 
    renderer, 
    extensions: [wikiLinkExtension as any] 
});


// --- CodeMirror Completion Logic ---
function wikiLinkCompletion(context: CompletionContext) {
    const word = context.matchBefore(/\[\[[^\]]*/);
    if (!word) return null;
    if (word.from == word.to && !context.explicit) return null;
    
    return {
        from: word.from + 2, 
        options: currentFilesCache
            .filter(f => !f.is_dir) 
            .map(f => ({
                label: f.name.replace(/\.md$/, ''),
                type: "text",
                apply: f.name.replace(/\.md$/, '') 
            }))
    };
}


// --- クラス定義 ---
class EditorPane {
    public id: string;
    public container: HTMLElement;
    public tabs: EditorTab[] = [];
    public activeTabId: string | null = null;
    private headerEl: HTMLElement;
    private contentArea: HTMLElement;
    private cmEditor: EditorView | null = null;

    constructor(private manager: PaneManager) {
        this.id = Math.random().toString(36).substring(2, 9);
        this.container = document.createElement("div");
        this.container.className = "pane";
        this.container.innerHTML = `<div class="pane-header"></div><div class="pane-content markdown-body"></div>`;
        this.headerEl = this.container.querySelector(".pane-header") as HTMLElement;
        this.contentArea = this.container.querySelector(".pane-content") as HTMLElement;
        this.container.addEventListener("mousedown", () => { this.manager.setActive(this); });
        this.renderEmpty();
        
        this.contentArea.addEventListener('click', async (e) => {
            const target = e.target as HTMLElement;
            if (target.classList.contains('wiki-link')) {
                e.preventDefault();
                e.stopPropagation();
                
                const linkTarget = target.getAttribute('data-target');
                if (linkTarget) {
                    const targetName = linkTarget.toLowerCase();
                    
                    let match = currentFilesCache.find(f => 
                        !f.is_dir && (
                            f.name.toLowerCase() === targetName || 
                            f.name.toLowerCase() === `${targetName}.md`
                        )
                    );

                    if (match) {
                        this.manager.openFileInActive({ name: match.name, path: match.path, is_dir: false });
                    } else {
                        if(confirm(`File "${linkTarget}" not found. Create it?`)) {
                            const root = localStorage.getItem("robsidian-last-path") || ".";
                            const separator = root.includes("/") ? "/" : "\\";
                            const newPath = `${root}${separator}${linkTarget}.md`;
                            
                            try {
                                await invoke("create_file", { path: newPath });
                                // ★修正2: グローバル変数経由で呼び出す
                                await loadFilesGlobal(root);
                                this.manager.openFileInActive({ name: `${linkTarget}.md`, path: newPath, is_dir: false });
                            } catch(err) {
                                alert("Failed to create file: " + err);
                            }
                        }
                    }
                }
            }
        });
    }

    public toJSON(): SavedPaneState {
        if (this.cmEditor && this.activeTabId) {
            const currentTab = this.tabs.find(t => t.id === this.activeTabId);
            if(currentTab) currentTab.contentCache = this.cmEditor.state.doc.toString();
        }
        return {
            width: this.container.style.width,
            flex: this.container.style.flex,
            tabs: this.tabs.map(t => ({
                path: t.file.path, name: t.file.name,
                scrollTop: this.activeTabId === t.id ? this.contentArea.scrollTop : t.scrollTop
            })),
            activeTabIndex: this.activeTabId ? this.tabs.findIndex(t => t.id === this.activeTabId) : -1
        };
    }

    public setActive(isActive: boolean) {
        if (isActive) this.container.classList.add("active");
        else this.container.classList.remove("active");
    }

    public async openFile(entry: FileEntry, initialScrollTop: number = 0) {
        const existingTab = this.tabs.find(t => t.file.path === entry.path);
        if (existingTab) { this.switchTab(existingTab.id); return; }
        let content = "";
        try { content = await invoke("read_file_content", { path: entry.path }) as string; } 
        catch (e) { content = `Cannot read file: ${entry.path}`; }
        const newTab: EditorTab = {
            id: Math.random().toString(36).substring(2, 9),
            file: entry, contentCache: content, scrollTop: initialScrollTop, isEditing: false, isDirty: false
        };
        this.tabs.push(newTab);
        this.switchTab(newTab.id);
        saveSession(); 
    }

    public switchTab(tabId: string) {
        if (this.cmEditor && this.activeTabId) {
            const currentTab = this.tabs.find(t => t.id === this.activeTabId);
            if (currentTab) currentTab.contentCache = this.cmEditor.state.doc.toString();
            this.cmEditor.destroy(); this.cmEditor = null;
        } else if (this.activeTabId) {
            const prevTab = this.tabs.find(t => t.id === this.activeTabId);
            if (prevTab) prevTab.scrollTop = this.contentArea.scrollTop;
        }
        this.activeTabId = tabId;
        this.renderHeader();
        this.renderContent();
        saveSession();
    }

    public closeTab(tabId: string) {
        const tabIndex = this.tabs.findIndex(t => t.id === tabId);
        if (tabIndex === -1) return;
        this.tabs.splice(tabIndex, 1);
        if (this.activeTabId === tabId) {
            this.activeTabId = null;
            if (this.tabs.length > 0) {
                const nextTab = this.tabs[Math.min(tabIndex, this.tabs.length - 1)];
                this.switchTab(nextTab.id);
            } else {
                this.renderHeader();
                this.renderEmpty();
            }
        } else {
            this.renderHeader();
        }
        saveSession();
    }

    private renderHeader() {
        this.headerEl.innerHTML = "";
        this.tabs.forEach((tab) => {
            const tabEl = document.createElement("div");
            tabEl.className = `editor-tab ${tab.id === this.activeTabId ? "active" : ""} ${tab.isDirty ? "dirty" : ""}`;
            tabEl.title = tab.file.path;
            tabEl.draggable = true;
            tabEl.innerHTML = `<span class="tab-title">${tab.file.name}</span><div class="tab-close">×</div>`;
            tabEl.addEventListener("click", (e) => { if ((e.target as HTMLElement).classList.contains("tab-close")) { e.stopPropagation(); this.closeTab(tab.id); } else { this.switchTab(tab.id); } });
            tabEl.addEventListener("dragstart", (e) => { draggingTabState = { paneId: this.id, tabId: tab.id }; e.dataTransfer!.effectAllowed = "move"; e.dataTransfer!.setData("text/plain", tab.id); requestAnimationFrame(() => tabEl.classList.add("dragging")); });
            tabEl.addEventListener("dragend", () => { tabEl.classList.remove("dragging"); draggingTabState = null; this.headerEl.querySelectorAll(".drag-over").forEach(el => el.classList.remove("drag-over")); });
            tabEl.addEventListener("dragover", (e) => { e.preventDefault(); if (draggingTabState && draggingTabState.paneId === this.id) { e.dataTransfer!.dropEffect = "move"; if (draggingTabState.tabId !== tab.id) tabEl.classList.add("drag-over"); } });
            tabEl.addEventListener("dragleave", () => { tabEl.classList.remove("drag-over"); });
            tabEl.addEventListener("drop", (e) => {
                e.preventDefault(); tabEl.classList.remove("drag-over");
                if (!draggingTabState || draggingTabState.paneId !== this.id) return;
                const sourceId = draggingTabState.tabId; const targetId = tab.id;
                if (sourceId === targetId) return;
                const sourceIndex = this.tabs.findIndex(t => t.id === sourceId); const targetIndex = this.tabs.findIndex(t => t.id === targetId);
                if (sourceIndex > -1 && targetIndex > -1) { const [movedTab] = this.tabs.splice(sourceIndex, 1); this.tabs.splice(targetIndex, 0, movedTab); this.renderHeader(); saveSession(); }
            });
            this.headerEl.appendChild(tabEl);
        });
    }

    private async renderContent() {
        const tab = this.tabs.find(t => t.id === this.activeTabId);
        if (!tab) { this.renderEmpty(); return; }
        this.contentArea.innerHTML = "";

        if (tab.isEditing) {
            this.contentArea.innerHTML = `
                <div class="editor-container">
                    <div style="padding-bottom:5px; padding-right:10px; display:flex; justify-content:flex-end; border-bottom:1px solid var(--border-color); background:var(--bg-sidebar);">
                        <button class="btn-cancel" style="margin-right:10px; background:none; border:none; color:#888; cursor:pointer;">Preview (Esc)</button>
                        <button class="btn-save primary" style="background:var(--accent-color); color:#fff; border:none; padding:4px 10px; border-radius:4px; cursor:pointer;">Save (Ctrl+S)</button>
                    </div>
                    <div class="editor-wrapper"></div>
                </div>
            `;
            const wrapper = this.contentArea.querySelector(".editor-wrapper") as HTMLElement;
            const isLightMode = document.body.classList.contains("light-theme");
            const extensions = [
                basicSetup, markdown(), 
                autocompletion({ override: [wikiLinkCompletion] }), 
                keymap.of([ ...defaultKeymap, ...historyKeymap, 
                    { key: "Mod-s", run: () => { save(); return true; } },
                    { key: "Escape", run: () => { tab.contentCache = this.cmEditor!.state.doc.toString(); tab.isEditing = false; this.renderContent(); return true; } }
                ]),
                EditorView.updateListener.of((update) => { if (update.docChanged) tab.isDirty = true; })
            ];
            if (!isLightMode) extensions.push(oneDark);
            this.cmEditor = new EditorView({ doc: tab.contentCache, extensions: extensions, parent: wrapper });
            this.cmEditor.focus();
            const save = async () => {
                try {
                    const newContent = this.cmEditor!.state.doc.toString();
                    await invoke("save_file_content", { path: tab.file.path, content: newContent });
                    tab.contentCache = newContent; tab.isEditing = false; tab.isDirty = false; this.cmEditor!.destroy(); this.cmEditor = null; this.renderContent();
                } catch(e) { alert("Save failed: " + e); }
            };
            this.contentArea.querySelector(".btn-save")?.addEventListener("click", save);
            this.contentArea.querySelector(".btn-cancel")?.addEventListener("click", () => { if(this.cmEditor) tab.contentCache = this.cmEditor.state.doc.toString(); tab.isEditing = false; this.cmEditor?.destroy(); this.cmEditor = null; this.renderContent(); });
        } else {
            const toolbar = document.createElement("div");
            toolbar.style.cssText = "position:sticky; top:0; background:var(--bg-main); z-index:10; padding-bottom:10px; border-bottom:1px solid var(--border-color); margin-bottom:20px; display:flex; justify-content:space-between; align-items:center;";
            toolbar.innerHTML = `
                <div style="font-size:0.8em; color:var(--text-muted); padding-left:20px;">${tab.file.path}</div>
                <div style="display:flex; gap:5px; padding-right:20px;">
                    <button class="pane-btn edit-internal" title="Edit">✏ Edit</button>
                    <button class="pane-btn edit-helix primary" title="Hx">Hx</button>
                </div>
            `;
            toolbar.querySelector(".edit-internal")?.addEventListener("click", () => { tab.isEditing = true; this.renderContent(); });
            toolbar.querySelector(".edit-helix")?.addEventListener("click", () => { if(window.robsidianOpenHelix) window.robsidianOpenHelix(tab.file); });
            this.contentArea.appendChild(toolbar);
            const body = document.createElement("div");
            body.className = "markdown-body"; 
            if (tab.file.name.endsWith(".md")) { body.innerHTML = await marked(tab.contentCache); }
            else { const escaped = tab.contentCache.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;"); body.innerHTML = `<pre><code>${escaped}</code></pre>`; }
            this.contentArea.appendChild(body);
            loadImagesInElement(body);
            setTimeout(() => { this.contentArea.scrollTop = tab.scrollTop; }, 0);
        }
    }

    public async renderEmpty() {
        this.activeTabId = null;
        this.headerEl.innerHTML = "";
        if (this.manager.panes.indexOf(this) === 0 && this.manager.panes.length === 1) {
             const html = await marked(`# Robsidian 💎\n**WikiLinks Enabled!**\n- Type \`[[\` to see autocomplete.\n- Click blue links to jump to files.`);
             this.contentArea.innerHTML = `<div style="padding:20px;">${html}</div>`;
        } else {
            this.contentArea.innerHTML = `<div class="pane-empty"><div style="font-size:2em; opacity:0.3;">◫</div><button class="btn-close-pane" style="margin-top:20px; background:none; border:1px solid #444; color:#888; padding:5px 10px; cursor:pointer;">Close Pane</button></div>`;
            this.contentArea.querySelector(".btn-close-pane")?.addEventListener("click", () => { this.manager.removePane(this); });
        }
    }
    public destroy() { if(this.cmEditor) this.cmEditor.destroy(); this.container.remove(); }
}

class PaneManager {
    public panes: EditorPane[] = [];
    public activePane: EditorPane | null = null;
    private container: HTMLElement;
    constructor(containerId: string) { const el = document.getElementById(containerId); if (!el) throw new Error(`Container ${containerId} not found`); this.container = el; }
    public addPane() {
        const pane = new EditorPane(this);
        if (this.panes.length > 0) { const resizer = document.createElement("div"); resizer.className = "pane-resizer"; this.container.appendChild(resizer); this.initResizer(resizer); }
        this.panes.push(pane); this.container.appendChild(pane.container); this.setActive(pane); saveSession(); return pane;
    }
    public removePane(pane: EditorPane) {
        if (this.panes.length <= 1) { pane.tabs = []; pane.renderEmpty(); this.resetLayout(); saveSession(); return; }
        const idx = this.panes.indexOf(pane);
        const prev = pane.container.previousElementSibling;
        if (prev && prev.classList.contains("pane-resizer")) prev.remove(); else { const next = pane.container.nextElementSibling; if (next && next.classList.contains("pane-resizer")) next.remove(); }
        pane.destroy(); this.panes.splice(idx, 1); this.resetLayout();
        if (this.activePane === pane) { const nextActive = this.panes[Math.min(idx, this.panes.length - 1)]; this.setActive(nextActive); } saveSession();
    }
    private resetLayout() { Array.from(this.container.children).forEach((child) => { if (child.classList.contains("pane")) { const el = child as HTMLElement; el.style.flex = "1"; el.style.width = ""; } }); }
    public setActive(pane: EditorPane) { this.panes.forEach(p => p.setActive(false)); this.activePane = pane; pane.setActive(true); }
    public openFileInActive(entry: FileEntry) { if (!this.activePane) this.addPane(); this.activePane!.openFile(entry); }
    public saveState(): SavedSession {
        const sidebarWidth = getComputedStyle(document.documentElement).getPropertyValue("--sidebar-width");
        const sidebarClosed = document.body.classList.contains("sidebar-closed");
        return { panes: this.panes.map(p => p.toJSON()), sidebarWidth, sidebarClosed };
    }
    private initResizer(resizer: HTMLElement) {
        let isResizing = false;
        resizer.addEventListener("mousedown", (e) => {
            isResizing = true;
            document.body.style.cursor = "col-resize";
            resizer.classList.add("resizing");
            const prev = resizer.previousElementSibling as HTMLElement;
            const next = resizer.nextElementSibling as HTMLElement;
            if (!prev || !next) return;
            const startX = e.clientX; 
            const startPrevWidth = prev.getBoundingClientRect().width;
            const startNextWidth = next.getBoundingClientRect().width;
            Array.from(this.container.children).forEach((child) => { if (child.classList.contains("pane")) { (child as HTMLElement).style.flex = "none"; (child as HTMLElement).style.width = `${child.getBoundingClientRect().width}px`; } });
            
            const onMouseMove = (moveEvent: MouseEvent) => {
                if (!isResizing) return;
                const delta = (moveEvent.clientX - startX) / currentZoom;
                const logicalPrev = startPrevWidth / currentZoom;
                const logicalNext = startNextWidth / currentZoom;
                const newPrev = Math.max(150, logicalPrev + delta);
                const newNext = Math.max(150, logicalNext - delta);
                if (newPrev > 150 && newNext > 150) { prev.style.width = `${newPrev}px`; next.style.width = `${newNext}px`; }
            };
            const onMouseUp = () => {
                isResizing = false; document.body.style.cursor = "default"; resizer.classList.remove("resizing");
                document.removeEventListener("mousemove", onMouseMove); document.removeEventListener("mouseup", onMouseUp);
                saveSession();
            };
            document.addEventListener("mousemove", onMouseMove); document.addEventListener("mouseup", onMouseUp);
        });
    }
}

let globalPaneManager: PaneManager | null = null;
function saveSession() {
    if (!globalPaneManager) return;
    if (saveSessionTimeout) clearTimeout(saveSessionTimeout);
    saveSessionTimeout = window.setTimeout(() => { const state = globalPaneManager!.saveState(); localStorage.setItem("robsidian-session", JSON.stringify(state)); }, 1000);
}
async function restoreSession(manager: PaneManager) {
    const json = localStorage.getItem("robsidian-session");
    if (!json) { manager.addPane(); return; }
    try {
        const session: SavedSession = JSON.parse(json);
        if (session.sidebarWidth) document.documentElement.style.setProperty("--sidebar-width", session.sidebarWidth);
        if (session.sidebarClosed) document.body.classList.add("sidebar-closed");
        if (!session.panes || session.panes.length === 0) { manager.addPane(); return; }
        for (const savedPane of session.panes) {
            const pane = manager.addPane();
            if (savedPane.width) pane.container.style.width = savedPane.width;
            if (savedPane.flex) pane.container.style.flex = savedPane.flex;
            for (const savedTab of savedPane.tabs) { await pane.openFile({ name: savedTab.name, path: savedTab.path, is_dir: false }, savedTab.scrollTop); }
            if (savedPane.activeTabIndex >= 0 && savedPane.activeTabIndex < pane.tabs.length) { pane.switchTab(pane.tabs[savedPane.activeTabIndex].id); }
        }
    } catch (e) { console.error("Failed to restore session:", e); manager.addPane(); }
}

window.addEventListener("DOMContentLoaded", async () => {
  const getEl = (id: string) => document.getElementById(id);
  const btnToggleSidebar = getEl("btn-toggle-sidebar");
  const btnOpenVault = getEl("btn-open-vault");
  const btnUp = getEl("btn-up") as HTMLButtonElement; 
  const inputSearch = getEl("input-search") as HTMLInputElement;
  const btnSplit = getEl("btn-split");
  const listEl = getEl("file-list");
  const pathDisplay = getEl("path-display");
  const btnSettings = getEl("btn-settings");
  const modalSettings = getEl("settings-modal");
  const inputFont = getEl("input-font") as HTMLInputElement;
  const displayCurrentFont = getEl("display-current-font");
  const btnToggleTheme = getEl("btn-toggle-theme");
  const btnSaveSettings = getEl("btn-save-settings");
  const btnCloseSettings = getEl("btn-close-settings");
  const btnResetLayout = getEl("btn-reset-layout");
  const btnNewFile = getEl("btn-new-file");
  const btnNewDir = getEl("btn-new-dir");
  const contextMenu = getEl("context-menu");
  const btnRename = getEl("menu-rename");
  const btnDelete = getEl("menu-delete");
  const tabsContainer = getEl("tabs-container");
  const btnAddTab = getEl("btn-add-tab");
  const resizer = getEl("resizer");
  const inputFontSize = getEl("input-font-size") as HTMLInputElement;
  const inputLineHeight = getEl("input-line-height") as HTMLInputElement;
  const valFontSize = getEl("val-font-size");
  const valLineHeight = getEl("val-line-height");

  applyZoom(currentZoom);

  window.addEventListener("wheel", (e) => {
      if (e.ctrlKey) { e.preventDefault(); const delta = -e.deltaY; const newZoom = currentZoom + (delta > 0 ? 0.1 : -0.1); applyZoom(newZoom); }
  }, { passive: false });

  window.addEventListener("keydown", (e) => {
      if (e.ctrlKey) {
          if (e.key === "=" || e.key === "+") { e.preventDefault(); applyZoom(currentZoom + 0.1); } 
          else if (e.key === "-") { e.preventDefault(); applyZoom(currentZoom - 0.1); } 
          else if (e.key === "0") { e.preventDefault(); applyZoom(1.0); }
      }
  });

  let isResizing = false;
  if(resizer) {
      resizer.addEventListener("mousedown", () => { isResizing = true; document.body.style.cursor = "col-resize"; resizer.classList.add("resizing"); });
      document.addEventListener("mousemove", (e) => { if (!isResizing) return; const rawX = e.clientX / currentZoom; const newWidth = Math.max(150, Math.min(rawX - 50, 600)); document.documentElement.style.setProperty("--sidebar-width", `${newWidth}px`); });
      document.addEventListener("mouseup", () => { if (isResizing) { isResizing = false; document.body.style.cursor = "default"; resizer.classList.remove("resizing"); saveSession(); } });
  }
  if(btnToggleSidebar) btnToggleSidebar.onclick = () => { document.body.classList.toggle("sidebar-closed"); saveSession(); };

  const savedTheme = localStorage.getItem("robsidian-theme");
  if (savedTheme === "light") { document.body.classList.add("light-theme"); if(btnToggleTheme) btnToggleTheme.innerText = "Switch to Dark Mode 🌙"; }
  const defaultFont = "'HackGen', 'HackGen Console', sans-serif";
  let currentFont = localStorage.getItem("robsidian-font") || defaultFont;
  let currentFontSize = localStorage.getItem("robsidian-font-size") || "14";
  let currentLineHeight = localStorage.getItem("robsidian-line-height") || "1.5";
  document.documentElement.style.setProperty("--font-family-ui", currentFont);
  document.documentElement.style.setProperty("--font-family-code", currentFont);
  document.documentElement.style.setProperty("--base-font-size", `${currentFontSize}px`);
  document.documentElement.style.setProperty("--base-line-height", currentLineHeight);

  if(btnSettings) btnSettings.onclick = () => { 
      if(displayCurrentFont) displayCurrentFont.innerText = `Current: ${currentFont}`;
      if(inputFontSize) inputFontSize.value = currentFontSize;
      if(valFontSize) valFontSize.innerText = currentFontSize;
      if(inputLineHeight) inputLineHeight.value = currentLineHeight;
      if(valLineHeight) valLineHeight.innerText = currentLineHeight;
      if(modalSettings) modalSettings.style.display = "flex"; 
  };
  if(btnCloseSettings) btnCloseSettings.onclick = () => { if(modalSettings) modalSettings.style.display = "none"; };
  if(btnToggleTheme) btnToggleTheme.onclick = () => {
    if (document.body.classList.contains("light-theme")) { document.body.classList.remove("light-theme"); btnToggleTheme.innerText = "Switch to Light Mode ☀"; localStorage.setItem("robsidian-theme", "dark"); }
    else { document.body.classList.add("light-theme"); btnToggleTheme.innerText = "Switch to Dark Mode 🌙"; localStorage.setItem("robsidian-theme", "light"); }
  };
  if(inputFontSize) inputFontSize.oninput = () => { if(valFontSize) valFontSize.innerText = inputFontSize.value; };
  if(inputLineHeight) inputLineHeight.oninput = () => { if(valLineHeight) valLineHeight.innerText = inputLineHeight.value; };
  if(btnSaveSettings) btnSaveSettings.onclick = () => { 
      if(inputFont && inputFont.value) { currentFont = inputFont.value; document.documentElement.style.setProperty("--font-family-ui", currentFont); document.documentElement.style.setProperty("--font-family-code", currentFont); localStorage.setItem("robsidian-font", currentFont); } 
      if(inputFontSize) { currentFontSize = inputFontSize.value; document.documentElement.style.setProperty("--base-font-size", `${currentFontSize}px`); localStorage.setItem("robsidian-font-size", currentFontSize); }
      if(inputLineHeight) { currentLineHeight = inputLineHeight.value; document.documentElement.style.setProperty("--base-line-height", currentLineHeight); localStorage.setItem("robsidian-line-height", currentLineHeight); }
      if(modalSettings) modalSettings.style.display = "none"; 
  };
  if(btnResetLayout) btnResetLayout.onclick = () => { if(confirm("Are you sure?")) { localStorage.clear(); location.reload(); } };

  if(btnOpenVault) btnOpenVault.onclick = async () => { const selected = await open({ directory: true }); if (selected && typeof selected === "string") { localStorage.setItem("robsidian-last-path", selected); await loadFiles(selected); } };
  let currentPath = localStorage.getItem("robsidian-last-path") || ".";
  if(btnUp) btnUp.onclick = () => { if (currentPath.length > 3) { const parent = currentPath.substring(0, Math.max(currentPath.lastIndexOf("\\"), currentPath.lastIndexOf("/"))); loadFiles(parent || "."); } };
  if(btnNewFile) btnNewFile.onclick = async () => { const f = prompt("Name:"); if(!f) return; try{ await invoke("create_file",{path:(currentPath==="."?"":currentPath+(currentPath.includes("/")?"/":"\\"))+f}); await loadFiles(currentPath); globalPaneManager?.openFileInActive({name:f,path:(currentPath==="."?"":currentPath+(currentPath.includes("/")?"/":"\\"))+f,is_dir:false}); }catch(e){alert(e);} };
  if(btnNewDir) btnNewDir.onclick = async () => { const d = prompt("Name:"); if(!d) return; try{ await invoke("create_directory",{path:(currentPath==="."?"":currentPath+(currentPath.includes("/")?"/":"\\"))+d}); await loadFiles(currentPath); }catch(e){alert(e);} };
  
  let searchTimeout: number | null = null;
  if(inputSearch && listEl) {
      inputSearch.addEventListener("input", () => {
          const query = inputSearch.value.trim();
          if(searchTimeout) clearTimeout(searchTimeout);
          searchTimeout = window.setTimeout(async () => {
              if(!query) { loadFiles(currentPath); return; }
              const results = await invoke("search_files", { rootPath: currentPath, query }) as SearchResult[];
              listEl.innerHTML = ""; if(results.length===0) listEl.innerHTML = "<li>No results</li>";
              results.forEach(r => {
                  const li = document.createElement("li"); li.style.padding = "5px"; li.style.borderBottom="1px solid var(--border-color)"; li.style.cursor="pointer"; li.innerHTML = `<div>📄 <b>${r.name}</b></div><div style="font-size:0.8em;color:#888">${r.snippet}</div>`;
                  li.onclick = () => globalPaneManager?.openFileInActive({ name: r.name, path: r.path, is_dir: false }); listEl.appendChild(li);
              });
          }, 300);
      });
  }

  let selectedEntryGlobal: FileEntry | null = null;
  document.addEventListener("click", (e) => { if (contextMenu && e.target !== contextMenu && !contextMenu.contains(e.target as Node)) contextMenu.style.display = "none"; });
  if(btnRename) btnRename.onclick = async () => { if (!selectedEntryGlobal) return; const n = prompt("Rename:", selectedEntryGlobal.name); if (!n) return; try { await invoke("rename_item", { oldPath: selectedEntryGlobal.path, newPath: selectedEntryGlobal.path.replace(selectedEntryGlobal.name, n) }); await loadFiles(currentPath); if(contextMenu) contextMenu.style.display = "none"; } catch(e){alert(e);} };
  if(btnDelete) btnDelete.onclick = async () => { if (!selectedEntryGlobal) return; if(confirm("Delete?")) try { await invoke("delete_item", { path: selectedEntryGlobal.path }); await loadFiles(currentPath); if(contextMenu) contextMenu.style.display = "none"; } catch(e){alert(e);} };

  function createTreeNode(entry: FileEntry): HTMLLIElement {
      const li = document.createElement("li"); li.className = "file-item";
      const row = document.createElement("div"); row.className = "tree-node";
      const arrow = document.createElement("span");
      if (entry.is_dir) { arrow.className = "tree-arrow"; arrow.innerText = "▶"; if (expandedPaths.has(entry.path)) arrow.classList.add("open"); } else { arrow.className = "tree-spacer"; }
      row.appendChild(arrow);
      const icon = document.createElement("span"); icon.className = "tree-icon"; icon.innerText = entry.is_dir ? "📂" : "📄"; row.appendChild(icon);
      const label = document.createElement("span"); label.className = "tree-label"; label.innerText = entry.name; row.appendChild(label);
      li.appendChild(row);
      let childrenContainer: HTMLUListElement | null = null;
      if (entry.is_dir) {
          childrenContainer = document.createElement("ul"); childrenContainer.className = "tree-children";
          if (expandedPaths.has(entry.path)) { childrenContainer.classList.add("show"); toggleDirectory(entry, arrow, childrenContainer, true); }
          li.appendChild(childrenContainer);
      }
      row.onclick = (e) => { e.stopPropagation(); document.querySelectorAll(".tree-node.selected").forEach(el => el.classList.remove("selected")); row.classList.add("selected"); if (entry.is_dir && childrenContainer) { toggleDirectory(entry, arrow, childrenContainer); } else { globalPaneManager?.openFileInActive(entry); } };
      row.oncontextmenu = (e) => { e.preventDefault(); e.stopPropagation(); selectedEntryGlobal = entry; if(contextMenu) { contextMenu.style.display = "flex"; contextMenu.style.left = `${e.pageX}px`; contextMenu.style.top = `${e.pageY}px`; } };
      return li;
  }

  async function toggleDirectory(entry: FileEntry, arrow: HTMLElement, container: HTMLUListElement, forceOpen = false) {
      const isClosed = !container.classList.contains("show");
      if (isClosed || forceOpen) {
          container.classList.add("show"); arrow.classList.add("open"); expandedPaths.add(entry.path);
          if (container.children.length === 0) {
              container.innerHTML = `<li style="padding-left:20px; color:#666;">Loading...</li>`;
              try {
                  const files = await invoke("list_files", { path: entry.path }) as FileEntry[];
                  // ★修正3: ディレクトリ展開時もキャッシュに追加
                  currentFilesCache = [...currentFilesCache, ...files];
                  currentFilesCache = Array.from(new Map(currentFilesCache.map(f => [f.path, f])).values()); // 重複排除

                  container.innerHTML = "";
                  files.sort((a, b) => { if (a.is_dir && !b.is_dir) return -1; if (!a.is_dir && b.is_dir) return 1; return a.name.localeCompare(b.name); });
                  if (files.length === 0) container.innerHTML = `<li style="padding-left:20px; color:#666; font-size:0.8em;">(Empty)</li>`; else files.forEach(child => container.appendChild(createTreeNode(child)));
              } catch (e) { console.error(e); container.innerHTML = `<li style="color:red;">Error loading</li>`; }
          }
      } else { container.classList.remove("show"); arrow.classList.remove("open"); expandedPaths.delete(entry.path); }
  }

  async function loadFiles(path: string) {
    if(!listEl || !pathDisplay || !btnUp) return;
    try {
      currentPath = path; localStorage.setItem("robsidian-last-path", path); pathDisplay.innerText = path === "." ? "ROOT" : path.split("\\").pop() || path; btnUp.disabled = (path === "." || path.length < 3); listEl.innerHTML = "";
      const files = await invoke("list_files", { path }) as FileEntry[];
      
      // ★修正4: キャッシュ初期化と追加
      currentFilesCache = files;

      files.sort((a, b) => { if (a.is_dir && !b.is_dir) return -1; if (!a.is_dir && b.is_dir) return 1; return a.name.localeCompare(b.name); });
      files.forEach((entry) => listEl.appendChild(createTreeNode(entry)));
    } catch (e) { console.error(e); }
  }

  // ★修正5: グローバル変数に関数を割り当て
  loadFilesGlobal = loadFiles;

  const paneManager = new PaneManager("panes-container");
  globalPaneManager = paneManager;
  if(btnSplit) btnSplit.onclick = () => paneManager.addPane();
  try { await restoreSession(paneManager); } catch (e) { console.error("Restore Error:", e); paneManager.addPane(); }
  try { await loadFiles(currentPath); } catch(e) { console.error("LoadFiles Error:", e); }

  window.robsidianOpenHelix = (entry: FileEntry) => { if (!isTermVisible) toggleTerminal(); let term = window.robsidianActiveTerm; if (!term) { term = createNewTab(`Hx: ${entry.name}`); } if (term) { const safePath = `"${entry.path}"`; term.sendInput(`hx ${safePath}\r`); } };
  const termWrapper = document.getElementById("terminal-wrapper"); const termResizer = document.getElementById("term-resizer");
  let isTermVisible = localStorage.getItem("robsidian-term-visible") !== "false";
  let termHeight = parseInt(localStorage.getItem("robsidian-term-height") || "300");
  if(termWrapper && termResizer) {
      termWrapper.style.height = `${termHeight}px`;
      let isTermResizing = false;
      termResizer.addEventListener("mousedown", (e) => { isTermResizing = true; document.body.style.cursor="row-resize"; e.preventDefault(); });
      document.addEventListener("mousemove", (e) => { 
          if(isTermResizing) {
              const visualHeight = window.innerHeight / currentZoom;
              const visualY = e.clientY / currentZoom;
              termWrapper.style.height = `${visualHeight - visualY}px`; 
          }
      });
      document.addEventListener("mouseup", () => { isTermResizing = false; document.body.style.cursor="default"; localStorage.setItem("robsidian-term-height", termWrapper.style.height); });
      if (!isTermVisible) termWrapper.classList.add("hidden");
  }
  const toggleTerminal = () => { if(!termWrapper) return; isTermVisible = !isTermVisible; if(isTermVisible) termWrapper.classList.remove("hidden"); else termWrapper.classList.add("hidden"); localStorage.setItem("robsidian-term-visible", isTermVisible.toString()); };
  window.addEventListener("keydown", (e) => { if((e.ctrlKey||e.metaKey) && e.key==="j") { e.preventDefault(); toggleTerminal(); }});
  const terminals = new Map<string, RobsidianTerminal>(); let activeTermId: string | null = null; let tabCounter = 0;
  function switchTab(id: string) { if (activeTermId && terminals.has(activeTermId)) { terminals.get(activeTermId)!.hide(); document.querySelector(`.term-tab[data-id="${activeTermId}"]`)?.classList.remove("active"); } activeTermId = id; const term = terminals.get(id); if(term) { term.show(); window.robsidianActiveTerm = term; } document.querySelector(`.term-tab[data-id="${id}"]`)?.classList.add("active"); }
  function createNewTab(title?: string) {
      if(!tabsContainer) return undefined; tabCounter++; const term = new RobsidianTerminal("terminal-container"); terminals.set(term.ptyId, term); const tab = document.createElement("div"); tab.className = "term-tab"; tab.dataset.id = term.ptyId; tab.innerHTML = `<span class="term-tab-title">${title || `Term ${tabCounter}`}</span><div class="term-tab-close">×</div>`; tab.onclick = (e) => { if((e.target as HTMLElement).classList.contains("term-tab-close")) { term.destroy(); terminals.delete(term.ptyId); tab.remove(); if(activeTermId === term.ptyId) { activeTermId = null; const next = terminals.keys().next().value; if(next) switchTab(next); } } else switchTab(term.ptyId); }; tabsContainer.appendChild(tab); switchTab(term.ptyId); return term;
  }
  if(btnAddTab) btnAddTab.onclick = () => createNewTab();
  try { createNewTab("Shell"); } catch(e) {}
});