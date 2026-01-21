mod terminal;

use std::process::Command;
use std::fs;
use serde::Serialize;
use font_loader::system_fonts;
use std::collections::HashSet;
use tauri::Manager;

#[derive(Serialize)]
struct FileEntry {
    name: String,
    path: String,
    is_dir: bool,
}

// ファイル一覧取得
#[tauri::command]
fn list_files(path: String) -> Result<Vec<FileEntry>, String> {
    let target_path = if path.is_empty() { ".".to_string() } else { path };
    
    let entries = fs::read_dir(&target_path).map_err(|e| e.to_string())?;
    
    let mut file_list = Vec::new();

    for entry in entries {
        if let Ok(entry) = entry {
            let path_buf = entry.path();
            let is_dir = path_buf.is_dir();
            
            if let Ok(name) = entry.file_name().into_string() {
                let full_path = path_buf.to_string_lossy().to_string();

                file_list.push(FileEntry {
                    name,
                    path: full_path,
                    is_dir,
                });
            }
        }
    }

    // フォルダを先に表示するようにソート
    file_list.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name))
    });

    Ok(file_list)
}

// ファイル読み込み
#[tauri::command]
fn read_file_content(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

// ★追加: 新規ファイル作成
#[tauri::command]
fn create_file(path: String) -> Result<(), String> {
    if std::path::Path::new(&path).exists() {
        return Err("File already exists".to_string());
    }
    std::fs::File::create(&path).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn rename_item(old_path: String, new_path: String) -> Result<(), String> {
    std::fs::rename(&old_path, &new_path).map_err(|e| e.to_string())
}

// ▼ 追加: 削除 (ファイルなら remove_file, フォルダなら remove_dir_all)
#[tauri::command]
fn delete_item(path: String) -> Result<(), String> {
    let metadata = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    
    if metadata.is_dir() {
        std::fs::remove_dir_all(&path).map_err(|e| e.to_string())
    } else {
        std::fs::remove_file(&path).map_err(|e| e.to_string())
    }
}

#[tauri::command]
fn save_file_content(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, &content).map_err(|e| e.to_string())
}

#[tauri::command]
fn create_directory(path: String) -> Result<(), String> {
    if std::path::Path::new(&path).exists() {
        return Err("Directory already exists".to_string());
    }
    std::fs::create_dir(&path).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_available_fonts() -> Vec<String> {
    // システムフォントをロード
    let fonts = system_fonts::query_all();

    // フォントファミリー名だけを取り出し、重複を削除 (HashSetを使用)
    let mut unique_fonts: HashSet<String> = HashSet::new();
    for font in fonts {
        unique_fonts.insert(font);
    }

    // リストに変換してソート
    let mut sorted_fonts: Vec<String> = unique_fonts.into_iter().collect();
    sorted_fonts.sort();

    sorted_fonts
}

// Helix起動
#[tauri::command]
fn open_in_helix(path: String) -> Result<String, String> {
    println!("Opening in Helix: {}", path);
    
    Command::new("cmd")
        .args(&["/c", "start", "hx", &path])
        .spawn()
        .map_err(|e| format!("Failed to launch terminal: {}", e))?;

    Ok(format!("Opened {} in Helix", path))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(terminal::TerminalState::new())
        .invoke_handler(tauri::generate_handler![
            open_in_helix,
            list_files,
            read_file_content,
            create_file,
            create_directory,
            rename_item,
            delete_item,
            save_file_content,
            get_available_fonts,
            terminal::create_terminal,
            terminal::write_to_terminal,
            terminal::resize_terminal
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
