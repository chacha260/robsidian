//! 日本語フォントの読み込み。

use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 日本語を表示できるシステムフォントの候補（先に見つかったものを使う）。
const CJK_FONTS: &[&str] = &[
    // Windows
    "C:/Windows/Fonts/YuGothM.ttc",
    "C:/Windows/Fonts/meiryo.ttc",
    "C:/Windows/Fonts/msgothic.ttc",
    // macOS
    "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/Library/Fonts/Arial Unicode.ttf",
    // Linux
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf",
    "/usr/share/fonts/truetype/fonts-japanese-gothic.ttf",
    "/usr/share/fonts/truetype/vlgothic/VL-Gothic-Regular.ttf",
    "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
];

/// 太字の日本語フォントの候補。
const CJK_BOLD_FONTS: &[&str] = &[
    "C:/Windows/Fonts/YuGothB.ttc",
    "C:/Windows/Fonts/meiryob.ttc",
    "/System/Library/Fonts/ヒラギノ角ゴシック W6.ttc",
    "/System/Library/Fonts/ヒラギノ角ゴシック W7.ttc",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Bold.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Bold.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Bold.ttc",
];

/// 太字の欧文フォントの候補（日本語の太字が無い環境でも、英字だけは太字にする）。
const LATIN_BOLD_FONTS: &[&str] = &[
    "C:/Windows/Fonts/segoeuib.ttf",
    "C:/Windows/Fonts/arialbd.ttf",
    "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
    "/usr/share/fonts/opentype/inter/Inter-Bold.otf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
];

/// 数式の変数に使う斜体（セリフ体）の候補。
const MATH_ITALIC_FONTS: &[&str] = &[
    "C:/Windows/Fonts/cambriai.ttf",
    "C:/Windows/Fonts/timesi.ttf",
    "/System/Library/Fonts/Supplemental/Times New Roman Italic.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSerif-Italic.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSerif-Italic.ttf",
    "/usr/share/fonts/truetype/noto/NotoSerif-Italic.ttf",
];

/// 数式の変数に使うフォントファミリー。斜体のフォントが無ければ通常のフォントで描かれる。
pub fn math_italic() -> egui::FontFamily {
    egui::FontFamily::Name("math-italic".into())
}

/// エディタの太字・見出しに使うフォントファミリー。太字のフォントが無ければ通常のフォントで描かれる。
pub fn bold() -> egui::FontFamily {
    egui::FontFamily::Name("bold".into())
}

fn first_readable(paths: &[&str]) -> Option<Vec<u8>> {
    paths.iter().find_map(|p| std::fs::read(p).ok())
}

/// egui同梱フォントにはCJKグリフが無いので、フォールバックとして日本語フォントを足す。
/// 優先順は 設定で指定したファイル → 環境変数 `ROBSIDIAN_FONT` → システムフォントの候補。
/// あわせて太字のファミリー（[`bold`]）を用意する。使った日本語フォントのパスを返す。
pub fn install(ctx: &egui::Context, configured: Option<&Path>) -> Option<PathBuf> {
    let env = std::env::var_os("ROBSIDIAN_FONT").map(PathBuf::from);
    let candidates = configured
        .map(Path::to_path_buf)
        .into_iter()
        .chain(env)
        .chain(CJK_FONTS.iter().map(PathBuf::from));
    let found = candidates
        .filter_map(|p| std::fs::read(&p).ok().map(|b| (p, b)))
        .next();
    let mut fonts = egui::FontDefinitions::default();
    let regular = fonts
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    if let Some((_, bytes)) = &found {
        fonts.font_data.insert(
            "cjk".to_owned(),
            Arc::new(egui::FontData::from_owned(bytes.clone())),
        );
        for family in fonts.families.values_mut() {
            family.push("cjk".to_owned());
        }
    }
    // 太字: 欧文の太字 → 日本語の太字 → 通常のフォント の順に字形を探す
    let mut bold_fonts: Vec<String> = Vec::new();
    for (name, paths) in [
        ("latin-bold", LATIN_BOLD_FONTS),
        ("cjk-bold", CJK_BOLD_FONTS),
    ] {
        if let Some(bytes) = first_readable(paths) {
            fonts
                .font_data
                .insert(name.to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
            bold_fonts.push(name.to_owned());
        }
    }
    // 数式の斜体: 斜体のセリフ体 → 通常のフォント
    let mut italic_fonts: Vec<String> = Vec::new();
    if let Some(bytes) = first_readable(MATH_ITALIC_FONTS) {
        fonts.font_data.insert(
            "math-italic".to_owned(),
            Arc::new(egui::FontData::from_owned(bytes)),
        );
        italic_fonts.push("math-italic".to_owned());
    }
    italic_fonts.extend(regular.iter().cloned());
    bold_fonts.extend(regular);
    if found.is_some() {
        bold_fonts.push("cjk".to_owned());
        italic_fonts.push("cjk".to_owned());
    }
    fonts.families.insert(bold(), bold_fonts);
    fonts.families.insert(math_italic(), italic_fonts);
    ctx.set_fonts(fonts);
    found.map(|(p, _)| p)
}
