import { 
  FileText, Search, Terminal, FilePlus, FolderPlus, 
  ArrowUp, FolderOpen, Settings, List, Link,
  ChevronLeft, ChevronRight, ChevronDown, ChevronUp,
  Folder, File, X, Plus, Menu, Palette, Puzzle, Code, RotateCw
} from 'lucide-static';
import { invoke } from "@tauri-apps/api/core";
import { convertFileSrc } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { marked } from "marked";
import mermaid from "mermaid";
import { RobsidianTerminal } from "./terminal";
import "./styles.css";

// --- CodeMirror Imports ---
import { EditorView, basicSetup } from "codemirror";
import { keymap } from "@codemirror/view";
import { defaultKeymap, historyKeymap } from "@codemirror/commands";
import { markdown } from "@codemirror/lang-markdown";
import { oneDark } from "@codemirror/theme-one-dark";
import { autocompletion, CompletionContext } from "@codemirror/autocomplete";

// --- Vim & Languages ---
import { vim } from "@replit/codemirror-vim";
import { javascript } from "@codemirror/lang-javascript";
import { rust } from "@codemirror/lang-rust";
import { python } from "@codemirror/lang-python";
import { html } from "@codemirror/lang-html";
import { css } from "@codemirror/lang-css";

// --- Highlight.js Imports ---
import hljs from "highlight.js";
import "highlight.js/styles/atom-one-dark.css";

// --- Global Variables ---
let draggingTabState: { paneId: string; tabId: string } | null = null;
let saveSessionTimeout: number | null = null;
const expandedPaths = new Set<string>();
let currentZoom = parseFloat(localStorage.getItem("robsidian-zoom") || "1.0");
let currentFilesCache: FileEntry[] = [];
let layoutDirection: "row" | "column" = "row";
let isVimMode = localStorage.getItem("robsidian-vim-mode") === "true";
let globalSidebarManager: SidebarManager | null = null;

let globalPluginManager: PluginManager | null = null;
let globalSnippetManager: SnippetManager | null = null;
let globalPaneManager: PaneManager | null = null;
let globalCommandManager: CommandManager | null = null;
let currentPath = localStorage.getItem("robsidian-last-path") || ".";

let loadFilesGlobal: (path: string) => Promise<void> = async () => {};

// ★デバウンス用ヘルパー関数を追加
function debounce<T extends (...args: any[]) => any>(
  func: T,
  wait: number
): (...args: Parameters<T>) => void {
  let timeout: number | null = null;
  return (...args: Parameters<T>) => {
    if (timeout) clearTimeout(timeout);
    timeout = window.setTimeout(() => func(...args), wait);
  };
}

// ★ デバッグ用カウンター
let saveCounter = 0;
let lastSaveTime = 0;

// --- Mermaid Init ---
mermaid.initialize({
  startOnLoad: false,
  theme: "dark",
  securityLevel: "loose",
});

// --- Type Definitions ---
declare global {
  interface Window {
    robsidianOpenHelix: (entry: FileEntry) => void;
    app: RobsidianApp;
    debugPaneManager?: PaneManager;
    debugSaveSession?: () => void;
    debugSaveNow?: () => void;
    debugClearSession?: () => void;
  }
}

interface Command {
  id: string;
  name: string;
  callback: () => void;
  hotkeys?: { modifiers: string[]; key: string }[];
}

interface MenuItem {
  title: string;
  icon?: string;
  onClick: (file: FileEntry) => void;
  class?: string;
}

interface PluginSettingsAPI {
  loadData: () => Promise<any>;
  saveData: (data: any) => Promise<void>;
  addSettingTab: (
    name: string,
    displayHandler: (containerEl: HTMLElement) => void,
  ) => void;
}

interface RobsidianApp {
  workspace: {
    split: () => void;
    toggleLayout: () => void;
    openFile: (path: string) => Promise<void>;
    openTerminal: (title?: string, cmd?: string) => void;
    getLeaves: () => EditorPane[];
    activeLeaf: EditorPane | null;
  };
  vault: {
    read: (path: string) => Promise<string>;
    create: (path: string) => Promise<void>;
    write: (path: string, content: string) => Promise<void>;
    createDir: (path: string) => Promise<void>;
    delete: (path: string) => Promise<void>;
    rename: (oldPath: string, newPath: string) => Promise<void>;
    list: (path: string) => Promise<FileEntry[]>;
    getResourcePath: (path: string) => string;
    exists: (path: string) => boolean;
    root: string;
  };
    sidebar?: { // ★追加
    registerView: (view: SidebarView) => void;
    showView: (side: "left" | "right", viewId: string) => void;
    toggleSidebar: (side: "left" | "right") => void;
  };
  commands: {
    register: (command: Command) => void;
    execute: (id: string) => void;
  };
  menus: {
    registerFileMenu: (item: MenuItem) => void;
  };
  statusBar: {
    setStatus: (message: string) => void;
    setMode: (mode: string) => void;
  };
  utils: {
    clipboardWrite: (text: string) => Promise<void>;
    clipboardRead: () => Promise<string>;
    notice: (message: string) => void;
  };
  plugins?: any;
  plugin?: PluginSettingsAPI;
}

interface PluginManifest {
  id: string;
  name: string;
  version: string;
  description?: string;
  main?: string;
  author?: string;
}
type PluginType = "plugin" | "theme";
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
type TabType = "editor" | "terminal";

interface EditorTab {
  id: string;
  type: TabType;
  file?: FileEntry;
  contentCache?: string | null;
  isEditing: boolean;
  isDirty: boolean;
  terminalInstance?: RobsidianTerminal;
  termTitle?: string;
  scrollTop: number;
  pollingId?: number;
  lastMtime?: number;
}
interface SavedTabState {
  type?: TabType;
  path?: string;
  name?: string;
  scrollTop: number;
}
interface SavedPaneState {
  width: string;
  flex: string;
  tabs: SavedTabState[];
  activeTabIndex: number;
}
interface SavedSession {
  panes: SavedPaneState[];
  sidebarWidth: string;
  sidebarClosed: boolean;
  layoutDirection?: "row" | "column";
}
// --- Sidebar Manager ---
interface SidebarView {
  id: string;
  name: string;
  icon: string;
  side: "left" | "right";
  render: (container: HTMLElement) => void;
  onShow?: () => void;
  onHide?: () => void;
}

class SidebarManager {
  private views: Map<string, SidebarView> = new Map();
  private activeViews: { left: string | null; right: string | null } = {
    left: "files",
    right: "outline",
  };

  constructor() {
    this.loadState();
    this.initializeUI();
    this.initializeToggleButtons();
  }

  private initializeToggleButtons() {
    // 左サイドバー開閉ボタン
    const toggleLeft = document.getElementById('toggle-sidebar-left');
    if (toggleLeft) {
      const iconSvg = toggleLeft.querySelector('svg');
      if (iconSvg) {
        this.updateToggleIcon('left');
      }

      toggleLeft.onclick = () => {
        this.toggleSidebar('left');
        this.updateToggleIcon('left');
      };
    }

    // 右サイドバー開閉ボタン
    const toggleRight = document.getElementById('toggle-sidebar-right');
    if (toggleRight) {
      const iconSvg = toggleRight.querySelector('svg');
      if (iconSvg) {
        this.updateToggleIcon('right');
      }

      toggleRight.onclick = () => {
        this.toggleSidebar('right');
        this.updateToggleIcon('right');
      };
    }
  }

  private updateToggleIcon(side: 'left' | 'right') {
    const isClosed = document.body.classList.contains(`sidebar-${side}-closed`);
    const button = document.getElementById(`toggle-sidebar-${side}`);
    if (!button) return;

    const iconSvg = button.querySelector('svg');
    if (!iconSvg) return;

    // アイコンの方向を決定
    let iconName: string;
    if (side === 'left') {
      iconName = isClosed ? 'chevron-right' : 'chevron-left';
    } else {
      iconName = isClosed ? 'chevron-left' : 'chevron-right';
    }

    // アイコンを置き換え
    const newIcon = createLucideIcon(iconName);
    iconSvg.innerHTML = newIcon.innerHTML;
    Array.from(newIcon.attributes).forEach((attr) => {
      if (attr.name !== 'class') {
        iconSvg.setAttribute(attr.name, attr.value);
      }
    });
  }

  public toggleSidebar(side: "left" | "right") {
    document.body.classList.toggle(`sidebar-${side}-closed`);
    this.updateToggleIcon(side);
    this.saveState();
  }

  private loadState() {
    const saved = localStorage.getItem("robsidian-sidebar-state");
    if (saved) {
      try {
        const state = JSON.parse(saved);
        this.activeViews = state.activeViews || this.activeViews;

        if (state.leftClosed) document.body.classList.add("sidebar-left-closed");
        if (state.rightClosed) document.body.classList.add("sidebar-right-closed");

        if (state.leftWidth) {
          document.documentElement.style.setProperty("--sidebar-left-width", state.leftWidth);
        }
        if (state.rightWidth) {
          document.documentElement.style.setProperty("--sidebar-right-width", state.rightWidth);
        }
      } catch (e) {
        console.error("Failed to load sidebar state", e);
      }
    }
  }

  private saveState() {
    const state = {
      activeViews: this.activeViews,
      leftClosed: document.body.classList.contains("sidebar-left-closed"),
      rightClosed: document.body.classList.contains("sidebar-right-closed"),
      leftWidth: getComputedStyle(document.documentElement).getPropertyValue("--sidebar-left-width"),
      rightWidth: getComputedStyle(document.documentElement).getPropertyValue("--sidebar-right-width"),
    };
    localStorage.setItem("robsidian-sidebar-state", JSON.stringify(state));
  }

  private initializeUI() {
    // 左サイドバータブ
    document.querySelectorAll("#sidebar-left .sidebar-tab").forEach((tab) => {
      tab.addEventListener("click", (e) => {
        const viewId = (e.target as HTMLElement).getAttribute("data-view");
        if (viewId) this.showView("left", viewId);
      });
    });

    // 右サイドバータブ
    document.querySelectorAll("#sidebar-right .sidebar-tab").forEach((tab) => {
      tab.addEventListener("click", (e) => {
        const viewId = (e.target as HTMLElement).getAttribute("data-view");
        if (viewId) this.showView("right", viewId);
      });
    });

    // 右サイドバー閉じるボタン
    const btnToggleRight = document.getElementById("btn-toggle-right");
    if (btnToggleRight) {
      btnToggleRight.onclick = () => this.toggleSidebar("right");
    }

    // リサイザー（左）
    this.initResizer("left");
    this.initResizer("right");
  }

  private initResizer(side: "left" | "right") {
    const resizer = document.getElementById(`resizer-${side}`);
    if (!resizer) return;

    let isResizing = false;

    resizer.addEventListener("mousedown", () => {
      isResizing = true;
      document.body.style.cursor = "col-resize";
      resizer.classList.add("resizing");
    });

    document.addEventListener("mousemove", (e) => {
      if (!isResizing) return;

      if (side === "left") {
        const newWidth = Math.max(200, Math.min(e.clientX, 600));
        document.documentElement.style.setProperty("--sidebar-left-width", `${newWidth}px`);
      } else {
        const newWidth = Math.max(200, Math.min(window.innerWidth - e.clientX, 600));
        document.documentElement.style.setProperty("--sidebar-right-width", `${newWidth}px`);
      }
    });

    document.addEventListener("mouseup", () => {
      if (isResizing) {
        isResizing = false;
        document.body.style.cursor = "default";
        resizer.classList.remove("resizing");
        this.saveState();
      }
    });
  }

  public registerView(view: SidebarView) {
    this.views.set(view.id, view);
    this.addTab(view);
    console.log(`📌 [Sidebar] Registered view: ${view.name} (${view.side})`);
  }

  private addTab(view: SidebarView) {
    const sidebar = document.getElementById(`sidebar-${view.side}`);
    if (!sidebar) return;

    const tabsContainer = sidebar.querySelector(".sidebar-tabs");
    const viewsContainer = sidebar.querySelector(".sidebar-views");
    if (!tabsContainer || !viewsContainer) return;

    // タブボタン作成
    const tab = document.createElement("button");
    tab.className = "sidebar-tab";
    tab.setAttribute("data-view", view.id);
    tab.innerHTML = `${view.icon} ${view.name}`;
    tab.onclick = () => this.showView(view.side, view.id);
    tabsContainer.appendChild(tab);

    // ビューコンテナ作成
    const viewDiv = document.createElement("div");
    viewDiv.className = "sidebar-view";
    viewDiv.setAttribute("data-view", view.id);
    view.render(viewDiv);
    viewsContainer.appendChild(viewDiv);
  }

  public showView(side: "left" | "right", viewId: string) {
    const sidebar = document.getElementById(`sidebar-${side}`);
    if (!sidebar) return;

    // タブのアクティブ切り替え
    sidebar.querySelectorAll(".sidebar-tab").forEach((tab) => {
      if (tab.getAttribute("data-view") === viewId) {
        tab.classList.add("active");
      } else {
        tab.classList.remove("active");
      }
    });

    // ビューの表示切り替え
    const oldViewId = this.activeViews[side];
    if (oldViewId) {
      const oldView = this.views.get(oldViewId);
      if (oldView?.onHide) oldView.onHide();
    }

    sidebar.querySelectorAll(".sidebar-view").forEach((view) => {
      if (view.getAttribute("data-view") === viewId) {
        view.classList.add("active");
      } else {
        view.classList.remove("active");
      }
    });

    this.activeViews[side] = viewId;

    const newView = this.views.get(viewId);
    if (newView?.onShow) newView.onShow();

    this.saveState();
  }

  public getSidebarAPI() {
    return {
      registerView: (view: SidebarView) => this.registerView(view),
      showView: (side: "left" | "right", viewId: string) => this.showView(side, viewId),
      toggleSidebar: (side: "left" | "right") => this.toggleSidebar(side),
    };
  }
}

// --- Snippet Manager ---
interface SnippetFile {
  name: string;
  path: string;
  enabled: boolean;
}

class SnippetManager {
  private snippets: Map<string, SnippetFile> = new Map();
  private enabledSnippets: Set<string> = new Set();
  private styleElements: Map<string, HTMLStyleElement> = new Map();

  constructor() {
    const saved = localStorage.getItem("robsidian-enabled-snippets");
    if (saved) {
      try {
        this.enabledSnippets = new Set(JSON.parse(saved));
      } catch (e) {
        console.error("Failed to load snippet settings", e);
      }
    }
  }

  public async scanSnippets(vaultPath: string) {
    console.log("✨ [Snippet] Scanning snippets...");
    this.snippets.clear();

    const sep = vaultPath.includes("/") ? "/" : "\\";
    const snippetsDir = `${vaultPath}${sep}.robsidian${sep}snippets`;

    try {
      const exists = await invoke("directory_exists", { path: snippetsDir }).catch(() => false);
      if (!exists) {
        console.log("✨ [Snippet] Directory not found, creating...");
        try {
          await invoke("create_directory", { path: snippetsDir });
        } catch (e) {
          console.error("Failed to create snippets directory", e);
        }
        return;
      }

      const entries = (await invoke("list_files", { path: snippetsDir })) as FileEntry[];

      for (const entry of entries) {
        if (!entry.is_dir && entry.name.endsWith(".css")) {
          const snippet: SnippetFile = {
            name: entry.name.replace(/\.css$/, ""),
            path: entry.path,
            enabled: this.enabledSnippets.has(entry.name),
          };
          this.snippets.set(entry.name, snippet);

          // 有効なスニペットを自動ロード
          if (snippet.enabled) {
            await this.enableSnippet(entry.name);
          }
        }
      }

      console.log(`✨ [Snippet] Found ${this.snippets.size} snippets`);
    } catch (e) {
      console.error("[SnippetManager] Error scanning snippets:", e);
    }
  }

  public async enableSnippet(filename: string) {
    const snippet = this.snippets.get(filename);
    if (!snippet) return;

    try {
      const cssContent = (await invoke("read_file_content", { path: snippet.path })) as string;

      let styleEl = this.styleElements.get(filename);
      if (!styleEl) {
        styleEl = document.createElement("style");
        styleEl.id = `snippet-${filename.replace(/[^a-zA-Z0-9]/g, "-")}`;
        document.head.appendChild(styleEl);
        this.styleElements.set(filename, styleEl);
      }

      styleEl.textContent = cssContent;
      snippet.enabled = true;
      this.enabledSnippets.add(filename);
      this.saveSettings();

      console.log("✨ [Snippet] Enabled:", filename);
    } catch (e) {
      console.error(`Failed to enable snippet ${filename}:`, e);
      alert(`Failed to load snippet: ${e}`);
    }
  }

  public disableSnippet(filename: string) {
    const snippet = this.snippets.get(filename);
    if (!snippet) return;

    const styleEl = this.styleElements.get(filename);
    if (styleEl) {
      styleEl.remove();
      this.styleElements.delete(filename);
    }

    snippet.enabled = false;
    this.enabledSnippets.delete(filename);
    this.saveSettings();

    console.log("✨ [Snippet] Disabled:", filename);
  }

  public getSnippets(): SnippetFile[] {
    return Array.from(this.snippets.values());
  }

  private saveSettings() {
    localStorage.setItem(
      "robsidian-enabled-snippets",
      JSON.stringify(Array.from(this.enabledSnippets))
    );
  }
}

// アイコンマップに追加のアイコンを登録
function initializeLucideIcons() {
  const iconMap: Record<string, string> = {
    'file-text': FileText,
    'search': Search,
    'terminal': Terminal,
    'file-plus': FilePlus,
    'folder-plus': FolderPlus,
    'arrow-up': ArrowUp,
    'folder-open': FolderOpen,
    'settings': Settings,
    'list': List,
    'link': Link,
    'chevron-left': ChevronLeft,
    'chevron-right': ChevronRight,
    'chevron-down': ChevronDown,
    'chevron-up': ChevronUp,
    'folder': Folder,
    'file': File,
    'x': X,
    'plus': Plus,
    'menu': Menu,
    'palette': Palette,
    'puzzle': Puzzle,
    'code': Code,
    'rotate-cw': RotateCw,
  };

  document.querySelectorAll('svg[data-icon]').forEach((svg) => {
    const iconName = svg.getAttribute('data-icon');
    if (iconName && iconMap[iconName]) {
      svg.outerHTML = iconMap[iconName];
    }
  });

  console.log('✨ [Icons] Lucide icons initialized');
}

function createLucideIcon(iconName: string): SVGElement {
  const iconMap: Record<string, string> = {
    'file-text': FileText,
    'search': Search,
    'terminal': Terminal,
    'file-plus': FilePlus,
    'folder-plus': FolderPlus,
    'arrow-up': ArrowUp,
    'folder-open': FolderOpen,
    'settings': Settings,
    'list': List,
    'link': Link,
    'chevron-left': ChevronLeft,
    'chevron-right': ChevronRight,
    'chevron-down': ChevronDown,
    'chevron-up': ChevronUp,
    'folder': Folder,
    'file': File,
    'x': X,
    'plus': Plus,
    'menu': Menu,
  };

  const svgString = iconMap[iconName];

  if (svgString) {
    const parser = new DOMParser();
    const doc = parser.parseFromString(svgString, 'image/svg+xml');
    const svgElement = doc.documentElement as unknown as SVGElement;
    svgElement.classList.add('lucide-icon');
    return svgElement;
  }

  const fallback = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  fallback.classList.add('lucide-icon');
  fallback.setAttribute('width', '24');
  fallback.setAttribute('height', '24');
  fallback.setAttribute('viewBox', '0 0 24 24');
  fallback.setAttribute('fill', 'none');
  fallback.setAttribute('stroke', 'currentColor');
  fallback.setAttribute('stroke-width', '2');
  return fallback;
}

// --- Helpers ---
function applyZoom(zoom: number) {
  currentZoom = Math.min(Math.max(zoom, 0.5), 3.0);
  const app = document.getElementById("app");
  if (app) {
    document.documentElement.style.setProperty(
      "--ui-zoom",
      currentZoom.toString(),
    );
    app.style.transform = `scale(${currentZoom})`;
    app.style.width = `calc(100vw / ${currentZoom})`;
    app.style.height = `calc(100vh / ${currentZoom})`;
  }
  localStorage.setItem("robsidian-zoom", currentZoom.toString());
}
window.addEventListener("resize", () => {
  applyZoom(currentZoom);
});

const showNotice = (message: string) => {
  const notice = document.createElement("div");
  notice.className = "notice-toast";
  notice.innerText = message;
  document.body.appendChild(notice);
  setTimeout(() => notice.classList.add("show"), 10);
  setTimeout(() => {
    notice.classList.remove("show");
    setTimeout(() => notice.remove(), 300);
  }, 3000);
};

// --- Markdown Renderer ---
const renderer = new marked.Renderer();
renderer.image = ({
  href,
  title,
  text,
}: {
  href: string;
  title: string | null;
  text: string;
}) => {
  if (!href) return "";
  const titleAttr = title ? ` title="${title}"` : "";
  let src = href;
  if (
    !href.startsWith("http") &&
    !href.startsWith("data:") &&
    !href.startsWith("asset:")
  ) {
    src = convertFileSrc(href);
  }
  return `<img src="${src}" alt="${text}"${titleAttr} style="max-width: 100%; border-radius: 4px;">`;
};
// @ts-ignore
renderer.code = (codeOrToken, langIfString) => {
  let text = "",
    lang = "";
  if (
    typeof codeOrToken === "object" &&
    codeOrToken !== null &&
    "text" in codeOrToken
  ) {
    text = codeOrToken.text || "";
    lang = codeOrToken.lang || "";
  } else {
    text = String(codeOrToken);
    lang = String(langIfString || "");
  }
  const cleanLang = lang.trim().toLowerCase();
  if (cleanLang === "mermaid") {
    return `<div class="mermaid">${text}</div>`;
  }
  const language =
    cleanLang && hljs.getLanguage(cleanLang) ? cleanLang : "plaintext";
  try {
    const highlighted = hljs.highlight(text, { language }).value;
    return `<pre><code class="hljs language-${language}">${highlighted}</code></pre>`;
  } catch (e) {
    return `<pre><code>${text}</code></pre>`;
  }
};
const wikiLinkExtension = {
  name: "wikiLink",
  level: "inline",
  start(src: string) {
    return src.match(/\[\[/)?.index;
  },
  tokenizer(src: string) {
    const rule = /^\[\[([^\]]+)\]\]/;
    const match = rule.exec(src);
    if (match)
      return { type: "wikiLink", raw: match[0], text: match[1].trim() };
  },
  renderer(token: any) {
    return `<span class="wiki-link" data-target="${token.text}">${token.text}</span>`;
  },
};
marked.use({ renderer, extensions: [wikiLinkExtension as any] });

function wikiLinkCompletion(context: CompletionContext) {
  const word = context.matchBefore(/\[\[[^\]]*/);
  if (!word) return null;
  if (word.from == word.to && !context.explicit) return null;
  return {
    from: word.from + 2,
    options: currentFilesCache
      .filter((f) => !f.is_dir)
      .map((f) => ({
        label: f.name.replace(/\.md$/, ""),
        type: "text",
        apply: f.name.replace(/\.md$/, ""),
      })),
  };
}

const proseTheme = EditorView.theme({
  "&": {
    fontSize: "var(--base-font-size)",
    fontFamily: "var(--font-family-ui)",
  },
  ".cm-content": {
    padding: "20px 0",
    maxWidth: "800px",
    margin: "0 auto",
  },
  ".cm-scroller": { fontFamily: "var(--font-family-ui) !important" },
  ".cm-activeLine": { backgroundColor: "rgba(255, 255, 255, 0.03)" },
  ".cm-selectionBackground": {
    backgroundColor: "rgba(59, 130, 246, 0.3) !important",
  },
});

// --- Command Manager ---
class CommandManager {
  public commands: Map<string, Command> = new Map();

  constructor() {
    window.addEventListener("keydown", (e) => {
      if ((e.ctrlKey || e.metaKey) && (e.key === "p" || e.key === "P")) {
        e.preventDefault();
        this.showPalette();
        return;
      }

      this.commands.forEach((cmd) => {
        if (cmd.hotkeys) {
          cmd.hotkeys.forEach((hk) => {
            const matchKey = e.key.toLowerCase() === hk.key.toLowerCase();
            const matchCtrl = hk.modifiers.includes("Mod")
              ? e.ctrlKey || e.metaKey
              : true;
            const matchShift = hk.modifiers.includes("Shift")
              ? e.shiftKey
              : true;
            if (matchKey && matchCtrl && matchShift) {
              e.preventDefault();
              cmd.callback();
            }
          });
        }
      });
    });
  }

  register(command: Command) {
    this.commands.set(command.id, command);
  }

  execute(id: string) {
    const cmd = this.commands.get(id);
    if (cmd) cmd.callback();
  }

// CommandManagerクラス内
  showPalette() {
    const overlay = document.getElementById("command-palette");
    const input = document.getElementById("palette-input") as HTMLInputElement;
    const list = document.getElementById("palette-list");
    if (!overlay || !input || !list) return;

    overlay.style.display = "flex";
    input.value = "";
    input.focus();

    // OS判定（WindowsならCtrl, MacならCmdを表示するため）
    const isMac = /Mac|iPod|iPhone|iPad/.test(navigator.platform);

    const renderItems = (filter: string) => {
      list.innerHTML = "";
      this.commands.forEach((cmd) => {
        if (
          filter &&
          !cmd.name.toLowerCase().includes(filter.toLowerCase())
        )
          return;
        const li = document.createElement("li");
        li.className = "palette-item";
        li.innerHTML = `<span>${cmd.name}</span>`;
        
        if (cmd.hotkeys && cmd.hotkeys.length > 0) {
          const k = cmd.hotkeys[0];
          // ★修正: "Mod" を環境に合わせて "Ctrl" または "Cmd" に変換して表示
          const displayModifiers = k.modifiers.map(m => {
            if (m === "Mod") return isMac ? "Cmd" : "Ctrl";
            return m;
          });
          li.innerHTML += `<span class="palette-key">${displayModifiers.join("+")}+${k.key.toUpperCase()}</span>`;
        }
        
        li.onclick = () => {
          overlay.style.display = "none";
          cmd.callback();
        };
        list.appendChild(li);
      });
    };

    renderItems("");

    input.oninput = () => renderItems(input.value);
    input.onkeydown = (e) => {
      if (e.key === "Escape") overlay.style.display = "none";
    };
    overlay.onclick = (e) => {
      if (e.target === overlay) overlay.style.display = "none";
    };
  }
}

// --- Plugin Manager ---
class PluginManager {
  private loadedPlugins: Map<string, any> = new Map();
  private availablePlugins: Map<
    string,
    { manifest: PluginManifest; dir: string; type: PluginType }
  > = new Map();
  private enabledPluginIds: Set<string> = new Set();
  public settingTabs: Map<string, (container: HTMLElement) => void> = new Map();

  constructor(private app: RobsidianApp) {
    const saved = localStorage.getItem("robsidian-enabled-plugins");
    if (saved) {
      try {
        this.enabledPluginIds = new Set(JSON.parse(saved));
      } catch (e) {}
    }
  }

  public async scanAll(vaultPath: string) {
    console.log("🔌 [Plugin] Scanning plugins...");
    this.availablePlugins.clear();
    await this.scanDirectory(vaultPath, "plugins", "plugin");
    await this.scanDirectory(vaultPath, "themes", "theme");
    console.log(
      `🔌 [Plugin] Found ${this.availablePlugins.size} plugins/themes`,
    );
    for (const id of this.enabledPluginIds) {
      if (this.availablePlugins.has(id)) {
        await this.enablePlugin(id);
      }
    }
    console.log("🔌 [Plugin] Scan complete");
  }

  private async scanDirectory(
    vaultPath: string,
    dirName: string,
    type: PluginType,
  ) {
    const sep = vaultPath.includes("/") ? "/" : "\\";
    const targetDir = `${vaultPath}${sep}.robsidian${sep}${dirName}`;
    try {
      const exists = await invoke("directory_exists", {
        path: targetDir,
      }).catch(() => false);
      if (!exists) return;
      const entries = (await invoke("list_files", {
        path: targetDir,
      })) as FileEntry[];
      for (const entry of entries) {
        if (entry.is_dir) await this.readManifest(entry.path, type);
      }
    } catch (e) {
      console.error(`[PluginManager] Error scanning ${dirName}:`, e);
    }
  }

  private async readManifest(pluginDir: string, type: PluginType) {
    const sep = pluginDir.includes("/") ? "/" : "\\";
    const manifestPath = `${pluginDir}${sep}manifest.json`;
    try {
      const exists = await invoke("file_exists", { path: manifestPath }).catch(
        () => false,
      );
      if (!exists) return;
      const content = (await invoke("read_file_content", {
        path: manifestPath,
      })) as string;
      const manifest: PluginManifest = JSON.parse(content);
      if (!manifest.id) return;
      this.availablePlugins.set(manifest.id, {
        manifest,
        dir: pluginDir,
        type,
      });
    } catch (e) {
      console.error(
        `[PluginManager] Failed to read manifest in ${pluginDir}`,
        e,
      );
    }
  }

  public async setTheme(id: string) {
    const currentThemes = this.getPluginsByType("theme");
    for (const theme of currentThemes) {
      if (this.isEnabled(theme.manifest.id) && theme.manifest.id !== id) {
        this.disablePlugin(theme.manifest.id);
      }
    }
    if (id) await this.enablePlugin(id);
  }

  public async enablePlugin(id: string) {
    console.log("🔌 [Plugin] Enabling:", id);
    console.log(
      " 📊 Panes before enable:",
      this.app.workspace.getLeaves().length,
    );

    if (this.loadedPlugins.has(id)) {
      console.warn(`Plugin ${id} is already loaded`);
      return;
    }

    const data = this.availablePlugins.get(id);
    if (!data) {
      console.error(`Plugin ${id} not found`);
      return;
    }

    const sep = data.dir.includes("/") ? "/" : "\\";

    try {
      if (data.type === "theme") {
        const cssPath = `${data.dir}${sep}styles.css`;
        const cssContent = (await invoke("read_file_content", {
          path: cssPath,
        })) as string;
        let styleEl = document.getElementById(`theme-style`);
        if (!styleEl) {
          styleEl = document.createElement("style");
          styleEl.id = `theme-style`;
          document.head.appendChild(styleEl);
        }
        styleEl.textContent = cssContent;
        this.loadedPlugins.set(id, { type: "theme", el: styleEl });
        this.enabledPluginIds.add(id);
        this.saveSettings();
        console.log(" ✅ Theme loaded:", id);
        return;
      }

      if (!data.manifest.main) return;

      const mainJsPath = `${data.dir}${sep}${data.manifest.main}`;
      const jsContent = (await invoke("read_file_content", {
        path: mainJsPath,
      })) as string;

      // セキュリティ: strictモードで実行
      const pluginFactory = new Function(
        "app",
        '"use strict";\n' + jsContent
      );

      const pluginApi: RobsidianApp = {
        ...this.app,
        plugin: {
          loadData: async () => {
            const saved = localStorage.getItem(`robsidian-plugin-data-${id}`);
            return saved ? JSON.parse(saved) : {};
          },
          saveData: async (saveData: any) => {
            localStorage.setItem(
              `robsidian-plugin-data-${id}`,
              JSON.stringify(saveData),
            );
          },
          addSettingTab: (
            _name: string,
            handler: (container: HTMLElement) => void,
          ) => {
            this.settingTabs.set(id, handler);
          },
        },
      };

      const pluginInstance = pluginFactory(pluginApi);

      if (pluginInstance && typeof pluginInstance.onload === "function") {
        console.log(" 🚀 Calling onload for:", id);

        // タイムアウト付きでonloadを呼ぶ（10秒制限）
        await Promise.race([
          pluginInstance.onload(pluginApi),
          new Promise((_, reject) =>
            setTimeout(() => reject(new Error("Plugin load timeout")), 10000)
          ),
        ]);

        console.log(
          " 📊 Panes after onload:",
          this.app.workspace.getLeaves().length,
        );
        this.loadedPlugins.set(id, pluginInstance);
        this.enabledPluginIds.add(id);
        this.saveSettings();
        console.log(" ✅ Plugin loaded:", id);
      }
    } catch (e) {
      console.error(`[PluginManager] Failed to enable ${id}:`, e);
      this.enabledPluginIds.delete(id);
      alert(`Failed to enable ${data.type} ${id}: ${e}`);
      throw e; // エラーを再スロー
    }
  }

  public disablePlugin(id: string) {
    const plugin = this.loadedPlugins.get(id);
    if (plugin) {
      if (plugin.type === "theme" && plugin.el) plugin.el.remove();
      else if (typeof plugin.onunload === "function")
        try {
          plugin.onunload();
        } catch (e) {
          console.error(e);
        }
      this.settingTabs.delete(id);
      this.loadedPlugins.delete(id);
      this.enabledPluginIds.delete(id);
      this.saveSettings();
    }
  }

  public isEnabled(id: string): boolean {
    return this.enabledPluginIds.has(id);
  }
  public getPluginsByType(type: PluginType) {
    return Array.from(this.availablePlugins.values()).filter(
      (p) => p.type === type,
    );
  }
  private saveSettings() {
    localStorage.setItem(
      "robsidian-enabled-plugins",
      JSON.stringify(Array.from(this.enabledPluginIds)),
    );
  }
}

// EditorPane Class
class EditorPane {
  public id: string;
  public container: HTMLElement;
  public tabs: EditorTab[] = [];
  public activeTabId: string | null = null;
  private headerEl: HTMLElement;
  private contentArea: HTMLElement;
  private cmEditor: EditorView | null = null;
  private editorExtensions: any[] | null = null;
  constructor(private manager: PaneManager) {
    this.id = Math.random().toString(36).substring(2, 9);
    this.container = document.createElement("div");
    this.container.className = "pane";
    this.container.innerHTML = `<div class="pane-header"></div><div class="pane-content markdown-body"></div>`;
    this.headerEl = this.container.querySelector(".pane-header") as HTMLElement;
    this.contentArea = this.container.querySelector(
      ".pane-content",
    ) as HTMLElement;
    this.container.addEventListener("mousedown", () => {
      this.manager.setActive(this);
    });
    this.renderEmpty();

    this.contentArea.addEventListener("click", async (e) => {
      const target = e.target as HTMLElement;
      if (target.classList.contains("wiki-link")) {
        e.preventDefault();
        e.stopPropagation();
        const linkTarget = target.getAttribute("data-target");
        if (linkTarget) {
          const targetName = linkTarget.toLowerCase();
          let match = currentFilesCache.find(
            (f) =>
              !f.is_dir &&
              (f.name.toLowerCase() === targetName ||
                f.name.toLowerCase() === `${targetName}.md`),
          );
          if (match)
            this.manager.openFileInActive({
              name: match.name,
              path: match.path,
              is_dir: false,
            });
          else if (confirm(`File "${linkTarget}" not found. Create it?`)) {
            const root = currentPath;
            const sep = root.includes("/") ? "/" : "\\";
            const newPath = `${root}${sep}${linkTarget}.md`;
            try {
              await invoke("create_file", { path: newPath });
              await loadFilesGlobal(root);
              this.manager.openFileInActive({
                name: `${linkTarget}.md`,
                path: newPath,
                is_dir: false,
              });
            } catch (err) {
              alert("Failed: " + err);
            }
          }
        }
      }
    });
  }
  public toJSON(): SavedPaneState {
    if (this.activeTabId) {
      const currentTab = this.tabs.find((t) => t.id === this.activeTabId);
      if (currentTab) {
        if (currentTab.type === "editor" && this.cmEditor)
          currentTab.contentCache = this.cmEditor.state.doc.toString();
        currentTab.scrollTop = this.contentArea.scrollTop;
      }
    }
    return {
      width: this.container.style.width,
      flex: this.container.style.flex,
      tabs: this.tabs.map((t) => ({
        type: t.type,
        path: t.file ? t.file.path : undefined,
        name: t.file ? t.file.name : t.termTitle,
        scrollTop: t.scrollTop,
      })),
      activeTabIndex: this.activeTabId
        ? this.tabs.findIndex((t) => t.id === this.activeTabId)
        : -1,
    };
  }
  public setActive(isActive: boolean) {
    if (isActive) this.container.classList.add("active");
    else this.container.classList.remove("active");
  }
  public openTerminal(title: string = "Terminal", cmd?: string) {
    const term = new RobsidianTerminal();
    const newTab: EditorTab = {
      id: Math.random().toString(36).substring(2, 9),
      type: "terminal",
      scrollTop: 0,
      isEditing: false,
      isDirty: false,
      terminalInstance: term,
      termTitle: title,
      contentCache: null,
    };
    if (cmd) setTimeout(() => term.sendInput(cmd + "\r"), 800);
    this.tabs.push(newTab);
    this.switchTab(newTab.id);
    saveSession();
  }
  public async openFile(entry: FileEntry, initialScrollTop: number = 0) {
    const existingTab = this.tabs.find(
      (t) => t.type === "editor" && t.file?.path === entry.path,
    );
    if (existingTab) {
      this.switchTab(existingTab.id);
      return;
    }
    let content = "";
    try {
      content = (await invoke("read_file_content", {
        path: entry.path,
      })) as string;
    } catch (e) {
      content = `Cannot read file: ${entry.path}`;
    }
    const newTab: EditorTab = {
      id: Math.random().toString(36).substring(2, 9),
      type: "editor",
      file: entry,
      contentCache: content,
      scrollTop: initialScrollTop,
      isEditing: false,
      isDirty: false,
    };
    this.startPolling(newTab);
    this.tabs.push(newTab);
    this.switchTab(newTab.id);
    saveSession();
  }
  private startPolling(tab: EditorTab) {
    if (!tab.file) return;
    if (tab.pollingId) window.clearInterval(tab.pollingId);

    invoke("get_file_mtime", { path: tab.file.path })
      .then((mtime) => {
        tab.lastMtime = mtime as number;
      })
      .catch(() => {});

    tab.pollingId = window.setInterval(async () => {
      // アクティブなタブでない、または編集中の場合はスキップ
      if (tab.isEditing || this.activeTabId !== tab.id) return;

      try {
        const currentMtime = (await invoke("get_file_mtime", {
          path: tab.file!.path,
        })) as number;

        if (tab.lastMtime !== currentMtime) {
          tab.lastMtime = currentMtime;
          const newContent = (await invoke("read_file_content", {
            path: tab.file!.path,
          })) as string;

          if (newContent !== tab.contentCache) {
            tab.contentCache = newContent;
            // アクティブタブの場合のみレンダリング
            if (this.activeTabId === tab.id) {
              this.renderContent();
            }
          }
        }
      } catch (e) {
        console.error("Polling error:", e);
      }
    }, 2000); // 2秒に延長（負荷軽減）
  }
  public switchTab(tabId: string) {
    if (this.activeTabId) {
      const currentTab = this.tabs.find((t) => t.id === this.activeTabId);
      if (currentTab) {
        if (currentTab.type === "editor") {
          if (this.cmEditor) {
            currentTab.contentCache = this.cmEditor.state.doc.toString();
            this.cmEditor.destroy();
            this.cmEditor = null;
          } else currentTab.scrollTop = this.contentArea.scrollTop;
        } else if (
          currentTab.type === "terminal" &&
          currentTab.terminalInstance
        )
          currentTab.terminalInstance.detach();
      }
    }
    this.activeTabId = tabId;
    this.renderHeader();
    this.renderContent();
    saveSession();
  }
  public closeTab(tabId: string) {
    const tab = this.tabs.find((t) => t.id === tabId);
    if (tab && tab.pollingId) {
      window.clearInterval(tab.pollingId);
      tab.pollingId = undefined;
    }
    if (tab && tab.type === "terminal" && tab.terminalInstance)
      tab.terminalInstance.destroy();
    const tabIndex = this.tabs.findIndex((t) => t.id === tabId);
    if (tabIndex === -1) return;
    this.tabs.splice(tabIndex, 1);
    if (this.activeTabId === tabId) {
      this.activeTabId = null;
      if (this.tabs.length > 0)
        this.switchTab(this.tabs[Math.min(tabIndex, this.tabs.length - 1)].id);
      else {
        this.renderHeader();
        this.renderEmpty();
      }
    } else this.renderHeader();
    saveSession();
  }
  private renderHeader() {
    this.headerEl.innerHTML = "";
    this.tabs.forEach((tab) => {
      const tabEl = document.createElement("div");
      tabEl.className = `editor-tab ${tab.id === this.activeTabId ? "active" : ""} ${tab.isDirty ? "dirty" : ""}`;
      tabEl.draggable = true;

      let iconElement: SVGElement;
      let title = "Untitled";

      if (tab.type === "editor" && tab.file) {
        iconElement = createLucideIcon('file-text');
        title = tab.file.name;
      } else if (tab.type === "terminal") {
        iconElement = createLucideIcon('terminal');
        title = tab.termTitle || "Terminal";
      } else {
        iconElement = createLucideIcon('file');
        title = "Untitled";
      }

      iconElement.style.width = '16px';
      iconElement.style.height = '16px';
      iconElement.style.marginRight = '8px';

      const titleSpan = document.createElement('span');
      titleSpan.className = 'tab-title';
      titleSpan.textContent = title;

      const closeBtn = document.createElement('div');
      closeBtn.className = 'tab-close';
      closeBtn.innerHTML = '×';

      tabEl.appendChild(iconElement);
      tabEl.appendChild(titleSpan);
      tabEl.appendChild(closeBtn);
      tabEl.addEventListener("click", (e) => {
        if ((e.target as HTMLElement).classList.contains("tab-close")) {
          e.stopPropagation();
          this.closeTab(tab.id);
        } else this.switchTab(tab.id);
      });
      tabEl.addEventListener("dragstart", (e) => {
        draggingTabState = { paneId: this.id, tabId: tab.id };
        e.dataTransfer!.setData("text/plain", tab.id);
        requestAnimationFrame(() => tabEl.classList.add("dragging"));
      });
      tabEl.addEventListener("dragend", () => {
        tabEl.classList.remove("dragging");
        draggingTabState = null;
        this.headerEl
          .querySelectorAll(".drag-over")
          .forEach((el) => el.classList.remove("drag-over"));
      });
      tabEl.addEventListener("dragover", (e) => {
        e.preventDefault();
        if (
          draggingTabState &&
          draggingTabState.paneId === this.id &&
          draggingTabState.tabId !== tab.id
        )
          tabEl.classList.add("drag-over");
      });
      tabEl.addEventListener("dragleave", () => {
        tabEl.classList.remove("drag-over");
      });
      tabEl.addEventListener("drop", (e) => {
        e.preventDefault();
        tabEl.classList.remove("drag-over");
        if (!draggingTabState || draggingTabState.paneId !== this.id) return;
        const sourceIdx = this.tabs.findIndex(
          (t) => t.id === draggingTabState!.tabId,
        );
        const targetIdx = this.tabs.findIndex((t) => t.id === tab.id);
        if (sourceIdx > -1 && targetIdx > -1) {
          const [moved] = this.tabs.splice(sourceIdx, 1);
          this.tabs.splice(targetIdx, 0, moved);
          this.renderHeader();
          saveSession();
        }
      });
      this.headerEl.appendChild(tabEl);
    });
  }
    private getEditorExtensions() {
    // キャッシュがあり、Vim/テーマ設定が変わっていなければ再利用
    if (this.editorExtensions) {
      return this.editorExtensions;
    }

    const exts = [
      basicSetup,
      markdown(),
      javascript(),
      rust(),
      python(),
      html(),
      css(),
      autocompletion({ override: [wikiLinkCompletion] }),
      EditorView.lineWrapping,
      proseTheme,
      keymap.of([
        ...defaultKeymap,
        ...historyKeymap,
        {
          key: "Mod-s",
          run: () => {
            const tab = this.tabs.find((t) => t.id === this.activeTabId);
            if (tab && tab.type === "editor") {
              this.saveCurrentTab();
            }
            return true;
          },
        },
        {
          key: "Escape",
          run: () => {
            const tab = this.tabs.find((t) => t.id === this.activeTabId);
            if (tab && this.cmEditor) {
              tab.contentCache = this.cmEditor.state.doc.toString();
              tab.isEditing = false;
              this.renderContent();
            }
            return true;
          },
        },
      ]),
      EditorView.updateListener.of((u) => {
        const tab = this.tabs.find((t) => t.id === this.activeTabId);
        if (!tab) return;

        if (u.docChanged) tab.isDirty = true;
        const pos = u.state.selection.main.head;
        const line = u.state.doc.lineAt(pos);
        const statusCursor = document.getElementById("status-cursor");
        if (statusCursor)
          statusCursor.innerText = `Ln ${line.number}, Col ${pos - line.from + 1}`;
      }),
    ];

    if (!document.body.classList.contains("light-theme")) exts.push(oneDark);
    if (isVimMode) exts.push(vim());

    this.editorExtensions = exts;
    return exts;
  }

  private saveCurrentTab() {
    const tab = this.tabs.find((t) => t.id === this.activeTabId);
    if (!tab || !tab.file || !this.cmEditor) return;

    const save = async () => {
      try {
        const c = this.cmEditor!.state.doc.toString();
        await invoke("save_file_content", {
          path: tab.file!.path,
          content: c,
        });
        tab.contentCache = c;
        tab.isEditing = false;
        tab.isDirty = false;
        this.cmEditor!.destroy();
        this.cmEditor = null;
        this.renderContent();
      } catch (e) {
        alert("Save failed: " + e);
      }
    };
    save();
  }
  private async renderContent() {
    const tab = this.tabs.find((t) => t.id === this.activeTabId);
    if (!tab) {
      this.renderEmpty();
      return;
    }
    this.contentArea.innerHTML = "";

    const statusMode = document.getElementById("status-mode");
    if (statusMode)
      statusMode.innerText =
        tab.type === "terminal" ? "TERMINAL" : isVimMode ? "VIM" : "NORMAL";

    if (tab.type === "terminal" && tab.terminalInstance) {
      this.contentArea.style.overflow = "hidden";
      tab.terminalInstance.attachTo(this.contentArea);
      return;
    }
    this.contentArea.style.overflow = "auto";

    if (tab.isEditing && tab.type === "editor" && tab.file) {
      this.contentArea.innerHTML = `<div class="editor-container"><div style="padding:5px;text-align:right;border-bottom:1px solid var(--border-color);background:var(--bg-sidebar);"><button class="btn-save primary">Save</button></div><div class="editor-wrapper"></div></div>`;
      const wrapper = this.contentArea.querySelector(
        ".editor-wrapper",
      ) as HTMLElement;

    this.cmEditor = new EditorView({
      doc: tab.contentCache || "",
      extensions: this.getEditorExtensions(),
      parent: wrapper,
    });
    this.contentArea
      .querySelector(".btn-save")
      ?.addEventListener("click", () => this.saveCurrentTab());
    } else if (tab.type === "editor" && tab.file) {
      const toolbar = document.createElement("div");
      toolbar.style.cssText =
        "position:sticky;top:0;background:var(--bg-main);z-index:10;padding:5px 20px;border-bottom:1px solid var(--border-color);display:flex;justify-content:space-between;align-items:center;";
      toolbar.innerHTML = `<div style="font-size:0.8em;color:var(--text-muted);">${tab.file.path}</div><div><button class="pane-btn edit-internal">Edit</button><button class="pane-btn edit-helix">Hx</button></div>`;
      toolbar.querySelector(".edit-internal")?.addEventListener("click", () => {
        tab.isEditing = true;
        this.renderContent();
      });
      toolbar.querySelector(".edit-helix")?.addEventListener("click", () => {
        if (window.robsidianOpenHelix) window.robsidianOpenHelix(tab.file!);
      });
      this.contentArea.appendChild(toolbar);
      const body = document.createElement("div");
      body.className = "markdown-body";
      if (tab.file.name.endsWith(".md"))
        body.innerHTML = await marked(tab.contentCache || "");
      else
        body.innerHTML = `<pre><code>${(tab.contentCache || "").replace(/</g, "&lt;")}</code></pre>`;
      this.contentArea.appendChild(body);
      try {
        if (document.body.contains(body)) {
          requestAnimationFrame(() =>
            mermaid.run({ nodes: body.querySelectorAll(".mermaid") }),
          );
        }
      } catch (e) {}
      setTimeout(() => {
        this.contentArea.scrollTop = tab.scrollTop;
      }, 0);
    }
  }
  public async renderEmpty() {
    this.activeTabId = null;
    this.headerEl.innerHTML = "";
    this.contentArea.innerHTML = `<div class="pane-empty"><div style="font-size:2em;opacity:0.3;">◫</div><button class="btn-close-pane" style="margin-top:20px;background:none;border:1px solid #444;color:#888;padding:5px 10px;cursor:pointer;">Close Pane</button></div>`;
    this.contentArea
      .querySelector(".btn-close-pane")
      ?.addEventListener("click", () => {
        this.manager.removePane(this);
      });
  }
  public destroy() {
    if (this.cmEditor) this.cmEditor.destroy();
    this.container.remove();
  }
}

class PaneManager {
  public panes: EditorPane[] = [];
  public activePane: EditorPane | null = null;
  private container: HTMLElement;
  constructor(id: string) {
    const el = document.getElementById(id);
    if (!el) throw new Error("No container");
    this.container = el;
    this.updateLayoutDirection();
  }
  public updateLayoutDirection() {
    this.container.style.flexDirection = layoutDirection;
    const btn = document.getElementById("btn-toggle-layout");
    if (btn) {
      btn.innerText = layoutDirection === "row" ? "║" : "═";
      btn.title =
        layoutDirection === "row"
          ? "Split Vertical (Click to switch)"
          : "Split Horizontal (Click to switch)";
    }
  }
  public toggleLayout() {
    console.log("🔄 [Layout] Toggle layout from", layoutDirection);
    layoutDirection = layoutDirection === "row" ? "column" : "row";
    this.panes.forEach((p) => {
      p.container.style.width = "";
      p.container.style.height = "";
      p.container.style.flex = "1";
    });
    this.updateLayoutDirection();
    console.log("🔄 [Layout] New layout:", layoutDirection);
    saveSession(true); // ★即座に保存
  }
  public addPane() {
    console.trace("📌 [Pane] addPane() called from:");
    const pane = new EditorPane(this);
    if (this.panes.length > 0) {
      const r = document.createElement("div");
      r.className = "pane-resizer";
      this.container.appendChild(r);
      this.initResizer(r);
    }
    this.panes.push(pane);
    this.container.appendChild(pane.container);
    this.setActive(pane);
    console.log("📊 [Pane] Total panes now:", this.panes.length);
    saveSession();
    return pane;
  }
  public removePane(p: EditorPane) {
    console.log("🗑️ [Pane] Removing pane, current count:", this.panes.length);
    if (this.panes.length <= 1) {
      p.tabs = [];
      p.renderEmpty();
      this.resetLayout();
      saveSession(true);
      return;
    }
    const idx = this.panes.indexOf(p);
    p.destroy();
    this.panes.splice(idx, 1);
    if (this.activePane === p)
      this.setActive(this.panes[Math.min(idx, this.panes.length - 1)]);
    this.resetLayout();
    console.log("📊 [Pane] Panes after remove:", this.panes.length);
    saveSession(true); // ★即座に保存
  }
  private resetLayout() {
    Array.from(this.container.children).forEach((child) => {
      if (child.classList.contains("pane")) {
        const el = child as HTMLElement;
        el.style.flex = "1";
        el.style.width = "";
        el.style.height = "";
      }
    });
  }
  public setActive(p: EditorPane) {
    this.panes.forEach((x) => x.setActive(false));
    this.activePane = p;
    p.setActive(true);
  }
  public openFileInActive(e: FileEntry) {
    if (!this.activePane) this.addPane();
    this.activePane!.openFile(e);
  }
  public openTerminalInActive() {
    if (!this.activePane) this.addPane();
    this.activePane!.openTerminal();
  }
  public saveState(): SavedSession {
    return {
      panes: this.panes.map((p) => p.toJSON()),
      sidebarWidth: getComputedStyle(
        document.documentElement,
      ).getPropertyValue("--sidebar-width"),
      sidebarClosed: document.body.classList.contains("sidebar-closed"),
      layoutDirection,
    };
  }
  private initResizer(resizer: HTMLElement) {
    let isResizing = false;
    resizer.addEventListener("mousedown", () => {
      isResizing = true;
      document.body.style.cursor =
        layoutDirection === "row" ? "col-resize" : "row-resize";
      resizer.classList.add("resizing");
    });
    document.addEventListener("mousemove", (e) => {
      if (!isResizing) return;
      const prev = resizer.previousElementSibling as HTMLElement;
      const next = resizer.nextElementSibling as HTMLElement;
      if (!prev || !next) return;
      const delta =
        layoutDirection === "row"
          ? e.movementX / currentZoom
          : e.movementY / currentZoom;
      if (layoutDirection === "row") {
        prev.style.width = `${prev.offsetWidth + delta}px`;
        next.style.width = `${next.offsetWidth - delta}px`;
      } else {
        prev.style.height = `${prev.offsetHeight + delta}px`;
        next.style.height = `${next.offsetHeight - delta}px`;
      }
      prev.style.flex = "none";
      next.style.flex = "none";
    });
    document.addEventListener("mouseup", () => {
      if (isResizing) {
        isResizing = false;
        document.body.style.cursor = "default";
        resizer.classList.remove("resizing");
        saveSession();
      }
    });
  }
}

// ★グローバル変数に追加
let isRestoring = false;

function saveSession(immediate = false) {
  if (!globalPaneManager) {
    console.warn("⚠️ saveSession called but no globalPaneManager");
    return;
  }

  // 復元中は immediate=true の場合のみ許可
  if (isRestoring && !immediate) {
    return;
  }

  const doSave = () => {
    const now = Date.now();
    // 最後の保存から100ms以内ならスキップ（immediate=falseの場合）
    if (!immediate && now - lastSaveTime < 100) {
      return;
    }
    lastSaveTime = now;

    const state = globalPaneManager!.saveState();
    try {
      localStorage.setItem("robsidian-session", JSON.stringify(state));
      console.log(`💾 [${++saveCounter}] Saved session:`, {
        panes: state.panes.length,
        layout: state.layoutDirection,
      });
    } catch (e) {
      console.error("Failed to save session:", e);
    }
  };

  if (immediate) {
    if (saveSessionTimeout) clearTimeout(saveSessionTimeout);
    doSave();
  } else {
    if (saveSessionTimeout) clearTimeout(saveSessionTimeout);
    saveSessionTimeout = window.setTimeout(doSave, 300);
  }
}

async function restoreSession(manager: PaneManager) {
  isRestoring = true;
  const json = localStorage.getItem("robsidian-session");
  console.log("🔍 [RESTORE START]");

  try {
    if (!json) {
      manager.addPane();
      return;
    }

    const s: SavedSession = JSON.parse(json);
    console.log("📊 [RESTORE] Restoring", s.panes.length, "panes");

    if (s.sidebarWidth)
      document.documentElement.style.setProperty("--sidebar-width", s.sidebarWidth);
    if (s.sidebarClosed) document.body.classList.add("sidebar-closed");
    if (s.layoutDirection) {
      layoutDirection = s.layoutDirection;
      manager.updateLayoutDirection();
    }

    if (!s.panes.length) {
      manager.addPane();
      return;
    }

    for (let idx = 0; idx < s.panes.length; idx++) {
      const sp = s.panes[idx];
      const p = manager.addPane();

      if (s.layoutDirection === "column") p.container.style.flex = sp.flex;
      else p.container.style.width = sp.width;

      // ★全てのタブ復元を待つ
      for (const t of sp.tabs) {
        if (t.type === "terminal") {
          p.openTerminal(t.name);
        } else if (t.path) {
          await p.openFile(
            { name: t.name!, path: t.path, is_dir: false },
            t.scrollTop,
          );
        }
      }

      // ★タブの存在を確認
      if (sp.activeTabIndex >= 0 && sp.activeTabIndex < p.tabs.length) {
        p.switchTab(p.tabs[sp.activeTabIndex].id);
      } else if (p.tabs.length > 0) {
        p.switchTab(p.tabs[0].id);
      }
    }

    console.log("✅ [RESTORE] Complete:", manager.panes.length, "panes");
  } catch (e) {
    console.error("❌ [RESTORE] Error:", e);
    if (manager.panes.length === 0) manager.addPane();
  } finally {
    isRestoring = false;
    // 復元完了後に一度だけ保存
    saveSession(true);
  }
}

function forceSaveSession() {
  if (!globalPaneManager) return;
  const state = globalPaneManager.saveState();
  localStorage.setItem("robsidian-session", JSON.stringify(state));
  console.log("🚪 [Exit] Final save:", state.panes.length, "panes");
}


// ★デバッグパネルの作成 (改良版)
function createDebugPanel() {
  // 保存された設定を読み込み
  let isVisible = localStorage.getItem("robsidian-show-debug") === "true";

  const panel = document.createElement("div");
  panel.id = "debug-panel";
  panel.style.cssText = `
    position: fixed;
    bottom: 10px;
    right: 10px;
    background: rgba(0,0,0,0.9);
    color: #0f0;
    padding: 12px;
    border-radius: 8px;
    font-family: 'Courier New', monospace;
    font-size: 11px;
    z-index: 100000;
    min-width: 280px;
    box-shadow: 0 4px 12px rgba(0,0,0,0.5);
    border: 1px solid #333;
    display: ${isVisible ? "block" : "none"}; /* 初期表示状態 */
  `;
  document.body.appendChild(panel);

  const update = () => {
    // 非表示ならDOM更新をスキップして負荷軽減
    if (panel.style.display === "none") return;

    const saved = JSON.parse(
      localStorage.getItem("robsidian-session") || "{}",
    );
    
    // パネルの中身を構築
    panel.innerHTML = `
      <div style="font-weight:bold; margin-bottom:8px; color:#0ff; border-bottom:1px solid #333; padding-bottom:4px; display:flex; justify-content:space-between;">
        <span>🔍 Debug Panel</span>
        <span style="color:#666; cursor:pointer;" onclick="document.getElementById('debug-panel').style.display='none'; localStorage.setItem('robsidian-show-debug', 'false');">×</span>
      </div>
      <div style="margin:3px 0;">▸ Current Panes: <span style="color:#ff0">${globalPaneManager?.panes?.length || 0}</span></div>
      <div style="margin:3px 0;">▸ Saved Panes: <span style="color:#ff0">${saved.panes?.length || 0}</span></div>
      <div style="margin:3px 0;">▸ Layout: <span style="color:#ff0">${layoutDirection}</span></div>
      <div style="margin:3px 0;">▸ Saved Layout: <span style="color:#ff0">${saved.layoutDirection || "none"}</span></div>
      <div style="margin:3px 0;">▸ Last Save: <span style="color:#ff0">${saveCounter}</span></div>
      <div style="margin-top:8px; display:flex; gap:5px;">
        <button id="debug-save-now" style="flex:1; padding:4px 8px; cursor:pointer; background:#282; border:1px solid #4a4; color:#fff; border-radius:4px; font-size:10px;">💾 Save Now</button>
        <button id="debug-clear" style="flex:1; padding:4px 8px; cursor:pointer; background:#822; border:1px solid #a44; color:#fff; border-radius:4px; font-size:10px;">🗑️ Clear</button>
      </div>
    `;

    panel.querySelector("#debug-save-now")?.addEventListener("click", () => {
      window.debugSaveNow?.();
    });

    panel.querySelector("#debug-clear")?.addEventListener("click", () => {
      window.debugClearSession?.();
    });
  };

  update();
  setInterval(update, 500);
}

window.addEventListener("DOMContentLoaded", async () => {
  console.log("🚀 [App] Starting Robsidian...");
  // Lucide Icons初期化
  initializeLucideIcons();
  // サイドバーマネージャー初期化
  console.log("📌 [App] Initializing SidebarManager...");
  globalSidebarManager = new SidebarManager();
  console.log("✨ [App] Initializing SnippetManager...");
  globalSnippetManager = new SnippetManager();
  if (currentPath) {
    await globalSnippetManager.scanSnippets(currentPath);
  }
  console.log("✅ [App] Initialization complete.");

  // Reload Snippets Button
  const btnReloadSnippets = document.getElementById("btn-reload-snippets");
  if (btnReloadSnippets) {
    btnReloadSnippets.onclick = async () => {
      if (globalSnippetManager && currentPath) {
        await globalSnippetManager.scanSnippets(currentPath);
        renderSettingsUI(); // 再レンダリング
        showNotice("Snippets reloaded");
      }
    };
  }
  const getEl = (id: string) => document.getElementById(id);
  const defaultFont = "'HackGen', 'HackGen Console', sans-serif";
  let currentFont = localStorage.getItem("robsidian-font") || defaultFont;
  let currentFontSize = localStorage.getItem("robsidian-font-size") || "14";
  let currentLineHeight =
    localStorage.getItem("robsidian-line-height") || "1.5";
  document.documentElement.style.setProperty("--font-family-ui", currentFont);
  document.documentElement.style.setProperty("--font-family-code", currentFont);
  document.documentElement.style.setProperty(
    "--base-font-size",
    `${currentFontSize}px`,
  );
  document.documentElement.style.setProperty(
    "--base-line-height",
    currentLineHeight,
  );

  applyZoom(currentZoom);

  window.addEventListener(
    "wheel",
    (e) => {
      if (e.ctrlKey) {
        e.preventDefault();
        const delta = -e.deltaY;
        const newZoom = currentZoom + (delta > 0 ? 0.1 : -0.1);
        applyZoom(newZoom);
      }
    },
    { passive: false },
  );

  window.addEventListener("keydown", (e) => {
    if (e.ctrlKey) {
      if (e.key === "=" || e.key === "+") {
        e.preventDefault();
        applyZoom(currentZoom + 0.1);
      } else if (e.key === "-") {
        e.preventDefault();
        applyZoom(currentZoom - 0.1);
      } else if (e.key === "0") {
        e.preventDefault();
        applyZoom(1.0);
      }
    }
  });

  window.addEventListener("beforeunload", () => {
    forceSaveSession();
  });

  console.log("📊 [App] Creating PaneManager...");
  const paneManager = new PaneManager("panes-container");
  globalPaneManager = paneManager;
  console.log("📊 [App] PaneManager created, panes:", paneManager.panes.length);

  // ★デバッグ用グローバル公開
  window.debugPaneManager = globalPaneManager;
  window.debugSaveSession = () => {
    if (globalPaneManager) {
      const state = globalPaneManager.saveState();
      localStorage.setItem("robsidian-session", JSON.stringify(state));
      console.log("💾 [Debug] Manual save:", state);
      showNotice(`Saved ${state.panes.length} panes`);
    }
  };
  window.debugSaveNow = window.debugSaveSession;
  window.debugClearSession = () => {
    if (confirm("Clear session and reload?")) {
      localStorage.removeItem("robsidian-session");
      location.reload();
    }
  };

  // ★デバッグパネル表示（常時有効）
  createDebugPanel();

  globalCommandManager = new CommandManager();
  globalCommandManager.register({
    id: "app:split-pane",
    name: "Split Pane",
    callback: () => globalPaneManager?.addPane(),
    hotkeys: [{ modifiers: ["Mod"], key: "\\" }],
  });
  globalCommandManager.register({
    id: "app:close-active-pane",
    name: "Close Active Pane",
    callback: () => {
      if (globalPaneManager?.activePane)
        globalPaneManager.removePane(globalPaneManager.activePane);
    },
    hotkeys: [],
  });
  globalCommandManager.register({
    id: "app:new-file",
    name: "New File",
    callback: async () => {
      const f = prompt("Name:");
      if (!f) return;
      try {
        await invoke("create_file", {
          path:
            (currentPath === "."
              ? ""
              : currentPath + (currentPath.includes("/") ? "/" : "\\")) + f,
        });
        await loadFilesGlobal(currentPath);
        globalPaneManager?.openFileInActive({
          name: f,
          path:
            (currentPath === "."
              ? ""
              : currentPath + (currentPath.includes("/") ? "/" : "\\")) + f,
          is_dir: false,
        });
      } catch (e) {
        alert(e);
      }
    },
    hotkeys: [{ modifiers: ["Mod"], key: "n" }],
  });
  globalCommandManager.register({
    id: "app:toggle-sidebar",
    name: "Toggle Sidebar",
    callback: () => {
      document.body.classList.toggle("sidebar-closed");
      saveSession();
    },
    hotkeys: [{ modifiers: ["Mod"], key: "b" }],
  });
  globalCommandManager.register({
    id: "app:open-terminal",
    name: "Open Terminal",
    callback: () => globalPaneManager?.openTerminalInActive(),
    hotkeys: [{ modifiers: ["Mod"], key: "j" }],
  });
  globalCommandManager.register({
    id: "editor:toggle-vim",
    name: "Toggle Vim Mode",
    callback: () => {
      isVimMode = !isVimMode;
      localStorage.setItem("robsidian-vim-mode", String(isVimMode));
      showNotice(`Vim Mode: ${isVimMode ? "ON" : "OFF"}`);
      if (globalPaneManager?.activePane)
        globalPaneManager.activePane.switchTab(
          globalPaneManager.activePane.activeTabId!,
        );
    },
    hotkeys: [],
  });
  globalCommandManager.register({
    id: "editor:toggle-vim",
    name: "Toggle Vim Mode",
    callback: () => {
      isVimMode = !isVimMode;
      localStorage.setItem("robsidian-vim-mode", String(isVimMode));
      showNotice(`Vim Mode: ${isVimMode ? "ON" : "OFF"}`);

      // エディタ設定のキャッシュをクリア
      if (globalPaneManager) {
        globalPaneManager.panes.forEach((pane: any) => {
          if (pane.editorExtensions) {
            pane.editorExtensions = null;
          }
        });

        // アクティブなタブを再レンダリング
        if (globalPaneManager.activePane && globalPaneManager.activePane.activeTabId) {
          globalPaneManager.activePane.switchTab(
            globalPaneManager.activePane.activeTabId
          );
        }
      }
    },
    hotkeys: [],
  });

  const sidebarActions = document.querySelector(".sidebar-actions");
  if (sidebarActions) {
    const btnRotate = document.createElement("button");
    btnRotate.id = "btn-toggle-layout";
    btnRotate.title = "Toggle Horizontal/Vertical Split";
    btnRotate.onclick = () => globalPaneManager?.toggleLayout();
    sidebarActions.appendChild(btnRotate);
    setTimeout(() => globalPaneManager?.updateLayoutDirection(), 0);
  }

  const btnToggleSidebar = getEl("btn-toggle-sidebar");
  const btnOpenVault = getEl("btn-open-vault");
  const btnUp = getEl("btn-up") as HTMLButtonElement;
  const inputSearch = getEl("input-search") as HTMLInputElement;
  const btnSplit = getEl("btn-split");
  const btnOpenTerminalTab = getEl("btn-open-terminal-tab");
  const listEl = getEl("file-list");
  const pathDisplay = getEl("path-display");
  const btnSettings = getEl("btn-settings");
  const modalSettings = getEl("settings-modal");
  const inputFont = getEl("input-font") as HTMLInputElement;
  const displayCurrentFont = getEl("display-current-font");
  const btnToggleTheme = getEl("btn-toggle-theme");
  const btnCloseSettings = getEl("btn-close-settings");
  const btnResetLayout = getEl("btn-reset-layout");
  const btnNewFile = getEl("btn-new-file");
  const btnNewDir = getEl("btn-new-dir");
  const contextMenu = getEl("context-menu");
  const btnAddTab = getEl("btn-add-tab");
  const resizer = getEl("resizer");
  if (!listEl) {
    console.error("Critical: file-list element not found!");
  }
  if (!pathDisplay) {
    console.error("Critical: path-display element not found!");
  }
  const inputFontSize = getEl("input-font-size") as HTMLInputElement;
  const inputLineHeight = getEl("input-line-height") as HTMLInputElement;
  const valFontSize = getEl("val-font-size");
  const valLineHeight = getEl("val-line-height");
  const inputShellPath = document.getElementById(
    "input-shell-path",
  ) as HTMLInputElement;

  const savedTheme = localStorage.getItem("robsidian-theme");
  if (savedTheme === "light") {
    document.body.classList.add("light-theme");
    if (btnToggleTheme) btnToggleTheme.innerText = "Switch to Dark Mode 🌙";
  } else {
    document.body.classList.remove("light-theme");
    if (btnToggleTheme) btnToggleTheme.innerText = "Switch to Light Mode ☀";
  }
  if (btnToggleTheme) {
    btnToggleTheme.onclick = () => {
      if (document.body.classList.contains("light-theme")) {
        document.body.classList.remove("light-theme");
        btnToggleTheme.innerText = "Switch to Light Mode ☀";
        localStorage.setItem("robsidian-theme", "dark");
      } else {
        document.body.classList.add("light-theme");
        btnToggleTheme.innerText = "Switch to Dark Mode 🌙";
        localStorage.setItem("robsidian-theme", "light");
      }
    };
  }

  const renderSettingsUI = () => {
    if (!globalPluginManager) return;
    const sidebarContainer = document.querySelector(".settings-sidebar");
    if (sidebarContainer) {
      sidebarContainer
        .querySelectorAll(".sidebar-item.plugin-setting")
        .forEach((el) => el.remove());
      globalPluginManager.settingTabs.forEach((handler, pluginId) => {
        const item = document.createElement("div");
        item.className = "sidebar-item plugin-setting";
        item.innerText = `⚙️ ${pluginId}`;
        item.dataset.target = `tab-plugin-${pluginId}`;
        item.addEventListener("click", () => {
          document
            .querySelectorAll(".sidebar-item")
            .forEach((i) => i.classList.remove("active"));
          item.classList.add("active");
          document
            .querySelectorAll(".settings-tab")
            .forEach((t) => t.classList.remove("active"));
          let pluginTab = document.getElementById(`tab-plugin-${pluginId}`);
          const contentContainer = document.querySelector(".settings-content");
          if (!pluginTab && contentContainer) {
            pluginTab = document.createElement("div");
            pluginTab.id = `tab-plugin-${pluginId}`;
            pluginTab.className = "settings-tab";
            pluginTab.innerHTML = `<h2>${pluginId} Settings</h2>`;
            const contentDiv = document.createElement("div");
            handler(contentDiv);
            pluginTab.appendChild(contentDiv);
            contentContainer.appendChild(pluginTab);
          }
          if (pluginTab) pluginTab.classList.add("active");
        });
        const footer = document.querySelector(".settings-footer-actions");
        sidebarContainer.insertBefore(item, footer);
      });
    }
    const renderList = (
      containerId: string,
      type: PluginType,
      emptyMsg: string,
    ) => {
      const container = document.getElementById(containerId);
      if (!container) return;
      container.innerHTML = "";
      const items = globalPluginManager!.getPluginsByType(type);
      if (items.length === 0) {
        container.innerHTML = `<p style='color:#888'>${emptyMsg}</p>`;
        return;
      }
      items.forEach(({ manifest }) => {
        const item = document.createElement("div");
        item.className = "plugin-item";
        item.style.cssText =
          "display:flex; justify-content:space-between; align-items:center; padding:10px; border-bottom:1px solid var(--border-color);";
        const info = document.createElement("div");
        info.innerHTML = `<div style="font-weight:bold">${manifest.name} <span style="font-size:0.8em; color:#888">v${manifest.version}</span></div><div style="font-size:0.8em; color:#aaa">${manifest.description || "No description"}</div>`;
        const toggle = document.createElement("input");
        toggle.type = "checkbox";
        toggle.checked = globalPluginManager!.isEnabled(manifest.id);
        toggle.onchange = (e) => {
          if ((e.target as HTMLInputElement).checked) {
            globalPluginManager!.enablePlugin(manifest.id);
          } else {
            globalPluginManager!.disablePlugin(manifest.id);
          }
        };
        item.appendChild(info);
        item.appendChild(toggle);
        container.appendChild(item);
      });
    };
    renderList(
      "plugin-list-container",
      "plugin",
      "No community plugins installed.",
    );
    // ★ Snippet List Rendering
    const renderSnippetList = () => {
      const container = document.getElementById("snippets-list-container");
      if (!container || !globalSnippetManager) return;

      container.innerHTML = "";
      const snippets = globalSnippetManager.getSnippets();

      if (snippets.length === 0) {
        container.innerHTML = `
          <p style='color:var(--text-muted); text-align:center; padding:20px;'>
            No snippets found.<br>
            <span style="font-size:0.85em;">Place <code>.css</code> files in <code>.robsidian/snippets/</code></span>
          </p>
        `;
        return;
      }

      snippets.forEach((snippet) => {
        const item = document.createElement("div");
        item.className = "snippet-item";
        item.style.cssText =
          "display:flex; justify-content:space-between; align-items:center; padding:12px; border-bottom:1px solid var(--border-color);";

        const info = document.createElement("div");
        info.innerHTML = `
          <div style="font-weight:500; margin-bottom:4px;">${snippet.name}</div>
          <div style="font-size:0.85em; color:var(--text-muted); font-family:var(--font-family-code);">${snippet.path}</div>
        `;

        const toggle = document.createElement("input");
        toggle.type = "checkbox";
        toggle.checked = snippet.enabled;
        toggle.onchange = async (e) => {
          const target = e.target as HTMLInputElement;
          if (target.checked) {
            await globalSnippetManager!.enableSnippet(snippet.name + ".css");
          } else {
            globalSnippetManager!.disableSnippet(snippet.name + ".css");
          }
          showNotice(
            `Snippet "${snippet.name}" ${target.checked ? "enabled" : "disabled"}`
          );
        };

        item.appendChild(info);
        item.appendChild(toggle);
        container.appendChild(item);
      });
    };
    const renderThemeList = () => {
      const container = document.getElementById("theme-list-container");
      if (!container) return;
      container.innerHTML = "";
      const themes = globalPluginManager!.getPluginsByType("theme");
      if (themes.length === 0) {
        container.innerHTML = "<p style='color:#888'>No themes found.</p>";
        return;
      }
      const defDiv = document.createElement("div");
      defDiv.style.padding = "5px";
      defDiv.innerHTML = `<label><input type="radio" name="theme-select" value="" ${!themes.some((t) => globalPluginManager!.isEnabled(t.manifest.id)) ? "checked" : ""}> Default Theme</label>`;
      defDiv
        .querySelector("input")
        ?.addEventListener("change", () => globalPluginManager!.setTheme(""));
      container.appendChild(defDiv);
      themes.forEach(({ manifest }) => {
        const div = document.createElement("div");
        div.style.padding = "5px";
        div.innerHTML = `<label><input type="radio" name="theme-select" value="${manifest.id}" ${globalPluginManager!.isEnabled(manifest.id) ? "checked" : ""}> ${manifest.name}</label>`;
        div
          .querySelector("input")
          ?.addEventListener("change", () =>
            globalPluginManager!.setTheme(manifest.id),
          );
        container.appendChild(div);
      });
    };
    renderThemeList();
    renderSnippetList();
    const editorTab = document.getElementById("tab-editor");
    if (editorTab && !document.getElementById("vim-toggle-container")) {
      const vimDiv = document.createElement("div");
      vimDiv.id = "vim-toggle-container";
      vimDiv.className = "setting-item";
      vimDiv.innerHTML = `<div class="setting-info"><div class="setting-name">Vim Mode</div><div class="setting-desc">Enable Vim keybindings for editor.</div></div><div class="setting-control"><input type="checkbox" id="chk-vim-mode" ${isVimMode ? "checked" : ""}></div>`;
      vimDiv.querySelector("input")?.addEventListener("change", (e: any) => {
        isVimMode = e.target.checked;
        localStorage.setItem("robsidian-vim-mode", String(isVimMode));
        if (globalPaneManager?.activePane)
          globalPaneManager.activePane.switchTab(
            globalPaneManager.activePane.activeTabId!,
          );
      });
      editorTab.appendChild(vimDiv);
    }
  };

// Zoom reset button
const btnResetZoom = document.getElementById("btn-reset-zoom");
if (btnResetZoom) {
  btnResetZoom.onclick = () => {
    applyZoom(1.0);
    const zoomDisplay = document.getElementById("current-zoom-display");
    if (zoomDisplay) zoomDisplay.innerText = "100%";
    showNotice("Zoom reset to 100%");
  };
}

// Settings modal open時にzoom表示を更新
if (btnSettings)
  btnSettings.onclick = () => {
    if (displayCurrentFont)
      displayCurrentFont.innerText = `Current: ${currentFont}`;
    if (inputFontSize) {
      inputFontSize.value = currentFontSize;
      if (valFontSize) valFontSize.innerText = `${currentFontSize}px`;
    }
    if (inputLineHeight) {
      inputLineHeight.value = currentLineHeight;
      if (valLineHeight) valLineHeight.innerText = currentLineHeight;
    }
    if (inputShellPath)
      inputShellPath.value = localStorage.getItem("robsidian-shell-path") || "";

    // Zoom表示を更新
    const zoomDisplay = document.getElementById("current-zoom-display");
    if (zoomDisplay) {
      zoomDisplay.innerText = `${Math.round(currentZoom * 100)}%`;
    }

    renderSettingsUI();
    if (modalSettings) modalSettings.style.display = "flex";
  };

  if (btnOpenTerminalTab)
    btnOpenTerminalTab.onclick = () => globalPaneManager?.openTerminalInActive();

  if (btnCloseSettings)
    btnCloseSettings.onclick = () => {
      if (modalSettings) modalSettings.style.display = "none";
      saveSettingsValues();
    };
  document
    .querySelectorAll(".sidebar-item:not(.plugin-setting)")
    .forEach((item) => {
      item.addEventListener("click", () => {
        document
          .querySelectorAll(".sidebar-item")
          .forEach((i) => i.classList.remove("active"));
        document
          .querySelectorAll(".plugin-setting")
          .forEach((i) => i.classList.remove("active"));
        item.classList.add("active");
        const targetId = item.getAttribute("data-target");
        document
          .querySelectorAll(".settings-tab")
          .forEach((t) => t.classList.remove("active"));
        const target = document.getElementById(targetId!);
        if (target) target.classList.add("active");
      });
    });
  function saveSettingsValues() {
    if (inputFont && inputFont.value) {
      currentFont = inputFont.value;
      document.documentElement.style.setProperty(
        "--font-family-ui",
        currentFont,
      );
      document.documentElement.style.setProperty(
        "--font-family-code",
        currentFont,
      );
      localStorage.setItem("robsidian-font", currentFont);
    }
    if (inputFontSize) {
      currentFontSize = inputFontSize.value;
      document.documentElement.style.setProperty(
        "--base-font-size",
        `${currentFontSize}px`,
      );
      localStorage.setItem("robsidian-font-size", currentFontSize);
    }
    if (inputLineHeight) {
      currentLineHeight = inputLineHeight.value;
      document.documentElement.style.setProperty(
        "--base-line-height",
        currentLineHeight,
      );
      localStorage.setItem("robsidian-line-height", currentLineHeight);
    }
    if (inputShellPath) {
      localStorage.setItem("robsidian-shell-path", inputShellPath.value.trim());
    }
  }
  if (inputFontSize)
    inputFontSize.oninput = () => {
      if (valFontSize) valFontSize.innerText = inputFontSize.value;
      saveSettingsValues();
    };
  if (inputLineHeight)
    inputLineHeight.oninput = () => {
      if (valLineHeight) valLineHeight.innerText = inputLineHeight.value;
      saveSettingsValues();
    };
  if (inputFont) inputFont.onchange = saveSettingsValues;
  if (inputShellPath) inputShellPath.onchange = saveSettingsValues;
  if (btnResetLayout)
    btnResetLayout.onclick = () => {
      if (confirm("Are you sure?")) {
        localStorage.clear();
        location.reload();
      }
    };
  if (btnOpenVault)
    btnOpenVault.onclick = async () => {
      const selected = await open({ directory: true });
      if (selected && typeof selected === "string") {
        localStorage.setItem("robsidian-last-path", selected);
        await loadFiles(selected);
      }
    };
  if (btnUp)
    btnUp.onclick = () => {
      if (currentPath.length > 3) {
        const parent = currentPath.substring(
          0,
          Math.max(currentPath.lastIndexOf("\\"), currentPath.lastIndexOf("/")),
        );
        loadFiles(parent || ".");
      }
    };
  if (btnNewFile)
    btnNewFile.onclick = async () => {
      const f = prompt("Name:");
      if (!f) return;
      try {
        await invoke("create_file", {
          path:
            (currentPath === "."
              ? ""
              : currentPath + (currentPath.includes("/") ? "/" : "\\")) + f,
        });
        await loadFiles(currentPath);
        globalPaneManager?.openFileInActive({
          name: f,
          path:
            (currentPath === "."
              ? ""
              : currentPath + (currentPath.includes("/") ? "/" : "\\")) + f,
          is_dir: false,
        });
      } catch (e) {
        alert(e);
      }
    };
  if (btnNewDir)
    btnNewDir.onclick = async () => {
      const d = prompt("Name:");
      if (!d) return;
      try {
        await invoke("create_directory", {
          path:
            (currentPath === "."
              ? ""
              : currentPath + (currentPath.includes("/") ? "/" : "\\")) + d,
        });
        await loadFiles(currentPath);
      } catch (e) {
        alert(e);
      }
    };
  if (btnAddTab)
    btnAddTab.onclick = () => globalPaneManager?.openTerminalInActive();

  if (inputSearch && listEl) {
    const debouncedSearch = debounce(async () => {
      const query = inputSearch.value.trim();

      if (!query) {
        loadFiles(currentPath);
        return;
      }

      try {
        const results = (await invoke("search_files", {
          rootPath: currentPath,
          query,
        })) as SearchResult[];

        if (!listEl) return;

        listEl.innerHTML = "";
        if (results.length === 0) {
          listEl.innerHTML = "<li style='padding:12px; color:var(--text-muted); text-align:center;'>No results found</li>";
          return;
        }

        results.forEach((r) => {
          const li = document.createElement("li");
          li.className = "file-item";
          const row = document.createElement("div");
          row.className = "tree-node";
          row.style.flexDirection = "column";
          row.style.alignItems = "flex-start";
          row.style.padding = "8px 12px";
          row.style.gap = "4px";

          const nameRow = document.createElement("div");
          nameRow.style.display = "flex";
          nameRow.style.alignItems = "center";
          nameRow.style.gap = "8px";
          nameRow.style.width = "100%";

          const iconEl = document.createElement("span");
          iconEl.className = "tree-icon";
          const icon = createLucideIcon(r.is_dir ? 'folder' : 'file-text');
          icon.style.width = '16px';
          icon.style.height = '16px';
          iconEl.appendChild(icon);

          const nameEl = document.createElement("span");
          nameEl.style.fontWeight = "500";
          nameEl.textContent = r.name;

          nameRow.appendChild(iconEl);
          nameRow.appendChild(nameEl);

          const snippetEl = document.createElement("div");
          snippetEl.style.fontSize = "11px";
          snippetEl.style.color = "var(--text-muted)";
          snippetEl.style.marginLeft = "24px";
          snippetEl.style.overflow = "hidden";
          snippetEl.style.textOverflow = "ellipsis";
          snippetEl.style.whiteSpace = "nowrap";
          snippetEl.textContent = r.snippet;

          row.appendChild(nameRow);
          row.appendChild(snippetEl);

          row.onclick = () => {
            if (!r.is_dir) {
              globalPaneManager?.openFileInActive({
                name: r.name,
                path: r.path,
                is_dir: false,
              });
            }
          };

          li.appendChild(row);
          listEl.appendChild(li);
        });
      } catch (error) {
        console.error("Search failed:", error);
        if (listEl) {
          listEl.innerHTML = "<li style='padding:12px; color:#ff3b30; text-align:center;'>Search failed. Check console for details.</li>";
        }
      }
    }, 300);

    inputSearch.addEventListener("input", debouncedSearch);
  }

  let fileMenuActions: MenuItem[] = [
    {
      title: "Rename",
      onClick: async (file) => {
        const n = prompt("Rename:", file.name);
        if (!n) return;
        try {
          await invoke("rename_item", {
            oldPath: file.path,
            newPath: file.path.replace(file.name, n),
          });
          await loadFilesGlobal(currentPath);
        } catch (e) {
          alert(e);
        }
      },
    },
    {
      title: "Delete",
      class: "delete",
      onClick: async (file) => {
        if (confirm(`Delete ${file.name}?`)) {
          try {
            await invoke("delete_item", { path: file.path });
            await loadFilesGlobal(currentPath);
          } catch (e) {
            alert(e);
          }
        }
      },
    },
  ];

  const renderContextMenu = (x: number, y: number, file: FileEntry) => {
    if (!contextMenu) return;
    contextMenu.innerHTML = "";
    fileMenuActions.forEach((action) => {
      const item = document.createElement("div");
      item.className = `menu-item ${action.class || ""}`;
      item.innerText = action.title;
      item.onclick = () => {
        action.onClick(file);
        contextMenu.style.display = "none";
      };
      contextMenu.appendChild(item);
    });
    contextMenu.style.display = "flex";
    contextMenu.style.left = `${x}px`;
    contextMenu.style.top = `${y}px`;
  };

  document.addEventListener("click", (e) => {
    if (
      contextMenu &&
      e.target !== contextMenu &&
      !contextMenu.contains(e.target as Node)
    )
      contextMenu.style.display = "none";
  });

  function createTreeNode(entry: FileEntry): HTMLLIElement {
    const li = document.createElement("li");
    li.className = "file-item";

    const row = document.createElement("div");
    row.className = "tree-node";

    const arrow = document.createElement("span");
    if (entry.is_dir) {
      arrow.className = "tree-arrow";
      const chevron = createLucideIcon('chevron-right');
      chevron.style.width = '14px';
      chevron.style.height = '14px';
      arrow.appendChild(chevron);

      if (expandedPaths.has(entry.path)) {
        arrow.classList.add('open');
        arrow.innerHTML = '';
        const chevronDown = createLucideIcon('chevron-down');
        chevronDown.style.width = '14px';
        chevronDown.style.height = '14px';
        arrow.appendChild(chevronDown);
      }
    } else {
      arrow.className = "tree-spacer";
    }

    row.appendChild(arrow);

    const icon = document.createElement("span");
    icon.className = "tree-icon";
    const iconElement = createLucideIcon(entry.is_dir ? 'folder' : 'file');
    iconElement.style.width = '16px';
    iconElement.style.height = '16px';
    icon.appendChild(iconElement);

    row.appendChild(icon);

    const label = document.createElement("span");
    label.className = "tree-label";
    label.textContent = entry.name;
    row.appendChild(label);

    li.appendChild(row);
    let childrenContainer: HTMLUListElement | null = null;
    if (entry.is_dir) {
      childrenContainer = document.createElement("ul");
      childrenContainer.className = "tree-children";
      if (expandedPaths.has(entry.path)) {
        childrenContainer.classList.add("show");
        toggleDirectory(entry, arrow, childrenContainer, true);
      }
      li.appendChild(childrenContainer);
    }
    row.onclick = (e) => {
      e.stopPropagation();
      document
        .querySelectorAll(".tree-node.selected")
        .forEach((el) => el.classList.remove("selected"));
      row.classList.add("selected");
      if (entry.is_dir && childrenContainer) {
        toggleDirectory(entry, arrow, childrenContainer);
      } else {
        globalPaneManager?.openFileInActive(entry);
      }
    };
    row.oncontextmenu = (e) => {
      e.preventDefault();
      e.stopPropagation();
      renderContextMenu(e.pageX, e.pageY, entry);
    };
    return li;
  }

  async function toggleDirectory(
    entry: FileEntry,
    arrow: HTMLElement,
    container: HTMLUListElement,
    forceOpen = false,
  ) {
    const isClosed = !container.classList.contains("show");
    if (isClosed || forceOpen) {
      container.classList.add("show");
      arrow.classList.add("open");
      expandedPaths.add(entry.path);
      if (container.children.length === 0) {
        container.innerHTML = `<li style="padding-left:20px; color:#666;">Loading...</li>`;
        try {
          const files = (await invoke("list_files", {
            path: entry.path,
          })) as FileEntry[];
          currentFilesCache = [...currentFilesCache, ...files];
          currentFilesCache = Array.from(
            new Map(currentFilesCache.map((f) => [f.path, f])).values(),
          );
          container.innerHTML = "";
          files.sort((a, b) => {
            if (a.is_dir && !b.is_dir) return -1;
            if (!a.is_dir && b.is_dir) return 1;
            return a.name.localeCompare(b.name);
          });
          if (files.length === 0)
            container.innerHTML = `<li style="padding-left:20px; color:#666; font-size:0.8em;">(Empty)</li>`;
          else
            files.forEach((child) =>
              container.appendChild(createTreeNode(child)),
            );
        } catch (e) {
          console.error(e);
          container.innerHTML = `<li style="color:red;">Error loading</li>`;
        }
      }
    } else {
      container.classList.remove("show");
      arrow.classList.remove("open");
      expandedPaths.delete(entry.path);
    }
  }

  async function loadFiles(path: string) {
    if (!listEl || !pathDisplay || !btnUp) return;
    try {
      currentPath = path;
      localStorage.setItem("robsidian-last-path", path);
      pathDisplay.innerText =
        path === "." ? "ROOT" : path.split("\\").pop() || path;
      btnUp.disabled = path === "." || path.length < 3;
      listEl.innerHTML = "";
      const files = (await invoke("list_files", { path })) as FileEntry[];
      currentFilesCache = files;
      files.sort((a, b) => {
        if (a.is_dir && !b.is_dir) return -1;
        if (!a.is_dir && b.is_dir) return 1;
        return a.name.localeCompare(b.name);
      });
      files.forEach((entry) => listEl.appendChild(createTreeNode(entry)));
    } catch (e) {
      console.error(e);
    }
  }

  loadFilesGlobal = loadFiles;

  let isResizing = false;
  if (resizer) {
    resizer.addEventListener("mousedown", () => {
      isResizing = true;
      document.body.style.cursor = "col-resize";
      resizer.classList.add("resizing");
    });
    document.addEventListener("mousemove", (e) => {
      if (!isResizing) return;
      const rawX = e.clientX / currentZoom;
      const newWidth = Math.max(150, Math.min(rawX - 50, 600));
      document.documentElement.style.setProperty(
        "--sidebar-width",
        `${newWidth}px`,
      );
    });
    document.addEventListener("mouseup", () => {
      if (isResizing) {
        isResizing = false;
        document.body.style.cursor = "default";
        resizer.classList.remove("resizing");
        saveSession();
      }
    });
  }

  if (btnToggleSidebar)
    btnToggleSidebar.onclick = () => {
      document.body.classList.toggle("sidebar-closed");
      saveSession();
    };

  if (btnSplit) btnSplit.onclick = () => paneManager.addPane();

  console.log("📊 [App] Before restoreSession, panes:", paneManager.panes.length);
  await restoreSession(paneManager);
  console.log("📊 [App] After restoreSession, panes:", paneManager.panes.length);

  await loadFiles(currentPath);
  console.log("📊 [App] After loadFiles, panes:", paneManager.panes.length);

  // API Initialization
  window.app = {
    workspace: {
      split: () => {
        paneManager.addPane();
      },
      toggleLayout: () => {
        paneManager.toggleLayout();
      },
      openFile: async (path: string) => {
        const name = path.split(/[\/\\]/).pop() || "Unknown";
        paneManager.openFileInActive({ name, path, is_dir: false });
      },
      openTerminal: (title, cmd) => {
        paneManager.activePane?.openTerminal(title, cmd);
      },
      getLeaves: () => paneManager.panes,
      activeLeaf: paneManager.activePane,
    },
    vault: {
      read: async (path: string) => await invoke("read_file_content", { path }),
      create: async (path: string) => await invoke("create_file", { path }),
      write: async (path: string, content: string) =>
        await invoke("save_file_content", { path, content }),
      createDir: async (path: string) => {
        try {
          await invoke("create_directory", { path });
        } catch (e: any) {
          if (!e.toString().includes("exists")) throw e;
        }
      },
      delete: async (path: string) => await invoke("delete_item", { path }),
      rename: async (oldPath: string, newPath: string) =>
        await invoke("rename_item", { oldPath, newPath }),
      list: async (path: string) => await invoke("list_files", { path }),
      getResourcePath: (path: string) => convertFileSrc(path),
      exists: () => true,
      root: currentPath,
    },
    sidebar: globalSidebarManager?.getSidebarAPI(),
    commands: {
      register: (cmd) => globalCommandManager?.register(cmd),
      execute: (id) => globalCommandManager?.execute(id),
    },
    menus: {
      registerFileMenu: (item) => {
        fileMenuActions.push(item);
      },
    },
    statusBar: {
      setStatus: (msg) => {
        const el = document.getElementById("status-plugin-info");
        if (el) el.innerText = msg;
      },
      setMode: (mode) => {
        const el = document.getElementById("status-mode");
        if (el) el.innerText = mode;
      },
    },
    utils: {
      clipboardWrite: async (text: string) =>
        await navigator.clipboard.writeText(text),
      clipboardRead: async () => await navigator.clipboard.readText(),
      notice: (message: string) => showNotice(message),
    },
  };

  console.log("🔌 [App] Before plugin scan, panes:", paneManager.panes.length);
  globalPluginManager = new PluginManager(window.app);
  if (currentPath) {
    await globalPluginManager.scanAll(currentPath);
  }
  console.log("🔌 [App] After plugin scan, panes:", paneManager.panes.length);

  window.robsidianOpenHelix = (entry: FileEntry) => {
    if (!globalPaneManager) return;
    if (!globalPaneManager.activePane) globalPaneManager.addPane();
    const pane = globalPaneManager.activePane!;
    const shellPath = localStorage.getItem("robsidian-shell-path") || "";
    const isCmd = shellPath.toLowerCase().includes("cmd");
    const normalizedPath = entry.path.replace(/\\/g, "/");
    let safePath: string;
    if (isCmd) {
      safePath = `"${normalizedPath}"`;
    } else {
      safePath = `'${normalizedPath}'`;
    }
    pane.openTerminal(`Hx: ${entry.name}`, `hx ${safePath}`);
  };

  console.log("✅ [App] Initialization complete. Final panes:", paneManager.panes.length);
});
