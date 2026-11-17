//! ノートのローカル履歴ダイアログ（版の一覧・現在との差分・復元）。

use crate::editor::Colors;
use eframe::egui::{self, RichText, ScrollArea};
use robsidian_core::history::{self, Version};
use similar::{ChangeTag, TextDiff};
use std::path::Path;

pub struct HistoryView {
    pub path: String,
    versions: Vec<Version>,
    selected: usize,
    /// (選んだ版の番号, その本文)
    loaded: Option<(usize, String)>,
    show_diff: bool,
}

pub enum Outcome {
    /// この本文に戻す
    Restore(String),
    Close,
}

impl HistoryView {
    pub fn new(root: &Path, path: &str) -> Self {
        Self {
            path: path.to_string(),
            versions: history::list(root, path),
            selected: 0,
            loaded: None,
            show_diff: true,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, current: &str, colors: &Colors) -> Option<Outcome> {
        if self
            .loaded
            .as_ref()
            .is_none_or(|(i, _)| *i != self.selected)
        {
            let text = self.versions.get(self.selected).and_then(|v| v.read().ok());
            self.loaded = Some((self.selected, text.unwrap_or_default()));
        }
        let mut outcome = None;
        let modal = egui::Modal::new(egui::Id::unique("history")).show(ctx, |ui| {
            let size = ctx.content_rect().size();
            ui.set_width((size.x * 0.8).min(1000.0));
            let body_height = (size.y * 0.65).min(620.0);
            ui.heading(format!("履歴: {}", self.path));
            ui.weak("保存で上書きされる直前の内容を、最大50件・30日分残しています（.robsidian/history）");
            ui.add_space(6.0);
            if self.versions.is_empty() {
                ui.label("このノートの履歴はまだありません。");
                ui.add_space(8.0);
                if ui.button("閉じる").clicked() {
                    outcome = Some(Outcome::Close);
                }
                return;
            }
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(190.0);
                    ScrollArea::vertical()
                        .id_salt("versions")
                        .max_height(body_height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                        for (i, v) in self.versions.iter().enumerate() {
                            ui.selectable_value(&mut self.selected, i, v.label());
                        }
                    });
                });
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.show_diff, true, "現在との差分");
                        ui.selectable_value(&mut self.show_diff, false, "この版の全文");
                    });
                    let text = self.loaded.as_ref().map_or("", |(_, t)| t.as_str());
                    ScrollArea::both()
                        .id_salt("version-body")
                        .max_height(body_height - 28.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                            if self.show_diff {
                                diff_ui(ui, text, current, colors);
                            } else {
                                ui.label(RichText::new(text).monospace());
                            }
                        });
                });
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("この版に戻す").on_hover_text("現在の内容も履歴に残ります").clicked() {
                    let text = self.loaded.as_ref().map_or_else(String::new, |(_, t)| t.clone());
                    outcome = Some(Outcome::Restore(text));
                }
                if ui.button("閉じる").clicked() {
                    outcome = Some(Outcome::Close);
                }
            });
        });
        if outcome.is_none() && modal.should_close() {
            outcome = Some(Outcome::Close);
        }
        outcome
    }
}

/// 選んだ版 → 現在 の行単位の差分。変更の無い部分は前後3行だけ見せる。
fn diff_ui(ui: &mut egui::Ui, old: &str, new: &str, colors: &Colors) {
    let diff = TextDiff::from_lines(old, new);
    let mut any = false;
    for (g, group) in diff.grouped_ops(3).iter().enumerate() {
        any = true;
        if g > 0 {
            ui.weak("⋯");
        }
        for op in group {
            for change in diff.iter_changes(op) {
                let line = change.value().trim_end_matches(['\n', '\r']);
                let (mark, color, bg) = match change.tag() {
                    ChangeTag::Delete => ("-", colors.text, colors.diff_del),
                    ChangeTag::Insert => ("+", colors.text, colors.diff_add),
                    ChangeTag::Equal => (" ", colors.dim, egui::Color32::TRANSPARENT),
                };
                let text = RichText::new(format!("{mark} {line}"))
                    .monospace()
                    .color(color)
                    .background_color(bg);
                ui.label(text);
            }
        }
    }
    if !any {
        ui.weak("現在の内容と同じです");
    }
}
