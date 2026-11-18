//! テーマ設定ファイル（`.robsidian/theme.toml`）を読み、エディタの配色と画面全体の色に当てる。
//! ファイルの更新時刻を見て読み直すので、書き換えるとすぐ反映される。

use crate::editor::Colors;
use eframe::egui::{self, Color32};
use robsidian_core::theme::{self, Rgba, Theme};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

const RECHECK: Duration = Duration::from_secs(1);

/// 画面全体（egui）の色のキー
const UI_KEYS: &[&str] = &["background", "input_bg", "selection", "ui_text", "link"];

/// エディタの色のキー（`Colors` の項目）
const EDITOR_KEYS: &[&str] = &[
    "text",
    "dim",
    "heading",
    "accent",
    "tag",
    "strong",
    "code",
    "code_bg",
    "quote",
    "mark_bg",
    "diff_add",
    "diff_del",
    "find_bg",
    "find_current",
    "warn_bg",
];

#[derive(Default)]
pub struct ThemeFile {
    theme: Theme,
    /// 読み直すたびに増える（画面の色や組版をやり直す目印）
    pub generation: u64,
    stamp: Option<SystemTime>,
    checked: Option<Instant>,
}

fn color(c: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

impl ThemeFile {
    /// 必要なら読み直す。読み直して誤りがあれば、その説明を返す。
    pub fn refresh(&mut self, root: &Path) -> Option<String> {
        if self.checked.is_some_and(|t| t.elapsed() < RECHECK) {
            return None;
        }
        self.checked = Some(Instant::now());
        let stamp = std::fs::metadata(theme::path(root))
            .and_then(|m| m.modified())
            .ok();
        if stamp == self.stamp {
            return None;
        }
        self.stamp = stamp;
        self.generation += 1;
        match theme::load(root) {
            Ok(t) => {
                self.theme = t.unwrap_or_default();
                let unknown: Vec<&str> = self
                    .theme
                    .dark
                    .keys()
                    .chain(self.theme.light.keys())
                    .map(String::as_str)
                    .filter(|k| !UI_KEYS.contains(k) && !EDITOR_KEYS.contains(k))
                    .collect();
                (!unknown.is_empty()).then(|| {
                    format!(
                        "theme.toml の知らないキー: {}（使えるのは {} と {}）",
                        unknown.join(", "),
                        EDITOR_KEYS.join(", "),
                        UI_KEYS.join(", ")
                    )
                })
            }
            Err(e) => {
                self.theme = Theme::default();
                Some(format!("{e:#}"))
            }
        }
    }

    /// 次の `refresh` で必ずファイルを確かめる
    pub fn recheck(&mut self) {
        self.checked = None;
    }

    /// vaultを開き直したとき
    pub fn reset(&mut self) {
        *self = Self {
            generation: self.generation + 1,
            ..Self::default()
        };
    }

    /// エディタの配色（既定の色にテーマの指定を重ねる）
    pub fn colors(&self, dark: bool) -> Colors {
        let mut c = Colors::new(dark);
        for (key, &v) in self.theme.colors(dark) {
            let slot = match key.as_str() {
                "text" => &mut c.text,
                "dim" => &mut c.dim,
                "heading" => &mut c.heading,
                "accent" => &mut c.accent,
                "tag" => &mut c.tag,
                "strong" => &mut c.strong,
                "code" => &mut c.code,
                "code_bg" => &mut c.code_bg,
                "quote" => &mut c.quote,
                "mark_bg" => &mut c.mark_bg,
                "diff_add" => &mut c.diff_add,
                "diff_del" => &mut c.diff_del,
                "find_bg" => &mut c.find_bg,
                "find_current" => &mut c.find_current,
                "warn_bg" => &mut c.warn_bg,
                _ => continue,
            };
            *slot = color(v);
        }
        c
    }

    /// 画面全体の色。既定の色から作り直すので、指定を消せば元に戻る。
    pub fn visuals(&self, dark: bool) -> egui::Visuals {
        let mut v = if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        let editor = self.colors(dark);
        let t = self.theme.colors(dark);
        if let Some(&c) = t.get("background") {
            v.panel_fill = color(c);
            v.window_fill = color(c);
        }
        if let Some(&c) = t.get("input_bg") {
            v.extreme_bg_color = color(c);
            v.text_edit_bg_color = Some(color(c));
        }
        if let Some(&c) = t.get("selection") {
            v.selection.bg_fill = color(c);
        }
        if let Some(&c) = t.get("ui_text") {
            v.override_text_color = Some(color(c));
        }
        // リンクの色は、指定が無ければエディタのアクセント色に合わせる
        v.hyperlink_color = t.get("link").map_or(editor.accent, |&c| color(c));
        v.code_bg_color = editor.code_bg;
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_overrides_and_reports_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".robsidian")).unwrap();
        let mut t = ThemeFile::default();
        assert!(t.refresh(dir.path()).is_none());
        assert_eq!(t.colors(true).accent, Colors::new(true).accent);

        std::fs::write(
            theme::path(dir.path()),
            "[dark]\naccent = \"#ff0000\"\nbackground = \"#000\"\n[light]\nnope = \"#fff\"\n",
        )
        .unwrap();
        t.recheck();
        let err = t.refresh(dir.path()).unwrap();
        assert!(err.contains("nope"), "{err}");
        assert_eq!(t.colors(true).accent, Color32::from_rgb(255, 0, 0));
        assert_eq!(t.colors(false).accent, Colors::new(false).accent);
        assert_eq!(t.visuals(true).panel_fill, Color32::BLACK);
        assert_eq!(
            t.visuals(true).hyperlink_color,
            Color32::from_rgb(255, 0, 0)
        );

        std::fs::remove_file(theme::path(dir.path())).unwrap();
        t.recheck();
        assert!(t.refresh(dir.path()).is_none());
        assert_eq!(t.colors(true).accent, Colors::new(true).accent);
    }
}
