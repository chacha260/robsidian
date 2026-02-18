// src/terminal.ts
import { Terminal } from 'xterm';
import { FitAddon } from 'xterm-addon-fit';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import 'xterm/css/xterm.css';

function generateId(): string {
  return Math.random().toString(36).substring(2, 15);
}

export class RobsidianTerminal {
  private term: Terminal;
  private fitAddon: FitAddon;
  public container: HTMLElement; // 外部からアクセス可能にする
  public ptyId: string;
  private unlistenFn: UnlistenFn | null = null;
  private resizeObserver: ResizeObserver;

  // コンストラクタ引数から parentId を削除
  constructor() {
    this.ptyId = generateId();

    // コンテナ作成（親にはまだ追加しない）
    this.container = document.createElement('div');
    this.container.className = 'terminal-instance';
    this.container.style.width = '100%';
    this.container.style.height = '100%';
    // 背景色を明示（チラつき防止）
    this.container.style.backgroundColor = '#1e1e1e';

    this.term = new Terminal({
      cursorBlink: true,
      fontFamily: '"HackGen", "Fira Code", Consolas, monospace',
      fontSize: 14,
      lineHeight: 1.2,
      theme: {
        background: '#1e1e1e',
        foreground: '#d4d4d4',
        cursor: '#aeafad',
      },
      allowProposedApi: true,
    });

    this.fitAddon = new FitAddon();
    this.term.loadAddon(this.fitAddon);
    
    // コンテナにxtermを開く
    this.term.open(this.container);
    
    // リサイズ監視
    this.resizeObserver = new ResizeObserver(() => {
        // DOMに接続されていて表示されている場合のみFit
        if (this.container.isConnected && this.container.offsetParent !== null) {
            this.fit();
        }
    });
    this.resizeObserver.observe(this.container);

    this.initBackend();
  }

  // ★重要: 指定された要素に自身をアタッチする
  public attachTo(parent: HTMLElement) {
    parent.appendChild(this.container);
    // レンダリング待ちをしてからfit
    requestAnimationFrame(() => {
        this.fit();
        this.term.focus();
    });
  }

  // ★重要: 親から自身を切り離す（インスタンスは破棄しない）
  public detach() {
    if (this.container.parentElement) {
        this.container.parentElement.removeChild(this.container);
    }
  }

  public fit() {
    try {
        this.fitAddon.fit();
        const dims = this.fitAddon.proposeDimensions();
        if (dims && !isNaN(dims.cols) && !isNaN(dims.rows)) {
            invoke('resize_terminal', {
                id: this.ptyId,
                rows: dims.rows,
                cols: dims.cols,
            }).catch(console.error);
        }
    } catch (e) {
        // console.warn("Fit skipped:", e);
    }
  }

  public async sendInput(data: string) {
    try {
      await invoke('write_to_terminal', { 
        id: this.ptyId, 
        data: data 
      });
      this.term.focus();
    } catch (e) {
      console.error("Failed to send input:", e);
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

      this.term.onResize((size: { cols: number, rows: number }) => {
        invoke('resize_terminal', {
          id: this.ptyId,
          rows: size.rows,
          cols: size.cols,
        }).catch(err => console.error('Resize failed:', err));
      });

      // ★修正: localStorageからシェル設定を取得して渡す
      const savedShell = localStorage.getItem("robsidian-shell-path") || "";
      
      // 第2引数 shell を追加
      await invoke('create_terminal', { 
          id: this.ptyId,
          shell: savedShell 
      });
      // 初期化完了後、少し待ってからFit（タイミング問題回避）
      setTimeout(() => this.fit(), 100);
      
    } catch (e) {
      this.term.writeln(`\r\n\x1b[31mFailed to initialize terminal: ${e}\x1b[0m`);
    }
  }

  public destroy() {
    if (this.unlistenFn) this.unlistenFn();
    this.resizeObserver.disconnect();
    this.term.dispose();
    this.container.remove();
  }
}