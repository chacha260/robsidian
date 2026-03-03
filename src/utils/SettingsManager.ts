// src/utils/SettingsManager.ts - 設定のバリデーションと管理
export class SettingsManager {
  /**
   * ズーム値のバリデーション
   */
  static validateZoom(zoom: number): number {
    const validated = Math.min(Math.max(zoom, 0.5), 3.0);
    if (validated !== zoom) {
      console.warn(`Zoom value ${zoom} clamped to ${validated}`);
    }
    return validated;
  }

  /**
   * フォントサイズのバリデーション
   */
  static validateFontSize(size: string): string {
    const num = parseInt(size);
    if (isNaN(num) || num < 8 || num > 32) {
      console.warn(`Invalid font size ${size}, using default 14`);
      return "14";
    }
    return size;
  }

  /**
   * 行間のバリデーション
   */
  static validateLineHeight(height: string): string {
    const num = parseFloat(height);
    if (isNaN(num) || num < 1.0 || num > 3.0) {
      console.warn(`Invalid line height ${height}, using default 1.5`);
      return "1.5";
    }
    return height;
  }

  /**
   * シェルパスのバリデーション
   */
  static validateShellPath(path: string): string {
    // 空文字列の場合はデフォルトシェル
    if (!path.trim()) return "";

    // 危険なパスを拒否
    const dangerous = ["..", ";", "|", "&", "$", "`", "(", ")", "<", ">"];
    for (const char of dangerous) {
      if (path.includes(char)) {
        console.warn(`Dangerous character detected in shell path: ${char}`);
        return "";
      }
    }

    return path.trim();
  }

  /**
   * フォント名のバリデーション
   */
  static validateFont(font: string): string {
    if (!font.trim()) {
      return "'HackGen', 'HackGen Console', sans-serif";
    }

    // 危険な文字を除去
    const sanitized = font.replace(/[;<>]/g, "");

    return sanitized.trim();
  }

  /**
   * 設定を安全に保存
   */
  static saveSetting(key: string, value: string): void {
    try {
      localStorage.setItem(key, value);
    } catch (e) {
      console.error(`Failed to save setting ${key}:`, e);

      // ストレージが満杯の場合、古い設定を削除
      if (e instanceof Error && e.name === "QuotaExceededError") {
        this.cleanupOldSettings();
        // 再試行
        try {
          localStorage.setItem(key, value);
        } catch (retryError) {
          console.error("Failed to save setting after cleanup:", retryError);
        }
      }
    }
  }

  /**
   * 設定を安全に読み込み
   */
  static loadSetting(key: string, defaultValue: string = ""): string {
    try {
      return localStorage.getItem(key) || defaultValue;
    } catch (e) {
      console.error(`Failed to load setting ${key}:`, e);
      return defaultValue;
    }
  }

  /**
   * 古い設定をクリーンアップ
   */
  private static cleanupOldSettings(): void {
    const keysToRemove: string[] = [];

    // エラーログなど、一時的なデータを削除
    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i);
      if (key && (key.startsWith("robsidian-temp-") || key.startsWith("debug-"))) {
        keysToRemove.push(key);
      }
    }

    keysToRemove.forEach(key => {
      try {
        localStorage.removeItem(key);
      } catch (e) {
        console.error(`Failed to remove ${key}:`, e);
      }
    });

    console.log(`Cleaned up ${keysToRemove.length} old settings`);
  }

  /**
   * 全設定をエクスポート
   */
  static exportSettings(): Record<string, string> {
    const settings: Record<string, string> = {};

    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i);
      if (key && key.startsWith("robsidian-")) {
        settings[key] = localStorage.getItem(key) || "";
      }
    }

    return settings;
  }

  /**
   * 設定をインポート
   */
  static importSettings(settings: Record<string, string>): void {
    Object.entries(settings).forEach(([key, value]) => {
      if (key.startsWith("robsidian-")) {
        this.saveSetting(key, value);
      }
    });
  }

  /**
   * 設定をリセット
   */
  static resetAllSettings(): void {
    const keysToRemove: string[] = [];

    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i);
      if (key && key.startsWith("robsidian-")) {
        keysToRemove.push(key);
      }
    }

    keysToRemove.forEach(key => {
      localStorage.removeItem(key);
    });

    console.log(`Reset ${keysToRemove.length} settings`);
  }
}