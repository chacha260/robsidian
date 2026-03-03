// src/backlinks.ts - バックリンク機能
export interface BacklinkEntry {
  file: string;
  path: string;
  context: string;
  line: number;
}

export async function findBacklinks(
  targetFile: string,
  allFiles: string[],
  readFile: (path: string) => Promise<string>
): Promise<BacklinkEntry[]> {
  const backlinks: BacklinkEntry[] = [];
  
  // ターゲットファイル名（拡張子なし）
  const targetName = targetFile.split(/[\/\\]/).pop()?.replace(/\.md$/, '') || '';

  for (const filePath of allFiles) {
    if (filePath === targetFile) continue;

    try {
      const content = await readFile(filePath);
      const lines = content.split('\n');

      lines.forEach((line, index) => {
        let isMatch = false;

        // 1. [[WikiLink]] 形式を検索
        const wikiLinkRegex = /\[\[([^\]]+)\]\]/g;
        let match;
        while ((match = wikiLinkRegex.exec(line)) !== null) {
          // パスが含まれている場合を考慮し、ファイル名だけを抽出して比較
          const linkedName = match[1].split(/[\/\\]/).pop()?.replace(/\.md$/, '') || '';
          if (linkedName.toLowerCase() === targetName.toLowerCase()) {
            isMatch = true;
            break;
          }
        }

        // 2. [text](link) 形式の標準Markdownリンクを検索
        if (!isMatch) {
          const mdLinkRegex = /\[([^\]]+)\]\(([^)]+)\)/g;
          while ((match = mdLinkRegex.exec(line)) !== null) {
            const linkPath = match[2].trim();
            
            // 外部リンク (httpなど) は対象外
            if (linkPath.startsWith('http') || linkPath.startsWith('mailto:')) continue;
            
            // リンクのパスからファイル名（拡張子なし）を抽出して比較
            const linkFileName = linkPath.split(/[\/\\]/).pop()?.replace(/\.md$/, '') || '';
            if (linkFileName.toLowerCase() === targetName.toLowerCase()) {
              isMatch = true;
              break;
            }
          }
        }

        // どちらかの形式でマッチしたらバックリンクに追加
        if (isMatch) {
          backlinks.push({
            file: filePath.split(/[\/\\]/).pop() || filePath,
            path: filePath,
            context: line.trim(),
            line: index + 1,
          });
        }
      });
    } catch (e) {
      console.error(`Failed to read ${filePath}:`, e);
    }
  }

  return backlinks;
}

export function renderBacklinks(
  container: HTMLElement,
  backlinks: BacklinkEntry[],
  onBacklinkClick: (path: string, line: number) => void
) {
  container.innerHTML = '';

  if (backlinks.length === 0) {
    container.innerHTML = `
      <div style="padding: 20px; color: var(--text-muted); text-align: center;">
        No backlinks found
      </div>
    `;
    return;
  }

  backlinks.forEach((backlink) => {
    const item = document.createElement('div');
    item.className = 'backlink-item';
    item.style.cssText = `
      padding: 12px;
      border-bottom: 1px solid var(--border-color);
      cursor: pointer;
      transition: background 0.2s;
    `;

    item.innerHTML = `
      <div style="font-weight: 500; margin-bottom: 4px; color: var(--text-main);">
        ${backlink.file}
      </div>
      <div style="font-size: 0.85em; color: var(--text-muted); font-family: var(--font-family-code);">
        ${backlink.context}
      </div>
    `;

    item.onmouseenter = () => {
      item.style.background = 'var(--bg-hover)';
    };

    item.onmouseleave = () => {
      item.style.background = '';
    };

    item.onclick = () => {
      onBacklinkClick(backlink.path, backlink.line);
    };

    container.appendChild(item);
  });
}