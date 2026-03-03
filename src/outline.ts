// src/outline.ts - アウトライン機能
export interface OutlineItem {
  level: number;
  text: string;
  line: number;
}

export function extractOutline(content: string): OutlineItem[] {
  const lines = content.split('\n');
  const outline: OutlineItem[] = [];

  lines.forEach((line, index) => {
    const match = line.match(/^(#{1,6})\s+(.+)$/);
    if (match) {
      outline.push({
        level: match[1].length,
        text: match[2].trim(),
        line: index + 1,
      });
    }
  });

  return outline;
}

export function renderOutline(
  container: HTMLElement,
  outline: OutlineItem[],
  onItemClick: (line: number) => void
) {
  container.innerHTML = '';

  if (outline.length === 0) {
    container.innerHTML = `
      <div style="padding: 20px; color: var(--text-muted); text-align: center;">
        No headings found
      </div>
    `;
    return;
  }

  const list = document.createElement('div');
  list.className = 'outline-list';

  outline.forEach((item) => {
    const itemEl = document.createElement('div');
    itemEl.className = `outline-item h${item.level}`;
    itemEl.style.paddingLeft = `${(item.level - 1) * 12 + 12}px`;
    itemEl.textContent = item.text;
    itemEl.onclick = () => onItemClick(item.line);
    list.appendChild(itemEl);
  });

  container.appendChild(list);
}