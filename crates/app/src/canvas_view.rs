//! `.canvas`（JSON Canvas）を開いたときの画面。表示と編集。
//!
//! - 空いた所をドラッグで移動、ホイールで拡大縮小
//! - カードはクリックで選び、ドラッグで動かす（グループは中のカードごと動く）。右下の角で大きさを変える
//! - 選んだカードの辺の丸からドラッグして、別のカードにつなぐ
//! - ダブルクリック: テキストは書き換え、ノートは開く、空いた所ならテキストカードを作る
//! - 右クリックで色・削除など。Delete で削除、Ctrl+Z / Ctrl+Shift+Z で元に戻す / やり直す
//!
//! 変更は操作が終わるたびにファイルへ書く（書くのはアプリ側。`take_save` で受け取る）。

use crate::editor::Colors;
use eframe::egui::{
    self, Color32, CornerRadius, FontId, Key, Modifiers, Pos2, Rect, Sense, Stroke, Vec2,
};
use robsidian_core::canvas::{self, Canvas, Edge, Node};
use robsidian_core::Vault;
use std::time::SystemTime;

pub enum Outcome {
    Open(String),
    OpenUrl(String),
    OpenExternal(String),
    Close,
}

#[derive(Clone, PartialEq, Debug)]
enum Sel {
    Node(String),
    Edge(String),
}

enum Drag {
    Pan,
    /// カードを動かす（一緒に動くカード, 動かす前）
    Move {
        ids: Vec<String>,
        before: Canvas,
    },
    Resize {
        id: String,
        before: Canvas,
    },
    /// 辺を引いている（元のカード, 元の辺, いまのポインタ）
    Connect {
        from: String,
        side: &'static str,
        to: Pos2,
    },
}

#[derive(Clone, PartialEq)]
enum EditTarget {
    /// テキストカードの本文
    Text(String),
    /// グループの名前
    GroupLabel(String),
    /// 辺のラベル
    EdgeLabel(String),
}

struct Editing {
    target: EditTarget,
    buffer: String,
    /// 最初のフレームで入力欄にフォーカスを移す
    focus: bool,
}

/// 「ノートを追加」の窓（検索語, 置く位置（キャンバスの座標））
struct Picker {
    query: String,
    at: (f64, f64),
}

const SIDES: [&str; 4] = ["top", "right", "bottom", "left"];
/// 元に戻せる回数
const UNDO_LIMIT: usize = 100;

pub struct CanvasView {
    pub path: String,
    /// 画面の左上に来る、キャンバス上の座標
    origin: Vec2,
    zoom: f32,
    /// 最初の描画で全体が収まるように合わせる
    fitted: bool,
    /// 読み込んだときのファイルの更新時刻（外で書き換えられたら読み直す）
    mtime: Option<SystemTime>,
    doc: Option<Result<Canvas, String>>,
    sel: Option<Sel>,
    drag: Option<Drag>,
    editing: Option<Editing>,
    /// 右クリックした対象（メニューを開いている間）
    menu: Option<Option<Sel>>,
    menu_at: (f64, f64),
    picker: Option<Picker>,
    undo: Vec<Canvas>,
    redo: Vec<Canvas>,
    /// ファイルに書く内容（アプリが `take_save` で受け取る）
    save: Option<String>,
}

fn rgb(spec: Option<&str>, fallback: Color32) -> Color32 {
    spec.and_then(canvas::color)
        .map_or(fallback, |[r, g, b]| Color32::from_rgb(r, g, b))
}

/// 見出しや強調の記号を外した、表示用の文字列（カードの中は簡易表示）
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
            mtime: None,
            doc: None,
            sel: None,
            drag: None,
            editing: None,
            menu: None,
            menu_at: (0.0, 0.0),
            picker: None,
            undo: Vec::new(),
            redo: Vec::new(),
            save: None,
        }
    }

    /// ファイルに書く内容があれば受け取る
    pub fn take_save(&mut self) -> Option<String> {
        self.save.take()
    }

    /// 書き終えたあとに呼ぶ（自分で書いた変更を、外からの変更として読み直さないように）
    pub fn saved(&mut self, vault: &Vault) {
        self.mtime = std::fs::metadata(vault.abs(&self.path))
            .and_then(|m| m.modified())
            .ok();
    }

    fn to_screen(&self, rect: Rect, x: f64, y: f64) -> Pos2 {
        rect.min + (Vec2::new(x as f32, y as f32) - self.origin) * self.zoom
    }

    fn to_canvas(&self, rect: Rect, p: Pos2) -> (f64, f64) {
        let v = self.origin + (p - rect.min) / self.zoom;
        (v.x as f64, v.y as f64)
    }

    fn node_rect(&self, rect: Rect, n: &Node) -> Rect {
        let min = self.to_screen(rect, n.x, n.y);
        Rect::from_min_size(min, Vec2::new(n.width as f32, n.height as f32) * self.zoom)
    }

    /// グループの名前の欄（枠の左上の外側）
    fn label_rect(&self, rect: Rect, n: &Node) -> Rect {
        let r = self.node_rect(rect, n);
        let h = (24.0 * self.zoom).clamp(14.0, 48.0);
        Rect::from_min_max(
            r.left_top() - Vec2::new(0.0, h),
            egui::pos2(
                r.left() + r.width().min(280.0 * self.zoom).max(60.0),
                r.top(),
            ),
        )
    }

    /// 辺の曲線の4つの制御点
    fn edge_points(&self, rect: Rect, c: &Canvas, e: &Edge) -> Option<[Pos2; 4]> {
        let (a, b) = (c.node(&e.from_node)?, c.node(&e.to_node)?);
        let (ra, rb) = (self.node_rect(rect, a), self.node_rect(rect, b));
        let side_a = e.from_side.as_deref().unwrap_or_else(|| guess_side(ra, rb));
        let side_b = e.to_side.as_deref().unwrap_or_else(|| guess_side(rb, ra));
        let (pa, na) = anchor(ra, side_a);
        let (pb, nb) = anchor(rb, side_b);
        let reach = ((pb - pa).length() * 0.4).max(30.0 * self.zoom);
        Some([pa, pa + na * reach, pb + nb * reach, pb])
    }

    /// その位置にあるカード（上に描かれたものから。グループは除く）
    fn hit_card(&self, rect: Rect, c: &Canvas, p: Pos2) -> Option<String> {
        c.nodes
            .iter()
            .rev()
            .filter(|n| n.kind != "group")
            .find(|n| self.node_rect(rect, n).contains(p))
            .map(|n| n.id.clone())
    }

    /// その位置にあるグループ。名前の欄と枠の近くだけで拾う
    /// （グループの中の空いた所は、ドラッグで画面を動かせるように）
    fn hit_group(&self, rect: Rect, c: &Canvas, p: Pos2) -> Option<String> {
        c.nodes
            .iter()
            .rev()
            .filter(|n| n.kind == "group")
            .find(|n| {
                let r = self.node_rect(rect, n);
                self.label_rect(rect, n).contains(p)
                    || (r.expand(4.0).contains(p) && !r.shrink(8.0).contains(p))
            })
            .map(|n| n.id.clone())
    }

    fn hit_node(&self, rect: Rect, c: &Canvas, p: Pos2) -> Option<String> {
        self.hit_card(rect, c, p)
            .or_else(|| self.hit_group(rect, c, p))
    }

    /// クリックした対象: カード → 辺 → グループの順（グループの枠の近くを通る辺も選べるように）
    fn hit(&self, rect: Rect, c: &Canvas, p: Pos2) -> Option<Sel> {
        self.hit_card(rect, c, p)
            .map(Sel::Node)
            .or_else(|| self.hit_edge(rect, c, p).map(Sel::Edge))
            .or_else(|| self.hit_group(rect, c, p).map(Sel::Node))
    }

    fn hit_edge(&self, rect: Rect, c: &Canvas, p: Pos2) -> Option<String> {
        c.edges.iter().rev().find_map(|e| {
            let pts = self.edge_points(rect, c, e)?;
            let curve = bezier(pts, 24);
            let near = curve
                .windows(2)
                .any(|w| distance_to_segment(p, w[0], w[1]) < 6.0);
            near.then(|| e.id.clone())
        })
    }

    /// つなぐための丸（辺の中点の少し外）
    fn handles(&self, rect: Rect, n: &Node) -> [(&'static str, Pos2); 4] {
        let r = self.node_rect(rect, n);
        SIDES.map(|s| {
            let (p, dir) = anchor(r, s);
            (s, p + dir * 10.0)
        })
    }

    fn resize_handle(&self, rect: Rect, n: &Node) -> Rect {
        Rect::from_center_size(self.node_rect(rect, n).right_bottom(), Vec2::splat(12.0))
    }

    /// 変更の前の状態を覚え、保存を予約する
    fn commit(&mut self, before: Canvas, after: &Canvas) {
        if before == *after {
            return;
        }
        self.undo.push(before);
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.save = Some(canvas::to_json(after));
    }

    pub fn show(&mut self, ui: &mut egui::Ui, vault: &Vault, colors: &Colors) -> Option<Outcome> {
        // 外で書き換えられたら読み直す（操作の途中は待つ）
        let abs = vault.abs(&self.path);
        let mtime = std::fs::metadata(&abs).and_then(|m| m.modified()).ok();
        if (self.doc.is_none() || self.mtime != mtime)
            && self.drag.is_none()
            && self.editing.is_none()
        {
            self.mtime = mtime;
            self.doc = Some(
                std::fs::read_to_string(&abs)
                    .map_err(|e| format!("読めません: {e}"))
                    .and_then(|t| canvas::parse(&t).map_err(|e| format!("{e:#}"))),
            );
        }
        let mut outcome = None;
        let mut add_text = false;
        let mut add_note = false;
        let (mut undo, mut redo) = (false, false);
        ui.horizontal(|ui| {
            let title = robsidian_core::vault::file_name(&self.path).trim_end_matches(".canvas");
            ui.heading(title);
            ui.weak("使い方").on_hover_text(
                "空いた所のドラッグで移動、ホイールで拡大縮小\n\
                 カードはクリックで選び、ドラッグで動かす（グループは名前をつかむと中ごと動く）\n\
                 選んだカードの丸からドラッグで、別のカードにつなぐ。右下の角で大きさを変える\n\
                 ダブルクリック: テキストは書き換え、ノートは開く、空いた所ならカードを作る\n\
                 右クリックで色・削除など。Delete で削除、Ctrl+Z / Ctrl+Shift+Z で元に戻す / やり直す",
            );
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
                ui.separator();
                redo = ui
                    .add_enabled(!self.redo.is_empty(), egui::Button::new("やり直す"))
                    .on_hover_text("Ctrl+Shift+Z")
                    .clicked();
                undo = ui
                    .add_enabled(!self.undo.is_empty(), egui::Button::new("元に戻す"))
                    .on_hover_text("Ctrl+Z")
                    .clicked();
                ui.separator();
                add_note = ui.button("ノートを追加").clicked();
                add_text = ui.button("カードを追加").clicked();
            });
        });
        ui.separator();
        let mut c = match self.doc.take() {
            Some(Ok(c)) => c,
            Some(Err(e)) => {
                ui.colored_label(ui.visuals().error_fg_color, &e);
                self.doc = Some(Err(e));
                return outcome;
            }
            None => return outcome,
        };
        let out = self.canvas_ui(ui, vault, colors, &mut c, add_text, add_note, undo, redo);
        self.doc = Some(Ok(c));
        outcome.or(out)
    }

    #[allow(clippy::too_many_arguments)]
    fn canvas_ui(
        &mut self,
        ui: &mut egui::Ui,
        vault: &Vault,
        colors: &Colors,
        c: &mut Canvas,
        add_text: bool,
        add_note: bool,
        undo: bool,
        redo: bool,
    ) -> Option<Outcome> {
        let mut outcome = None;
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, colors.code_bg.gamma_multiply(0.5));

        // 全体が収まるように合わせる
        if !self.fitted {
            self.fitted = true;
            match c.bounds() {
                Some((l, t, r, b)) => {
                    let (w, h) = ((r - l) as f32, (b - t) as f32);
                    let margin = 40.0;
                    self.zoom = ((rect.width() - margin * 2.0) / w.max(1.0))
                        .min((rect.height() - margin * 2.0) / h.max(1.0))
                        .clamp(0.1, 1.5);
                    let center = Vec2::new((l + r) as f32 / 2.0, (t + b) as f32 / 2.0);
                    self.origin = center - rect.size() / 2.0 / self.zoom;
                }
                None => {
                    self.zoom = 1.0;
                    self.origin = -rect.size() / 2.0;
                }
            }
        }

        // ---- 見出しのボタンとキー ----
        let center = self.to_canvas(rect, rect.center());
        if add_text {
            self.add_text_at(c, center);
        }
        if add_note {
            self.picker = Some(Picker {
                query: String::new(),
                at: center,
            });
        }
        let keys_here = self.editing.is_none()
            && self.picker.is_none()
            && (response.hovered() || response.has_focus() || self.sel.is_some());
        let (mut undo, mut redo, mut delete, mut escape) = (undo, redo, false, false);
        if keys_here {
            ui.input_mut(|i| {
                redo |= i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)
                    || i.consume_key(Modifiers::COMMAND, Key::Y);
                undo |= i.consume_key(Modifiers::COMMAND, Key::Z);
                delete |= i.consume_key(Modifiers::NONE, Key::Delete)
                    || i.consume_key(Modifiers::NONE, Key::Backspace);
                escape |= i.consume_key(Modifiers::NONE, Key::Escape);
            });
        }
        if undo {
            if let Some(prev) = self.undo.pop() {
                self.redo.push(std::mem::replace(c, prev));
                self.save = Some(canvas::to_json(c));
            }
        }
        if redo {
            if let Some(next) = self.redo.pop() {
                self.undo.push(std::mem::replace(c, next));
                self.save = Some(canvas::to_json(c));
            }
        }
        if delete {
            if let Some(sel) = self.sel.take() {
                self.delete(c, &sel);
            }
        }
        if escape {
            self.sel = None;
        }
        // 消えたものを選んだままにしない
        if let Some(sel) = &self.sel {
            let alive = match sel {
                Sel::Node(id) => c.node(id).is_some(),
                Sel::Edge(id) => c.edges.iter().any(|e| &e.id == id),
            };
            if !alive {
                self.sel = None;
            }
        }

        // ---- ドラッグ ----
        let pointer = response.interact_pointer_pos().or(response.hover_pos());
        if response.drag_started_by(egui::PointerButton::Primary) {
            let p = response
                .interact_pointer_pos()
                .map(|p| p - response.drag_delta())
                .unwrap_or(rect.center());
            self.drag = Some(self.start_drag(rect, c, p));
        } else if response.drag_started() {
            // 中・右ボタンのドラッグは、いつでも画面の移動
            self.drag = Some(Drag::Pan);
        }
        if response.dragged() {
            let d = response.drag_delta() / self.zoom;
            match &mut self.drag {
                Some(Drag::Pan) | None => self.origin -= response.drag_delta() / self.zoom,
                Some(Drag::Move { ids, .. }) => {
                    for id in ids.iter() {
                        if let Some(n) = c.node_mut(id) {
                            n.x += d.x as f64;
                            n.y += d.y as f64;
                        }
                    }
                }
                Some(Drag::Resize { id, .. }) => {
                    if let Some(n) = c.node_mut(id) {
                        n.width = (n.width + d.x as f64).max(60.0);
                        n.height = (n.height + d.y as f64).max(40.0);
                    }
                }
                Some(Drag::Connect { to, .. }) => {
                    if let Some(p) = pointer {
                        *to = p;
                    }
                }
            }
        }
        if response.drag_stopped() {
            match self.drag.take() {
                Some(Drag::Move { ids, before }) => {
                    for id in &ids {
                        if let Some(n) = c.node_mut(id) {
                            n.x = n.x.round();
                            n.y = n.y.round();
                        }
                    }
                    self.commit(before, c);
                }
                Some(Drag::Resize { id, before }) => {
                    if let Some(n) = c.node_mut(&id) {
                        n.width = n.width.round();
                        n.height = n.height.round();
                    }
                    self.commit(before, c);
                }
                Some(Drag::Connect { from, side, to }) => {
                    let target = self
                        .hit_node(rect, c, to)
                        .filter(|id| *id != from)
                        .and_then(|id| c.node(&id).map(|n| (id, self.node_rect(rect, n))));
                    if let Some((id, r)) = target {
                        let to_side = SIDES
                            .into_iter()
                            .min_by(|a, b| {
                                let da = (anchor(r, a).0 - to).length();
                                let db = (anchor(r, b).0 - to).length();
                                da.total_cmp(&db)
                            })
                            .unwrap_or("left");
                        let before = c.clone();
                        let edge = Edge::new(&from, side, &id, to_side);
                        self.sel = Some(Sel::Edge(edge.id.clone()));
                        c.edges.push(edge);
                        self.commit(before, c);
                    }
                }
                Some(Drag::Pan) | None => {}
            }
        }

        // ---- ホイールで拡大縮小 ----
        if let Some(p) = response.hover_pos() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            let factor = ui.input(|i| i.zoom_delta()) * (scroll / 400.0).exp();
            if (factor - 1.0).abs() > f32::EPSILON {
                let before = self.origin + (p - rect.min) / self.zoom;
                self.zoom = (self.zoom * factor).clamp(0.1, 4.0);
                self.origin = before - (p - rect.min) / self.zoom;
            }
        }

        // ---- クリック ----
        if response.clicked() {
            if let Some(p) = response.interact_pointer_pos() {
                self.sel = self.hit(rect, c, p);
                if let Some(Sel::Node(id)) = &self.sel {
                    c.bring_to_front(id);
                }
            }
        }
        if response.double_clicked() {
            if let Some(p) = response.interact_pointer_pos() {
                outcome = self.double_click(rect, vault, c, p);
            }
        }
        if response.secondary_clicked() {
            let p = response.interact_pointer_pos().unwrap_or(rect.center());
            let target = self.hit(rect, c, p);
            self.sel = target.clone();
            self.menu = Some(target);
            self.menu_at = self.to_canvas(rect, p);
        }
        let mut menu_open = false;
        response.context_menu(|ui| {
            menu_open = true;
            if let Some(target) = self.menu.clone() {
                if let Some(o) = self.menu_ui(ui, c, target) {
                    outcome = Some(o);
                }
            }
        });
        if !menu_open {
            self.menu = None;
        }

        // ---- 描画 ----
        self.paint(ui, &painter, rect, vault, colors, c);
        if let Some(Drag::Connect { from, side, to }) = &self.drag {
            if let Some(n) = c.node(from) {
                let (a, dir) = anchor(self.node_rect(rect, n), side);
                let reach = ((*to - a).length() * 0.4).max(30.0);
                painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                    [a, a + dir * reach, *to, *to],
                    false,
                    Color32::TRANSPARENT,
                    Stroke::new(2.0, colors.accent),
                ));
            }
        }
        if c.nodes.is_empty() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "空のキャンバスです。ダブルクリックでカードを作ります",
                FontId::proportional(16.0),
                colors.dim,
            );
        }

        // ---- 書き換えの欄 ----
        self.editing_ui(ui, rect, c);
        self.picker_ui(ui, vault, c);

        // カーソルの形
        if response.hovered() && self.drag.is_none() {
            if let Some(p) = response.hover_pos() {
                let over_handle = self
                    .selected_node(c)
                    .is_some_and(|n| self.resize_handle(rect, n).contains(p));
                let icon = if over_handle {
                    egui::CursorIcon::ResizeNwSe
                } else {
                    match self.hit(rect, c, p) {
                        Some(Sel::Node(_)) => egui::CursorIcon::Move,
                        Some(Sel::Edge(_)) => egui::CursorIcon::PointingHand,
                        None => egui::CursorIcon::Grab,
                    }
                };
                ui.ctx().set_cursor_icon(icon);
            }
        }
        outcome
    }

    fn selected_node<'a>(&self, c: &'a Canvas) -> Option<&'a Node> {
        match &self.sel {
            Some(Sel::Node(id)) => c.node(id),
            _ => None,
        }
    }

    fn start_drag(&mut self, rect: Rect, c: &mut Canvas, p: Pos2) -> Drag {
        // 選んでいるカードの、大きさを変える角とつなぐ丸
        if let Some(n) = self.selected_node(c) {
            if self.resize_handle(rect, n).contains(p) {
                return Drag::Resize {
                    id: n.id.clone(),
                    before: c.clone(),
                };
            }
            if let Some((side, _)) = self
                .handles(rect, n)
                .into_iter()
                .find(|(_, h)| (*h - p).length() < 8.0)
            {
                return Drag::Connect {
                    from: n.id.clone(),
                    side,
                    to: p,
                };
            }
        }
        match self.hit(rect, c, p) {
            Some(Sel::Node(id)) => {
                let before = c.clone();
                c.bring_to_front(&id);
                let ids = c.moving_with(&id);
                self.sel = Some(Sel::Node(id));
                Drag::Move { ids, before }
            }
            Some(Sel::Edge(id)) => {
                self.sel = Some(Sel::Edge(id));
                Drag::Pan
            }
            None => Drag::Pan,
        }
    }

    fn double_click(
        &mut self,
        rect: Rect,
        vault: &Vault,
        c: &mut Canvas,
        p: Pos2,
    ) -> Option<Outcome> {
        let hit = self.hit(rect, c, p);
        if let Some(Sel::Node(id)) = hit {
            let n = c.node(&id)?;
            match n.kind.as_str() {
                "text" => self.start_edit(EditTarget::Text(id), n.text.clone()),
                "group" => self.start_edit(EditTarget::GroupLabel(id), n.label.clone()),
                "file" => {
                    let f = n.file.clone()?;
                    return Some(if vault.note(&f).is_some() {
                        Outcome::Open(f)
                    } else {
                        Outcome::OpenExternal(f)
                    });
                }
                "link" => return n.url.clone().map(Outcome::OpenUrl),
                _ => {}
            }
        } else if let Some(Sel::Edge(id)) = hit {
            let label = c.edges.iter().find(|e| e.id == id)?.label.clone();
            self.start_edit(EditTarget::EdgeLabel(id), label);
        } else {
            let at = self.to_canvas(rect, p);
            self.add_text_at(c, at);
        }
        None
    }

    fn start_edit(&mut self, target: EditTarget, text: Option<String>) {
        self.editing = Some(Editing {
            target,
            buffer: text.unwrap_or_default(),
            focus: true,
        });
    }

    /// テキストカードを作って、すぐ書けるようにする（`at` はカードの中心）
    fn add_text_at(&mut self, c: &mut Canvas, at: (f64, f64)) {
        let before = c.clone();
        let (w, h) = (250.0, 60.0);
        let mut n = Node::new(
            "text",
            (at.0 - w / 2.0).round(),
            (at.1 - h / 2.0).round(),
            w,
            h,
        );
        n.text = Some(String::new());
        let id = n.id.clone();
        c.nodes.push(n);
        self.commit(before, c);
        self.sel = Some(Sel::Node(id.clone()));
        self.start_edit(EditTarget::Text(id), None);
    }

    fn delete(&mut self, c: &mut Canvas, sel: &Sel) {
        let before = c.clone();
        match sel {
            Sel::Node(id) => c.remove_node(id),
            Sel::Edge(id) => c.remove_edge(id),
        }
        self.commit(before, c);
    }

    fn menu_ui(
        &mut self,
        ui: &mut egui::Ui,
        c: &mut Canvas,
        target: Option<Sel>,
    ) -> Option<Outcome> {
        let mut outcome = None;
        let mut close = false;
        match &target {
            Some(sel) => {
                ui.label("色");
                ui.horizontal(|ui| {
                    let mut pick = |ui: &mut egui::Ui, spec: Option<&str>, fill: Color32| {
                        let b = egui::Button::new("").fill(fill).min_size(Vec2::splat(18.0));
                        let hint = spec.map_or("色なし".to_string(), |s| format!("色 {s}"));
                        if ui.add(b).on_hover_text(hint).clicked() {
                            let before = c.clone();
                            let color = spec.map(str::to_string);
                            match sel {
                                Sel::Node(id) => {
                                    if let Some(n) = c.node_mut(id) {
                                        n.color = color;
                                    }
                                }
                                Sel::Edge(id) => {
                                    if let Some(e) = c.edge_mut(id) {
                                        e.color = color;
                                    }
                                }
                            }
                            self.commit(before, c);
                            close = true;
                        }
                    };
                    pick(ui, None, ui.visuals().panel_fill);
                    for spec in ["1", "2", "3", "4", "5", "6"] {
                        pick(ui, Some(spec), rgb(Some(spec), Color32::GRAY));
                    }
                });
                ui.separator();
                match sel {
                    Sel::Node(id) => {
                        let n = c.node(id).cloned();
                        match n.as_ref().map(|n| n.kind.as_str()) {
                            Some("text") if ui.button("テキストを編集").clicked() => {
                                self.start_edit(
                                    EditTarget::Text(id.clone()),
                                    n.and_then(|n| n.text),
                                );
                                close = true;
                            }
                            Some("group") if ui.button("名前を変える").clicked() => {
                                self.start_edit(
                                    EditTarget::GroupLabel(id.clone()),
                                    n.and_then(|n| n.label),
                                );
                                close = true;
                            }
                            Some("file") if ui.button("ノートを開く").clicked() => {
                                outcome = n.and_then(|n| n.file).map(Outcome::Open);
                                close = true;
                            }
                            Some("link") if ui.button("ブラウザで開く").clicked() => {
                                outcome = n.and_then(|n| n.url).map(Outcome::OpenUrl);
                                close = true;
                            }
                            _ => {}
                        }
                    }
                    Sel::Edge(id) => {
                        if ui.button("ラベルを編集").clicked() {
                            let label = c
                                .edges
                                .iter()
                                .find(|e| &e.id == id)
                                .and_then(|e| e.label.clone());
                            self.start_edit(EditTarget::EdgeLabel(id.clone()), label);
                            close = true;
                        }
                        ui.horizontal(|ui| {
                            ui.label("矢印");
                            for (text, from, to) in [
                                ("→", None, None),
                                ("←", Some("arrow"), Some("none")),
                                ("↔", Some("arrow"), None),
                                ("なし", None, Some("none")),
                            ] {
                                if ui.button(text).clicked() {
                                    let before = c.clone();
                                    if let Some(e) = c.edge_mut(id) {
                                        e.from_end = from.map(str::to_string);
                                        e.to_end = to.map(str::to_string);
                                    }
                                    self.commit(before, c);
                                    close = true;
                                }
                            }
                        });
                    }
                }
                if ui.button("削除").clicked() {
                    self.delete(c, sel);
                    self.sel = None;
                    close = true;
                }
            }
            None => {
                let at = self.menu_at;
                if ui.button("テキストカードを追加").clicked() {
                    self.add_text_at(c, at);
                    close = true;
                }
                if ui.button("ノートを追加…").clicked() {
                    self.picker = Some(Picker {
                        query: String::new(),
                        at,
                    });
                    close = true;
                }
                if ui.button("グループを追加").clicked() {
                    let before = c.clone();
                    let mut g = Node::new("group", at.0.round(), at.1.round(), 400.0, 300.0);
                    g.label = Some("グループ".to_string());
                    let id = g.id.clone();
                    // グループは他のカードより後ろに描く
                    c.nodes.insert(0, g);
                    self.commit(before, c);
                    self.sel = Some(Sel::Node(id.clone()));
                    self.start_edit(EditTarget::GroupLabel(id), Some("グループ".to_string()));
                    close = true;
                }
            }
        }
        if close {
            ui.close();
        }
        outcome
    }

    /// テキストカード・グループの名前・辺のラベルを書き換える欄
    fn editing_ui(&mut self, ui: &mut egui::Ui, rect: Rect, c: &mut Canvas) {
        let Some(ed) = self.editing.as_mut() else {
            return;
        };
        let z = self.zoom;
        let font = FontId::proportional((14.0 * z).clamp(9.0, 40.0));
        let area = match &ed.target {
            EditTarget::Text(id) => c.node(id).map(|n| {
                let r = Rect::from_min_size(
                    rect.min + (Vec2::new(n.x as f32, n.y as f32) - self.origin) * z,
                    Vec2::new(n.width as f32, n.height as f32) * z,
                );
                r.shrink(4.0)
            }),
            EditTarget::GroupLabel(id) => c.node(id).map(|n| {
                let r = Rect::from_min_size(
                    rect.min + (Vec2::new(n.x as f32, n.y as f32) - self.origin) * z,
                    Vec2::new(n.width as f32, n.height as f32) * z,
                );
                Rect::from_min_size(r.left_top() - Vec2::new(0.0, 30.0), Vec2::new(240.0, 26.0))
            }),
            EditTarget::EdgeLabel(id) => c.edges.iter().find(|e| &e.id == id).and_then(|e| {
                let (a, b) = (c.node(&e.from_node)?, c.node(&e.to_node)?);
                let mid = |n: &Node| {
                    rect.min
                        + (Vec2::new((n.x + n.width / 2.0) as f32, (n.y + n.height / 2.0) as f32)
                            - self.origin)
                            * z
                };
                let m = mid(a) + (mid(b) - mid(a)) / 2.0;
                Some(Rect::from_center_size(m, Vec2::new(200.0, 26.0)))
            }),
        };
        let Some(area) = area else {
            self.editing = None;
            return;
        };
        let multiline = matches!(ed.target, EditTarget::Text(_));
        let edit = if multiline {
            egui::TextEdit::multiline(&mut ed.buffer)
        } else {
            egui::TextEdit::singleline(&mut ed.buffer)
        };
        let resp = ui.put(
            area.intersect(rect),
            edit.font(font).hint_text(if multiline {
                "テキスト（Esc で確定）"
            } else {
                "名前"
            }),
        );
        if ed.focus {
            resp.request_focus();
            ed.focus = false;
        }
        let esc = resp.has_focus() && ui.input(|i| i.key_pressed(Key::Escape));
        if !(esc || resp.lost_focus()) {
            return;
        }
        let Some(ed) = self.editing.take() else {
            return;
        };
        let before = c.clone();
        let text = (!ed.buffer.trim().is_empty()).then(|| ed.buffer.clone());
        match &ed.target {
            EditTarget::Text(id) => {
                if let Some(n) = c.node_mut(id) {
                    n.text = Some(ed.buffer.clone());
                    // 中身が収まるように、カードを縦に伸ばす（縮めはしない）
                    let lines = ed.buffer.lines().count().max(1) as f64;
                    n.height = n.height.max((lines * 22.0 + 24.0).round());
                }
            }
            EditTarget::GroupLabel(id) => {
                if let Some(n) = c.node_mut(id) {
                    n.label = text;
                }
            }
            EditTarget::EdgeLabel(id) => {
                if let Some(e) = c.edge_mut(id) {
                    e.label = text;
                }
            }
        }
        self.commit(before, c);
    }

    /// 「ノートを追加」の窓
    fn picker_ui(&mut self, ui: &mut egui::Ui, vault: &Vault, c: &mut Canvas) {
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        let mut open = true;
        let mut chosen = None;
        egui::Window::new("ノートを追加")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(320.0)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ui.ctx(), |ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut picker.query).hint_text("ノート・画像の名前"),
                );
                r.request_focus();
                let q = picker.query.to_lowercase();
                let mut items: Vec<&str> = vault
                    .notes()
                    .map(|n| n.path.as_str())
                    .chain(vault.files().filter(|f| !f.ends_with(".canvas")))
                    .filter(|p| p.to_lowercase().contains(&q))
                    .collect();
                items.sort_unstable();
                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .show(ui, |ui| {
                        for p in items.iter().take(100) {
                            if ui.selectable_label(false, *p).clicked() {
                                chosen = Some(p.to_string());
                            }
                        }
                    });
                if ui.input(|i| i.key_pressed(Key::Enter)) {
                    chosen = items.first().map(|p| p.to_string());
                }
            });
        let at = picker.at;
        if let Some(file) = chosen {
            let before = c.clone();
            let (w, h) = if crate::preview::is_image(&file) {
                (400.0, 300.0)
            } else {
                (400.0, 400.0)
            };
            let mut n = Node::new(
                "file",
                (at.0 - w / 2.0).round(),
                (at.1 - h / 2.0).round(),
                w,
                h,
            );
            n.file = Some(file);
            self.sel = Some(Sel::Node(n.id.clone()));
            c.nodes.push(n);
            self.commit(before, c);
            open = false;
        }
        if !open || ui.input(|i| i.key_pressed(Key::Escape)) {
            self.picker = None;
        }
    }

    fn paint(
        &self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        rect: Rect,
        vault: &Vault,
        colors: &Colors,
        c: &Canvas,
    ) {
        let z = self.zoom;
        let selected = |id: &str| match &self.sel {
            Some(Sel::Node(s)) | Some(Sel::Edge(s)) => s == id,
            None => false,
        };
        let editing = |id: &str| {
            self.editing.as_ref().is_some_and(|e| match &e.target {
                EditTarget::Text(t) | EditTarget::GroupLabel(t) | EditTarget::EdgeLabel(t) => {
                    t == id
                }
            })
        };
        // グループ（いちばん下）
        for n in c.nodes.iter().filter(|n| n.kind == "group") {
            let r = self.node_rect(rect, n);
            let col = rgb(n.color.as_deref(), colors.dim);
            painter.rect(
                r,
                CornerRadius::same(8),
                col.gamma_multiply(0.08),
                Stroke::new(
                    if selected(&n.id) { 2.5 } else { 1.5 },
                    if selected(&n.id) {
                        colors.accent
                    } else {
                        col.gamma_multiply(0.8)
                    },
                ),
                egui::StrokeKind::Inside,
            );
            if let Some(label) = n.label.as_ref().filter(|_| !editing(&n.id)) {
                painter.text(
                    r.left_top() + Vec2::new(0.0, -4.0),
                    egui::Align2::LEFT_BOTTOM,
                    label,
                    FontId::proportional((15.0 * z).clamp(8.0, 40.0)),
                    col,
                );
            }
        }
        // 辺
        for e in &c.edges {
            let Some(points) = self.edge_points(rect, c, e) else {
                continue;
            };
            let col = if selected(&e.id) {
                colors.accent
            } else {
                rgb(e.color.as_deref(), colors.dim)
            };
            let width = (2.0 * z).clamp(1.0, 4.0) + if selected(&e.id) { 1.5 } else { 0.0 };
            painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                points,
                false,
                Color32::TRANSPARENT,
                Stroke::new(width, col),
            ));
            let size = (10.0 * z).clamp(5.0, 18.0);
            let dir = |a: Pos2, b: Pos2| (a - b).normalized();
            if e.arrow_at_to() {
                arrow(painter, points[3], dir(points[3], points[2]), size, col);
            }
            if e.arrow_at_from() {
                arrow(painter, points[0], dir(points[0], points[1]), size, col);
            }
            if let Some(label) = e.label.as_ref().filter(|_| !editing(&e.id)) {
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
        // カード
        let font = (14.0 * z).clamp(6.0, 40.0);
        for n in c.nodes.iter().filter(|n| n.kind != "group") {
            let r = self.node_rect(rect, n);
            if !r.intersects(rect) {
                continue;
            }
            let col = rgb(n.color.as_deref(), colors.dim.gamma_multiply(0.8));
            painter.rect(
                r,
                CornerRadius::same(6),
                ui.visuals().panel_fill,
                Stroke::new((1.5 * z).clamp(1.0, 3.0), col),
                egui::StrokeKind::Inside,
            );
            let inner = r.shrink(8.0 * z);
            let clip = painter.with_clip_rect(inner.intersect(rect));
            match n.kind.as_str() {
                "text" if !editing(&n.id) => {
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
        // 選んでいるカードの枠と、つなぐ丸・大きさを変える角
        if let Some(n) = self.selected_node(c) {
            let r = self.node_rect(rect, n);
            if n.kind != "group" {
                painter.rect_stroke(
                    r.expand(2.0),
                    CornerRadius::same(7),
                    Stroke::new(2.0, colors.accent),
                    egui::StrokeKind::Outside,
                );
            }
            for (_, h) in self.handles(rect, n) {
                painter.circle(
                    h,
                    4.5,
                    ui.visuals().panel_fill,
                    Stroke::new(1.5, colors.accent),
                );
            }
            painter.rect_filled(self.resize_handle(rect, n).shrink(2.0), 2.0, colors.accent);
        }
    }
}

/// 辺の指定が無いとき、相手のカードのある向きの辺を選ぶ
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

/// カードの辺の中点と、外向きの向き
fn anchor(r: Rect, side: &str) -> (Pos2, Vec2) {
    match side {
        "top" => (r.center_top(), Vec2::new(0.0, -1.0)),
        "bottom" => (r.center_bottom(), Vec2::new(0.0, 1.0)),
        "left" => (r.left_center(), Vec2::new(-1.0, 0.0)),
        _ => (r.right_center(), Vec2::new(1.0, 0.0)),
    }
}

/// 3次ベジェ曲線を折れ線にする
fn bezier(p: [Pos2; 4], steps: usize) -> Vec<Pos2> {
    (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let u = 1.0 - t;
            let v = p[0].to_vec2() * (u * u * u)
                + p[1].to_vec2() * (3.0 * u * u * t)
                + p[2].to_vec2() * (3.0 * u * t * t)
                + p[3].to_vec2() * (t * t * t);
            v.to_pos2()
        })
        .collect()
}

fn distance_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(f32::EPSILON)).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_curves_and_sides() {
        let pts = [
            Pos2::new(0.0, 0.0),
            Pos2::new(50.0, 0.0),
            Pos2::new(50.0, 100.0),
            Pos2::new(100.0, 100.0),
        ];
        let line = bezier(pts, 10);
        assert_eq!(line.first(), Some(&pts[0]));
        assert_eq!(line.last(), Some(&pts[3]));
        assert!((line[5] - Pos2::new(50.0, 50.0)).length() < 0.01);
        assert!(distance_to_segment(Pos2::new(5.0, 3.0), pts[0], pts[1]) - 3.0 < 1e-4);
        let a = Rect::from_min_size(Pos2::ZERO, Vec2::splat(10.0));
        let b = Rect::from_min_size(Pos2::new(100.0, 0.0), Vec2::splat(10.0));
        assert_eq!(guess_side(a, b), "right");
        assert_eq!(guess_side(b, a), "left");
    }
}
