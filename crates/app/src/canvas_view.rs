//! `.canvas`（JSON Canvas）を開いたときの画面。表示のみ（編集はしない）。
//! ドラッグで移動、ホイールで拡大縮小。ファイルのノードはクリックでノートを開き、リンクのノードはブラウザで開く。

use crate::editor::Colors;
use eframe::egui::{self, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use robsidian_core::canvas::{self, Canvas, Node};
use robsidian_core::Vault;
use std::time::SystemTime;

pub enum Outcome {
    Open(String),
    OpenUrl(String),
    OpenExternal(String),
    Close,
}

pub struct CanvasView {
    pub path: String,
    /// 画面の左上に来る、キャンバス上の座標
    origin: Vec2,
    zoom: f32,
    /// 最初の描画で全体が収まるように合わせる
    fitted: bool,
    cache: Option<(Option<SystemTime>, Result<Canvas, String>)>,
}

fn rgb(spec: Option<&str>, fallback: Color32) -> Color32 {
    spec.and_then(canvas::color)
        .map_or(fallback, |[r, g, b]| Color32::from_rgb(r, g, b))
}

/// 見出しや強調の記号を外した、表示用の文字列（ノードの中は簡易表示）
fn plain(text: &str) -> String {
    text.lines()
        .map(|l| {
            let t = l.trim_start_matches('#').trim_start();
            t.replace("**", "")
                .replace("__", "")
                .replace("[[", "")
                .replace("]]", "")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

impl CanvasView {
    pub fn new(path: String) -> Self {
        Self {
            path,
            origin: Vec2::ZERO,
            zoom: 1.0,
            fitted: false,
            cache: None,
        }
    }

    fn to_screen(&self, rect: Rect, x: f64, y: f64) -> Pos2 {
        rect.min + (Vec2::new(x as f32, y as f32) - self.origin) * self.zoom
    }

    fn node_rect(&self, rect: Rect, n: &Node) -> Rect {
        let min = self.to_screen(rect, n.x, n.y);
        Rect::from_min_size(min, Vec2::new(n.width as f32, n.height as f32) * self.zoom)
    }

    pub fn show(&mut self, ui: &mut egui::Ui, vault: &Vault, colors: &Colors) -> Option<Outcome> {
        let abs = vault.abs(&self.path);
        let mtime = std::fs::metadata(&abs).and_then(|m| m.modified()).ok();
        if self.cache.as_ref().is_none_or(|(t, _)| *t != mtime) {
            let parsed = std::fs::read_to_string(&abs)
                .map_err(|e| format!("読めません: {e}"))
                .and_then(|t| canvas::parse(&t).map_err(|e| format!("{e:#}")));
            self.cache = Some((mtime, parsed));
        }
        let mut outcome = None;
        ui.horizontal(|ui| {
            let title = robsidian_core::vault::file_name(&self.path).trim_end_matches(".canvas");
            ui.heading(title);
            ui.weak("ドラッグで移動・ホイールで拡大縮小・ノートはクリックで開く（表示のみ）");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("閉じる").clicked() {
                    outcome = Some(Outcome::Close);
                }
                if ui
                    .button("ファイルを開く")
                    .on_hover_text("この .canvas ファイルを、既定のアプリで開きます")
                    .clicked()
                {
                    outcome = Some(Outcome::OpenExternal(self.path.clone()));
                }
                if ui.button("全体を表示").clicked() {
                    self.fitted = false;
                }
            });
        });
        ui.separator();
        let canvas = match &self.cache {
            Some((_, Ok(c))) => c.clone(),
            Some((_, Err(e))) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
                return outcome;
            }
            None => return outcome,
        };
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, colors.code_bg.gamma_multiply(0.5));

        // 全体が収まるように合わせる
        if !self.fitted {
            self.fitted = true;
            if let Some((l, t, r, b)) = canvas.bounds() {
                let (w, h) = ((r - l) as f32, (b - t) as f32);
                let margin = 40.0;
                self.zoom = ((rect.width() - margin * 2.0) / w.max(1.0))
                    .min((rect.height() - margin * 2.0) / h.max(1.0))
                    .clamp(0.1, 1.5);
                let center = Vec2::new((l + r) as f32 / 2.0, (t + b) as f32 / 2.0);
                self.origin = center - rect.size() / 2.0 / self.zoom;
            }
        }
        // 移動と拡大縮小
        if response.dragged() {
            self.origin -= response.drag_delta() / self.zoom;
        }
        if let Some(pointer) = response.hover_pos() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            let factor = ui.input(|i| i.zoom_delta()) * (scroll / 400.0).exp();
            if (factor - 1.0).abs() > f32::EPSILON {
                let before = self.origin + (pointer - rect.min) / self.zoom;
                self.zoom = (self.zoom * factor).clamp(0.1, 4.0);
                self.origin = before - (pointer - rect.min) / self.zoom;
            }
        }
        if canvas.nodes.is_empty() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "空のキャンバスです",
                FontId::proportional(16.0),
                colors.dim,
            );
            return outcome;
        }

        let z = self.zoom;
        // グループ（いちばん下）
        for n in canvas.nodes.iter().filter(|n| n.kind == "group") {
            let r = self.node_rect(rect, n);
            let c = rgb(n.color.as_deref(), colors.dim);
            painter.rect(
                r,
                CornerRadius::same(8),
                c.gamma_multiply(0.08),
                Stroke::new(1.5, c.gamma_multiply(0.8)),
                egui::StrokeKind::Inside,
            );
            if let Some(label) = &n.label {
                painter.text(
                    r.left_top() + Vec2::new(0.0, -4.0),
                    egui::Align2::LEFT_BOTTOM,
                    label,
                    FontId::proportional((15.0 * z).clamp(8.0, 40.0)),
                    c,
                );
            }
        }
        // 辺
        for e in &canvas.edges {
            let (Some(a), Some(b)) = (canvas.node(&e.from_node), canvas.node(&e.to_node)) else {
                continue;
            };
            let (ra, rb) = (self.node_rect(rect, a), self.node_rect(rect, b));
            let side_a = e.from_side.as_deref().unwrap_or_else(|| guess_side(ra, rb));
            let side_b = e.to_side.as_deref().unwrap_or_else(|| guess_side(rb, ra));
            let (pa, na) = anchor(ra, side_a);
            let (pb, nb) = anchor(rb, side_b);
            let reach = ((pb - pa).length() * 0.4).max(30.0 * z);
            let c = rgb(e.color.as_deref(), colors.dim);
            let stroke = Stroke::new((2.0 * z).clamp(1.0, 4.0), c);
            let points = [pa, pa + na * reach, pb + nb * reach, pb];
            painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                points,
                false,
                Color32::TRANSPARENT,
                stroke,
            ));
            let size = (10.0 * z).clamp(5.0, 18.0);
            if e.arrow_at_to() {
                arrow(&painter, pb, -nb, size, c);
            }
            if e.arrow_at_from() {
                arrow(&painter, pa, -na, size, c);
            }
            if let Some(label) = &e.label {
                // 3次ベジェ曲線の真ん中
                let mid = (points[0].to_vec2()
                    + points[1].to_vec2() * 3.0
                    + points[2].to_vec2() * 3.0
                    + points[3].to_vec2())
                    / 8.0;
                let galley = painter.layout_no_wrap(
                    label.clone(),
                    FontId::proportional((13.0 * z).clamp(8.0, 28.0)),
                    colors.text,
                );
                let at = mid.to_pos2() - galley.size() / 2.0;
                painter.rect_filled(
                    Rect::from_min_size(at, galley.size()).expand(3.0),
                    3.0,
                    ui.visuals().panel_fill,
                );
                painter.galley(at, galley, colors.text);
            }
        }
        // ノード
        let font = (14.0 * z).clamp(6.0, 40.0);
        for n in canvas.nodes.iter().filter(|n| n.kind != "group") {
            let r = self.node_rect(rect, n);
            if !r.intersects(rect) {
                continue;
            }
            let c = rgb(n.color.as_deref(), colors.dim.gamma_multiply(0.8));
            painter.rect(
                r,
                CornerRadius::same(6),
                ui.visuals().panel_fill,
                Stroke::new((1.5 * z).clamp(1.0, 3.0), c),
                egui::StrokeKind::Inside,
            );
            let inner = r.shrink(8.0 * z);
            let clip = painter.with_clip_rect(inner.intersect(rect));
            match n.kind.as_str() {
                "text" => {
                    let text = plain(n.text.as_deref().unwrap_or_default());
                    let galley =
                        clip.layout(text, FontId::proportional(font), colors.text, inner.width());
                    clip.galley(inner.min, galley, colors.text);
                }
                "file" => {
                    let file = n.file.clone().unwrap_or_default();
                    let title = robsidian_core::vault::file_name(&file)
                        .trim_end_matches(".md")
                        .to_string();
                    let title_galley = clip.layout(
                        title,
                        FontId::new(font * 1.1, crate::fonts::bold()),
                        colors.heading,
                        inner.width(),
                    );
                    let title_h = title_galley.size().y;
                    clip.galley(inner.min, title_galley, colors.heading);
                    let body = inner.with_min_y(inner.min.y + title_h + 4.0 * z);
                    if crate::preview::is_image(&file) {
                        let uri = crate::preview::file_uri(vault, &file);
                        egui::Image::new(uri)
                            .fit_to_exact_size(body.size())
                            .paint_at(ui, body.intersect(rect));
                    } else if let Some(note) = vault.note(&file) {
                        let start = note.parsed.body_start.min(note.text.len());
                        let excerpt: String = note.text[start..]
                            .lines()
                            .take(30)
                            .collect::<Vec<_>>()
                            .join("\n");
                        let galley = clip.layout(
                            plain(&excerpt),
                            FontId::proportional(font * 0.9),
                            colors.dim,
                            body.width(),
                        );
                        clip.galley(body.min, galley, colors.dim);
                    } else {
                        clip.text(
                            body.min,
                            egui::Align2::LEFT_TOP,
                            "（見つかりません）",
                            FontId::proportional(font * 0.9),
                            colors.dim,
                        );
                    }
                }
                "link" => {
                    let url = n.url.clone().unwrap_or_default();
                    let galley = clip.layout(
                        url,
                        FontId::proportional(font),
                        colors.accent,
                        inner.width(),
                    );
                    clip.galley(inner.min, galley, colors.accent);
                }
                _ => {}
            }
        }
        // クリック: 上にあるノードから探す（グループは除く）
        if response.clicked() {
            if let Some(p) = response.interact_pointer_pos() {
                let hit = canvas
                    .nodes
                    .iter()
                    .rev()
                    .filter(|n| n.kind != "group")
                    .find(|n| self.node_rect(rect, n).contains(p));
                if let Some(n) = hit {
                    match n.kind.as_str() {
                        "file" => {
                            if let Some(f) = &n.file {
                                if vault.note(f).is_some() {
                                    outcome = Some(Outcome::Open(f.clone()));
                                } else {
                                    outcome = Some(Outcome::OpenExternal(f.clone()));
                                }
                            }
                        }
                        "link" => outcome = n.url.clone().map(Outcome::OpenUrl),
                        _ => {}
                    }
                }
            }
        }
        if response.hovered() {
            let over_link = response.hover_pos().is_some_and(|p| {
                canvas.nodes.iter().any(|n| {
                    matches!(n.kind.as_str(), "file" | "link")
                        && self.node_rect(rect, n).contains(p)
                })
            });
            ui.ctx().set_cursor_icon(if over_link {
                egui::CursorIcon::PointingHand
            } else {
                egui::CursorIcon::Grab
            });
        }
        outcome
    }
}

/// 辺の指定が無いとき、相手のノードのある向きの辺を選ぶ
fn guess_side(from: Rect, to: Rect) -> &'static str {
    let d = to.center() - from.center();
    if d.x.abs() > d.y.abs() {
        if d.x > 0.0 {
            "right"
        } else {
            "left"
        }
    } else if d.y > 0.0 {
        "bottom"
    } else {
        "top"
    }
}

/// ノードの辺の中点と、外向きの向き
fn anchor(r: Rect, side: &str) -> (Pos2, Vec2) {
    match side {
        "top" => (r.center_top(), Vec2::new(0.0, -1.0)),
        "bottom" => (r.center_bottom(), Vec2::new(0.0, 1.0)),
        "left" => (r.left_center(), Vec2::new(-1.0, 0.0)),
        _ => (r.right_center(), Vec2::new(1.0, 0.0)),
    }
}

/// `tip` に向かう矢印（`dir` は矢印の進む向き）
fn arrow(painter: &egui::Painter, tip: Pos2, dir: Vec2, size: f32, color: Color32) {
    let dir = dir.normalized();
    let side = Vec2::new(-dir.y, dir.x);
    let back = tip - dir * size;
    painter.add(egui::Shape::convex_polygon(
        vec![tip, back + side * size * 0.5, back - side * size * 0.5],
        color,
        Stroke::NONE,
    ));
}
