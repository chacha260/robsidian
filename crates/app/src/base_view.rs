//! `.base` ファイルを開いたときの画面（Bases）。表かカードで出す。ビューを切り替え、名前をクリックでノートを開く。

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
                if t.kind == "cards" {
                    if let Some(path) = cards(ui, vault, t) {
                        outcome = Some(Outcome::Open(path));
                    }
                    return outcome;
                }
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

/// カードのビュー。クリックしたノートのパスを返す
fn cards(ui: &mut egui::Ui, vault: &Vault, t: &Table) -> Option<String> {
    let mut open = None;
    let width = t.card_size.unwrap_or(200.0).clamp(80.0, 600.0);
    let image_h = width * t.image_aspect.unwrap_or(0.6).clamp(0.1, 3.0);
    let has_images = t.rows.iter().any(|r| r.image.is_some());
    let contain = t.image_fit.as_deref() == Some("contain");
    let name_col = t.columns.iter().position(|(k, _)| k == "file.name");
    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        // 1行に入る枚数ずつ並べ、行の中は上端をそろえる
        let gap = 12.0;
        let per_row = (((ui.available_width() + gap) / (width + gap)).floor() as usize).max(1);
        for chunk in t.rows.chunks(per_row) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
                for row in chunk {
                    let frame = egui::Frame::group(ui.style())
                        .corner_radius(6.0)
                        .inner_margin(0.0);
                    // 折り返しの中でも、カードの中身は縦に並べる
                    let resp = ui
                        .allocate_ui_with_layout(
                            egui::vec2(width, 0.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                frame
                                    .show(ui, |ui| {
                                        ui.set_width(width);
                                        ui.set_max_width(width);
                                        if has_images {
                                            let (rect, _) = ui.allocate_exact_size(
                                                egui::vec2(width, image_h),
                                                egui::Sense::hover(),
                                            );
                                            ui.painter().rect_filled(
                                                rect,
                                                egui::CornerRadius {
                                                    nw: 6,
                                                    ne: 6,
                                                    sw: 0,
                                                    se: 0,
                                                },
                                                ui.visuals().faint_bg_color,
                                            );
                                            if let Some(img) = &row.image {
                                                let uri = if img.contains("://") {
                                                    img.clone()
                                                } else {
                                                    crate::preview::file_uri(vault, img)
                                                };
                                                paint_image(ui, &uri, rect, contain);
                                            }
                                        }
                                        egui::Frame::new()
                                            .inner_margin(egui::Margin::symmetric(10, 8))
                                            .show(ui, |ui| {
                                                ui.set_width(width - 20.0);
                                                let title = name_col
                                                    .and_then(|i| row.cells.get(i).cloned())
                                                    .unwrap_or_else(|| {
                                                        robsidian_core::vault::file_name(&row.path)
                                                            .trim_end_matches(".md")
                                                            .to_string()
                                                    });
                                                ui.add(
                                                    egui::Label::new(RichText::new(title).strong())
                                                        .wrap(),
                                                );
                                                for (cell, (key, name)) in
                                                    row.cells.iter().zip(&t.columns)
                                                {
                                                    if key == "file.name" || cell.is_empty() {
                                                        continue;
                                                    }
                                                    ui.horizontal_wrapped(|ui| {
                                                        ui.spacing_mut().item_spacing.x = 6.0;
                                                        ui.weak(name);
                                                        ui.label(cell);
                                                    });
                                                }
                                            });
                                    })
                                    .response
                            },
                        )
                        .inner;
                    let resp = resp.interact(egui::Sense::click());
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        open = Some(row.path.clone());
                    }
                }
            });
            ui.add_space(gap);
        }
        if t.rows.is_empty() {
            ui.weak("当てはまるノートはありません");
        }
    });
    open
}

/// 画像を枠に描く。`contain` なら全体を収め、そうでなければ枠いっぱいに切り抜く（cover）
fn paint_image(ui: &egui::Ui, uri: &str, rect: egui::Rect, contain: bool) {
    let poll = ui.ctx().try_load_texture(
        uri,
        egui::TextureOptions::LINEAR,
        egui::load::SizeHint::default(),
    );
    let Ok(egui::load::TexturePoll::Ready { texture }) = poll else {
        return;
    };
    let size = texture.size;
    if size.x <= 0.0 || size.y <= 0.0 {
        return;
    }
    let full = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    let (dest, uv) = if contain {
        let scale = (rect.width() / size.x).min(rect.height() / size.y);
        (
            egui::Rect::from_center_size(rect.center(), size * scale),
            full,
        )
    } else {
        // 枠と画像の縦横比の違うぶんだけ、画像の端を切る
        let (frame_ratio, image_ratio) = (rect.width() / rect.height(), size.x / size.y);
        let uv = if image_ratio > frame_ratio {
            let w = frame_ratio / image_ratio;
            egui::Rect::from_min_max(
                egui::pos2((1.0 - w) / 2.0, 0.0),
                egui::pos2((1.0 + w) / 2.0, 1.0),
            )
        } else {
            let h = image_ratio / frame_ratio;
            egui::Rect::from_min_max(
                egui::pos2(0.0, (1.0 - h) / 2.0),
                egui::pos2(1.0, (1.0 + h) / 2.0),
            )
        };
        (rect, uv)
    };
    ui.painter()
        .with_clip_rect(rect)
        .image(texture.id, dest, uv, egui::Color32::WHITE);
}
