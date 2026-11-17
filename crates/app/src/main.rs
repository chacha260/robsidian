//! Robsidian — Obsidian互換vaultのためのネイティブMarkdownノートアプリ。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod editor;
mod graph;
mod highlight;
mod history_view;
mod preview;

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

/// egui同梱フォントにはCJKグリフが無いので、システムフォントをフォールバックに足す。
/// `ROBSIDIAN_FONT` で任意のフォントファイルを指定できる。
fn install_cjk_font(ctx: &egui::Context) {
    let custom = std::env::var_os("ROBSIDIAN_FONT").map(PathBuf::from);
    let candidates = custom
        .iter()
        .map(PathBuf::as_path)
        .chain(CJK_FONTS.iter().map(Path::new));
    let Some(bytes) = candidates.filter_map(|p| std::fs::read(p).ok()).next() else {
        eprintln!("robsidian: 日本語フォントが見つかりません（ROBSIDIAN_FONT で指定できます）");
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "cjk".to_owned(),
        Arc::new(egui::FontData::from_owned(bytes)),
    );
    for family in fonts.families.values_mut() {
        family.push("cjk".to_owned());
    }
    ctx.set_fonts(fonts);
}

fn main() -> eframe::Result {
    let vault = std::env::args_os().nth(1).map(PathBuf::from);
    let viewport = egui::ViewportBuilder::default()
        .with_title("Robsidian")
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([640.0, 400.0]);
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icon-256.png"));
    let viewport = match icon {
        Ok(icon) => viewport.with_icon(icon),
        Err(_) => viewport,
    };
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "robsidian",
        options,
        Box::new(move |cc| {
            install_cjk_font(&cc.egui_ctx);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(app::App::new(cc, vault)))
        }),
    )
}
