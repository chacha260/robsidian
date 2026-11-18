//! Canvas（`.canvas`）: 公開仕様の JSON Canvas（https://jsoncanvas.org/）を読む。表示はGUI側で行う。
//!
//! ノードは `text`（Markdownの文章）・`file`（vault内のファイル）・`link`（URL）・`group`（枠）の4種類。
//! 辺はノードとノードを結び、どちらの端に矢印を付けるかを持つ。

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Canvas {
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Node {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// `"1"`〜`"6"`（赤・橙・黄・緑・水色・紫）か `"#rrggbb"`
    #[serde(default)]
    pub color: Option<String>,
    /// text ノードの本文
    #[serde(default)]
    pub text: Option<String>,
    /// file ノードのパス（vault のルートから）
    #[serde(default)]
    pub file: Option<String>,
    /// file ノードの見出しやブロック（`#見出し`）
    #[serde(default)]
    pub subpath: Option<String>,
    /// link ノードの URL
    #[serde(default)]
    pub url: Option<String>,
    /// group ノードの名前
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Edge {
    pub id: String,
    pub from_node: String,
    #[serde(default)]
    pub from_side: Option<String>,
    /// `"none"`（既定）か `"arrow"`
    #[serde(default)]
    pub from_end: Option<String>,
    pub to_node: String,
    #[serde(default)]
    pub to_side: Option<String>,
    /// `"arrow"`（既定）か `"none"`
    #[serde(default)]
    pub to_end: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
}

impl Edge {
    pub fn arrow_at_from(&self) -> bool {
        self.from_end.as_deref() == Some("arrow")
    }

    pub fn arrow_at_to(&self) -> bool {
        self.to_end.as_deref() != Some("none")
    }
}

/// 色の指定を RGB にする（プリセットは Obsidian の既定の色に近いもの）
pub fn color(spec: &str) -> Option<[u8; 3]> {
    Some(match spec {
        "1" => [0xe9, 0x3e, 0x3e],
        "2" => [0xf0, 0x8c, 0x2e],
        "3" => [0xe0, 0xc0, 0x2a],
        "4" => [0x3e, 0xb3, 0x5e],
        "5" => [0x2e, 0xb6, 0xc8],
        "6" => [0xa8, 0x5c, 0xe8],
        hex => {
            let h = hex.strip_prefix('#')?;
            if h.len() != 6 || !h.is_ascii() {
                return None;
            }
            let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
            [byte(0)?, byte(2)?, byte(4)?]
        }
    })
}

pub fn parse(text: &str) -> Result<Canvas> {
    if text.trim().is_empty() {
        return Ok(Canvas {
            nodes: Vec::new(),
            edges: Vec::new(),
        });
    }
    serde_json::from_str(text).context("Canvas（JSON Canvas）の形式が正しくありません")
}

impl Canvas {
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// すべてのノードを囲む範囲 (左, 上, 右, 下)。ノードが無ければ None
    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        let mut it = self.nodes.iter();
        let first = it.next()?;
        let mut b = (
            first.x,
            first.y,
            first.x + first.width,
            first.y + first.height,
        );
        for n in it {
            b.0 = b.0.min(n.x);
            b.1 = b.1.min(n.y);
            b.2 = b.2.max(n.x + n.width);
            b.3 = b.3.max(n.y + n.height);
        }
        Some(b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_json_canvas() {
        let c = parse(
            r##"{
  "nodes": [
    {"id":"g","type":"group","x":-20,"y":-20,"width":500,"height":300,"label":"まとめ","color":"4"},
    {"id":"a","type":"text","x":0,"y":0,"width":200,"height":80,"text":"# 見出し\n本文"},
    {"id":"b","type":"file","x":260,"y":0,"width":200,"height":120,"file":"notes/b.md","subpath":"#節"},
    {"id":"c","type":"link","x":0,"y":400,"width":200,"height":60,"url":"https://example.com","color":"#ff8800"}
  ],
  "edges": [
    {"id":"e1","fromNode":"a","fromSide":"right","toNode":"b","toSide":"left","label":"参照"},
    {"id":"e2","fromNode":"b","toNode":"c","fromEnd":"arrow","toEnd":"none"}
  ]
}"##,
        )
        .unwrap();
        assert_eq!(c.nodes.len(), 4);
        assert_eq!(c.node("b").unwrap().file.as_deref(), Some("notes/b.md"));
        assert_eq!(c.edges[0].label.as_deref(), Some("参照"));
        assert!(c.edges[0].arrow_at_to() && !c.edges[0].arrow_at_from());
        assert!(!c.edges[1].arrow_at_to() && c.edges[1].arrow_at_from());
        assert_eq!(c.bounds(), Some((-20.0, -20.0, 480.0, 460.0)));
        assert_eq!(color("4"), Some([0x3e, 0xb3, 0x5e]));
        assert_eq!(color("#ff8800"), Some([0xff, 0x88, 0x00]));
        assert_eq!(color("#ff88"), None);
        assert!(parse("").unwrap().nodes.is_empty());
        assert!(parse("{nodes:").is_err());
    }
}
