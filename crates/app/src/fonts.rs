//! 日本語フォントの読み込み。

use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 日本語を表示できるシステムフォントの候補（先に見つかったものを使う）。
const CJK_FONTS: &[&str] = &[
    // Windows
    "C:/Windows/Fonts/YuGothM.ttc",
    "C:/Windows/Fonts/meiryo.ttc",
    "C:/Windows/Fonts/msgothic.ttc",
    // macOS
    "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/Library/Fonts/Arial Unicode.ttf",
    // Linux
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf",
    "/usr/share/fonts/truetype/fonts-japanese-gothic.ttf",
    "/usr/share/fonts/truetype/vlgothic/VL-Gothic-Regular.ttf",
    "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
];

/// egui同梱フォントにはCJKグリフが無いので、フォールバックとして日本語フォントを足す。
/// 優先順は 設定で指定したファイル → 環境変数 `ROBSIDIAN_FONT` → システムフォントの候補。
/// 使ったフォントのパスを返す（見つからなければ None）。
pub fn install(ctx: &egui::Context, configured: Option<&Path>) -> Option<PathBuf> {
    let env = std::env::var_os("ROBSIDIAN_FONT").map(PathBuf::from);
    let candidates = configured
        .map(Path::to_path_buf)
        .into_iter()
        .chain(env)
        .chain(CJK_FONTS.iter().map(PathBuf::from));
    let (path, bytes) = candidates
        .filter_map(|p| std::fs::read(&p).ok().map(|b| (p, b)))
        .next()?;
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "cjk".to_owned(),
        Arc::new(egui::FontData::from_owned(bytes)),
    );
    for family in fonts.families.values_mut() {
        family.push("cjk".to_owned());
    }
    ctx.set_fonts(fonts);
    Some(path)
}
