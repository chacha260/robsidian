// src/utils/ErrorHandler.ts - エラーハンドリングユーティリティ
import { invoke } from "@tauri-apps/api/core";

export class ErrorHandler {
  private static errorLog: Array<{timestamp: string; context: string; error: string}> = [];
  private static maxLogSize = 100;

  /**
   * エラーをユーザーフレンドリーな形で処理
   */
  static handle(error: unknown, context: string): void {
    console.error(`[${context}]`, error);

    let userMessage = "An error occurred. ";
    let errorDetails = "";

    if (error instanceof Error) {
      errorDetails = error.message;

      // ★ユーザーフレンドリーなメッセージに変換
      if (error.message.includes("ENOENT") || error.message.includes("not found")) {
        userMessage += "File or directory not found.";
      } else if (error.message.includes("EACCES") || error.message.includes("permission")) {
        userMessage += "Permission denied. Check file permissions.";
      } else if (error.message.includes("ENOSPC")) {
        userMessage += "Not enough disk space.";
      } else if (error.message.includes("EEXIST")) {
        userMessage += "File or directory already exists.";
      } else if (error.message.includes("timeout")) {
        userMessage += "Operation timed out. Please try again.";
      } else if (error.message.includes("network") || error.message.includes("fetch")) {
        userMessage += "Network error. Check your connection.";
      } else {
        userMessage += "Please try again or check the console for details.";
      }
    } else {
      errorDetails = String(error);
      userMessage += "An unexpected error occurred.";
    }

    // ★通知表示
    this.showNotice(userMessage, "error");

    // ★エラーログに記録
    this.logError(context, errorDetails);
  }

  /**
   * 成功メッセージ表示
   */
  static success(message: string): void {
    this.showNotice(message, "success");
  }

  /**
   * 警告メッセージ表示
   */
  static warn(message: string): void {
    this.showNotice(message, "warning");
    console.warn(message);
  }

  /**
   * 情報メッセージ表示
   */
  static info(message: string): void {
    this.showNotice(message, "info");
  }

  /**
   * 通知表示（拡張版）
   */
  private static showNotice(message: string, type: "success" | "error" | "warning" | "info" = "info"): void {
    const notice = document.createElement("div");
    notice.className = `notice-toast notice-${type}`;

    // アイコン追加
    const icons = {
      success: "✓",
      error: "✗",
      warning: "⚠",
      info: "ℹ"
    };

    notice.innerHTML = `<span class="notice-icon">${icons[type]}</span><span class="notice-message">${message}</span>`;
    document.body.appendChild(notice);

    setTimeout(() => notice.classList.add("show"), 10);
    setTimeout(() => {
      notice.classList.remove("show");
      setTimeout(() => notice.remove(), 300);
    }, 3000);
  }

  /**
   * エラーログに記録
   */
  private static logError(context: string, error: string): void {
    const timestamp = new Date().toISOString();

    // メモリ内ログ
    this.errorLog.push({ timestamp, context, error });

    // 最大サイズを超えたら古いログを削除
    if (this.errorLog.length > this.maxLogSize) {
      this.errorLog.shift();
    }

    // ファイルへの保存（非同期、失敗しても無視）
    this.saveErrorLog(timestamp, context, error).catch(() => {
      // ログ保存失敗は無視
    });
  }

  /**
   * エラーログをファイルに保存
   */
  private static async saveErrorLog(timestamp: string, context: string, error: string): Promise<void> {
    try {
      const logEntry = `[${timestamp}] [${context}] ${error}\n`;

      // .robsidian/logs/error.log に追記
      await invoke("append_to_log", {
        content: logEntry
      });
    } catch (e) {
      // ログ保存失敗は無視
    }
  }

  /**
   * エラーログを取得
   */
  static getErrorLog(): Array<{timestamp: string; context: string; error: string}> {
    return [...this.errorLog];
  }

  /**
   * エラーログをクリア
   */
  static clearErrorLog(): void {
    this.errorLog = [];
  }

  /**
   * エラーログをエクスポート
   */
  static exportErrorLog(): string {
    return this.errorLog
      .map(log => `[${log.timestamp}] [${log.context}] ${log.error}`)
      .join("\n");
  }
}