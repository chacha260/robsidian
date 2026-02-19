use rayon::prelude::*;
use serde::Serialize;
use std::fs;
use walkdir::WalkDir;

#[derive(Serialize)]
pub struct SearchResult {
    path: String,
    name: String,
    is_dir: bool,
    snippet: String,
}

#[tauri::command]
pub fn search_files(root_path: String, query: String) -> Result<Vec<SearchResult>, String> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }

    let query_lower = query.to_lowercase();
    let max_file_size = 5 * 1024 * 1024; // 5MB制限

    // まず全エントリを収集（並列化前）
    let entries: Vec<_> = WalkDir::new(&root_path)
        .max_depth(10)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|entry| {
            // 早期フィルタリング: 隠しファイルをスキップ
            let file_name = entry.file_name().to_string_lossy();
            !file_name.starts_with('.') || file_name == "."
        })
        .collect();

    // 並列処理でマッチング
    let mut results: Vec<SearchResult> = entries
        .into_par_iter()
        .filter_map(|entry| {
            let path = entry.path();
            let path_str = path.to_string_lossy().to_string();
            let file_name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry.file_type().is_dir();

            // 1. ファイル名マッチ
            if file_name.to_lowercase().contains(&query_lower) {
                return Some(SearchResult {
                    path: path_str,
                    name: file_name,
                    is_dir,
                    snippet: if is_dir {
                        "Directory".to_string()
                    } else {
                        "Filename match".to_string()
                    },
                });
            }

            // 2. 全文検索（ファイルのみ、サイズ制限あり）
            if !is_dir {
                // ファイルサイズチェック
                if let Ok(metadata) = entry.metadata() {
                    if metadata.len() > max_file_size {
                        return None;
                    }
                }

                if let Ok(content) = fs::read_to_string(path) {
                    if let Some(byte_idx) = content.to_lowercase().find(&query_lower) {
                        let snippet = extract_snippet(&content, byte_idx, 30, 40);
                        return Some(SearchResult {
                            path: path_str,
                            name: file_name,
                            is_dir,
                            snippet,
                        });
                    }
                }
            }

            None
        })
        .collect();

    // 結果を最大100件に制限（並列処理の後）
    results.truncate(100);

    Ok(results)
}
/// UTF-8安全なスニペット抽出
/// 
/// # Arguments
/// * `content` - 元のテキスト
/// * `match_pos` - マッチした位置（バイト単位）
/// * `before_chars` - マッチ位置の前に含める文字数
/// * `after_chars` - マッチ位置の後に含める文字数
fn extract_snippet(content: &str, match_pos: usize, before_chars: usize, after_chars: usize) -> String {
    let chars: Vec<char> = content.chars().collect();

    // マッチ位置を文字インデックスに変換
    let mut byte_count = 0;
    let mut char_idx = 0;

    for (idx, ch) in content.chars().enumerate() {
        if byte_count >= match_pos {
            char_idx = idx;
            break;
        }
        byte_count += ch.len_utf8();
    }

    // 前後の範囲を計算
    let start = char_idx.saturating_sub(before_chars);
    let end = (char_idx + after_chars).min(chars.len());

    // スニペットを構築
    let snippet: String = chars[start..end].iter().collect();

    // 改行をスペースに置換
    let snippet = snippet.replace('\n', " ").replace('\r', " ");

    // 前後に省略記号を追加
    let prefix = if start > 0 { "..." } else { "" };
    let suffix = if end < chars.len() { "..." } else { "" };

    format!("{}{}{}", prefix, snippet.trim(), suffix)
}