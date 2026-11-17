//! 数式の描画。`robsidian_core::math` の木を、egui の文字と線で2次元に組む。
//!
//! 箱（[`MathBox`]）は「幅・ベースラインより上の高さ・下の深さ」を持ち、中身は
//! 箱の左端・ベースラインを原点とした位置で持つ（yは下向きが正）。

use crate::fonts;
use eframe::egui::{self, Color32, FontFamily, FontId, Galley, Pos2, Stroke, Vec2};
use robsidian_core::math::{self, Accent, Node};
use std::sync::Arc;

#[derive(Clone)]
enum Item {
    /// 文字列（位置は左上）
    Text(Vec2, Arc<Galley>),
    /// 折れ線（線の太さは描くときに決める）
    Line(Vec<Vec2>),
    Dot(Vec2, f32),
}

#[derive(Clone, Default)]
pub struct MathBox {
    pub w: f32,
    /// ベースラインより上
    pub asc: f32,
    /// ベースラインより下
    pub desc: f32,
    items: Vec<Item>,
    /// 線の太さ
    stroke: f32,
}

impl MathBox {
    pub fn height(&self) -> f32 {
        self.asc + self.desc
    }

    pub fn size(&self) -> Vec2 {
        Vec2::new(self.w, self.height())
    }

    /// 子の箱を (dx, dy) ずらして取り込む（大きさは呼び出し側で決める）
    fn put(&mut self, child: &MathBox, dx: f32, dy: f32) {
        let d = Vec2::new(dx, dy);
        self.stroke = self.stroke.max(child.stroke);
        for item in &child.items {
            self.items.push(match item {
                Item::Text(p, g) => Item::Text(*p + d, g.clone()),
                Item::Line(pts) => Item::Line(pts.iter().map(|p| *p + d).collect()),
                Item::Dot(p, r) => Item::Dot(*p + d, *r),
            });
        }
    }

    fn line(&mut self, pts: Vec<Vec2>) {
        self.items.push(Item::Line(pts));
    }

    /// 左端・ベースラインの位置を指定して描く
    pub fn paint(&self, painter: &egui::Painter, origin: Pos2, color: Color32) {
        let stroke = Stroke::new(self.stroke.max(1.0), color);
        for item in &self.items {
            match item {
                Item::Text(p, g) => painter.galley(origin + *p, g.clone(), color),
                Item::Line(pts) => {
                    let pts: Vec<Pos2> = pts.iter().map(|p| origin + *p).collect();
                    painter.add(egui::Shape::line(pts, stroke));
                }
                Item::Dot(p, r) => {
                    painter.circle_filled(origin + *p, *r, color);
                }
            }
        }
    }
}

/// 演算子・関係記号（前後に少し空ける）
fn is_operator(s: &str) -> bool {
    matches!(
        s,
        "+" | "−"
            | "-"
            | "="
            | "<"
            | ">"
            | "×"
            | "÷"
            | "⋅"
            | "±"
            | "∓"
            | "≤"
            | "≥"
            | "≠"
            | "≈"
            | "≡"
            | "∼"
            | "≃"
            | "≅"
            | "∝"
            | "≪"
            | "≫"
            | "∈"
            | "∉"
            | "∋"
            | "⊂"
            | "⊃"
            | "⊆"
            | "⊇"
            | "∪"
            | "∩"
            | "∖"
            | "∧"
            | "∨"
            | "⊕"
            | "⊗"
            | "→"
            | "←"
            | "↔"
            | "⇒"
            | "⇐"
            | "⇔"
            | "↦"
            | "⟶"
            | "⟵"
            | "∘"
            | "∗"
    )
}

/// 記号1つの要素ならその文字
fn operator(node: &Node) -> Option<&str> {
    match node {
        Node::Text { text, .. } => Some(if text == "-" { "−" } else { text }),
        _ => None,
    }
}

/// 白抜き文字（ℝ など）を元のラテン文字に戻す
fn from_double_struck(c: char) -> Option<char> {
    Some(match c {
        'ℂ' => 'C',
        'ℍ' => 'H',
        'ℕ' => 'N',
        'ℙ' => 'P',
        'ℚ' => 'Q',
        'ℝ' => 'R',
        'ℤ' => 'Z',
        '\u{1D538}'..='\u{1D551}' => char::from_u32('A' as u32 + (c as u32 - 0x1D538))?,
        _ => return None,
    })
}

struct Layouter<'a, 'b> {
    fonts: &'a mut egui::epaint::text::FontsView<'b>,
    color: Color32,
}

impl Layouter<'_, '_> {
    fn text(&mut self, text: &str, size: f32, family: FontFamily) -> MathBox {
        let mut family = family;
        let mut text = text.to_string();
        // フォントに字形が無い記号は、近い字形に置き換える（豆腐を出さない）
        if !self
            .fonts
            .has_glyphs(&FontId::new(size, family.clone()), &text)
        {
            let mut struck = false;
            text = text
                .chars()
                .map(|c| {
                    if self.fonts.has_glyph(&FontId::new(size, family.clone()), c) {
                        return c;
                    }
                    if let Some(latin) = from_double_struck(c) {
                        struck = true;
                        return latin;
                    }
                    match c {
                        '⋅' | '∙' => '·',
                        '⟶' => '→',
                        '⟵' => '←',
                        '⟨' => '〈',
                        '⟩' => '〉',
                        '∗' => '*',
                        '∖' => '\\',
                        '′' => '\'',
                        '−' => '-',
                        c => c,
                    }
                })
                .collect();
            // ℝ などは太字で代用する
            if struck {
                family = fonts::bold();
            }
        }
        let galley = self
            .fonts
            .layout_no_wrap(text, FontId::new(size, family), self.color);
        // ベースラインの位置（最初の文字から。文字が無ければ大きさから見積もる）
        let baseline = galley
            .rows
            .first()
            .and_then(|r| r.row.glyphs.first().map(|g| r.pos.y + g.pos.y))
            .unwrap_or(size * 0.8);
        let h = galley.size().y;
        MathBox {
            w: galley.size().x,
            asc: baseline,
            desc: (h - baseline).max(0.0),
            items: vec![Item::Text(Vec2::new(0.0, -baseline), galley)],
            stroke: (size * 0.055).max(1.0),
        }
    }

    fn hcat(&mut self, boxes: Vec<MathBox>) -> MathBox {
        let mut out = MathBox::default();
        for b in boxes {
            out.put(&b, out.w, 0.0);
            out.w += b.w;
            out.asc = out.asc.max(b.asc);
            out.desc = out.desc.max(b.desc);
        }
        out
    }

    fn pad(b: MathBox, left: f32, right: f32) -> MathBox {
        let mut out = MathBox {
            stroke: b.stroke,
            ..Default::default()
        };
        out.put(&b, left, 0.0);
        out.w = b.w + left + right;
        out.asc = b.asc;
        out.desc = b.desc;
        out
    }

    /// 数式の軸（分数線や演算子の中心）の、ベースラインからの高さ
    fn axis(size: f32) -> f32 {
        size * 0.27
    }

    fn lay(&mut self, node: &Node, size: f32, display: bool) -> MathBox {
        let script = (size * 0.7).max(7.0);
        match node {
            Node::Text { text, italic } => {
                let text = if text == "-" { "−" } else { text.as_str() };
                let family = if *italic && text.chars().all(|c| c.is_ascii_alphabetic()) {
                    fonts::math_italic()
                } else {
                    FontFamily::Proportional
                };
                self.text(text, size, family)
            }
            Node::Upright(text) => self.text(text, size, FontFamily::Proportional),
            Node::Row(items) => {
                let mut boxes = Vec::with_capacity(items.len());
                for (i, n) in items.iter().enumerate() {
                    let b = self.lay(n, size, display);
                    // 二項演算子・関係記号の前後を空ける。先頭や、演算子・開き括弧の直後（-b など単項）は空けない
                    let binary = operator(n).is_some_and(is_operator)
                        && i > 0
                        && !operator(&items[i - 1])
                            .is_some_and(|p| is_operator(p) || matches!(p, "(" | "[" | "{" | ","));
                    boxes.push(if binary {
                        let gap = size * 0.2;
                        Self::pad(b, gap, gap)
                    } else {
                        b
                    });
                }
                self.hcat(boxes)
            }
            Node::Space(em) => MathBox {
                w: size * em,
                ..Default::default()
            },
            Node::Frac { num, den, line } => {
                let inner = if display { size } else { (size * 0.8).max(7.0) };
                let n = self.lay(num, inner, false);
                let d = self.lay(den, inner, false);
                let a = Self::axis(size);
                let t = (size * 0.055).max(1.0);
                let gap = size * 0.14;
                let pad = size * 0.12;
                let w = n.w.max(d.w) + pad * 2.0;
                let mut out = MathBox {
                    w,
                    stroke: t,
                    ..Default::default()
                };
                let num_base = -(a + t / 2.0 + gap + n.desc);
                let den_base = -a + t / 2.0 + gap + d.asc;
                out.put(&n, (w - n.w) / 2.0, num_base);
                out.put(&d, (w - d.w) / 2.0, den_base);
                if *line {
                    out.line(vec![Vec2::new(pad * 0.5, -a), Vec2::new(w - pad * 0.5, -a)]);
                }
                out.asc = -num_base + n.asc;
                out.desc = (den_base + d.desc).max(0.0);
                Self::pad(out, size * 0.06, size * 0.06)
            }
            Node::Sqrt { index, body } => {
                let b = self.lay(body, size, display);
                let gap = size * 0.12;
                let t = (size * 0.055).max(1.0);
                let sign = size * 0.55;
                let top = -(b.asc + gap);
                let bottom = b.desc;
                let mid = bottom - (bottom - top) * 0.45;
                let idx = index
                    .as_ref()
                    .map(|i| self.lay(i, (size * 0.55).max(6.0), false));
                // 根号の指数の分だけ右へずらす
                let shift = idx.as_ref().map_or(0.0, |i| (i.w - sign * 0.35).max(0.0));
                let mut out = MathBox {
                    stroke: t,
                    ..Default::default()
                };
                out.line(vec![
                    Vec2::new(shift, mid),
                    Vec2::new(shift + sign * 0.25, mid - size * 0.06),
                    Vec2::new(shift + sign * 0.5, bottom),
                    Vec2::new(shift + sign, top),
                    Vec2::new(shift + sign + b.w + size * 0.1, top),
                ]);
                out.put(&b, shift + sign + size * 0.05, 0.0);
                if let Some(i) = &idx {
                    out.put(i, 0.0, mid - i.desc - size * 0.05);
                }
                out.w = shift + sign + b.w + size * 0.15;
                out.asc = (-top + t).max(idx.as_ref().map_or(0.0, |i| -mid + i.height()));
                out.desc = bottom + size * 0.04;
                out
            }
            Node::Scripts { base, sub, sup } => {
                let limits = display && matches!(**base, Node::BigOp { limits: true, .. });
                let b = self.lay(base, size, display);
                let sub = sub.as_ref().map(|n| self.lay(n, script, false));
                let sup = sup.as_ref().map(|n| self.lay(n, script, false));
                let mut out = MathBox {
                    stroke: b.stroke,
                    ..Default::default()
                };
                if limits {
                    // 総和などの添字は上下に置く
                    let gap = size * 0.1;
                    let w =
                        b.w.max(sub.as_ref().map_or(0.0, |s| s.w))
                            .max(sup.as_ref().map_or(0.0, |s| s.w));
                    out.put(&b, (w - b.w) / 2.0, 0.0);
                    out.asc = b.asc;
                    out.desc = b.desc;
                    if let Some(s) = &sup {
                        let y = -(b.asc + gap + s.desc);
                        out.put(s, (w - s.w) / 2.0, y);
                        out.asc = -y + s.asc;
                    }
                    if let Some(s) = &sub {
                        let y = b.desc + gap + s.asc;
                        out.put(s, (w - s.w) / 2.0, y);
                        out.desc = y + s.desc;
                    }
                    out.w = w;
                    return Self::pad(out, size * 0.08, size * 0.08);
                }
                out.put(&b, 0.0, 0.0);
                let x = b.w + size * 0.04;
                out.asc = b.asc;
                out.desc = b.desc;
                let mut w = b.w;
                if let Some(s) = &sup {
                    let y = -(b.asc - s.asc * 0.55).max(size * 0.38);
                    out.put(s, x, y);
                    out.asc = out.asc.max(-y + s.asc);
                    w = w.max(x + s.w);
                }
                if let Some(s) = &sub {
                    let mut y = (b.desc + s.asc * 0.2).max(size * 0.22);
                    if sup.is_some() {
                        y = y.max(size * 0.3);
                    }
                    out.put(s, x, y);
                    out.desc = out.desc.max(y + s.desc);
                    w = w.max(x + s.w);
                }
                out.w = w + size * 0.04;
                out
            }
            Node::BigOp { symbol, limits } => {
                if symbol.chars().count() > 1 {
                    // lim / max などの関数名
                    return Self::pad(
                        self.text(symbol, size, FontFamily::Proportional),
                        size * 0.08,
                        size * 0.08,
                    );
                }
                let scale = if display && *limits {
                    1.7
                } else if display {
                    1.5
                } else {
                    1.2
                };
                let g = self.text(symbol, size * scale, FontFamily::Proportional);
                // 記号の上下の中心を数式の軸にそろえる
                let center = (g.asc - g.desc) / 2.0;
                let dy = center - Self::axis(size);
                let mut out = MathBox {
                    stroke: g.stroke,
                    ..Default::default()
                };
                out.put(&g, 0.0, dy);
                out.w = g.w;
                out.asc = g.asc - dy;
                out.desc = (g.desc + dy).max(0.0);
                Self::pad(out, size * 0.1, size * 0.12)
            }
            Node::Accent { mark, body } => {
                let b = self.lay(body, size, display);
                let mut out = MathBox {
                    stroke: b.stroke,
                    ..Default::default()
                };
                out.put(&b, 0.0, 0.0);
                out.w = b.w;
                out.asc = b.asc;
                out.desc = b.desc;
                let y = -(b.asc + size * 0.06);
                let (l, r) = (b.w * 0.1, b.w * 0.9);
                let c = b.w / 2.0;
                let h = size * 0.12;
                match mark {
                    Accent::Bar => out.line(vec![Vec2::new(l, y), Vec2::new(r, y)]),
                    Accent::Underline => {
                        let y = b.desc + size * 0.06;
                        out.line(vec![Vec2::new(l, y), Vec2::new(r, y)]);
                        out.desc += size * 0.1;
                    }
                    Accent::Vec => {
                        let r = r.max(l + size * 0.4);
                        out.line(vec![Vec2::new(l, y), Vec2::new(r, y)]);
                        out.line(vec![
                            Vec2::new(r - h, y - h * 0.7),
                            Vec2::new(r, y),
                            Vec2::new(r - h, y + h * 0.7),
                        ]);
                    }
                    Accent::Hat => out.line(vec![
                        Vec2::new(c - size * 0.18, y),
                        Vec2::new(c, y - h),
                        Vec2::new(c + size * 0.18, y),
                    ]),
                    Accent::Tilde => {
                        let pts = (0..=8)
                            .map(|i| {
                                let t = i as f32 / 8.0;
                                let x = c - size * 0.2 + t * size * 0.4;
                                Vec2::new(
                                    x,
                                    y - h * 0.5 - (t * std::f32::consts::TAU).sin() * h * 0.4,
                                )
                            })
                            .collect();
                        out.line(pts);
                    }
                    Accent::Dot => out
                        .items
                        .push(Item::Dot(Vec2::new(c, y - h * 0.4), size * 0.05)),
                    Accent::DDot => {
                        out.items.push(Item::Dot(
                            Vec2::new(c - size * 0.1, y - h * 0.4),
                            size * 0.05,
                        ));
                        out.items.push(Item::Dot(
                            Vec2::new(c + size * 0.1, y - h * 0.4),
                            size * 0.05,
                        ));
                    }
                }
                if *mark != Accent::Underline {
                    out.asc += h + size * 0.08;
                }
                out
            }
            Node::Delim { open, close, body } => {
                let b = self.lay(body, size, display);
                self.fence(b, open, close, size)
            }
            Node::Matrix {
                rows,
                open,
                close,
                align_pairs,
            } => {
                let cells: Vec<Vec<MathBox>> = rows
                    .iter()
                    .map(|r| r.iter().map(|c| self.lay(c, size, false)).collect())
                    .collect();
                let cols = cells.iter().map(Vec::len).max().unwrap_or(0);
                let mut col_w = vec![0.0f32; cols];
                for row in &cells {
                    for (i, c) in row.iter().enumerate() {
                        col_w[i] = col_w[i].max(c.w);
                    }
                }
                let row_gap = size * 0.3;
                let col_gap = if *align_pairs { size * 0.1 } else { size * 0.9 };
                let mut out = MathBox {
                    stroke: (size * 0.055).max(1.0),
                    ..Default::default()
                };
                let mut y = 0.0;
                let mut first_asc = 0.0;
                for (r, row) in cells.iter().enumerate() {
                    let asc = row.iter().map(|c| c.asc).fold(size * 0.7, f32::max);
                    let desc = row.iter().map(|c| c.desc).fold(size * 0.25, f32::max);
                    if r == 0 {
                        first_asc = asc;
                    } else {
                        y += asc;
                    }
                    let mut x = 0.0;
                    for (i, c) in row.iter().enumerate() {
                        // aligned: 奇数列は右寄せ、偶数列は左寄せ。cases は左寄せ、行列は中央
                        let dx = if *align_pairs {
                            if i % 2 == 0 {
                                col_w[i] - c.w
                            } else {
                                0.0
                            }
                        } else if open == "{" && close.is_empty() {
                            0.0
                        } else {
                            (col_w[i] - c.w) / 2.0
                        };
                        out.put(c, x + dx, y);
                        x += col_w[i]
                            + if *align_pairs && i % 2 == 0 {
                                0.0
                            } else {
                                col_gap
                            };
                    }
                    y += desc + row_gap;
                }
                let total = first_asc + y - row_gap;
                out.w = col_w.iter().sum::<f32>() + col_gap * cols.saturating_sub(1) as f32;
                // 全体の上下の中心を数式の軸にそろえる
                let top = -Self::axis(size) - total / 2.0;
                let mut centered = MathBox {
                    stroke: out.stroke,
                    ..Default::default()
                };
                centered.put(&out, 0.0, top + first_asc);
                centered.w = out.w;
                centered.asc = -top;
                centered.desc = (top + total).max(0.0);
                if open.is_empty() && close.is_empty() {
                    centered
                } else {
                    self.fence(centered, open, close, size)
                }
            }
        }
    }

    /// 中身の高さに合わせた括弧で囲む
    fn fence(&mut self, body: MathBox, open: &str, close: &str, size: f32) -> MathBox {
        let ext = size * 0.08;
        let top = -(body.asc + ext);
        let bottom = body.desc + ext;
        let tall = bottom - top > size * 1.35;
        let mut out = MathBox {
            stroke: body.stroke.max((size * 0.055).max(1.0)),
            ..Default::default()
        };
        let mut x = 0.0;
        if !open.is_empty() {
            let d = self.delim(open, top, bottom, size, tall, false);
            out.put(&d, 0.0, 0.0);
            x = d.w;
            out.asc = out.asc.max(d.asc);
            out.desc = out.desc.max(d.desc);
        }
        out.put(&body, x, 0.0);
        x += body.w;
        if !close.is_empty() {
            let d = self.delim(close, top, bottom, size, tall, true);
            out.put(&d, x, 0.0);
            x += d.w;
            out.asc = out.asc.max(d.asc);
            out.desc = out.desc.max(d.desc);
        }
        out.w = x;
        out.asc = out.asc.max(body.asc);
        out.desc = out.desc.max(body.desc);
        out
    }

    /// 括弧1つ。中身が低ければ文字のまま、高ければ線で伸ばして描く
    fn delim(
        &mut self,
        d: &str,
        top: f32,
        bottom: f32,
        size: f32,
        tall: bool,
        right: bool,
    ) -> MathBox {
        if !tall {
            return self.text(d, size, FontFamily::Proportional);
        }
        let w = size * 0.42;
        let (l, r) = (w * 0.25, w * 0.75);
        let mid = (top + bottom) / 2.0;
        // 左右対称な形は、左側の形を鏡に映して作る
        let flip = |x: f32| if right { w - x } else { x };
        let pts: Vec<Vec<Vec2>> = match d {
            "(" | ")" => vec![(0..=12)
                .map(|i| {
                    let t = i as f32 / 12.0;
                    let y = top + (bottom - top) * t;
                    let bulge = (t * std::f32::consts::PI).sin();
                    Vec2::new(flip(r - (r - l) * bulge), y)
                })
                .collect()],
            "[" | "]" => vec![vec![
                Vec2::new(flip(r), top),
                Vec2::new(flip(l), top),
                Vec2::new(flip(l), bottom),
                Vec2::new(flip(r), bottom),
            ]],
            "{" | "}" => vec![vec![
                Vec2::new(flip(r), top),
                Vec2::new(flip((l + r) / 2.0), top + size * 0.1),
                Vec2::new(flip((l + r) / 2.0), mid - size * 0.12),
                Vec2::new(flip(l), mid),
                Vec2::new(flip((l + r) / 2.0), mid + size * 0.12),
                Vec2::new(flip((l + r) / 2.0), bottom - size * 0.1),
                Vec2::new(flip(r), bottom),
            ]],
            "|" => vec![vec![Vec2::new(w / 2.0, top), Vec2::new(w / 2.0, bottom)]],
            "‖" => vec![
                vec![Vec2::new(w * 0.35, top), Vec2::new(w * 0.35, bottom)],
                vec![Vec2::new(w * 0.65, top), Vec2::new(w * 0.65, bottom)],
            ],
            "⟨" | "⟩" => vec![vec![
                Vec2::new(flip(r), top),
                Vec2::new(flip(l), mid),
                Vec2::new(flip(r), bottom),
            ]],
            _ => {
                // 線で描けない括弧は、高さに合わせた大きさの文字にする
                let g = self.text(d, (bottom - top) * 0.85, FontFamily::Proportional);
                let dy = (g.asc - g.desc) / 2.0 - (-mid);
                let mut out = MathBox {
                    stroke: g.stroke,
                    ..Default::default()
                };
                out.put(&g, 0.0, dy);
                out.w = g.w;
                out.asc = g.asc - dy;
                out.desc = (g.desc + dy).max(0.0);
                return out;
            }
        };
        let mut out = MathBox {
            w,
            asc: -top,
            desc: bottom.max(0.0),
            stroke: (size * 0.055).max(1.0),
            items: Vec::new(),
        };
        for p in pts {
            out.line(p);
        }
        out
    }
}

/// 数式を組む。`display` は表示数式（`$$…$$`）か。
pub fn layout(ctx: &egui::Context, tex: &str, size: f32, display: bool, color: Color32) -> MathBox {
    let node = math::parse(tex, display);
    ctx.fonts_mut(|f| {
        let mut l = Layouter { fonts: f, color };
        let b = l.lay(&node, size, display);
        Layouter::pad(b, size * 0.05, size * 0.05)
    })
}

/// egui_commonmark の数式描画（`render_math_fn`）から呼ぶ。
pub fn show(ui: &mut egui::Ui, tex: &str, inline: bool) {
    let size = egui::TextStyle::Body.resolve(ui.style()).size;
    let color = ui.visuals().text_color();
    let b = layout(
        ui.ctx(),
        tex,
        if inline { size } else { size * 1.15 },
        !inline,
        color,
    );
    if inline {
        let (rect, resp) = ui.allocate_exact_size(b.size(), egui::Sense::hover());
        b.paint(
            ui.painter(),
            Pos2::new(rect.left(), rect.top() + b.asc),
            color,
        );
        resp.on_hover_text(tex);
    } else {
        // 表示数式は1行を使い、中央に置く
        let pad = size * 0.4;
        let w = ui.available_width().max(b.w);
        let (rect, resp) =
            ui.allocate_exact_size(Vec2::new(w, b.height() + pad * 2.0), egui::Sense::hover());
        let origin = Pos2::new(rect.center().x - b.w / 2.0, rect.top() + pad + b.asc);
        b.paint(ui.painter(), origin, color);
        resp.on_hover_text(tex);
    }
}
