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
  private container: HTMLElement;
  public ptyId: string; // IDは外部から参照したいのでpublicに
  private unlistenFn: UnlistenFn | null = null;
  private resizeObserver: ResizeObserver;

  constructor(parentId: string) {
    const parent = document.getElementById(parentId);
    if (!parent) throw new Error(`Parent element ${parentId} not found`);

    this.ptyId = generateId();

    this.container = document.createElement('div');
    this.container.className = 'terminal-instance';
    // 初期状態は非表示にする（タブ切り替えで表示するため）
    this.container.style.display = 'none';
    this.container.style.width = '100%';
    this.container.style.height = '100%';
    parent.appendChild(this.container);

    this.term = new Terminal({
      cursorBlink: true,
      fontFamily: '"JetBrains Mono", "Fira Code", Consolas, monospace',
      fontSize: 14,
      lineHeight: 1.2,
      // ★修正: themeオブジェクトの中に色設定を入れる
      theme: {
        background: '#1e1e1e',
        foreground: '#d4d4d4',
        cursor: '#aeafad',
      },
      allowProposedApi: true,
    });

    this.fitAddon = new FitAddon();
    this.term.loadAddon(this.fitAddon);
    
    this.term.open(this.container);
    
    this.resizeObserver = new ResizeObserver(() => {
        // 表示されているときだけfitする
        if (this.container.style.display !== 'none') {
            this.fit();
        }
    });
    this.resizeObserver.observe(this.container);

    this.initBackend();
  }

  // ★追加: 表示切替
  public show() {
    this.container.style.display = 'block';
    this.fit();
    this.term.focus();
  }

  // ★追加: 表示切替
  public hide() {
    this.container.style.display = 'none';
  }

  // ★追加: フォーカス用ラッパー (private termへのアクセスのため)
  public focus() {
    this.term.focus();
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
        console.error("Fit error:", e);
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

      await invoke('create_terminal', { id: this.ptyId });
      
      console.log(`Robsidian Terminal (${this.ptyId}) Connected.`);
      
      // 生成直後は表示されないかもしれないので、show()でfitさせる
    } catch (e) {
      this.term.writeln(`\r\n\x1b[31mFailed to initialize terminal: ${e}\x1b[0m`);
      console.error(e);
    }
  }

  public destroy() {
    if (this.unlistenFn) this.unlistenFn();
    this.resizeObserver.disconnect();
    this.term.dispose();
    this.container.remove();
  }
}