// src/imageHandler.ts - 画像ペースト機能
import { invoke } from "@tauri-apps/api/core";

export async function setupImagePasteHandler(
  editorView: any,
  currentFilePath: string
) {
  const handlePaste = async (event: ClipboardEvent) => {
    const items = event.clipboardData?.items;
    if (!items) return;

    for (const item of Array.from(items)) {
      if (item.type.startsWith('image/')) {
        event.preventDefault();

        const file = item.getAsFile();
        if (!file) continue;

        try {
          // 画像をBase64に変換
          const base64 = await fileToBase64(file);

          // assetsフォルダに保存
          const imagePath = await saveImage(base64, currentFilePath, file.type);

          // エディタに挿入
          const cursor = editorView.state.selection.main.head;
          const imageMarkdown = `![](${imagePath})`;

          editorView.dispatch({
            changes: {
              from: cursor,
              to: cursor,
              insert: imageMarkdown,
            },
          });

          console.log('✅ Image pasted:', imagePath);
        } catch (e) {
          console.error('Failed to paste image:', e);
          alert(`Failed to paste image: ${e}`);
        }
      }
    }
  };

  return handlePaste;
}

async function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = reader.result as string;
      // "data:image/png;base64," を除去
      const base64 = result.split(',')[1];
      resolve(base64);
    };
    reader.onerror = reject;
    reader.readAsDataURL(file);
  });
}

async function saveImage(
  base64Data: string,
  currentFilePath: string,
  mimeType: string
): Promise<string> {
  // ファイル拡張子を決定
  const ext = mimeType.split('/')[1] || 'png';

  // ファイル名を生成（タイムスタンプ）
  const timestamp = Date.now();
  const filename = `pasted-${timestamp}.${ext}`;

  // assetsディレクトリのパスを取得
  const sep = currentFilePath.includes('/') ? '/' : '\\';
  const vaultRoot = currentFilePath.substring(0, currentFilePath.lastIndexOf(sep));
  const assetsDir = `${vaultRoot}${sep}assets`;

  // assetsディレクトリを作成
  try {
    await invoke('create_directory', { path: assetsDir });
  } catch (e) {
    // 既に存在する場合は無視
  }

  // 画像を保存
  const imagePath = `${assetsDir}${sep}${filename}`;
  await invoke('save_image_base64', {
    path: imagePath,
    base64Data,
  });

  // 相対パスを返す
  return `assets/${filename}`;
}