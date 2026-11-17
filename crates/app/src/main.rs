//! Robsidian — Obsidian互換vaultのためのネイティブMarkdownノートアプリ。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod edit;
mod editor;
mod external;
mod find;
mod fonts;
mod graph;
mod highlight;
mod history_view;
mod hover;
mod preview;
mod props_ui;
mod settings;
mod theme;
mod vim;
mod workspace;

use eframe::egui;
use std::path::PathBuf;

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
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(app::App::new(cc, vault)))
        }),
    )
}
