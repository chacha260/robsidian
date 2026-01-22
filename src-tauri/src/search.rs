// src-tauri/src/search.rs
use std::fs;
use std::path::Path;
use walkdir::WalkDir;
use serde::Serialize;

#[derive(Serialize)]
pub struct SearchResult {
    path: String,
    name: String,
    is_dir: bool, // 互換性のため残すが検索結果は基本ファイルのみ
    snippet: String, // ヒットした行の抜粋（オプション）
}

#[tauri::command]
pub async fn search_files(root_path: String, query: String) -> Result<Vec<SearchResult>, String> {
    if query.trim().is_empty() {
        return Ok(vec![]);
    }

    let mut results = Vec::new();
    let query_lower = query.to_lowercase();

    // WalkDirで再帰的に探索
    for entry in WalkDir::new(&root_path).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        
        // ファイルのみ対象（ディレクトリはスキップ）
        if path.is_file() {
            // Markdownとテキストファイルのみ対象にする（バイナリ除外）
            if let Some(ext) = path.extension() {
                let ext_str = ext.to_string_lossy().to_lowercase();
                if ext_str != "md" && ext_str != "txt" {
                    continue;
                }
            } else {
                continue;
            }

            // ファイル読み込み
            if let Ok(content) = fs::read_to_string(path) {
                // コンテンツ検索 (大文字小文字無視)
                if content.to_lowercase().contains(&query_lower) {
                    // ヒットした行を探してスニペットを作る（簡易実装）
                    let snippet = content.lines()
                        .find(|line| line.to_lowercase().contains(&query_lower))
                        .map(|line| line.trim().to_string())
                        .unwrap_or_default();

                    // 名前取得
                    let name = path.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "Unknown".to_string());

                    // パス取得
                    let full_path = path.to_string_lossy().to_string();

                    results.push(SearchResult {
                        path: full_path,
                        name,
                        is_dir: false,
                        snippet,
                    });

                    // 結果が多すぎると重くなるので制限（例えば100件）
                    if results.len() >= 100 {
                        break;
                    }
                }
            }
        }
    }

    Ok(results)
}