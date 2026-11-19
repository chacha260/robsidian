//! リボン（画面の左端に縦に並ぶアイコン）のアイコン。
//!
//! フォントに記号の字が無いことがあるので、すべて線と図形で描く（Lucide 風の細い線）。

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Shape, Stroke, Vec2};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    /// クイックスイッチャー（虫めがね）
    QuickSwitch,
    /// グラフビュー（つながった点）
    Graph,
    /// 新しいキャンバス（並んだ枠）
    Canvas,
    /// デイリーノート（カレンダー）
    Daily,
    /// テンプレートの挿入（重なった紙）
    Template,
    /// コマンドパレット（`>_`）
    Palette,
    /// ランダムノート（さいころ）
    Random,
    /// 新しいベース（表）
    Bases,
    /// スライド（スクリーン）
    Slides,
    /// 音声録音（マイク）
    Mic,
    /// ワークスペース（並んだ窓）
    Workspaces,
    /// ユニークノート（＋の付いた紙）
    UniqueNote,
    /// 設定（歯車）
    Gear,
    /// 左サイドバー（左に仕切りのある枠）
    SidebarLeft,
    /// 右サイドバー（右に仕切りのある枠）
    SidebarRight,
}

/// アイコンのボタン。`size` はボタンの一辺、`active` なら色を付ける（録音中など）
pub fn button(ui: &mut egui::Ui, icon: Icon, size: f32, active: Option<Color32>) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        ui.painter()
            .rect_filled(rect, 4.0, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    let color = active.unwrap_or(if response.hovered() {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().weak_text_color()
    });
    let inner = Rect::from_center_size(rect.center(), Vec2::splat(18.0_f32.min(size - 4.0)));
    let bg = if response.hovered() {
        ui.visuals().widgets.hovered.weak_bg_fill
    } else {
        ui.visuals().panel_fill
    };
    paint(ui.painter(), inner, icon, color, bg);
    response
}

/// 18x18 の枠の中の、(0〜1, 0〜1) の位置
fn at(r: Rect, x: f32, y: f32) -> Pos2 {
    r.min + Vec2::new(x * r.width(), y * r.height())
}

/// アイコンを描く。`bg` は穴を抜くときの背景の色
pub fn paint(p: &egui::Painter, r: Rect, icon: Icon, color: Color32, bg: Color32) {
    let w = r.width();
    let stroke = Stroke::new((w / 11.0).max(1.4), color);
    let line = |a: (f32, f32), b: (f32, f32)| {
        p.line_segment([at(r, a.0, a.1), at(r, b.0, b.1)], stroke);
    };
    let rect = |x0: f32, y0: f32, x1: f32, y1: f32, round: f32| {
        p.rect_stroke(
            Rect::from_min_max(at(r, x0, y0), at(r, x1, y1)),
            round,
            stroke,
            egui::StrokeKind::Middle,
        );
    };
    match icon {
        Icon::QuickSwitch => {
            p.circle_stroke(at(r, 0.42, 0.42), w * 0.3, stroke);
            line((0.64, 0.64), (0.92, 0.92));
        }
        Icon::Graph => {
            // 3つの点を線でつないだ網（点は中抜きの丸）
            let pts = [(0.2, 0.24), (0.8, 0.3), (0.46, 0.8)];
            for (a, b) in [(0, 1), (1, 2), (2, 0)] {
                line(pts[a], pts[b]);
            }
            for (x, y) in pts {
                p.circle(at(r, x, y), w * 0.14, bg, stroke);
            }
        }
        Icon::Canvas => {
            rect(0.08, 0.1, 0.5, 0.55, 2.0);
            rect(0.6, 0.1, 0.92, 0.4, 2.0);
            rect(0.38, 0.65, 0.92, 0.92, 2.0);
            line((0.5, 0.33), (0.6, 0.25));
        }
        Icon::Daily => {
            rect(0.1, 0.18, 0.9, 0.92, 2.5);
            line((0.1, 0.4), (0.9, 0.4));
            line((0.32, 0.06), (0.32, 0.26));
            line((0.68, 0.06), (0.68, 0.26));
            p.circle_filled(at(r, 0.35, 0.62), w * 0.06, color);
            p.circle_filled(at(r, 0.65, 0.62), w * 0.06, color);
        }
        Icon::Template => {
            rect(0.28, 0.06, 0.9, 0.78, 2.0);
            // 後ろの紙は、前の紙に隠れるところを描かない
            line((0.1, 0.24), (0.1, 0.94));
            line((0.1, 0.94), (0.72, 0.94));
            line((0.42, 0.3), (0.76, 0.3));
            line((0.42, 0.48), (0.76, 0.48));
        }
        Icon::Palette => {
            rect(0.06, 0.12, 0.94, 0.88, 2.5);
            line((0.24, 0.36), (0.42, 0.5));
            line((0.42, 0.5), (0.24, 0.64));
            line((0.5, 0.66), (0.74, 0.66));
        }
        Icon::Random => {
            rect(0.1, 0.1, 0.9, 0.9, 3.0);
            for (x, y) in [
                (0.32, 0.32),
                (0.68, 0.32),
                (0.5, 0.5),
                (0.32, 0.68),
                (0.68, 0.68),
            ] {
                p.circle_filled(at(r, x, y), w * 0.07, color);
            }
        }
        Icon::Bases => {
            rect(0.08, 0.12, 0.92, 0.88, 2.0);
            line((0.08, 0.37), (0.92, 0.37));
            line((0.08, 0.62), (0.92, 0.62));
            line((0.4, 0.12), (0.4, 0.88));
        }
        Icon::Slides => {
            rect(0.06, 0.12, 0.94, 0.68, 2.0);
            line((0.5, 0.68), (0.5, 0.86));
            line((0.3, 0.92), (0.5, 0.86));
            line((0.7, 0.92), (0.5, 0.86));
        }
        Icon::Mic => {
            p.rect_stroke(
                Rect::from_min_max(at(r, 0.34, 0.04), at(r, 0.66, 0.6)),
                w * 0.16,
                stroke,
                egui::StrokeKind::Middle,
            );
            // 受けの弧
            let arc: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let t = std::f32::consts::PI * i as f32 / 12.0;
                    at(r, 0.5 - 0.3 * t.cos(), 0.42 + 0.3 * t.sin())
                })
                .collect();
            p.add(Shape::line(arc, stroke));
            line((0.5, 0.72), (0.5, 0.94));
        }
        Icon::Workspaces => {
            rect(0.06, 0.12, 0.94, 0.88, 2.0);
            line((0.06, 0.32), (0.94, 0.32));
            line((0.42, 0.32), (0.42, 0.88));
        }
        Icon::UniqueNote => {
            // 右上を折った紙と ＋
            p.add(Shape::line(
                vec![
                    at(r, 0.62, 0.06),
                    at(r, 0.16, 0.06),
                    at(r, 0.16, 0.94),
                    at(r, 0.84, 0.94),
                    at(r, 0.84, 0.28),
                    at(r, 0.62, 0.06),
                    at(r, 0.62, 0.28),
                    at(r, 0.84, 0.28),
                ],
                stroke,
            ));
            line((0.5, 0.46), (0.5, 0.78));
            line((0.34, 0.62), (0.66, 0.62));
        }
        Icon::SidebarLeft => {
            rect(0.06, 0.12, 0.94, 0.88, 2.5);
            line((0.38, 0.12), (0.38, 0.88));
        }
        Icon::SidebarRight => {
            rect(0.06, 0.12, 0.94, 0.88, 2.5);
            line((0.62, 0.12), (0.62, 0.88));
        }
        Icon::Gear => {
            let c = r.center();
            let rad = w / 2.0;
            // 8枚の歯（台形）と、本体の円、真ん中の穴
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let dir = Vec2::new(a.cos(), a.sin());
                let side = Vec2::new(-dir.y, dir.x);
                p.add(Shape::convex_polygon(
                    vec![
                        c + dir * rad * 0.55 - side * rad * 0.2,
                        c + dir * rad - side * rad * 0.13,
                        c + dir * rad + side * rad * 0.13,
                        c + dir * rad * 0.55 + side * rad * 0.2,
                    ],
                    color,
                    Stroke::NONE,
                ));
            }
            p.circle_filled(c, rad * 0.72, color);
            p.circle_filled(c, rad * 0.3, bg);
        }
    }
}
