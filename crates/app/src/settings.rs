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
    /// このプラグインの許可を求める
    pub trust_plugin: Option<String>,
}

pub struct SettingsView {
    /// 入力途中のフォントのパス（「適用」で設定に入れる）
    font_input: String,
    /// プラグインの一覧（catalog.json）を探す欄
    catalog: CatalogState,
}

/// プラグインの一覧を探す欄の状態。読み込みと導入は別スレッドで行う
#[derive(Default)]
struct CatalogState {
    loading:
        Option<std::sync::mpsc::Receiver<anyhow::Result<robsidian_core::plugin_catalog::Catalog>>>,
    /// 読み込んだ一覧（とその URL）、または失敗の説明
    loaded: Option<(
        String,
        Result<robsidian_core::plugin_catalog::Catalog, String>,
    )>,
    query: String,
    installing: Option<(
        String,
        std::sync::mpsc::Receiver<anyhow::Result<std::path::PathBuf>>,
    )>,
    message: Option<String>,
}

impl SettingsView {
    pub fn new(cfg: &Config) -> Self {
        Self {
            font_input: cfg.font_path.clone(),
            catalog: CatalogState::default(),
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        cfg: &mut Config,
        vault: Option<&Vault>,
        current_note: Option<&str>,
        font_in_use: Option<&str>,
        plugins: &crate::plugins::Plugins,
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
                            ui.label("リボン");
                            ui.checkbox(&mut cfg.show_ribbon, "左端にアイコンの列を出す")
                                .on_hover_text("有効なコアプラグインのアイコンと、設定の歯車を並べます");
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
                    ui.weak("（フォーマット変換・Publish・Sync・Web ビューアは Robsidian にありません）");
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

                    if let Some(vault) = vault {
                        plugins_section(ui, cfg, vault.root(), plugins, &mut out);
                        catalog_section(ui, cfg, vault.root(), &mut self.catalog);
                    }

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

/// 「プラグイン」の欄: 一覧・説明・使う / 使わない・許可・読めなかった理由
fn plugins_section(
    ui: &mut egui::Ui,
    cfg: &mut Config,
    root: &std::path::Path,
    plugins: &crate::plugins::Plugins,
    out: &mut Outcome,
) {
    use crate::plugins::{trust_key, usable, Usable};
    section(ui, "プラグイン");
    ui.weak(
        "プラグインは .robsidian/plugins/<id>/plugin.toml に置きます（書き方は README の「プラグイン」）。\
         実行の前に、中身を確かめて許可してください。",
    );
    if plugins.list.is_empty() && plugins.broken.is_empty() {
        ui.weak("このvaultにプラグインはありません");
    }
    for p in &plugins.list {
        let key = trust_key(root, &p.id);
        let m = &p.manifest;
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let mut on = !cfg.disabled_plugins.contains(&key);
            if ui
                .checkbox(&mut on, RichText::new(&m.name).strong())
                .changed()
            {
                if on {
                    cfg.disabled_plugins.remove(&key);
                } else {
                    cfg.disabled_plugins.insert(key.clone());
                }
            }
            ui.weak(format!(
                "{}{}",
                p.id,
                m.version
                    .as_deref()
                    .map(|v| format!(" ・ {v}"))
                    .unwrap_or_default()
            ));
            match usable(cfg, root, p) {
                Usable::Untrusted => {
                    if ui.small_button("中身を見て許可する…").clicked() {
                        out.trust_plugin = Some(p.id.clone());
                    }
                }
                Usable::Yes | Usable::Disabled => {
                    if cfg.trusted_commands.get(&key) == Some(&p.text) {
                        ui.weak("許可済み");
                        if ui.small_button("許可を取り消す").clicked() {
                            cfg.trusted_commands.remove(&key);
                        }
                    } else if ui.small_button("中身を見て許可する…").clicked() {
                        out.trust_plugin = Some(p.id.clone());
                    }
                }
            }
        });
        if let Some(d) = &m.description {
            ui.weak(d);
        }
        // 動き方と、WASM なら権限（外部プロセスは制限できないことも示す）
        let how = match m.mode {
            robsidian_core::plugin::Mode::Once => "外部プロセス（権限の制限なし）".to_string(),
            robsidian_core::plugin::Mode::Server => {
                "外部プロセス・常駐（権限の制限なし）".to_string()
            }
            robsidian_core::plugin::Mode::Wasm if m.permissions.is_empty() => {
                "WASM（権限なし: Robsidian に問い合わせない）".to_string()
            }
            robsidian_core::plugin::Mode::Wasm => format!(
                "WASM（権限: {}）",
                m.permissions
                    .iter()
                    .map(|p| p.label())
                    .collect::<Vec<_>>()
                    .join("・")
            ),
        };
        ui.label(RichText::new(how).small());
        let mut parts: Vec<String> = Vec::new();
        parts.extend(m.command.iter().map(|c| format!("コマンド「{}」", c.name)));
        parts.extend(
            m.codeblock
                .iter()
                .map(|c| format!("コードブロック ```{}", c.lang)),
        );
        parts.extend(m.panel.iter().map(|c| format!("パネル「{}」", c.title)));
        parts.extend(m.event.iter().map(|e| {
            format!(
                "イベント（{}）",
                match e.on {
                    robsidian_core::plugin::EventKind::Save => "保存したとき",
                    robsidian_core::plugin::EventKind::Open => "開いたとき",
                }
            )
        }));
        if !parts.is_empty() {
            ui.label(RichText::new(parts.join(" ・ ")).small());
        }
    }
    for (id, why) in &plugins.broken {
        ui.colored_label(ui.visuals().error_fg_color, format!("{id}: {why}"));
    }
}

/// 「プラグインを探す」の欄: 一覧（catalog.json）の URL・検索・入れる
fn catalog_section(
    ui: &mut egui::Ui,
    cfg: &mut Config,
    root: &std::path::Path,
    st: &mut CatalogState,
) {
    use robsidian_core::plugin_catalog;
    // 読み込みと導入の結果を受け取る
    if let Some(rx) = &st.loading {
        if let Ok(r) = rx.try_recv() {
            st.loaded = Some((cfg.plugin_catalog.clone(), r.map_err(|e| format!("{e:#}"))));
            st.loading = None;
        }
    }
    if let Some((id, rx)) = &st.installing {
        if let Ok(r) = rx.try_recv() {
            st.message = Some(match r {
                Ok(_) => format!(
                    "「{id}」を入れました。上の一覧で「中身を見て許可する…」を押してから使ってください"
                ),
                Err(e) => format!("「{id}」を入れられませんでした: {e:#}"),
            });
            st.installing = None;
        }
    }
    if st.loading.is_some() || st.installing.is_some() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
    }
    ui.add_space(6.0);
    egui::CollapsingHeader::new(RichText::new("プラグインを探す").strong())
        .id_salt("plugin-catalog")
        .show(ui, |ui| {
            ui.weak("プラグインの一覧（catalog.json）の URL。ファイルのパスでもよい（書き方は examples/catalog）");
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut cfg.plugin_catalog)
                        .hint_text("https://…/catalog.json")
                        .desired_width(380.0),
                );
                let url = cfg.plugin_catalog.trim().to_string();
                if ui
                    .add_enabled(st.loading.is_none() && !url.is_empty(), egui::Button::new("読み込む"))
                    .clicked()
                {
                    let (tx, rx) = std::sync::mpsc::channel();
                    std::thread::spawn(move || {
                        let _ = tx.send(plugin_catalog::load(&url));
                    });
                    st.loading = Some(rx);
                    st.message = None;
                }
            });
            if st.loading.is_some() {
                ui.weak("読み込んでいます…");
            }
            if let Some(m) = &st.message {
                ui.label(RichText::new(m).small());
            }
            let Some((url, loaded)) = &st.loaded else {
                return;
            };
            let catalog = match loaded {
                Ok(c) => c,
                Err(e) => {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                    return;
                }
            };
            ui.add(egui::TextEdit::singleline(&mut st.query).hint_text("絞り込み"));
            let hits = plugin_catalog::search(catalog, &st.query);
            if hits.is_empty() {
                ui.weak("一致するプラグインはありません");
            }
            let mut install = None;
            for e in hits {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&e.name).strong());
                    ui.weak(format!(
                        "{}{}",
                        e.id,
                        e.version.as_deref().map(|v| format!(" ・ {v}")).unwrap_or_default()
                    ));
                    let installed = robsidian_core::plugin::dir(root).join(&e.id).exists();
                    let label = if installed { "入れ直す" } else { "入れる" };
                    if ui
                        .add_enabled(st.installing.is_none(), egui::Button::new(label).small())
                        .clicked()
                    {
                        install = Some(e.clone());
                    }
                });
                if let Some(d) = &e.description {
                    ui.weak(d);
                }
                let mut meta = Vec::new();
                if let Some(a) = &e.author {
                    meta.push(format!("作者: {a}"));
                }
                if let Some(h) = &e.homepage {
                    meta.push(h.clone());
                }
                meta.push(format!("ファイル: {}", e.files.keys().cloned().collect::<Vec<_>>().join(", ")));
                ui.label(RichText::new(meta.join(" ・ ")).small());
            }
            if let Some(entry) = install {
                let (tx, rx) = std::sync::mpsc::channel();
                let (root, url) = (root.to_path_buf(), url.clone());
                let id = entry.id.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(plugin_catalog::install(&root, &entry, &url));
                });
                st.installing = Some((id, rx));
                st.message = None;
            }
            if let Some((id, _)) = &st.installing {
                ui.weak(format!("「{id}」を入れています…"));
            }
        });
}
