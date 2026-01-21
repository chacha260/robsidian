use portable_pty::{native_pty_system, CommandBuilder, PtySize, MasterPty};
use std::{
    collections::HashMap,
    io::{Read, Write},
    sync::{Arc, Mutex, mpsc},
    thread,
};
use tauri::{AppHandle, Emitter};
use anyhow::{Context, Result};

// セッション構造体: PTYの制御権(Master)と、書き込みスレッドへの送信機(Sender)を持つ
pub struct TerminalSession {
    pub master: Box<dyn MasterPty + Send>,
    pub writer_sender: mpsc::Sender<String>,
}

// ステート: HashMapの中身を TerminalSession に変更
pub struct TerminalState {
    pub sessions: Arc<Mutex<HashMap<String, TerminalSession>>>,
}

impl TerminalState {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

// -------------------------------------------------------------------------
// Commands
// -------------------------------------------------------------------------

#[tauri::command]
pub async fn create_terminal(
    id: String,
    app_handle: AppHandle,
    state: tauri::State<'_, TerminalState>,
) -> Result<(), String> {
    match spawn_pty_process(id, app_handle, state) {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to create terminal: {}", e)),
    }
}

#[tauri::command]
pub async fn write_to_terminal(
    id: String,
    data: String,
    state: tauri::State<'_, TerminalState>,
) -> Result<(), String> {
    let sessions = state.sessions.lock().map_err(|_| "Failed to lock mutex")?;
    
    if let Some(session) = sessions.get(&id) {
        // Channelを通じて書き込み専用スレッドにデータを送る
        // これなら take_writer を毎回呼ぶ必要がない
        session.writer_sender.send(data).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn resize_terminal(
    id: String,
    rows: u16,
    cols: u16,
    state: tauri::State<'_, TerminalState>,
) -> Result<(), String> {
    let sessions = state.sessions.lock().map_err(|_| "Failed to lock mutex")?;
    
    if let Some(session) = sessions.get(&id) {
        session.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

// -------------------------------------------------------------------------
// Internal Logic
// -------------------------------------------------------------------------

fn spawn_pty_process(
    id: String,
    app_handle: AppHandle,
    state: tauri::State<'_, TerminalState>,
) -> Result<()> {
    let pty_system = native_pty_system();

    // 1. PTYペアの作成
    let pair = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }).context("Failed to open PTY")?;

    // 2. シェルコマンドの決定
    #[cfg(target_os = "windows")]
    let cmd = CommandBuilder::new("powershell.exe");
    #[cfg(not(target_os = "windows"))]
    let cmd = CommandBuilder::new("bash");

    // 3. 子プロセス起動
    let _child = pair.slave.spawn_command(cmd).context("Failed to spawn shell")?;

    // 4. 書き込み用スレッドの作成 (Channel受信 -> PTY書き込み)
    // ここで take_writer を【1回だけ】実行し、スレッド内に所有権を移動させる
    let mut writer = pair.master.take_writer().context("Failed to take writer")?;
    let (tx, rx) = mpsc::channel::<String>();

    thread::spawn(move || {
        // Channelから送られてくる文字列を待ち受けるループ
        while let Ok(data) = rx.recv() {
            if let Err(_) = writer.write_all(data.as_bytes()) {
                break; // 書き込み失敗ならループ終了
            }
            // flushは必須ではない場合が多いが、念の為入れても良い
            let _ = writer.flush();
        }
    });

    // 5. 読み込み用スレッドの作成 (PTY読み込み -> Frontend送信)
    let mut reader = pair.master.try_clone_reader().context("Failed to clone reader")?;
    let event_id = id.clone();
    
    thread::spawn(move || {
        let mut buffer = [0u8; 1024];
        loop {
            match reader.read(&mut buffer) {
                Ok(n) if n > 0 => {
                    let output = String::from_utf8_lossy(&buffer[..n]).to_string();
                    let event_name = format!("term-data:{}", event_id);
                    let _ = app_handle.emit(&event_name, output);
                }
                Ok(_) => break, // EOF
                Err(_) => break, // Error
            }
        }
    });

    // 6. ステートに保存 (MasterとSender)
    let mut sessions = state.sessions.lock().unwrap();
    sessions.insert(id, TerminalSession {
        master: pair.master, // Box化済みのmasterをそのまま入れる
        writer_sender: tx,
    });

    Ok(())
}