mod search;
mod terminal;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(terminal::TerminalState::new())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            // ★修正: terminal::とsearch::のプレフィックスを付ける
            terminal::create_terminal,
            terminal::write_to_terminal,
            terminal::resize_terminal,
            search::search_files,
            read_file_content,
            save_file_content,
            create_file,
            create_directory,
            delete_item,
            rename_item,
            get_file_mtime,
            file_exists,
            directory_exists,
            list_files,
            append_to_log,
            save_image_base64,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// ============================================================================
// セキュリティ: パス検証
// ============================================================================

/// パスを検証（パストラバーサル対策 - 絶対パスを許可）
fn validate_path(path: &str) -> Result<PathBuf, String> {
    let path_buf = Path::new(path);

    // ★修正: 絶対パスの場合はそのまま正規化して返す
    if path_buf.is_absolute() {
        // 正規化を試みる
        match path_buf.canonicalize() {
            Ok(canonical) => return Ok(canonical),
            Err(_) => {
                // ファイルが存在しない場合は親ディレクトリで検証
                if let Some(parent) = path_buf.parent() {
                    if parent.exists() {
                        return Ok(path_buf.to_path_buf());
                    }
                }
                return Err(format!("Invalid path: {}", path));
            }
        }
    }

    // 相対パスの場合は現在のディレクトリを基準にする
    let base_dir = std::env::current_dir().map_err(|e| e.to_string())?;
    let target_path = base_dir.join(path_buf);

    // 正規化
    let canonical = match target_path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            // ファイルが存在しない場合は親ディレクトリで検証
            let parent = target_path.parent()
                .ok_or("Invalid path: no parent")?;

            if parent.exists() {
                return Ok(target_path);
            }

            return Err(format!("Invalid path: {}", path));
        }
    };

    // ★修正: 相対パスの場合のみベースディレクトリチェック
    if !canonical.starts_with(&base_dir) {
        return Err(format!("Relative path traversal detected"));
    }

    Ok(canonical)
}

/// ファイルサイズを検証
fn validate_file_size(path: &Path, max_size: u64) -> Result<(), String> {
    let metadata = fs::metadata(path)
        .map_err(|e| format!("Failed to get file metadata: {}", e))?;

    if metadata.len() > max_size {
        return Err(format!("File too large: {} bytes (max: {} bytes)", metadata.len(), max_size));
    }
    Ok(())
}

// ============================================================================
// ファイル操作コマンド（セキュリティ強化版）
// ============================================================================
#[derive(serde::Serialize)]
struct FileEntry {
    name: String,
    path: String,
    is_dir: bool,
}

const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10MB

#[tauri::command]
fn read_file_content(path: String) -> Result<String, String> {
    let safe_path = validate_path(&path)?;
    // ファイルサイズ制限
    validate_file_size(&safe_path, MAX_FILE_SIZE)?;
    fs::read_to_string(safe_path).map_err(|e| e.to_string())
}

#[tauri::command]
fn save_file_content(path: String, content: String) -> Result<(), String> {
    let safe_path = validate_path(&path)?;

    // コンテンツサイズ制限
    if content.len() > MAX_FILE_SIZE as usize {
        return Err(format!("Content too large: {} bytes (max: {} bytes)", content.len(), MAX_FILE_SIZE));
    }

    // ★バックアップ作成（既存ファイルの場合）
    if safe_path.exists() {
        let backup_path = format!("{}.backup", safe_path.display());
        if let Err(e) = fs::copy(&safe_path, &backup_path) {
            eprintln!("Warning: Failed to create backup: {}", e);
        }
    }

    fs::write(safe_path, content).map_err(|e| e.to_string())
}

#[tauri::command]
fn create_file(path: String) -> Result<(), String> {
    let safe_path = validate_path(&path)?;

    if safe_path.exists() {
        return Err("File already exists".to_string());
    }

    // 親ディレクトリが存在するか確認
    if let Some(parent) = safe_path.parent() {
        if !parent.exists() {
            return Err("Parent directory does not exist".to_string());
        }
    }

    std::fs::File::create(safe_path).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn create_directory(path: String) -> Result<(), String> {
    let safe_path = validate_path(&path)?;

    if safe_path.exists() {
        return Err("Directory already exists".to_string());
    }

    fs::create_dir_all(safe_path).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_item(path: String) -> Result<(), String> {
    let safe_path = validate_path(&path)?;

    if !safe_path.exists() {
        return Err("File or directory does not exist".to_string());
    }

    if safe_path.is_dir() {
        fs::remove_dir_all(safe_path).map_err(|e| e.to_string())
    } else {
        fs::remove_file(safe_path).map_err(|e| e.to_string())
    }
}

#[tauri::command]
fn rename_item(old_path: String, new_path: String) -> Result<(), String> {
    let safe_old_path = validate_path(&old_path)?;
    let safe_new_path = validate_path(&new_path)?;

    if !safe_old_path.exists() {
        return Err("Source file or directory does not exist".to_string());
    }

    if safe_new_path.exists() {
        return Err("Destination already exists".to_string());
    }

    fs::rename(safe_old_path, safe_new_path).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_files(path: String) -> Result<Vec<FileEntry>, String> {
    let safe_path = validate_path(&path)?;
    let mut entries = Vec::new();

    let read_dir = fs::read_dir(&safe_path).map_err(|e| e.to_string())?;

    for entry in read_dir {
        let entry = entry.map_err(|e| e.to_string())?;
        let path_buf = entry.path();
        let is_dir = path_buf.is_dir();
        let name = path_buf
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();

        entries.push(FileEntry {
            name,
            path: path_buf.to_string_lossy().to_string(),
            is_dir,
        });
    }

    Ok(entries)
}

#[tauri::command]
fn get_file_mtime(path: String) -> Result<u64, String> {
    let safe_path = validate_path(&path)?;
    let metadata = fs::metadata(safe_path).map_err(|e| e.to_string())?;
    let mtime = metadata.modified().map_err(|e| e.to_string())?;
    Ok(mtime
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs())
}

#[tauri::command]
fn file_exists(path: String) -> bool {
    if let Ok(safe_path) = validate_path(&path) {
        safe_path.exists() && safe_path.is_file()
    } else {
        false
    }
}

#[tauri::command]
fn directory_exists(path: String) -> bool {
    if let Ok(safe_path) = validate_path(&path) {
        safe_path.exists() && safe_path.is_dir()
    } else {
        false
    }
}

#[tauri::command]
fn save_image_base64(path: String, base64_data: String) -> Result<(), String> {
    use base64::{engine::general_purpose, Engine as _};

    let safe_path = validate_path(&path)?;

    // Base64デコード
    let image_data = general_purpose::STANDARD
        .decode(base64_data)
        .map_err(|e| format!("Failed to decode base64: {}", e))?;

    // ファイルに書き込み
    fs::write(safe_path, image_data).map_err(|e| e.to_string())?;

    Ok(())
}

// ============================================================================
// ログ機能
// ============================================================================
#[tauri::command]
fn append_to_log(content: String) -> Result<(), String> {
    use std::fs::OpenOptions;
    use std::io::Write;

    // ★修正: 現在のディレクトリを基準にする
    let base_dir = std::env::current_dir().map_err(|e| e.to_string())?;
    let log_dir = base_dir.join(".robsidian").join("logs");

    // ログディレクトリ作成
    if !log_dir.exists() {
        fs::create_dir_all(&log_dir).map_err(|e| e.to_string())?;
    }

    let log_path = log_dir.join("error.log");

    // ★ログファイルサイズ制限（1MB）
    if log_path.exists() {
        let metadata = fs::metadata(&log_path).map_err(|e| e.to_string())?;
        if metadata.len() > 1024 * 1024 {
            // ローテーション
            let backup_path = log_dir.join("error.log.old");
            fs::rename(&log_path, backup_path).map_err(|e| e.to_string())?;
        }
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .map_err(|e| e.to_string())?;

    writeln!(file, "{}", content).map_err(|e| e.to_string())?;

    Ok(())
}