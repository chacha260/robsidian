//! エディタの上に出すプロパティ（frontmatter）の欄。

use crate::editor::Colors;
use eframe::egui::{self, RichText, TextEdit};
use robsidian_core::props::{self, Value};
use std::collections::HashMap;

/// リストとして扱うキー（Obsidianの既定のプロパティ）。
const LIST_KEYS: [&str; 3] = ["tags", "aliases", "cssclasses"];

#[derive(Default)]
pub struct PropsPanel {
    /// どのノートの欄か（ノートが変わったら入力途中の内容を捨てる）
    path: String,
    /// キー → 入力途中の値（リストはカンマ区切り）
    buffers: HashMap<String, String>,
    /// 追加しようとしているキー（Some なら追加欄を出す）
    pub adding: Option<String>,
    focus_new: bool,
}

fn display(v: &Value) -> String {
    match v {
        Value::Text(t) => t.clone(),
        Value::List(items) => items.join(", "),
        Value::Raw(r) => r.clone(),
    }
}

fn parse_list(s: &str) -> Vec<String> {
    s.split([',', '、'])
        .map(|t| t.trim().trim_start_matches('#').to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

impl PropsPanel {
    /// 「プロパティを追加」を始める（frontmatterが無いノートでも欄を出す）。
    pub fn start_adding(&mut self) {
        self.adding = Some(String::new());
        self.focus_new = true;
    }

    /// 欄を描く。プロパティを変えたら、新しい本文を返す。
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        path: &str,
        text: &str,
        colors: &Colors,
    ) -> Option<String> {
        if self.path != path {
            self.path = path.to_string();
            self.buffers.clear();
            if self.adding.as_deref().is_some_and(|k| !k.is_empty()) {
                self.adding = None;
            }
        }
        let list = props::read(text);
        if list.is_none() && self.adding.is_none() {
            return None;
        }
        let list = list.unwrap_or_default();
        let mut result = None;
        egui::Frame::NONE
            .fill(colors.code_bg.gamma_multiply(0.6))
            .corner_radius(6.0)
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("プロパティ").small().color(colors.dim));
                // 表の中では使える幅が小さく見積もられるので、入力欄の幅は先に決めておく
                let field_width = (ui.available_width() - 150.0).clamp(120.0, 520.0);
                for p in &list {
                    ui.horizontal(|ui| {
                        // 名前の列は幅をそろえる
                        ui.add_sized(
                            [96.0, ui.spacing().interact_size.y],
                            egui::Label::new(RichText::new(&p.key).color(colors.dim)).truncate(),
                        );
                        match &p.value {
                            Value::Raw(raw) => {
                                ui.label(
                                    RichText::new(raw.lines().skip(1).collect::<Vec<_>>().join(" / "))
                                        .monospace()
                                        .weak(),
                                )
                                .on_hover_text(
                                    "この形はここでは編集できません。ソース表示（Ctrl+E）で編集してください",
                                );
                            }
                            value => {
                                let id = egui::Id::unique(("prop", path, &p.key));
                                let focused = ui.memory(|m| m.has_focus(id));
                                let buf = self.buffers.entry(p.key.clone()).or_default();
                                if !focused {
                                    *buf = display(value);
                                }
                                let hint = if matches!(value, Value::List(_)) { "カンマ区切り" } else { "空" };
                                let edit = ui.add(
                                    TextEdit::singleline(buf)
                                        .id(id)
                                        .hint_text(hint)
                                        .desired_width(field_width),
                                );
                                // 確定（Enterかフォーカスを外したとき）に書き戻す
                                if edit.lost_focus() {
                                    let new = match value {
                                        Value::List(_) => Value::List(parse_list(buf)),
                                        _ => Value::Text(buf.trim().to_string()),
                                    };
                                    if new != *value {
                                        result = Some(props::set(text, &p.key, &new));
                                    }
                                }
                            }
                        }
                        if ui.small_button("×").on_hover_text("このプロパティを消す").clicked() {
                            result = Some(props::remove(text, &p.key));
                        }
                    });
                }
                match self.adding.take() {
                    Some(mut key) => {
                        let mut keep = true;
                        let focus_new = std::mem::take(&mut self.focus_new);
                        ui.horizontal(|ui| {
                            let edit = ui.add(
                                TextEdit::singleline(&mut key)
                                    .hint_text("新しいプロパティの名前")
                                    .desired_width(200.0),
                            );
                            if focus_new {
                                edit.request_focus();
                            }
                            let enter =
                                edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            let ok = ui.small_button("追加").clicked();
                            let cancel = ui.small_button("やめる").clicked()
                                || edit.lost_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Escape));
                            let name = key.trim().to_string();
                            if (enter || ok) && !name.is_empty() {
                                let value = if LIST_KEYS.contains(&name.as_str()) {
                                    Value::List(Vec::new())
                                } else {
                                    Value::Text(String::new())
                                };
                                if !list.iter().any(|p| p.key == name) {
                                    result = Some(props::set(text, &name, &value));
                                }
                                keep = false;
                                // 追加した値の欄にすぐ入力できるようにする
                                ui.memory_mut(|m| {
                                    m.request_focus(egui::Id::unique(("prop", path, &name)))
                                });
                            } else if cancel {
                                keep = false;
                            }
                        });
                        if keep {
                            self.adding = Some(key);
                        }
                    }
                    None => {
                        if ui
                            .add(egui::Button::new(RichText::new("＋ 追加").small()).frame(false))
                            .clicked()
                        {
                            self.start_adding();
                        }
                    }
                }
            });
        ui.add_space(6.0);
        result
    }
}
