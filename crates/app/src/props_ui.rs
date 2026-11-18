//! エディタの上に出すプロパティ（frontmatter）の欄。
//!
//! プロパティごとに型（テキスト・リスト・数値・チェックボックス・日付・日付と時刻）を持ち、
//! 型に合った入力欄を出す。型は Obsidian と同じく `.obsidian/types.json` に保存し、
//! 指定が無ければ値から推測する。行の先頭の型の印をクリックすると型を変えられる。

use crate::editor::Colors;
use chrono::{Datelike, Duration, Local, NaiveDate};
use eframe::egui::{self, RichText, TextEdit};
use robsidian_core::props::{self, PropType, Value};
use robsidian_core::Vault;
use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

#[derive(Default)]
pub struct PropsPanel {
    /// どのノートの欄か（ノートが変わったら入力途中の内容を捨てる）
    path: String,
    /// キー → 入力途中の値（リストはカンマ区切り）
    buffers: HashMap<String, String>,
    /// 追加しようとしているキー（Some なら追加欄を出す）
    pub adding: Option<String>,
    focus_new: bool,
    /// 型の指定（`types.json`）。読んだ時刻と中身。ときどき読み直す
    types: Option<(Instant, BTreeMap<String, PropType>)>,
    /// 追加するときに候補に出す、vault で使われている名前
    known_keys: Option<Vec<String>>,
    /// カレンダーで表示している月（キー → その月の1日）
    months: HashMap<String, NaiveDate>,
    /// 型を変えた（アプリが `types.json` に書く）
    pub pending_type: Option<(String, PropType)>,
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

/// 型の印（行の先頭の1文字）
fn mark(ty: PropType) -> &'static str {
    match ty {
        PropType::Text => "文",
        PropType::List => "列",
        PropType::Number => "数",
        PropType::Checkbox => "真",
        PropType::Date => "日",
        PropType::DateTime => "時",
    }
}

/// 日付の欄で起きたこと
enum DateEdit {
    Set(NaiveDate),
    Clear,
}

/// 月のカレンダー。日をクリックしたら返す
fn calendar(
    ui: &mut egui::Ui,
    month: &mut NaiveDate,
    selected: Option<NaiveDate>,
    colors: &Colors,
) -> Option<DateEdit> {
    let mut out = None;
    ui.horizontal(|ui| {
        if ui.small_button("<").on_hover_text("前の月").clicked() {
            *month = month
                .checked_sub_months(chrono::Months::new(1))
                .unwrap_or(*month);
        }
        ui.label(RichText::new(format!("{}年{}月", month.year(), month.month())).strong());
        if ui.small_button(">").on_hover_text("次の月").clicked() {
            *month = month
                .checked_add_months(chrono::Months::new(1))
                .unwrap_or(*month);
        }
    });
    let today = Local::now().date_naive();
    egui::Grid::new("calendar")
        .spacing([2.0, 2.0])
        .min_col_width(26.0)
        .show(ui, |ui| {
            for (i, w) in ["日", "月", "火", "水", "木", "金", "土"]
                .iter()
                .enumerate()
            {
                let c = match i {
                    0 => egui::Color32::from_rgb(0xe0, 0x60, 0x60),
                    6 => egui::Color32::from_rgb(0x60, 0x90, 0xe0),
                    _ => colors.dim,
                };
                ui.label(RichText::new(*w).small().color(c));
            }
            ui.end_row();
            // その月の1日の曜日まで空ける
            let lead = month.weekday().num_days_from_sunday() as usize;
            for _ in 0..lead {
                ui.label("");
            }
            let mut day = *month;
            let mut col = lead;
            while day.month() == month.month() {
                let text = RichText::new(day.day().to_string());
                let text = if day == today {
                    text.strong().underline()
                } else {
                    text
                };
                if ui
                    .add_sized(
                        [26.0, 22.0],
                        egui::Button::selectable(selected == Some(day), text),
                    )
                    .clicked()
                {
                    out = Some(DateEdit::Set(day));
                }
                col += 1;
                if col.is_multiple_of(7) {
                    ui.end_row();
                }
                day += Duration::days(1);
            }
        });
    ui.horizontal(|ui| {
        if ui.small_button("今日").clicked() {
            out = Some(DateEdit::Set(today));
        }
        if ui.small_button("消す").clicked() {
            out = Some(DateEdit::Clear);
        }
    });
    out
}

impl PropsPanel {
    /// 「プロパティを追加」を始める（frontmatterが無いノートでも欄を出す）。
    pub fn start_adding(&mut self) {
        self.adding = Some(String::new());
        self.focus_new = true;
    }

    fn types(&mut self, vault: &Vault) -> &BTreeMap<String, PropType> {
        // 外で書き換えられることもあるので、数秒ごとに読み直す
        let stale = self
            .types
            .as_ref()
            .is_none_or(|(at, _)| at.elapsed().as_secs() >= 3);
        if stale {
            self.types = Some((Instant::now(), vault.property_types()));
        }
        &self.types.as_ref().expect("上で入れた").1
    }

    /// 型を変えたあとに呼ぶ（読み直させる）
    pub fn forget_types(&mut self) {
        self.types = None;
    }

    /// 欄を描く。プロパティを変えたら、新しい本文を返す。
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        path: &str,
        text: &str,
        colors: &Colors,
        vault: &Vault,
    ) -> Option<String> {
        if self.path != path {
            self.path = path.to_string();
            self.buffers.clear();
            self.months.clear();
            if self.adding.as_deref().is_some_and(|k| !k.is_empty()) {
                self.adding = None;
            }
        }
        let list = props::read(text);
        if list.is_none() && self.adding.is_none() {
            return None;
        }
        let list = list.unwrap_or_default();
        let types = self.types(vault).clone();
        let mut result = None;
        egui::Frame::NONE
            .fill(colors.code_bg.gamma_multiply(0.6))
            .corner_radius(6.0)
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("プロパティ").small().color(colors.dim));
                // 表の中では使える幅が小さく見積もられるので、入力欄の幅は先に決めておく
                let field_width = (ui.available_width() - 180.0).clamp(120.0, 520.0);
                for p in &list {
                    let ty = types
                        .get(&p.key)
                        .copied()
                        .unwrap_or_else(|| PropType::infer(&p.key, &p.value));
                    ui.horizontal(|ui| {
                        self.type_button(ui, &p.key, ty, colors);
                        // 名前の列は幅をそろえる
                        ui.add_sized(
                            [96.0, ui.spacing().interact_size.y],
                            egui::Label::new(RichText::new(&p.key).color(colors.dim)).truncate(),
                        );
                        if let Some(new) = self.value_ui(ui, path, text, p, ty, field_width, colors)
                        {
                            result = Some(new);
                        }
                        if ui
                            .small_button("×")
                            .on_hover_text("このプロパティを消す")
                            .clicked()
                        {
                            result = Some(props::remove(text, &p.key));
                        }
                    });
                }
                if let Some(new) = self.adding_ui(ui, path, text, &list, &types, vault) {
                    result = Some(new);
                }
            });
        ui.add_space(6.0);
        result
    }

    /// 行の先頭の型の印。クリックで型を選ぶ
    fn type_button(&mut self, ui: &mut egui::Ui, key: &str, ty: PropType, colors: &Colors) {
        let resp = ui
            .add(
                egui::Button::new(RichText::new(mark(ty)).small().color(colors.dim))
                    .frame(false)
                    .min_size(egui::vec2(18.0, 0.0)),
            )
            .on_hover_text(format!("{}（クリックで型を変える）", ty.label()));
        egui::Popup::menu(&resp).show(|ui| {
            for t in PropType::ALL {
                if ui
                    .selectable_label(t == ty, format!("{}  {}", mark(t), t.label()))
                    .clicked()
                {
                    if t != ty {
                        self.pending_type = Some((key.to_string(), t));
                    }
                    ui.close();
                }
            }
        });
    }

    /// 型に合った値の欄。変えたら新しい本文を返す
    #[allow(clippy::too_many_arguments)]
    fn value_ui(
        &mut self,
        ui: &mut egui::Ui,
        path: &str,
        text: &str,
        p: &props::Prop,
        ty: PropType,
        width: f32,
        colors: &Colors,
    ) -> Option<String> {
        let value = &p.value;
        if let Value::Raw(raw) = value {
            ui.label(
                RichText::new(raw.lines().skip(1).collect::<Vec<_>>().join(" / "))
                    .monospace()
                    .weak(),
            )
            .on_hover_text(
                "この形はここでは編集できません。ソース表示（Ctrl+E）で編集してください",
            );
            return None;
        }
        let current = display(value);
        match ty {
            PropType::Checkbox => {
                let mut on = current.trim() == "true";
                if ui.checkbox(&mut on, "").changed() {
                    return Some(props::set_plain(
                        text,
                        &p.key,
                        if on { "true" } else { "false" },
                    ));
                }
                None
            }
            PropType::Date | PropType::DateTime => {
                self.date_ui(ui, path, text, &p.key, &current, ty, width, colors)
            }
            _ => {
                let id = egui::Id::unique(("prop", path, &p.key));
                let focused = ui.memory(|m| m.has_focus(id));
                let buf = self.buffers.entry(p.key.clone()).or_default();
                if !focused {
                    *buf = current.clone();
                }
                let number_ok = ty != PropType::Number
                    || buf.trim().is_empty()
                    || buf.trim().parse::<f64>().is_ok();
                let hint = match ty {
                    PropType::List => "カンマ区切り",
                    PropType::Number => "数値",
                    _ => "空",
                };
                let mut edit = TextEdit::singleline(buf)
                    .id(id)
                    .hint_text(hint)
                    .desired_width(width);
                if !number_ok {
                    edit = edit.text_color(ui.visuals().error_fg_color);
                }
                let resp = ui.add(edit);
                if !number_ok {
                    resp.clone()
                        .on_hover_text("数値ではありません（確定しても書き込みません）");
                }
                // 確定（Enterかフォーカスを外したとき）に書き戻す
                if resp.lost_focus() {
                    let new = match ty {
                        PropType::List => Value::List(parse_list(buf)),
                        _ => Value::Text(buf.trim().to_string()),
                    };
                    if new != *value && number_ok {
                        return Some(props::set(text, &p.key, &new));
                    }
                }
                None
            }
        }
    }

    /// 日付（と時刻）の欄。押すとカレンダーが開く。時刻は欄に直接書く
    #[allow(clippy::too_many_arguments)]
    fn date_ui(
        &mut self,
        ui: &mut egui::Ui,
        path: &str,
        text: &str,
        key: &str,
        current: &str,
        ty: PropType,
        width: f32,
        colors: &Colors,
    ) -> Option<String> {
        let date =
            props::parse_date(current).or_else(|| props::parse_datetime(current).map(|d| d.date()));
        let time = props::parse_datetime(current)
            .map(|d| d.format("%H:%M").to_string())
            .unwrap_or_else(|| "00:00".to_string());
        let shown = match date {
            Some(d) => d.format("%Y-%m-%d").to_string(),
            None if current.is_empty() => "日付を選ぶ".to_string(),
            // 日付として読めない値は、そのまま見せる（押せば日付を選び直せる）
            None => current.to_string(),
        };
        let mut result = None;
        let button = egui::Button::new(if date.is_some() {
            RichText::new(&shown)
        } else {
            RichText::new(&shown).color(colors.dim)
        });
        let resp = ui.add(button).on_hover_text("クリックでカレンダーを開く");
        let write = |d: NaiveDate, time: &str| -> String {
            let v = match ty {
                PropType::DateTime => format!("{}T{time}", d.format("%Y-%m-%d")),
                _ => d.format("%Y-%m-%d").to_string(),
            };
            props::set(text, key, &Value::Text(v))
        };
        let month = self.months.entry(key.to_string()).or_insert_with(|| {
            let d = date.unwrap_or_else(|| Local::now().date_naive());
            d.with_day(1).unwrap_or(d)
        });
        egui::Popup::menu(&resp)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| match calendar(ui, month, date, colors) {
                Some(DateEdit::Set(d)) => {
                    result = Some(write(d, &time));
                    ui.close();
                }
                Some(DateEdit::Clear) => {
                    result = Some(props::set(text, key, &Value::Text(String::new())));
                    ui.close();
                }
                None => {}
            });
        if ty == PropType::DateTime {
            let id = egui::Id::unique(("prop-time", path, key));
            let focused = ui.memory(|m| m.has_focus(id));
            let buf = self.buffers.entry(format!("{key}\u{0}time")).or_default();
            if !focused {
                *buf = time.clone();
            }
            let resp = ui.add(
                TextEdit::singleline(buf)
                    .id(id)
                    .hint_text("HH:MM")
                    .desired_width(56.0),
            );
            if resp.lost_focus() {
                let t = buf.trim();
                let ok = chrono::NaiveTime::parse_from_str(t, "%H:%M").is_ok();
                if ok && t != time {
                    let d = date.unwrap_or_else(|| Local::now().date_naive());
                    result = Some(write(d, t));
                }
            }
        }
        let _ = width;
        result
    }

    /// 「＋ 追加」と、新しいプロパティの名前の欄（vault で使われている名前を候補に出す）
    fn adding_ui(
        &mut self,
        ui: &mut egui::Ui,
        path: &str,
        text: &str,
        list: &[props::Prop],
        types: &BTreeMap<String, PropType>,
        vault: &Vault,
    ) -> Option<String> {
        let mut result = None;
        let Some(mut key) = self.adding.take() else {
            self.known_keys = None;
            if ui
                .add(egui::Button::new(RichText::new("＋ 追加").small()).frame(false))
                .clicked()
            {
                self.start_adding();
            }
            return None;
        };
        let known = self.known_keys.get_or_insert_with(|| vault.property_keys());
        let mut keep = true;
        let focus_new = std::mem::take(&mut self.focus_new);
        let mut chosen: Option<String> = None;
        ui.horizontal(|ui| {
            let edit = ui.add(
                TextEdit::singleline(&mut key)
                    .hint_text("新しいプロパティの名前")
                    .desired_width(200.0),
            );
            if focus_new {
                edit.request_focus();
            }
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let ok = ui.small_button("追加").clicked();
            let cancel = ui.small_button("やめる").clicked()
                || edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape));
            if (enter || ok) && !key.trim().is_empty() {
                chosen = Some(key.trim().to_string());
            } else if cancel {
                keep = false;
            }
        });
        // 候補（入力した文字を含む、まだ無い名前）
        let q = key.trim().to_lowercase();
        let hits: Vec<&String> = known
            .iter()
            .filter(|k| !list.iter().any(|p| &p.key == *k))
            .filter(|k| q.is_empty() || k.to_lowercase().contains(&q))
            .take(8)
            .collect();
        if !hits.is_empty() && chosen.is_none() && keep {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("候補").small().weak());
                for k in hits {
                    if ui.small_button(k).clicked() {
                        chosen = Some(k.clone());
                    }
                }
            });
        }
        if let Some(name) = chosen {
            let ty = types.get(&name).copied().unwrap_or(
                if matches!(name.as_str(), "tags" | "aliases" | "cssclasses") {
                    PropType::List
                } else {
                    PropType::Text
                },
            );
            if !list.iter().any(|p| p.key == name) {
                result = Some(match ty {
                    PropType::List => props::set(text, &name, &Value::List(Vec::new())),
                    PropType::Checkbox => props::set_plain(text, &name, "false"),
                    _ => props::set(text, &name, &Value::Text(String::new())),
                });
            }
            keep = false;
            // 追加した値の欄にすぐ入力できるようにする
            ui.memory_mut(|m| m.request_focus(egui::Id::unique(("prop", path, &name))));
        }
        if keep {
            self.adding = Some(key);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_are_distinct() {
        let marks: std::collections::HashSet<&str> =
            PropType::ALL.iter().map(|t| mark(*t)).collect();
        assert_eq!(marks.len(), PropType::ALL.len());
        assert_eq!(parse_list("a, #b、c ,"), ["a", "b", "c"]);
    }
}
