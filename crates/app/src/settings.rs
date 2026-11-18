//! 設定画面。変更はすぐに反映し、閉じるときに保存する。

use crate::config::{Config, Features, ViewMode, FEATURES};
use eframe::egui::{self, RichText, Slider};
use robsidian_core::Vault;

/// 設定画面で起きた、App側で反映が必要な変更。
#[derive(Default)]
pub struct Outcome {
    pub close: bool,
    /// フォントの指定を変えた（再読み込みが必要）
    pub font: bool,
}

pub struct SettingsView {
    /// 入力途中のフォントのパス（「適用」で設定に入れる）
    font_input: String,
}

impl SettingsView {
    pub fn new(cfg: &Config) -> Self {
        Self {
            font_input: cfg.font_path.clone(),
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        cfg: &mut Config,
        vault: Option<&Vault>,
        current_note: Option<&str>,
        font_in_use: Option<&str>,
    ) -> Outcome {
        let mut out = Outcome::default();
        let modal = egui::Modal::new(egui::Id::unique("settings")).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.heading("設定");
            ui.add_space(4.0);
            let height = (ctx.content_rect().height() * 0.7).min(640.0);
            egui::ScrollArea::vertical()
                .max_height(height)
                .show(ui, |ui| {
                    section(ui, "外観");
                    egui::Grid::new("look")
                        .num_columns(2)
                        .spacing([16.0, 8.0])
                        .show(ui, |ui| {
                            ui.label("テーマ");
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut cfg.dark, true, "ダーク");
                                ui.radio_value(&mut cfg.dark, false, "ライト");
                            });
                            ui.end_row();
                            ui.label("文字サイズ");
                            ui.add(Slider::new(&mut cfg.font_size, 11.0..=24.0).step_by(1.0));
                            ui.end_row();
                            ui.label("読みやすい行幅");
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut cfg.readable_width, "本文の幅を制限する");
                                ui.add_enabled(
                                    cfg.readable_width,
                                    Slider::new(&mut cfg.max_width, 480.0..=1400.0)
                                        .step_by(20.0)
                                        .suffix(" px"),
                                );
                            });
                            ui.end_row();
                            ui.label("スクロール");
                            ui.checkbox(&mut cfg.smooth_scroll, "マウスホイールでなめらかにスクロールする")
                                .on_hover_text("オンにすると、ホイールを回してから少し遅れて、なめらかに動きます");
                            ui.end_row();
                            ui.label("表示モード");
                            ui.horizontal(|ui| {
                                for mode in ViewMode::ALL {
                                    ui.selectable_value(&mut cfg.view_mode, mode, mode.label());
                                }
                            });
                            ui.end_row();
                        });
                    ui.add_space(4.0);
                    ui.label("日本語フォント（空なら自動で探す）");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.font_input)
                                .hint_text("例: C:/Windows/Fonts/meiryo.ttc")
                                .desired_width(400.0),
                        );
                        if ui.button("適用").clicked() {
                            cfg.font_path = self.font_input.trim().to_string();
                            out.font = true;
                        }
                    });
                    if let Some(path) = font_in_use {
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("使用中: {path}")).small().weak(),
                            )
                            .truncate(),
                        );
                    } else {
                        ui.colored_label(
                            ui.visuals().warn_fg_color,
                            "日本語フォントが見つかっていません",
                        );
                    }

                    section(ui, "エディタ");
                    ui.horizontal(|ui| {
                        ui.label("キー操作");
                        let mut style = match (cfg.vim, cfg.helix) {
                            (false, _) => 0,
                            (true, false) => 1,
                            (true, true) => 2,
                        };
                        ui.radio_value(&mut style, 0, "標準");
                        ui.radio_value(&mut style, 1, "Vim")
                            .on_hover_text("Escでノーマルモード、i / a などで挿入モード。ノーマルモード中はIMEを止めます");
                        ui.radio_value(&mut style, 2, "Helix")
                            .on_hover_text("w / x / % などで選んでから d / c / y で操作します。v で選択を広げるモード");
                        cfg.vim = style > 0;
                        cfg.helix = style == 2;
                    });
                ui.checkbox(&mut cfg.smart_lists, "Enterでリストを続け、Tabで字下げする");
                    ui.checkbox(&mut cfg.auto_pair, "括弧を自動で閉じ、選択範囲を記号で囲む");
                    ui.horizontal(|ui| {
                        ui.label("自動保存までの時間");
                        ui.add(Slider::new(&mut cfg.autosave_secs, 0.3..=10.0).suffix(" 秒"));
                    });

                    section(ui, "プレビュー");
                    ui.horizontal(|ui| {
                        ui.label("Mermaid図のコマンド");
                        ui.add(egui::TextEdit::singleline(&mut cfg.mermaid_command).desired_width(240.0))
                            .on_hover_text("後ろに -i 入力 -o 出力 -b transparent -t テーマ を付けて実行します。空にすると使いません");
                    });
                    ui.weak("mermaid-cli（mmdc）が入っていれば、```mermaid の図を画像にして表示します");

                    section(ui, "コアプラグイン");
                    ui.weak("Obsidian のコアプラグインに当たる機能です。使わないものはオフにできます");
                    egui::Grid::new("features")
                        .num_columns(2)
                        .spacing([24.0, 6.0])
                        .show(ui, |ui| {
                            for (i, info) in FEATURES.iter().enumerate() {
                                ui.checkbox((info.value)(&mut cfg.features), info.name)
                                    .on_hover_text(info.hint);
                                if i % 2 == 1 {
                                    ui.end_row();
                                }
                            }
                        });
                    ui.horizontal(|ui| {
                        if ui.small_button("すべてオン").clicked() {
                            cfg.features = Features::default();
                        }
                        if ui.small_button("すべてオフ").clicked() {
                            for info in FEATURES {
                                *(info.value)(&mut cfg.features) = false;
                            }
                        }
                    });

                    section(ui, "ローカル履歴");
                    ui.weak("保存で上書きされる直前の内容を .robsidian/history に残します");
                    egui::Grid::new("history")
                        .num_columns(2)
                        .spacing([16.0, 8.0])
                        .show(ui, |ui| {
                            ui.label("残す件数（ノートごと）");
                            ui.add(Slider::new(&mut cfg.history_versions, 0..=200).suffix(" 件"));
                            ui.end_row();
                            ui.label("残す日数");
                            ui.add(Slider::new(&mut cfg.history_days, 1..=365).suffix(" 日"));
                            ui.end_row();
                        });
                    if cfg.history_versions == 0 {
                        ui.colored_label(ui.visuals().warn_fg_color, "履歴を残しません");
                    }

                    if let Some(vault) = vault {
                        section(ui, "このvault");
                        ui.weak("次の項目はObsidianの設定（.obsidian フォルダ）に従います");
                        egui::Grid::new("vault")
                            .num_columns(2)
                            .spacing([16.0, 6.0])
                            .show(ui, |ui| {
                                ui.label("場所");
                                value(ui, vault.root().display().to_string());
                                ui.end_row();
                                ui.label("テンプレート");
                                ui.weak(format!(
                                    "{}/（{}件）",
                                    vault.template_folder(),
                                    vault.templates().len()
                                ));
                                ui.end_row();
                                ui.label("今日のデイリーノート");
                                value(ui, vault.daily_path(chrono::Local::now().date_naive()));
                                ui.end_row();
                                ui.label("貼り付けた画像の保存先");
                                let sample = vault
                                    .attachment_path(current_note.unwrap_or("note.md"), "画像.png");
                                ui.weak(match sample.rsplit_once('/') {
                                    Some((dir, _)) => format!("{dir}/"),
                                    None => "vaultの直下".to_string(),
                                });
                                ui.end_row();
                            });
                    }
                });
            ui.add_space(8.0);
            ui.separator();
            if ui.button("閉じる").clicked() {
                out.close = true;
            }
        });
        if modal.should_close() {
            out.close = true;
        }
        out
    }
}

/// 長い値は省略して表示する（ホバーで全体を見られる）。
fn value(ui: &mut egui::Ui, text: String) {
    ui.add(egui::Label::new(RichText::new(&text).weak()).truncate())
        .on_hover_text(text);
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(title).strong().size(16.0));
    ui.separator();
}
