//! ユーザーコマンド: `.robsidian/commands.toml` に書いたシェルコマンドを、パレットやショートカットから実行する。
//!
//! ```toml
//! [[command]]
//! name = "選択範囲を翻訳"
//! run = "trans -b :en"
//! input = "selection"   # none / selection / note
//! output = "replace"    # none / replace / insert / notice
//! key = "ctrl+alt+t"
//! ```
//!
//! コマンドはvaultのフォルダで `sh -c`（Windowsは `cmd /C`）として実行し、入力は標準入力に渡す。
//! 環境変数 `ROB_VAULT`・`ROB_FILE`・`ROB_NOTE`・`ROB_LINE` で、vaultと現在のノートを知らせる。

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// 標準入力に渡すもの
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Input {
    #[default]
    None,
    /// 選択範囲（選択が無ければ空）
    Selection,
    /// ノートの本文すべて
    Note,
}

/// 標準出力をどう使うか
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Output {
    /// 捨てる
    None,
    /// 入力（選択範囲 / ノート全体）を置き換える。入力が無ければカーソル位置に挿入する
    Replace,
    /// カーソル位置（選択があればその後ろ）に挿入する
    Insert,
    /// 通知として表示する
    #[default]
    Notice,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserCommand {
    pub name: String,
    pub run: String,
    #[serde(default)]
    pub input: Input,
    #[serde(default)]
    pub output: Output,
    /// 例: `ctrl+alt+t`
    #[serde(default)]
    pub key: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    command: Vec<UserCommand>,
}

/// 設定ファイルの場所
pub fn path(root: &Path) -> PathBuf {
    root.join(".robsidian").join("commands.toml")
}

pub fn parse(text: &str) -> Result<Vec<UserCommand>> {
    let file: File = toml::from_str(text)?;
    for c in &file.command {
        if c.name.trim().is_empty() {
            bail!("name が空のコマンドがあります");
        }
        if c.run.trim().is_empty() {
            bail!("コマンド「{}」の run が空です", c.name);
        }
    }
    Ok(file.command)
}

/// 設定ファイルの本文と、解析したコマンド。ファイルが無ければ `None`。
pub fn load(root: &Path) -> Result<Option<(String, Vec<UserCommand>)>> {
    let p = path(root);
    let text = match std::fs::read_to_string(&p) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", p.display())),
    };
    let list =
        parse(&text).with_context(|| format!("{} の書き方が正しくありません", p.display()))?;
    Ok(Some((text, list)))
}

/// 実行時の状況
#[derive(Debug, Clone, Default)]
pub struct Env {
    /// 現在のノートのvault内パス
    pub note: Option<String>,
    /// カーソルの行（1始まり）
    pub line: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Finished {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

impl Finished {
    /// 失敗したときに見せる1行
    pub fn error_line(&self) -> String {
        let line = self.stderr.lines().find(|l| !l.trim().is_empty());
        line.unwrap_or("終了コードが0ではありません")
            .trim()
            .to_string()
    }
}

fn shell(run: &str) -> Command {
    if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(run);
        c
    } else {
        let mut c = Command::new("sh");
        c.arg("-c").arg(run);
        c
    }
}

/// コマンドを実行して終わるまで待つ（呼び出し側で別スレッドに載せる）。
pub fn execute(cmd: &UserCommand, root: &Path, env: &Env, input: &str) -> Result<Finished> {
    let mut c = shell(&cmd.run);
    c.current_dir(root)
        .env("ROB_VAULT", root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(note) = &env.note {
        c.env("ROB_NOTE", note).env("ROB_FILE", root.join(note));
    }
    if let Some(line) = env.line {
        c.env("ROB_LINE", line.to_string());
    }
    let mut child = c
        .spawn()
        .with_context(|| format!("「{}」を起動できません", cmd.name))?;
    // 大きな入力でも詰まらないよう、書き込みは別スレッドで行う
    let writer = child.stdin.take().map(|mut stdin| {
        let input = input.to_string();
        std::thread::spawn(move || {
            let _ = stdin.write_all(input.as_bytes());
        })
    });
    let out = child.wait_with_output()?;
    if let Some(w) = writer {
        let _ = w.join();
    }
    Ok(Finished {
        success: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

/// 出力で入力を置き換えるときの本文。入力の末尾に改行が無ければ、出力の末尾の改行を1つ落とす
/// （`echo` などは必ず改行を付けるため、選択した単語を置き換えると行が割れてしまう）。
pub fn fit_output(input: &str, output: &str) -> String {
    if input.ends_with('\n') {
        return output.to_string();
    }
    let trimmed = output
        .strip_suffix("\r\n")
        .or_else(|| output.strip_suffix('\n'))
        .unwrap_or(output);
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands() {
        let list = parse(
            r#"
[[command]]
name = "大文字に"
run = "tr a-z A-Z"
input = "selection"
output = "replace"
key = "ctrl+alt+u"

[[command]]
name = "日付"
run = "date"
"#,
        )
        .unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].input, Input::Selection);
        assert_eq!(list[0].output, Output::Replace);
        assert_eq!(list[0].key.as_deref(), Some("ctrl+alt+u"));
        assert_eq!(list[1].input, Input::None);
        assert_eq!(list[1].output, Output::Notice);
        assert!(parse("[[command]]\nname = \"x\"\nrun = \"\"\n").is_err());
        assert!(parse("[[command]]\nname = \"x\"\nrun = \"y\"\noutput = \"bogus\"\n").is_err());
        assert!(parse("[[command]]\nname = \"x\"\nrun = \"y\"\ntypo = 1\n").is_err());
        assert!(parse("").unwrap().is_empty());
    }

    #[test]
    fn trims_one_trailing_newline() {
        assert_eq!(fit_output("abc", "ABC\n"), "ABC");
        assert_eq!(fit_output("abc\n", "ABC\n"), "ABC\n");
        assert_eq!(fit_output("abc", "ABC\n\n"), "ABC\n");
        assert_eq!(fit_output("", "x"), "x");
    }

    #[cfg(unix)]
    #[test]
    fn runs_with_stdin_and_env() {
        let dir = tempfile::tempdir().unwrap();
        let cmd = UserCommand {
            name: "t".into(),
            run: "printf '%s|%s|' \"$ROB_NOTE\" \"$ROB_LINE\"; tr a-z A-Z".into(),
            input: Input::Selection,
            output: Output::Replace,
            key: None,
        };
        let env = Env {
            note: Some("a/b.md".into()),
            line: Some(3),
        };
        let done = execute(&cmd, dir.path(), &env, "hello").unwrap();
        assert!(done.success);
        assert_eq!(done.stdout, "a/b.md|3|HELLO");

        let fail = UserCommand {
            run: "echo oops >&2; exit 2".into(),
            ..cmd
        };
        let done = execute(&fail, dir.path(), &Env::default(), "").unwrap();
        assert!(!done.success);
        assert_eq!(done.error_line(), "oops");
    }
}
