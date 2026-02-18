use rayon::prelude::*;
use serde::Serialize;
use std::fs;
use std::path::Path;
use walkdir::WalkDir; // 並列処理用

#[derive(Serialize)]
pub struct SearchResult {
    path: String,
    name: String,
    is_dir: bool,
    snippet: String,
}

#[tauri::command]
pub fn search_files(root_path: String, query: String) -> Vec<SearchResult> {
    let query_lower = query.to_lowercase();

    // WalkDirでファイルを列挙し、Rayonで並列処理して検索
    let results: Vec<SearchResult> = WalkDir::new(&root_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .par_bridge() // 並列イテレータに変換
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

            // 2. 全文検索 (ファイルの中身も探す)
            if !is_dir {
                // テキストファイルっぽくないものや巨大なファイルはスキップする簡易ガード
                if let Ok(metadata) = entry.metadata() {
                    if metadata.len() > 1024 * 1024 {
                        return None;
                    } // 1MB以上はスキップ
                }

                if let Ok(content) = fs::read_to_string(path) {
                    if let Some(idx) = content.to_lowercase().find(&query_lower) {
                        // マッチした周辺のテキストを切り抜く
                        let start = if idx > 20 { idx - 20 } else { 0 };
                        let end = std::cmp::min(idx + 40, content.len());
                        let snippet = format!("...{}...", &content[start..end].replace("\n", " "));

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

    // 結果を最大100件に制限して返す
    results.into_iter().take(100).collect()
}
