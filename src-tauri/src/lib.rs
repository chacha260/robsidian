mod search;
mod terminal; 

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(terminal::TerminalState::new())

        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())

        .invoke_handler(tauri::generate_handler![
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
            list_files
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

use std::fs;
use std::path::Path;
use std::time::SystemTime;

#[derive(serde::Serialize)]
struct FileEntry {
    name: String,
    path: String,
    is_dir: bool,
}

#[tauri::command]
fn read_file_content(path: String) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| e.to_string())
}

#[tauri::command]
fn save_file_content(path: String, content: String) -> Result<(), String> {
    fs::write(path, content).map_err(|e| e.to_string())
}

#[tauri::command]
fn create_file(path: String) -> Result<(), String> {
    if Path::new(&path).exists() {
        return Err("File already exists".to_string());
    }
    fs::File::create(path).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn create_directory(path: String) -> Result<(), String> {
    if Path::new(&path).exists() {
        return Err("Directory already exists".to_string());
    }
    fs::create_dir_all(path).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_item(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    if p.is_dir() {
        fs::remove_dir_all(p).map_err(|e| e.to_string())
    } else {
        fs::remove_file(p).map_err(|e| e.to_string())
    }
}

#[tauri::command]
fn rename_item(old_path: String, new_path: String) -> Result<(), String> {
    fs::rename(old_path, new_path).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_files(path: String) -> Result<Vec<FileEntry>, String> {
    let mut entries = Vec::new();
    let read_dir = fs::read_dir(&path).map_err(|e| e.to_string())?;

    for entry in read_dir {
        let entry = entry.map_err(|e| e.to_string())?;
        let path_buf = entry.path();
        let is_dir = path_buf.is_dir();
        let name = path_buf.file_name().unwrap().to_string_lossy().to_string();
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
    let metadata = fs::metadata(path).map_err(|e| e.to_string())?;
    let mtime = metadata.modified().map_err(|e| e.to_string())?;
    Ok(mtime.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs())
}

#[tauri::command]
fn file_exists(path: String) -> bool {
    Path::new(&path).exists() && Path::new(&path).is_file()
}

#[tauri::command]
fn directory_exists(path: String) -> bool {
    Path::new(&path).exists() && Path::new(&path).is_dir()
}