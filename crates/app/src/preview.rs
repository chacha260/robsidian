//! プレビュー用にMarkdownを変換する。
//!
//! `[[wikilink]]` をクリック可能な通常のMarkdownリンク（`rob:` スキーム）へ、
//! 埋め込み画像を絶対パスの `file://` URIへ書き換える。

use robsidian_core::{parse, LinkKind, Vault};

pub const SCHEME: &str = "rob:";

const IMAGE_EXTS: [&str; 7] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"];

pub fn is_image(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, ext)| IMAGE_EXTS.contains(&ext.to_lowercase().as_str()))
}

/// URLとして安全な文字以外をすべてパーセントエンコードする。
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `rob:` リンクから元のリンク先文字列を取り出す。
pub fn decode_link(url: &str) -> Option<String> {
    url.strip_prefix(SCHEME).map(parse::percent_decode)
}

pub fn file_uri(vault: &Vault, rel: &str) -> String {
    let abs = vault.abs(rel).display().to_string().replace('\\', "/");
    if abs.starts_with('/') {
        format!("file://{abs}")
    } else {
        format!("file:///{abs}")
    }
}

pub fn to_preview(text: &str, from: &str, vault: &Vault) -> String {
    let parsed = parse::parse(text);
    let mut out = text.to_string();
    for link in parsed.links.iter().rev() {
        let image = link
            .embed
            .then(|| vault.resolve_file(&link.target, from))
            .flatten()
            .filter(|p| is_image(p));
        match link.kind {
            LinkKind::Wiki => {
                let replacement = if let Some(img) = image {
                    format!("![{}](<{}>)", link.target, file_uri(vault, img))
                } else {
                    let label = match (&link.alias, &link.heading) {
                        (Some(a), _) if !a.is_empty() => a.clone(),
                        (_, Some(h)) => format!("{} › {h}", link.target),
                        _ => link.target.clone(),
                    };
                    format!("[{label}]({SCHEME}{})", encode(&link.target))
                };
                out.replace_range(link.full.clone(), &replacement);
            }
            LinkKind::Markdown => {
                if let Some(img) = image {
                    out.replace_range(link.span.clone(), &format!("<{}>", file_uri(vault, img)));
                } else if !link.embed && vault.resolve(&link.target, from).is_some() {
                    out.replace_range(
                        link.span.clone(),
                        &format!("{SCHEME}{}", encode(&link.target)),
                    );
                }
            }
        }
    }
    out.split_off(parsed.body_start)
}
