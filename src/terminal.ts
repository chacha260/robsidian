// src/terminal.ts
import { Terminal } from 'xterm';
import { FitAddon } from 'xterm-addon-fit';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import 'xterm/css/xterm.css';

// 簡易的なID生成
function generateId(): string {
  return Math.random().toString(36).substring(2, 15);
}

export class RobsidianTerminal {
  private term: Terminal;
  private fitAddon: FitAddon;
  private container: HTMLElement;
  private ptyId: string;
  private unlistenFn: UnlistenFn | null = null;
  private resizeObserver: ResizeObserver; // 追加

  constructor(parentId: string) {
    const parent = document.getElementById(parentId);
    if (!parent) throw new Error(`Parent element ${parentId} not found`);

    this.ptyId = generateId();

    this.container = document.createElement('div');
    this.container.className = 'terminal-instance'; 
    parent.appendChild(this.container);

    this.term = new Terminal({
      cursorBlink: true,
      fontFamily: '"JetBrains Mono", "Fira Code", Consolas, monospace',
      fontSize: 14,
      lineHeight: 1.2, // 行間を少し調整
      background: '#1e1e1e',
      foreground: '#d4d4d4',
      cursor: '#aeafad',
      allowProposedApi: true,
    });

    this.fitAddon = new FitAddon();
    this.term.loadAddon(this.fitAddon);
    
    this.term.open(this.container);
    
    // ★修正: ResizeObserverを使って、コンテナのサイズ変化を完璧に検知する
    this.resizeObserver = new ResizeObserver(() => {
        // サイズ変更を検知したらFitを実行
        this.fit();
    });
    this.resizeObserver.observe(this.container);

    // Backend接続
    this.initBackend();
  }

  // サイズ合わせ処理を切り出し
  public fit() {
    try {
        this.fitAddon.fit();
        // Backendにも新しいサイズを通知 (ここが重要！)
        // fitAddon.proposeDimensions() で計算された列数・行数を取得
        const dims = this.fitAddon.proposeDimensions();
        if (dims && !isNaN(dims.cols) && !isNaN(dims.rows)) {
            invoke('resize_terminal', {
                id: this.ptyId,
                rows: dims.rows,
                cols: dims.cols,
            }).catch(console.error);
        }
    } catch (e) {
        console.error("Fit error:", e);
    }
  }

  private async initBackend() {
    try {
      this.unlistenFn = await listen<string>(`term-data:${this.ptyId}`, (event) => {
        this.term.write(event.payload);
      });

      this.term.onData((data: string) => {
        invoke('write_to_terminal', { 
            id: this.ptyId, 
            data: data 
        }).catch(err => console.error('Failed to write to pty:', err));
      });

      // xterm内でのリサイズイベント（念の為残す）
      this.term.onResize((size: { cols: number, rows: number }) => {
        invoke('resize_terminal', {
          id: this.ptyId,
          rows: size.rows,
          cols: size.cols,
        }).catch(err => console.error('Resize failed:', err));
      });

      await invoke('create_terminal', { id: this.ptyId });
      
      console.log(`Robsidian Terminal (${this.ptyId}) Connected.`);
      
      // 初期サイズ合わせとフォーカス
      setTimeout(() => {
          this.fit();
          this.term.focus();
      }, 200);

    } catch (e) {
      this.term.writeln(`\r\n\x1b[31mFailed to initialize terminal: ${e}\x1b[0m`);
      console.error(e);
    }
  }

  public destroy() {
    if (this.unlistenFn) this.unlistenFn();
    this.resizeObserver.disconnect(); // 監視停止
    this.term.dispose();
    this.container.remove();
  }
}