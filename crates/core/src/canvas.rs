//! Canvas（`.canvas`）: 公開仕様の JSON Canvas（https://jsoncanvas.org/）を読み書きする。描画はGUI側で行う。
//!
//! ノードは `text`（Markdownの文章）・`file`（vault内のファイル）・`link`（URL）・`group`（枠）の4種類。
//! 辺はノードとノードを結び、どちらの端に矢印を付けるかを持つ。
//! 知らない項目（Obsidian やほかのアプリが足したもの）は `extra` に取っておき、書き出すときに戻す。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Canvas {
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// 座標と大きさは、整数なら整数で書く（Obsidian と同じ形にして、差分を小さくする）
fn number<S: Serializer>(v: &f64, s: S) -> Result<S::Ok, S::Error> {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        s.serialize_i64(*v as i64)
    } else {
        s.serialize_f64(*v)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Node {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// text ノードの本文
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// file ノードのパス（vault のルートから）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// file ノードの見出しやブロック（`#見出し`）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subpath: Option<String>,
    /// link ノードの URL
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// group ノードの名前
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(serialize_with = "number")]
    pub x: f64,
    #[serde(serialize_with = "number")]
    pub y: f64,
    #[serde(serialize_with = "number")]
    pub width: f64,
    #[serde(serialize_with = "number")]
    pub height: f64,
    /// `"1"`〜`"6"`（赤・橙・黄・緑・水色・紫）か `"#rrggbb"`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Edge {
    pub id: String,
    pub from_node: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_side: Option<String>,
    /// `"none"`（既定）か `"arrow"`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_end: Option<String>,
    pub to_node: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_side: Option<String>,
    /// `"arrow"`（既定）か `"none"`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_end: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
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
        return Ok(Canvas::default());
    }
    serde_json::from_str(text).context("Canvas（JSON Canvas）の形式が正しくありません")
}

/// ファイルに書く形（Obsidian と同じくタブで字下げ）
pub fn to_json(canvas: &Canvas) -> String {
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"\t");
    let mut ser = serde_json::Serializer::with_formatter(&mut out, formatter);
    canvas
        .serialize(&mut ser)
        .expect("Canvas は必ずJSONにできる");
    String::from_utf8(out).expect("serde_json はUTF-8を書く")
}

/// 新しいノード・辺の ID（Obsidian と同じ16桁の16進数）
pub fn new_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    // RandomState は作るたびに鍵が変わるので、乱数の代わりになる
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    format!("{:016x}", h.finish())
}

impl Default for Canvas {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            extra: Map::new(),
        }
    }
}

impl Node {
    pub fn new(kind: &str, x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            id: new_id(),
            kind: kind.to_string(),
            text: None,
            file: None,
            subpath: None,
            url: None,
            label: None,
            x,
            y,
            width,
            height,
            color: None,
            extra: Map::new(),
        }
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }

    /// `other` がこのノードの中にすっぽり入っているか（グループの中身の判定）
    pub fn encloses(&self, other: &Node) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.x + other.width <= self.x + self.width
            && other.y + other.height <= self.y + self.height
    }
}

impl Edge {
    pub fn new(from: &str, from_side: &str, to: &str, to_side: &str) -> Self {
        Self {
            id: new_id(),
            from_node: from.to_string(),
            from_side: Some(from_side.to_string()),
            from_end: None,
            to_node: to.to_string(),
            to_side: Some(to_side.to_string()),
            to_end: None,
            color: None,
            label: None,
            extra: Map::new(),
        }
    }
}

impl Canvas {
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_mut(&mut self, id: &str) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    pub fn edge_mut(&mut self, id: &str) -> Option<&mut Edge> {
        self.edges.iter_mut().find(|e| e.id == id)
    }

    /// ノードと、それにつながる辺を消す
    pub fn remove_node(&mut self, id: &str) {
        self.nodes.retain(|n| n.id != id);
        self.edges.retain(|e| e.from_node != id && e.to_node != id);
    }

    pub fn remove_edge(&mut self, id: &str) {
        self.edges.retain(|e| e.id != id);
    }

    /// ノードを動かすときに一緒に動くノード（自分と、グループならその中にあるもの）
    pub fn moving_with(&self, id: &str) -> Vec<String> {
        let Some(node) = self.node(id) else {
            return Vec::new();
        };
        let mut ids = vec![id.to_string()];
        if node.kind == "group" {
            ids.extend(
                self.nodes
                    .iter()
                    .filter(|n| n.id != id && node.encloses(n))
                    .map(|n| n.id.clone()),
            );
        }
        ids
    }

    /// 前面のノードへ（配列の後ろほど上に描かれる）。グループは他のノードより後ろのままにする
    pub fn bring_to_front(&mut self, id: &str) {
        if let Some(i) = self
            .nodes
            .iter()
            .position(|n| n.id == id && n.kind != "group")
        {
            let n = self.nodes.remove(i);
            self.nodes.push(n);
        }
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

    #[test]
    fn writes_back_with_unknown_fields() {
        let src = r##"{"nodes":[{"id":"a","type":"text","text":"x","x":10,"y":-20.5,"width":200,"height":80,"styleAttributes":{"shape":"pill"}},{"id":"b","type":"file","file":"b.md","x":300,"y":0,"width":100,"height":100}],"edges":[{"id":"e","fromNode":"a","fromSide":"right","toNode":"b","toSide":"left","custom":1}],"metadata":{"version":"1.0"}}"##;
        let mut c = parse(src).unwrap();
        let out = to_json(&c);
        // 整数は整数のまま、知らない項目も残る
        assert!(out.contains("\"x\": 10,"), "{out}");
        assert!(out.contains("\"y\": -20.5,"), "{out}");
        assert!(out.contains("\"styleAttributes\""));
        assert!(out.contains("\"custom\": 1"));
        assert!(out.contains("\"metadata\""));
        assert!(out.contains("\n\t\"nodes\""));
        assert!(!out.contains("\"color\""));
        assert_eq!(parse(&out).unwrap(), c);

        // ノードを消すと、つながる辺も消える
        c.remove_node("a");
        assert_eq!(c.nodes.len(), 1);
        assert!(c.edges.is_empty());
    }

    #[test]
    fn groups_move_their_members() {
        let mut c = Canvas::default();
        let mut g = Node::new("group", 0.0, 0.0, 500.0, 300.0);
        g.id = "g".into();
        let mut inside = Node::new("text", 10.0, 10.0, 100.0, 50.0);
        inside.id = "in".into();
        let mut outside = Node::new("text", 450.0, 10.0, 100.0, 50.0);
        outside.id = "out".into();
        c.nodes = vec![g, inside, outside];
        assert_eq!(c.moving_with("g"), ["g", "in"]);
        assert_eq!(c.moving_with("in"), ["in"]);
        c.bring_to_front("in");
        assert_eq!(c.nodes.last().unwrap().id, "in");
        assert_ne!(new_id(), new_id());
        assert_eq!(new_id().len(), 16);
    }
}
