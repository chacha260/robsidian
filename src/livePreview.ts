// src/livePreview.ts - Live Preview機能（StateField方式）

import {
  Decoration,
  DecorationSet,
  EditorView,
  WidgetType,
} from "@codemirror/view";
import { Extension, StateField } from "@codemirror/state";
import { convertFileSrc } from "@tauri-apps/api/core";
import hljs from "highlight.js";
import mermaid from "mermaid";

// ============================================================================
// Widget Decorations（変更なし）
// ============================================================================
class BoldWidget extends WidgetType {
  constructor(private text: string) { super(); }
  toDOM() {
    const strong = document.createElement("strong");
    strong.textContent = this.text;
    strong.className = "cm-bold-rendered";
    return strong;
  }
  ignoreEvent() { return false; }
}

class ItalicWidget extends WidgetType {
  constructor(private text: string) { super(); }
  toDOM() {
    const em = document.createElement("em");
    em.textContent = this.text;
    em.className = "cm-italic-rendered";
    return em;
  }
  ignoreEvent() { return false; }
}

class InlineCodeWidget extends WidgetType {
  constructor(private text: string) { super(); }
  toDOM() {
    const code = document.createElement("code");
    code.textContent = this.text;
    code.className = "cm-code-rendered";
    return code;
  }
  ignoreEvent() { return false; }
}

class CodeBlockWidget extends WidgetType {
  constructor(private code: string, private language: string) { super(); }
  toDOM() {
    const container = document.createElement("div");
    container.className = "cm-code-block-rendered";
    container.style.cssText = "margin: 1em 0; cursor: text;";

    if (this.language) {
      const label = document.createElement("div");
      label.className = "code-block-language";
      label.textContent = this.language;
      container.appendChild(label);
    }

    const pre = document.createElement("pre");
    pre.style.margin = "0";
    const code = document.createElement("code");

    if (this.language && hljs.getLanguage(this.language)) {
      try {
        const highlighted = hljs.highlight(this.code, { language: this.language });
        code.innerHTML = highlighted.value;
        code.className = `hljs language-${this.language}`;
      } catch (e) {
        code.textContent = this.code;
      }
    } else {
      code.textContent = this.code;
    }

    pre.appendChild(code);
    container.appendChild(pre);
    return container;
  }
  ignoreEvent() { return false; }
}

class MermaidWidget extends WidgetType {
  constructor(private code: string) { super(); }
  toDOM() {
    const container = document.createElement("div");
    container.className = "cm-mermaid-rendered";
    container.style.cssText = "margin: 1em 0; padding: 20px; cursor: text;";

    const id = `mermaid-${Math.random().toString(36).substr(2, 9)}`;
    container.innerHTML = '<div style="padding: 20px; text-align: center; color: var(--text-muted);">Rendering diagram...</div>';

    mermaid.render(`mermaid-svg-${id}`, this.code)
      .then((result) => {
        container.innerHTML = result.svg;
        const svg = container.querySelector('svg');
        if (svg) {
          svg.style.maxWidth = '100%';
          svg.style.height = 'auto';
        }
      })
      .catch((error) => {
        container.innerHTML = `<pre style="color: red; padding: 20px;">Mermaid Error: ${error.message}</pre>`;
      });

    return container;
  }
  ignoreEvent() { return false; }
}

class TableWidget extends WidgetType {
  constructor(private lines: string[]) { super(); }
  toDOM() {
    const container = document.createElement("div");
    container.className = "cm-table-rendered";
    container.style.cssText = "margin: 1em 0; cursor: text;";

    const table = document.createElement("table");
    table.className = "markdown-table";

    const headerRow = this.lines[0];
    const headers = headerRow.split("|").map((h) => h.trim()).filter((h) => h);

    const thead = document.createElement("thead");
    const headerTr = document.createElement("tr");
    headers.forEach((h) => {
      const th = document.createElement("th");
      th.textContent = h;
      headerTr.appendChild(th);
    });
    thead.appendChild(headerTr);
    table.appendChild(thead);

    const tbody = document.createElement("tbody");
    for (let i = 2; i < this.lines.length; i++) {
      const row = this.lines[i];
      const cells = row.split("|").map((c) => c.trim()).filter((c) => c);
      const tr = document.createElement("tr");
      cells.forEach((c) => {
        const td = document.createElement("td");
        td.textContent = c;
        tr.appendChild(td);
      });
      tbody.appendChild(tr);
    }
    table.appendChild(tbody);

    container.appendChild(table);
    return container;
  }
  ignoreEvent() { return false; }
}

class ListItemWidget extends WidgetType {
  constructor(
    private markerType: "unordered" | "ordered",
    private markerText: string,
    private content: string,
    private indentLevel: number = 0
  ) { super(); }

  toDOM() {
    const span = document.createElement("span");
    span.className = "cm-list-item-rendered";
    span.style.cssText = "display: inline; cursor: text;";

    if (this.indentLevel > 0) {
      span.style.paddingLeft = `${this.indentLevel * 24}px`;
    }

    let marker = "";
    if (this.markerType === "ordered") {
      marker = this.markerText;
    } else {
      const markers = ["•", "◦", "▪", "▫"];
      marker = markers[Math.min(this.indentLevel, markers.length - 1)];
    }

    const markerSpan = document.createElement("span");
    markerSpan.className = "cm-list-marker";
    markerSpan.textContent = marker + " ";
    span.appendChild(markerSpan);

    const contentSpan = document.createElement("span");
    contentSpan.innerHTML = this.parseInlineMarkdown(this.content);
    span.appendChild(contentSpan);

    return span;
  }

  private parseInlineMarkdown(text: string): string {
    let html = this.escapeHtml(text);
    html = html.replace(/\*\*\*([^*]+)\*\*\*/g, '<strong><em>$1</em></strong>');
    html = html.replace(/___([^_]+)___/g, '<strong><em>$1</em></strong>');
    html = html.replace(/\*\*([^*]+)\*\*/g, '<strong class="cm-bold-rendered">$1</strong>');
    html = html.replace(/__([^_]+)__/g, '<u class="cm-underline-rendered">$1</u>');
    html = html.replace(/(?<!\*)\*([^*]+)\*(?!\*)/g, '<em class="cm-italic-rendered">$1</em>');
    html = html.replace(/(?<!_)_([^_]+)_(?!_)/g, '<em class="cm-italic-rendered">$1</em>');
    html = html.replace(/~~([^~]+)~~/g, '<del class="cm-strikethrough-rendered">$1</del>');
    html = html.replace(/==([^=]+)==/g, '<mark class="cm-highlight-rendered">$1</mark>');
    html = html.replace(/`([^`]+)`/g, (_, code) => {
      return `<code class="cm-code-rendered">${this.escapeHtml(code)}</code>`;
    });
    html = html.replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_, text, url) => {
      const safeUrl = this.sanitizeUrl(url);
      const safeText = this.escapeHtml(text);
      return `<a href="${safeUrl}" class="cm-link-rendered" target="_blank" rel="noopener noreferrer">${safeText}</a>`;
    });
    html = html.replace(/\^([^^]+)\^/g, '<sup class="cm-superscript-rendered">$1</sup>');
    html = html.replace(/(?<!~)~([^~]+)~(?!~)/g, '<sub class="cm-subscript-rendered">$1</sub>');
    return html;
  }

  private escapeHtml(text: string): string {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
  }

  private sanitizeUrl(url: string): string {
    const trimmed = url.trim();
    const dangerous = /^(javascript|data|vbscript|file):/i;
    if (dangerous.test(trimmed)) return '#blocked-url';
    if (!/^(https?:|mailto:|\/|\.\/|\.\.\/|#)/.test(trimmed)) return '#invalid-url';
    return trimmed;
  }

  ignoreEvent() { return false; }
}

class ImageWidget extends WidgetType {
  constructor(private altText: string, private url: string, private currentFilePath: string) { super(); }
  toDOM() {
    const img = document.createElement("img");
    img.alt = this.altText;
    img.className = "cm-image-rendered";
    img.style.cssText = "max-width: 100%; border-radius: var(--radius-md); box-shadow: var(--shadow-md); margin: 1em 0; display: block; cursor: pointer;";

    let finalUrl = this.url;
    if (!this.url.startsWith("http") && !this.url.startsWith("data:") && !this.url.startsWith("asset:")) {
      const sep = this.currentFilePath.includes("/") ? "/" : "\\";
      const dirPath = this.currentFilePath.substring(0, this.currentFilePath.lastIndexOf(sep));
      const absolutePath = `${dirPath}${sep}${this.url.replace(/\//g, sep)}`;
      try {
        finalUrl = convertFileSrc(absolutePath);
      } catch (e) {
        console.error("Failed to convert image path:", e);
      }
    }
    img.src = finalUrl;
    return img;
  }
  ignoreEvent() { return false; }
}

class CheckboxWidget extends WidgetType {
  constructor(
    private checked: boolean,
    private content: string,
    private indentLevel: number,
    private lineFrom: number,
    private lineTo: number
  ) { super(); }

  toDOM(view: EditorView) { // ★ toDOM は view を受け取れる
    const container = document.createElement("span");
    container.className = "cm-checkbox-rendered";
    container.style.cssText = "display: inline; cursor: text;";

    if (this.indentLevel > 0) {
      container.style.paddingLeft = `${this.indentLevel * 24}px`;
    }

    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = this.checked;
    checkbox.className = "cm-checkbox-input";

    checkbox.onmousedown = (e) => e.stopPropagation();

    // ★ view を使って更新
    checkbox.onchange = (e) => {
      e.stopPropagation();
      const newChar = this.checked ? ' ' : 'x';
      const currentText = view.state.doc.sliceString(this.lineFrom, this.lineTo);
      const newText = currentText.replace(/\[[ xX]?\]/, `[${newChar}]`);
      view.dispatch({
        changes: { from: this.lineFrom, to: this.lineTo, insert: newText }
      });
    };

    const text = document.createElement("span");
    text.className = "cm-checkbox-text";
    text.textContent = this.content;
    if (this.checked) {
      text.style.textDecoration = "line-through";
      text.style.opacity = "0.6";
    }

    container.appendChild(checkbox);
    container.appendChild(text);
    return container;
  }

  ignoreEvent(e: Event): boolean {
    return !!(e.target && (e.target as HTMLElement).tagName === "INPUT");
  }
}

class BlockquoteInlineWidget extends WidgetType {
  constructor(private content: string) { super(); }
  toDOM() {
    const span = document.createElement("span");
    span.className = "cm-blockquote-inline-rendered";
    span.style.cssText = "display: inline; cursor: text;";
    const marker = document.createElement("span");
    marker.className = "cm-blockquote-marker";
    marker.textContent = "▎ ";
    const text = document.createElement("span");
    text.textContent = this.content;
    span.appendChild(marker);
    span.appendChild(text);
    return span;
  }
  ignoreEvent() { return false; }
}

class HorizontalRuleWidget extends WidgetType {
  toDOM() {
    const hr = document.createElement("hr");
    hr.className = "cm-hr-rendered";
    hr.style.cssText = "border: none; border-top: 2px solid var(--border-color); margin: 2em 0; cursor: default; display: block;";
    return hr;
  }
  ignoreEvent() { return false; }
}

class LinkWidget extends WidgetType {
  constructor(private text: string, private url: string) { super(); }
  toDOM() {
    const a = document.createElement("a");
    a.textContent = this.text;
    a.href = this.url;
    a.className = "cm-link-rendered";
    a.target = "_blank";
    a.rel = "noopener noreferrer";
    a.onmousedown = (e) => {
      if (e.ctrlKey || e.metaKey) {
        e.stopPropagation();
      }
    };
    return a;
  }
  ignoreEvent(e: Event) {
    if ((e.type === "mousedown" || e.type === "click") && e instanceof MouseEvent) {
      if (e.ctrlKey || e.metaKey) {
        return true;
      }
    }
    return false;
  }
}

class StrikethroughWidget extends WidgetType {
  constructor(private text: string) { super(); }
  toDOM() {
    const del = document.createElement("del");
    del.textContent = this.text;
    del.className = "cm-strikethrough-rendered";
    return del;
  }
  ignoreEvent() { return false; }
}

class UnderlineWidget extends WidgetType {
  constructor(private text: string) { super(); }
  toDOM() {
    const u = document.createElement("u");
    u.textContent = this.text;
    u.className = "cm-underline-rendered";
    return u;
  }
  ignoreEvent() { return false; }
}

class HighlightWidget extends WidgetType {
  constructor(private text: string) { super(); }
  toDOM() {
    const mark = document.createElement("mark");
    mark.textContent = this.text;
    mark.className = "cm-highlight-rendered";
    return mark;
  }
  ignoreEvent() { return false; }
}

class SuperscriptWidget extends WidgetType {
  constructor(private text: string) { super(); }
  toDOM() {
    const sup = document.createElement("sup");
    sup.textContent = this.text;
    sup.className = "cm-superscript-rendered";
    return sup;
  }
  ignoreEvent() { return false; }
}

class SubscriptWidget extends WidgetType {
  constructor(private text: string) { super(); }
  toDOM() {
    const sub = document.createElement("sub");
    sub.textContent = this.text;
    sub.className = "cm-subscript-rendered";
    return sub;
  }
  ignoreEvent() { return false; }
}

// ============================================================================
// ヘルパー関数
// ============================================================================

function getActiveLine(state: any): number {
  const pos = state.selection.main.head;
  return state.doc.lineAt(pos).number;
}

function calculateIndentLevel(indent: string): number {
  let level = 0;
  for (let i = 0; i < indent.length; i++) {
    if (indent[i] === '\t') {
      level++;
    } else if (indent[i] === ' ') {
      if (i + 1 < indent.length && indent[i + 1] === ' ') {
        level++;
        i++;
      }
    }
  }
  return level;
}

function isCursorInBlock(state: any, blockStart: number, blockEnd: number): boolean {
  const cursor = state.selection.main.head;
  return cursor >= blockStart && cursor <= blockEnd;
}

// ============================================================================
// ブロック要素の検出
// ============================================================================

interface BlockElement {
  type: "codeblock" | "mermaid" | "table";
  startLine: number;
  endLine: number;
  content: string[];
  language?: string;
  from: number;
  to: number;
}

function findBlockElements(state: any): BlockElement[] {
  const blocks: BlockElement[] = [];
  const doc = state.doc;
  let i = 1;

  while (i <= doc.lines) {
    const line = doc.line(i);
    const lineText = line.text;

    const codeBlockMatch = lineText.match(/^```(\w*)$/);
    if (codeBlockMatch) {
      const language = codeBlockMatch[1] || "";
      const startLine = i;
      const content: string[] = [];
      const blockFrom = line.from;
      i++;

      while (i <= doc.lines) {
        const innerLine = doc.line(i);
        const innerText = innerLine.text;

        if (innerText.trim() === "```") {
          const isMermaid = language.toLowerCase() === "mermaid";
          blocks.push({
            type: isMermaid ? "mermaid" : "codeblock",
            startLine,
            endLine: i,
            content,
            language: isMermaid ? undefined : language,
            from: blockFrom,
            to: innerLine.to,
          });
          break;
        }
        content.push(innerText);
        i++;
      }
      i++;
      continue;
    }

    if (lineText.trim().startsWith("|") && lineText.trim().endsWith("|")) {
      const startLine = i;
      const content: string[] = [lineText];
      const blockFrom = line.from;
      i++;

      if (i <= doc.lines) {
        const sepLine = doc.line(i);
        const sepText = sepLine.text.trim();
        const isSeparator = /^\|[\s\-:]+\|$/.test(sepText) || /^\|(\s*:?-+:?\s*\|)+$/.test(sepText);

        if (isSeparator) {
          content.push(sepText);
          i++;

          while (i <= doc.lines) {
            const tableLine = doc.line(i);
            const tableText = tableLine.text.trim();
            if (!tableText.startsWith("|") || !tableText.endsWith("|")) break;
            content.push(tableText);
            i++;
          }

          blocks.push({
            type: "table",
            startLine,
            endLine: i - 1,
            content,
            from: blockFrom,
            to: doc.line(i - 1).to,
          });
          continue;
        }
      }
    }

    i++;
  }

  return blocks;
}

// ============================================================================
// デコレーション生成（state を受け取るように修正）
// ============================================================================

function buildDecorations(state: any): DecorationSet {
  const widgets: any[] = [];
  const activeLine = getActiveLine(state);
  const doc = state.doc;
  const blocks = findBlockElements(state);

  // ★ ブロック要素の処理
  for (const block of blocks) {
    const isEditing = isCursorInBlock(state, block.from, block.to);
    if (isEditing) continue;

    if (block.type === "codeblock") {
      widgets.push(
        Decoration.replace({
          widget: new CodeBlockWidget(block.content.join("\n"), block.language || ""),
        }).range(block.from, block.to)
      );
    } else if (block.type === "mermaid") {
      widgets.push(
        Decoration.replace({
          widget: new MermaidWidget(block.content.join("\n")),
        }).range(block.from, block.to)
      );
    } else if (block.type === "table") {
      widgets.push(
        Decoration.replace({
          widget: new TableWidget(block.content),
        }).range(block.from, block.to)
      );
    }
  }

  const blockLines = new Set<number>();
  for (const block of blocks) {
    for (let i = block.startLine; i <= block.endLine; i++) {
      blockLines.add(i);
    }
  }

  const checkboxPattern = /^([\t ]*)[-*+]\s+\[([ xX]?)\]\s*(.*)$/;

  for (let lineNum = 1; lineNum <= doc.lines; lineNum++) {
    if (blockLines.has(lineNum)) continue;

    const line = doc.line(lineNum);
    const lineText = line.text;
    const isActiveLine = (lineNum === activeLine);

    // 1. まず見出しの処理を行う (カーソル行でも見出しの大きさは維持するため)
    const headingMatch = lineText.match(/^(#{1,6})\s+(.+)$/);
    if (headingMatch) {
      const level = headingMatch[1].length;
      const markerLength = level + 1; // '#' の数 + 半角スペース

      // 行全体をインラインのまま見出しとしてスタイリング
      widgets.push(
        Decoration.line({
          class: `cm-heading-line cm-heading-${level}`,
        }).range(line.from)
      );

      if (isActiveLine) {
        // カーソルがある行: '#' を表示（少し薄く・アクセントカラーにする等の装飾）
        widgets.push(
          Decoration.mark({
            class: "cm-heading-marker",
          }).range(line.from, line.from + markerLength)
        );
      } else {
        // カーソルがない行: '#' を完全に隠す (Obsidianと同じ挙動)
        widgets.push(
          Decoration.replace({}).range(line.from, line.from + markerLength)
        );
      }
      continue;
    }

    // 2. 見出し以外のインライン装飾は、カーソル行では適用しない（生のMarkdownを見せるため）
    if (isActiveLine) continue;

    // チェックボックス（view は不要なので null を渡す）
    const checkboxMatch = lineText.match(checkboxPattern);
    if (checkboxMatch) {
      const indentLevel = calculateIndentLevel(checkboxMatch[1]);
      const checked = checkboxMatch[2].toLowerCase() === 'x';
      const content = checkboxMatch[3];
      widgets.push(
        Decoration.replace({
          widget: new CheckboxWidget(checked, content, indentLevel, line.from, line.to),
          // ★ null ではなく state を渡す
        }).range(line.from, line.to)
      );
      continue;
    }

    // 水平線
    if (/^(\*\*\*|---|___)$/.test(lineText)) {
      widgets.push(
        Decoration.replace({
          widget: new HorizontalRuleWidget(),
        }).range(line.from, line.to)
      );
      continue;
    }

    // 箇条書き（順序なし）
    const ulMatch = lineText.match(/^([\t ]*)[-*+]\s+(.+)$/);
    if (ulMatch) {
      const indentLevel = calculateIndentLevel(ulMatch[1]);
      widgets.push(
        Decoration.replace({
          widget: new ListItemWidget("unordered", "", ulMatch[2], indentLevel),
        }).range(line.from, line.to)
      );
      continue;
    }

    // 箇条書き（順序あり）
    const olMatch = lineText.match(/^([\t ]*)(\d+\.)\s+(.+)$/);
    if (olMatch) {
      const indentLevel = calculateIndentLevel(olMatch[1]);
      widgets.push(
        Decoration.replace({
          widget: new ListItemWidget("ordered", olMatch[2], olMatch[3], indentLevel),
        }).range(line.from, line.to)
      );
      continue;
    }

    // 引用
    const blockquoteMatch = lineText.match(/^>\s+(.+)$/);
    if (blockquoteMatch) {
      widgets.push(
        Decoration.replace({
          widget: new BlockquoteInlineWidget(blockquoteMatch[1]),
        }).range(line.from, line.to)
      );
      continue;
    }

    // インライン記法
    // 画像
    const imagePattern = /!\[([^\]]*)\]\(([^)]+)\)/g;
    let imageMatch: RegExpMatchArray | null;
    imagePattern.lastIndex = 0;
    while ((imageMatch = imagePattern.exec(lineText)) !== null) {
      if (imageMatch.index === undefined) continue;
      const from = line.from + imageMatch.index;
      const to = from + imageMatch[0].length;
      const currentFilePath = (window as any).app?.workspace?.activeLeaf?.tabs?.find((t: any) => t.id === (window as any).app?.workspace?.activeLeaf?.activeTabId)?.file?.path || "";
      widgets.push(
        Decoration.replace({
          widget: new ImageWidget(imageMatch[1], imageMatch[2], currentFilePath),
        }).range(from, to)
      );
    }

    // 太字
    const boldPattern = /\*\*([^*]+)\*\*/g;
    let boldMatch: RegExpMatchArray | null;
    boldPattern.lastIndex = 0;
    while ((boldMatch = boldPattern.exec(lineText)) !== null) {
      if (boldMatch.index === undefined) continue;
      const from = line.from + boldMatch.index;
      const to = from + boldMatch[0].length;
      widgets.push(
        Decoration.replace({
          widget: new BoldWidget(boldMatch[1]),
        }).range(from, to)
      );
    }

    // イタリック
    const italicPattern = /(?<!\*)\*([^*]+)\*(?!\*)/g;
    let italicMatch: RegExpMatchArray | null;
    italicPattern.lastIndex = 0;
    while ((italicMatch = italicPattern.exec(lineText)) !== null) {
      if (italicMatch.index === undefined) continue;
      const from = line.from + italicMatch.index;
      const to = from + italicMatch[0].length;
      widgets.push(
        Decoration.replace({
          widget: new ItalicWidget(italicMatch[1]),
        }).range(from, to)
      );
    }

    // インラインコード
    const codePattern = /`([^`]+)`/g;
    let codeMatch: RegExpMatchArray | null;
    codePattern.lastIndex = 0;
    while ((codeMatch = codePattern.exec(lineText)) !== null) {
      if (codeMatch.index === undefined) continue;
      const from = line.from + codeMatch.index;
      const to = from + codeMatch[0].length;
      widgets.push(
        Decoration.replace({
          widget: new InlineCodeWidget(codeMatch[1]),
        }).range(from, to)
      );
    }

    // リンク
    const linkPattern = /(?<!!)\[([^\]]+)\]\(([^)]+)\)/g;
    let linkMatch: RegExpMatchArray | null;
    linkPattern.lastIndex = 0;
    while ((linkMatch = linkPattern.exec(lineText)) !== null) {
      if (linkMatch.index === undefined) continue;
      const from = line.from + linkMatch.index;
      const to = from + linkMatch[0].length;
      widgets.push(
        Decoration.replace({
          widget: new LinkWidget(linkMatch[1], linkMatch[2]),
        }).range(from, to)
      );
    }

    // 打ち消し線
    const strikethroughPattern = /~~([^~]+)~~/g;
    let strikeMatch: RegExpMatchArray | null;
    strikethroughPattern.lastIndex = 0;
    while ((strikeMatch = strikethroughPattern.exec(lineText)) !== null) {
      if (strikeMatch.index === undefined) continue;
      const from = line.from + strikeMatch.index;
      const to = from + strikeMatch[0].length;
      widgets.push(
        Decoration.replace({
          widget: new StrikethroughWidget(strikeMatch[1]),
        }).range(from, to)
      );
    }

    // 下線
    const underlinePattern = /__([^_]+)__/g;
    let underlineMatch: RegExpMatchArray | null;
    underlinePattern.lastIndex = 0;
    while ((underlineMatch = underlinePattern.exec(lineText)) !== null) {
      if (underlineMatch.index === undefined) continue;
      const from = line.from + underlineMatch.index;
      const to = from + underlineMatch[0].length;
      widgets.push(
        Decoration.replace({
          widget: new UnderlineWidget(underlineMatch[1]),
        }).range(from, to)
      );
    }

    // ハイライト
    const highlightPattern = /==([^=]+)==/g;
    let highlightMatch: RegExpMatchArray | null;
    highlightPattern.lastIndex = 0;
    while ((highlightMatch = highlightPattern.exec(lineText)) !== null) {
      if (highlightMatch.index === undefined) continue;
      const from = line.from + highlightMatch.index;
      const to = from + highlightMatch[0].length;
      widgets.push(
        Decoration.replace({
          widget: new HighlightWidget(highlightMatch[1]),
        }).range(from, to)
      );
    }

    // 上付き文字
    const superscriptPattern = /\^([^^]+)\^/g;
    let superMatch: RegExpMatchArray | null;
    superscriptPattern.lastIndex = 0;
    while ((superMatch = superscriptPattern.exec(lineText)) !== null) {
      if (superMatch.index === undefined) continue;
      const from = line.from + superMatch.index;
      const to = from + superMatch[0].length;
      widgets.push(
        Decoration.replace({
          widget: new SuperscriptWidget(superMatch[1]),
        }).range(from, to)
      );
    }

    // 下付き文字
    const subscriptPattern = /(?<!~)~([^~]+)~(?!~)/g;
    let subMatch: RegExpMatchArray | null;
    subscriptPattern.lastIndex = 0;
    while ((subMatch = subscriptPattern.exec(lineText)) !== null) {
      if (subMatch.index === undefined) continue;
      const from = line.from + subMatch.index;
      const to = from + subMatch[0].length;
      widgets.push(
        Decoration.replace({
          widget: new SubscriptWidget(subMatch[1]),
        }).range(from, to)
      );
    }
  }

  widgets.sort((a, b) => {
    if (a.from !== b.from) return a.from - b.from;
    return a.to - b.to;
  });

  return Decoration.set(widgets);
}

// ============================================================================
// StateField（プラグインではなくStateFieldを使用）
// ============================================================================

export const livePreviewField = StateField.define<DecorationSet>({
  create(state) {
    console.log("🚀 LivePreview StateField created");
    return buildDecorations(state); // ★ これは正しい
  },
  update(decorations, tr) {
    console.log("🔄 LivePreview update, docChanged:", tr.docChanged, "selection:", !!tr.selection);
    if (tr.docChanged || tr.selection) {
      return buildDecorations(tr.state);
    }
    return decorations.map(tr.changes);
  },
  provide(field) {
    return EditorView.decorations.from(field);
  },
});

// ============================================================================
// CSS テーマ
// ============================================================================

const livePreviewTheme = EditorView.theme({
  ".cm-gutters": {
    backgroundColor: "var(--bg-main)",
    color: "var(--text-faint)",
    borderRight: "1px solid var(--border-color)",
  },
  ".cm-activeLineGutter": {
    backgroundColor: "var(--bg-activity)",
    color: "var(--text-main)",
  },
  ".cm-heading-line": {
    fontWeight: "600",
    color: "var(--text-main)",
    lineHeight: "1.2",
    paddingTop: "6px",
    paddingBottom: "2px",
    textDecoration: "none !important", // 念のため大枠の下線をリセット
  },
  
  // ★ ここがポイント：テキスト自体（内側のspan要素など）に付く余計な下線を強制的に消す
  ".cm-heading-line *": {
    textDecoration: "none !important",
    borderBottom: "none !important",
  },

  // 私たちが設定した美しいフルワイドのボーダーだけを残す
  ".cm-heading-1": { 
    fontSize: "1.8em", 
    color: "var(--accent-color)", 
    borderBottom: "2px solid var(--accent-color) !important", 
    paddingBottom: "4px" 
  },
  ".cm-heading-2": { 
    fontSize: "1.5em", 
    color: "var(--accent-color)", 
    borderBottom: "1px solid var(--accent-color) !important", 
    paddingBottom: "4px" 
  },
  ".cm-heading-3": { 
    fontSize: "1.25em", 
    color: "var(--accent-color)"
  },
  ".cm-heading-4": { 
    fontSize: "1.1em", 
    color: "var(--accent-color)"
  },
  ".cm-heading-5": { 
    fontSize: "1.0em", 
    color: "var(--accent-color)"
  },
  ".cm-heading-6": { 
    fontSize: "0.9em", 
    color: "var(--accent-color)"
  },
  
  ".cm-heading-marker": {
    color: "var(--text-muted)",
    opacity: "0.6",
    fontWeight: "normal",
    fontFamily: "var(--font-family-code)",
  },

  ".cm-bold-rendered": { fontWeight: "700" },
  ".cm-italic-rendered": { fontStyle: "italic" },
  ".cm-code-rendered": {
    backgroundColor: "var(--bg-activity)",
    padding: "2px 6px",
    borderRadius: "var(--radius-sm)",
    fontFamily: "var(--font-family-code)",
    fontSize: "0.9em",
  },
  ".cm-strikethrough-rendered": { textDecoration: "line-through", opacity: "0.7" },
  ".cm-underline-rendered": {
    textDecoration: "underline",
    textDecorationColor: "var(--accent-color)",
    textDecorationThickness: "1.5px",
    textUnderlineOffset: "2px",
  },
  ".cm-highlight-rendered": {
    background: "linear-gradient(transparent 50%, rgba(255, 235, 59, 0.5) 50%)",
    padding: "0 2px",
    borderRadius: "2px",
  },
  ".cm-superscript-rendered": { fontSize: "0.75em", verticalAlign: "super", lineHeight: "0" },
  ".cm-subscript-rendered": { fontSize: "0.75em", verticalAlign: "sub", lineHeight: "0" },

  ".cm-link-rendered": {
    color: "var(--accent-color)",
    textDecoration: "none",
    cursor: "pointer",
    borderBottom: "1px solid transparent",
    transition: "all var(--transition-fast)",
  },
  ".cm-link-rendered:hover": {
    color: "var(--accent-hover)",
    borderBottomColor: "var(--accent-hover)",
  },

  ".cm-code-block-rendered": {
    margin: "1em 0",
    borderRadius: "var(--radius-md)",
    overflow: "hidden",
    border: "1px solid var(--border-color)",
    display: "block",
    backgroundColor: "var(--bg-activity)",
  },
  ".code-block-language": {
    fontSize: "0.75em",
    padding: "4px 12px",
    backgroundColor: "var(--bg-activity)",
    color: "var(--text-muted)",
    borderBottom: "1px solid var(--border-color)",
  },
  ".cm-code-block-rendered pre": {
    margin: "0",
    padding: "16px",
    backgroundColor: "var(--bg-activity)",
    overflow: "auto",
  },

  ".cm-mermaid-rendered": {
    margin: "1em 0",
    padding: "20px",
    backgroundColor: "var(--bg-sidebar)",
    borderRadius: "var(--radius-md)",
    border: "1px solid var(--border-color)",
    textAlign: "center",
    display: "block",
  },

  ".cm-table-rendered": { margin: "1em 0", overflow: "auto", display: "block" },
  ".cm-table-rendered table": {
    width: "100%",
    borderCollapse: "collapse",
    border: "1px solid var(--border-color)",
  },
  ".cm-table-rendered th, .cm-table-rendered td": {
    padding: "8px 12px",
    border: "1px solid var(--border-color)",
    textAlign: "left",
  },
  ".cm-table-rendered th": { backgroundColor: "var(--bg-activity)", fontWeight: "600" },

  ".cm-list-item-rendered": { display: "inline", cursor: "text" },
  ".cm-list-marker": {
    color: "var(--accent-color)",
    fontWeight: "600",
    marginRight: "6px",
    userSelect: "none",
    fontSize: "1.1em",
  },

  ".cm-checkbox-rendered": { display: "inline", cursor: "text" },
  ".cm-checkbox-input": {
    width: "16px",
    height: "16px",
    marginRight: "8px",
    cursor: "pointer",
    accentColor: "var(--accent-color)",
  },
  ".cm-checkbox-text": { transition: "all var(--transition-fast)" },

  ".cm-blockquote-inline-rendered": {
    display: "inline",
    cursor: "text",
    color: "var(--text-muted)",
    fontStyle: "italic",
  },
  ".cm-blockquote-marker": {
    color: "var(--accent-color)",
    fontWeight: "700",
    marginRight: "8px",
    fontSize: "1.2em",
  },

  ".cm-hr-rendered": {
    border: "none",
    borderTop: "2px solid var(--border-color)",
    margin: "2em 0",
    cursor: "default",
  },

  ".cm-image-rendered": {
    maxWidth: "100%",
    borderRadius: "var(--radius-md)",
    boxShadow: "var(--shadow-md)",
    margin: "1em 0",
    display: "block",
    cursor: "pointer",
  },

  ".cm-activeLine": { backgroundColor: "rgba(255, 255, 255, 0.03)" },
});

// ============================================================================
// エクスポート
// ============================================================================

export function livePreview(): Extension {
  return [livePreviewField, livePreviewTheme];
}