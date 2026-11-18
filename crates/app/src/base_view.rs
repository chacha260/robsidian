//! `.base` ファイルを開いたときの表の画面（Bases）。ビューを切り替え、名前をクリックでノートを開く。

use eframe::egui::{self, RichText, ScrollArea};
use robsidian_core::bases::{self, Table};
use robsidian_core::Vault;
use std::time::SystemTime;

pub enum Outcome {
    Open(String),
    OpenExternal(String),
    Close,
}

/// (索引の世代, ファイルの更新時刻, ビュー) が変わったら作り直す
type CacheKey = (u64, Option<SystemTime>, Option<String>);

pub struct BaseView {
    pub path: String,
    view: Option<String>,
    cache: Option<(CacheKey, Vec<String>, Result<Table, String>)>,
}

impl BaseView {
    pub fn new(path: String) -> Self {
        Self {
            path,
            view: None,
            cache: None,
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, vault: &Vault, generation: u64) -> Option<Outcome> {
        let abs = vault.abs(&self.path);
        let mtime = std::fs::metadata(&abs).and_then(|m| m.modified()).ok();
        let key = (generation, mtime, self.view.clone());
        if self.cache.as_ref().is_none_or(|(k, _, _)| *k != key) {
            let parsed = std::fs::read_to_string(&abs)
                .map_err(|e| format!("読めません: {e}"))
                .and_then(|y| bases::parse(&y).map_err(|e| format!("{e:#}")));
            let (names, table) = match parsed {
                Ok(base) => {
                    let names = base.views.iter().map(|v| v.name.clone()).collect();
                    let table = bases::run(vault, &base, self.view.as_deref())
                        .map_err(|e| format!("{e:#}"));
                    (names, table)
                }
                Err(e) => (Vec::new(), Err(e)),
            };
            self.cache = Some((key, names, table));
        }
        let (_, names, table) = self.cache.as_ref()?;
        let mut outcome = None;
        ui.horizontal(|ui| {
            let title = robsidian_core::vault::file_name(&self.path).trim_end_matches(".base");
            ui.heading(title);
            ui.add_space(12.0);
            let current = table.as_ref().ok().map(|t| t.view.clone());
            for name in names {
                let selected = current.as_deref() == Some(name.as_str());
                if ui.selectable_label(selected, name).clicked() {
                    self.view = Some(name.clone());
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("閉じる").clicked() {
                    outcome = Some(Outcome::Close);
                }
                if ui
                    .button("ファイルを開く")
                    .on_hover_text("この .base ファイルを、既定のアプリで開いて編集します")
                    .clicked()
                {
                    outcome = Some(Outcome::OpenExternal(self.path.clone()));
                }
            });
        });
        ui.separator();
        match table {
            Err(e) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
            Ok(t) => {
                ui.weak(if t.total > t.rows.len() {
                    format!("{}件中{}件", t.total, t.rows.len())
                } else {
                    format!("{}件", t.total)
                });
                ui.add_space(4.0);
                ScrollArea::both().auto_shrink(false).show(ui, |ui| {
                    egui::Grid::new(("base-table", &self.path))
                        .striped(true)
                        .spacing([18.0, 6.0])
                        .show(ui, |ui| {
                            for (_, name) in &t.columns {
                                ui.label(RichText::new(name).strong());
                            }
                            ui.end_row();
                            for row in &t.rows {
                                for (cell, (key, _)) in row.cells.iter().zip(&t.columns) {
                                    if key == "file.name" {
                                        if ui.link(cell).clicked() {
                                            outcome = Some(Outcome::Open(row.path.clone()));
                                        }
                                    } else {
                                        ui.label(cell);
                                    }
                                }
                                ui.end_row();
                            }
                        });
                    if t.rows.is_empty() {
                        ui.weak("当てはまるノートはありません");
                    }
                });
            }
        }
        outcome
    }
}
