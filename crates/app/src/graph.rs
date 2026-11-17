//! リンクグラフ表示（力学モデルによる簡易レイアウト）。

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use robsidian_core::Vault;
use std::collections::{HashMap, HashSet, VecDeque};

const MAX_NODES: usize = 1200;

struct Node {
    path: String,
    name: String,
    pos: Vec2,
    vel: Vec2,
    degree: usize,
}

#[derive(Default)]
pub struct Graph {
    nodes: Vec<Node>,
    edges: Vec<(usize, usize)>,
    key: Option<(u64, bool, String)>,
    pub local: bool,
    pan: Vec2,
    zoom: f32,
    alpha: f32,
    /// ユーザーが動かすまでは全体が収まるよう自動でズームする
    fit: bool,
}

impl Graph {
    pub fn new(local: bool) -> Self {
        Self {
            local,
            ..Default::default()
        }
    }

    fn rebuild(&mut self, vault: &Vault, center: Option<&str>) {
        let all_edges = vault.edges();
        let mut keep: Option<HashSet<&str>> = None;
        if let (true, Some(center)) = (self.local, center) {
            // 現在のノートから2ホップ以内
            let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
            for (a, b) in &all_edges {
                adj.entry(a).or_default().push(b);
                adj.entry(b).or_default().push(a);
            }
            let mut seen: HashSet<&str> = HashSet::from([center]);
            let mut queue = VecDeque::from([(center, 0)]);
            while let Some((node, depth)) = queue.pop_front() {
                if depth == 2 || seen.len() >= MAX_NODES {
                    continue;
                }
                for next in adj.get(node).into_iter().flatten() {
                    if seen.insert(next) {
                        queue.push_back((next, depth + 1));
                    }
                }
            }
            keep = Some(seen);
        }

        let mut degree: HashMap<&str, usize> = HashMap::new();
        for (a, b) in &all_edges {
            *degree.entry(a).or_default() += 1;
            *degree.entry(b).or_default() += 1;
        }
        let mut paths: Vec<&str> = vault
            .notes()
            .map(|n| n.path.as_str())
            .filter(|p| keep.as_ref().is_none_or(|k| k.contains(p)))
            .collect();
        if paths.len() > MAX_NODES {
            // 大きすぎるvaultではリンクの多いノートだけを描く
            paths.sort_by_key(|p| std::cmp::Reverse(degree.get(p).copied().unwrap_or(0)));
            paths.truncate(MAX_NODES);
        }

        let index: HashMap<&str, usize> = paths.iter().enumerate().map(|(i, p)| (*p, i)).collect();
        let radius = 30.0 * (paths.len() as f32).sqrt();
        self.nodes = paths
            .iter()
            .enumerate()
            .map(|(i, p)| {
                // 黄金角で渦巻き状に初期配置する（毎回同じ形になる）
                let r = radius * ((i as f32 + 0.5) / paths.len() as f32).sqrt();
                let a = i as f32 * 2.399_963;
                Node {
                    path: p.to_string(),
                    name: vault.note(p).map_or_else(String::new, |n| n.name.clone()),
                    pos: Vec2::new(r * a.cos(), r * a.sin()),
                    vel: Vec2::ZERO,
                    degree: degree.get(p).copied().unwrap_or(0),
                }
            })
            .collect();
        self.edges = all_edges
            .iter()
            .filter_map(|(a, b)| Some((*index.get(a)?, *index.get(b)?)))
            .collect();
        self.pan = Vec2::ZERO;
        self.zoom = 1.0;
        self.alpha = 1.0;
        self.fit = true;
    }

    fn step(&mut self) {
        let n = self.nodes.len();
        let mut force = vec![Vec2::ZERO; n];
        for i in 0..n {
            for j in i + 1..n {
                let d = self.nodes[i].pos - self.nodes[j].pos;
                let dist2 = d.length_sq().max(25.0);
                let f = d * (3000.0 / (dist2 * dist2.sqrt()));
                force[i] += f;
                force[j] -= f;
            }
            force[i] -= self.nodes[i].pos * 0.012;
        }
        for &(a, b) in &self.edges {
            let d = self.nodes[b].pos - self.nodes[a].pos;
            let f = d * 0.03 * (1.0 - 60.0 / d.length().max(1.0));
            force[a] += f;
            force[b] -= f;
        }
        for (node, f) in self.nodes.iter_mut().zip(force) {
            node.vel = (node.vel + f * self.alpha) * 0.82;
            node.pos += node.vel.clamp(Vec2::splat(-30.0), Vec2::splat(30.0));
        }
        self.alpha *= 0.985;
    }

    /// グラフを描画する。ノードがクリックされたらそのパスを返す。
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        vault: &Vault,
        generation: u64,
        current: Option<&str>,
    ) -> Option<String> {
        let key = (
            generation,
            self.local,
            if self.local {
                current.unwrap_or("").to_string()
            } else {
                String::new()
            },
        );
        if self.key.as_ref() != Some(&key) {
            self.rebuild(vault, current);
            self.key = Some(key);
        }
        if self.alpha > 0.02 {
            self.step();
            ui.ctx().request_repaint();
        }

        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        if response.dragged() {
            self.pan += response.drag_delta();
            self.fit = false;
        }
        if response.hovered() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = pinch * (scroll * 0.002).exp();
            if factor != 1.0 {
                if let Some(mouse) = response.hover_pos() {
                    // マウス位置を固定点にしてズームする
                    let anchor = mouse - rect.center() - self.pan;
                    self.pan -= anchor * (factor - 1.0);
                }
                self.zoom = (self.zoom * factor).clamp(0.05, 8.0);
                self.fit = false;
            }
        }

        if self.fit && !self.nodes.is_empty() {
            let extent = self
                .nodes
                .iter()
                .fold(Vec2::splat(1.0), |m, n| m.max(n.pos.abs()));
            let scale = (rect.size() * 0.5 - Vec2::splat(50.0)).max(Vec2::splat(10.0)) / extent;
            self.zoom = scale.min_elem().clamp(0.05, 2.2);
            self.pan = Vec2::ZERO;
        }

        let origin = rect.center() + self.pan;
        let zoom = self.zoom;
        let to_screen = |p: Vec2| -> Pos2 { origin + p * zoom };
        let node_radius = |n: &Node| (3.0 + (n.degree as f32).sqrt() * 1.5) * zoom.clamp(0.5, 1.5);

        let hovered = response.hover_pos().and_then(|mouse| {
            self.nodes
                .iter()
                .enumerate()
                .map(|(i, n)| (i, to_screen(n.pos).distance(mouse), node_radius(n)))
                .filter(|&(_, dist, r)| dist < r + 6.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _, _)| i)
        });

        let visuals = ui.visuals();
        let accent = visuals.selection.bg_fill;
        let text_color = visuals.text_color();
        let edge_color = visuals.weak_text_color().gamma_multiply(0.35);
        let node_color = visuals.weak_text_color();
        let painter = ui.painter_at(rect);
        let visible = rect.expand(40.0);

        for &(a, b) in &self.edges {
            let (pa, pb) = (to_screen(self.nodes[a].pos), to_screen(self.nodes[b].pos));
            if !visible.intersects(Rect::from_two_pos(pa, pb)) {
                continue;
            }
            let lit = hovered == Some(a) || hovered == Some(b);
            let stroke = if lit {
                Stroke::new(1.5, accent)
            } else {
                Stroke::new(1.0, edge_color)
            };
            painter.line_segment([pa, pb], stroke);
        }
        let show_labels = self.nodes.len() <= 60 || zoom > 1.6;
        for (i, n) in self.nodes.iter().enumerate() {
            let p = to_screen(n.pos);
            if !visible.contains(p) {
                continue;
            }
            let is_current = current == Some(n.path.as_str());
            let is_hovered = hovered == Some(i);
            let color = if is_current || is_hovered {
                accent
            } else {
                node_color
            };
            painter.circle_filled(p, node_radius(n), color);
            if show_labels || is_hovered || is_current {
                painter.text(
                    p + Vec2::new(0.0, node_radius(n) + 2.0),
                    egui::Align2::CENTER_TOP,
                    &n.name,
                    egui::FontId::proportional(11.0),
                    if is_hovered || is_current {
                        text_color
                    } else {
                        node_color
                    },
                );
            }
        }
        if self.nodes.is_empty() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "表示できるノートがありません",
                egui::FontId::proportional(14.0),
                Color32::GRAY,
            );
        }

        if hovered.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        (response.clicked())
            .then_some(hovered)
            .flatten()
            .map(|i| self.nodes[i].path.clone())
    }
}
