//! Bases（Obsidian の `.base`）: ノートのプロパティを表やカードにして、絞り込み・並べ替えをする。
//!
//! Obsidian の書き方のうち、よく使う部分を読む（Obsidian の仕様との突き合わせはまだで、互換は一部）。
//!
//! ```yaml
//! filters:
//!   and:
//!     - file.hasTag("book")
//!     - 'status != "done"'
//! formulas:
//!   税込: 'if(price, (price * 1.1).round() + "円")'
//! properties:
//!   status:
//!     displayName: 状態
//!   formula.税込:
//!     displayName: 税込価格
//! views:
//!   - type: table
//!     name: 読書中
//!     order: [file.name, author, status, formula.税込]
//!     sort:
//!       - property: file.mtime
//!         direction: DESC
//!     limit: 20
//!   - type: cards
//!     name: 表紙
//!     image: note.cover
//! ```
//!
//! 条件式と計算列の式は [`crate::formula`] で計算する。未対応の関数やメソッドはエラーとして見せる。

use crate::formula::{self, Ctx, Expr, Val};
use crate::props;
use crate::{Note, Vault};
use anyhow::{anyhow, bail, Context, Result};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use yaml_rust2::{Yaml, YamlLoader};

#[derive(Debug, Clone, PartialEq)]
pub enum Filter {
    And(Vec<Filter>),
    Or(Vec<Filter>),
    /// 中のどれにも当てはまらない
    Not(Vec<Filter>),
    Expr(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct View {
    /// `table` か `cards`（それ以外は表として出す）
    pub kind: String,
    pub name: String,
    pub filters: Option<Filter>,
    pub order: Vec<String>,
    /// (プロパティ, 降順か)
    pub sort: Vec<(String, bool)>,
    pub limit: Option<usize>,
    /// カードの画像にするプロパティ（`note.cover` など）
    pub image: Option<String>,
    /// カードの幅（px）
    pub card_size: Option<f32>,
    /// 画像の縦横比（高さ / 幅）
    pub image_aspect: Option<f32>,
    /// 画像を枠いっぱいに切り抜くか（`cover`）、全体を収めるか（`contain`）
    pub image_fit: Option<String>,
}

impl View {
    fn table() -> Self {
        View {
            kind: "table".into(),
            name: "表".into(),
            filters: None,
            order: Vec::new(),
            sort: Vec::new(),
            limit: None,
            image: None,
            card_size: None,
            image_aspect: None,
            image_fit: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Base {
    pub filters: Option<Filter>,
    /// 計算列（名前 → 式）
    pub formulas: BTreeMap<String, String>,
    pub views: Vec<View>,
    /// プロパティ → 表示名
    pub display: BTreeMap<String, String>,
}

/// 表にした結果
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Table {
    pub view: String,
    /// ビューの種類（`table` / `cards`）
    pub kind: String,
    /// (プロパティ, 表示名)
    pub columns: Vec<(String, String)>,
    pub rows: Vec<Row>,
    /// 当てはまったノートの数（`limit` で切る前）
    pub total: usize,
    pub card_size: Option<f32>,
    pub image_aspect: Option<f32>,
    pub image_fit: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub path: String,
    pub cells: Vec<String>,
    /// カードの画像（vault 内のパスか URL）
    pub image: Option<String>,
}

fn filter_of(y: &Yaml) -> Result<Option<Filter>> {
    Ok(match y {
        Yaml::BadValue | Yaml::Null => None,
        Yaml::String(s) => Some(Filter::Expr(s.clone())),
        Yaml::Boolean(b) => Some(Filter::Expr(b.to_string())),
        Yaml::Hash(h) => {
            let (k, v) = h.iter().next().ok_or_else(|| anyhow!("filters が空です"))?;
            let list = match v {
                Yaml::Array(items) => items
                    .iter()
                    .filter_map(|i| filter_of(i).transpose())
                    .collect::<Result<Vec<_>>>()?,
                other => filter_of(other)?.into_iter().collect(),
            };
            Some(match k.as_str() {
                Some("and") => Filter::And(list),
                Some("or") => Filter::Or(list),
                Some("not") => Filter::Not(list),
                _ => bail!("filters には and / or / not を書いてください"),
            })
        }
        Yaml::Array(items) => Some(Filter::And(
            items
                .iter()
                .filter_map(|i| filter_of(i).transpose())
                .collect::<Result<Vec<_>>>()?,
        )),
        other => bail!("filters の書き方が正しくありません: {other:?}"),
    })
}

fn text_of(y: &Yaml) -> Option<String> {
    match y {
        Yaml::String(s) => Some(s.clone()),
        Yaml::Integer(i) => Some(i.to_string()),
        Yaml::Real(r) => Some(r.clone()),
        Yaml::Boolean(b) => Some(b.to_string()),
        _ => None,
    }
}

fn number_of(y: &Yaml) -> Option<f32> {
    match y {
        Yaml::Integer(i) => Some(*i as f32),
        Yaml::Real(r) => r.parse().ok(),
        Yaml::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// `.base` の本文（YAML）を読む。
pub fn parse(text: &str) -> Result<Base> {
    let docs = YamlLoader::load_from_str(text).map_err(|e| anyhow!("YAML の誤り: {e}"))?;
    let Some(doc) = docs.into_iter().next() else {
        return Ok(Base::default());
    };
    let mut base = Base {
        filters: filter_of(&doc["filters"])?,
        ..Base::default()
    };
    if let Yaml::Hash(formulas) = &doc["formulas"] {
        for (k, v) in formulas {
            if let (Some(k), Some(e)) = (text_of(k), text_of(v)) {
                base.formulas.insert(k, e);
            }
        }
    }
    if let Yaml::Hash(props) = &doc["properties"] {
        for (k, v) in props {
            if let (Some(k), Some(name)) = (k.as_str(), v["displayName"].as_str()) {
                base.display.insert(k.to_string(), name.to_string());
            }
        }
    }
    if let Yaml::Array(views) = &doc["views"] {
        for (i, v) in views.iter().enumerate() {
            let order = match &v["order"] {
                Yaml::Array(cols) => cols.iter().filter_map(text_of).collect(),
                _ => Vec::new(),
            };
            let sort = match &v["sort"] {
                Yaml::Array(items) => items
                    .iter()
                    .filter_map(|s| {
                        let prop = text_of(&s["property"])?;
                        let desc = s["direction"]
                            .as_str()
                            .is_some_and(|d| d.eq_ignore_ascii_case("desc"));
                        Some((prop, desc))
                    })
                    .collect(),
                _ => Vec::new(),
            };
            base.views.push(View {
                kind: v["type"].as_str().unwrap_or("table").to_string(),
                name: text_of(&v["name"]).unwrap_or_else(|| format!("ビュー{}", i + 1)),
                filters: filter_of(&v["filters"])?,
                order,
                sort,
                limit: v["limit"].as_i64().and_then(|n| usize::try_from(n).ok()),
                image: text_of(&v["image"]),
                card_size: number_of(&v["cardSize"]),
                image_aspect: number_of(&v["imageAspectRatio"]),
                image_fit: text_of(&v["imageFit"]),
            });
        }
    }
    if base.views.is_empty() {
        base.views.push(View::table());
    }
    Ok(base)
}

/// 式を読んでおいた絞り込み
enum Compiled {
    And(Vec<Compiled>),
    Or(Vec<Compiled>),
    Not(Vec<Compiled>),
    Expr(Expr),
}

fn compile(f: &Filter) -> Result<Compiled> {
    let all = |list: &[Filter]| list.iter().map(compile).collect::<Result<Vec<_>>>();
    Ok(match f {
        Filter::And(l) => Compiled::And(all(l)?),
        Filter::Or(l) => Compiled::Or(all(l)?),
        Filter::Not(l) => Compiled::Not(all(l)?),
        Filter::Expr(e) => {
            Compiled::Expr(formula::parse(e).with_context(|| format!("条件を読めません: {e}"))?)
        }
    })
}

fn matches(ctx: &Ctx, f: &Compiled) -> Result<bool> {
    Ok(match f {
        Compiled::And(list) => {
            for f in list {
                if !matches(ctx, f)? {
                    return Ok(false);
                }
            }
            true
        }
        Compiled::Or(list) => {
            for f in list {
                if matches(ctx, f)? {
                    return Ok(true);
                }
            }
            list.is_empty()
        }
        Compiled::Not(list) => {
            for f in list {
                if matches(ctx, f)? {
                    return Ok(false);
                }
            }
            true
        }
        Compiled::Expr(e) => ctx.eval(e)?.truthy(),
    })
}

/// 画像のプロパティの値（`[[表紙.png]]`・`表紙.png`・URL）を、vault 内のパスか URL にする
fn image_of(vault: &Vault, note: &Note, v: &Val) -> Option<String> {
    let first = match v {
        Val::List(l) => l.first()?.display(),
        other => other.display(),
    };
    let t = first.trim();
    if t.starts_with("http://") || t.starts_with("https://") {
        return Some(t.to_string());
    }
    let t = t
        .trim_start_matches('!')
        .trim_start_matches("[[")
        .trim_end_matches("]]");
    let t = t.split('|').next()?.trim();
    if t.is_empty() {
        return None;
    }
    vault.resolve_file(t, &note.path).map(str::to_string)
}

/// ビューの名前（無ければ最初のビュー）で表を作る。
pub fn run(vault: &Vault, base: &Base, view: Option<&str>) -> Result<Table> {
    let v = match view {
        Some(name) => base
            .views
            .iter()
            .find(|v| v.name == name)
            .ok_or_else(|| anyhow!("ビューがありません: {name}"))?,
        None => &base.views[0],
    };
    let formulas: BTreeMap<String, Expr> = base
        .formulas
        .iter()
        .map(|(k, e)| {
            formula::parse(e)
                .with_context(|| format!("計算列 {k} の式を読めません: {e}"))
                .map(|x| (k.clone(), x))
        })
        .collect::<Result<_>>()?;
    let filters: Vec<Compiled> = [&base.filters, &v.filters]
        .into_iter()
        .flatten()
        .map(compile)
        .collect::<Result<_>>()?;
    let mut hits: Vec<(&Note, Vec<props::Prop>)> = Vec::new();
    for note in vault.notes() {
        let fm = props::read(&note.text).unwrap_or_default();
        let ctx = Ctx {
            vault,
            note,
            fm: &fm,
            formulas: &formulas,
            depth: 0,
        };
        let mut ok = true;
        for f in &filters {
            if !matches(&ctx, f)? {
                ok = false;
                break;
            }
        }
        if ok {
            hits.push((note, fm));
        }
    }
    // 並べ替えの値は先に計算しておく（比べるたびに式を計算しないように）
    let sort = if v.sort.is_empty() {
        vec![("file.name".to_string(), false)]
    } else {
        v.sort.clone()
    };
    let mut keyed: Vec<(Vec<Val>, &Note, Vec<props::Prop>)> = hits
        .into_iter()
        .map(|(note, fm)| {
            let ctx = Ctx {
                vault,
                note,
                fm: &fm,
                formulas: &formulas,
                depth: 0,
            };
            let keys = sort
                .iter()
                .map(|(prop, _)| ctx.column(prop))
                .collect::<Result<Vec<_>>>()?;
            Ok((keys, note, fm))
        })
        .collect::<Result<_>>()?;
    keyed.sort_by(|(ka, a, _), (kb, b, _)| {
        for ((x, y), (_, desc)) in ka.iter().zip(kb).zip(&sort) {
            // 値の無いものは常に後ろ
            let ord = match (x, y) {
                (Val::Null, Val::Null) => Ordering::Equal,
                (Val::Null, _) => return Ordering::Greater,
                (_, Val::Null) => return Ordering::Less,
                _ => x.compare(y).unwrap_or(Ordering::Equal),
            };
            let ord = if *desc { ord.reverse() } else { ord };
            if ord != Ordering::Equal {
                return ord;
            }
        }
        a.path.cmp(&b.path)
    });
    let total = keyed.len();
    if let Some(n) = v.limit {
        keyed.truncate(n);
    }
    // 列（指定が無ければ、名前と、当てはまったノートの frontmatter のキーと、計算列）
    let keys: Vec<String> = if v.order.is_empty() {
        let mut seen = BTreeSet::new();
        let mut keys = vec!["file.name".to_string()];
        for (_, _, fm) in &keyed {
            for p in fm {
                if seen.insert(p.key.clone()) {
                    keys.push(p.key.clone());
                }
            }
        }
        keys.truncate(8);
        keys.extend(base.formulas.keys().map(|k| format!("formula.{k}")));
        keys
    } else {
        v.order.clone()
    };
    let columns = keys
        .iter()
        .map(|k| {
            let shown = base
                .display
                .get(k)
                .or_else(|| base.display.get(k.strip_prefix("note.").unwrap_or(k)))
                .cloned()
                .unwrap_or_else(|| match k.as_str() {
                    "file.name" => "名前".to_string(),
                    "file.path" => "パス".to_string(),
                    "file.folder" => "フォルダ".to_string(),
                    "file.mtime" => "更新".to_string(),
                    "file.ctime" => "作成".to_string(),
                    "file.tags" => "タグ".to_string(),
                    other => other
                        .strip_prefix("note.")
                        .or_else(|| other.strip_prefix("formula."))
                        .unwrap_or(other)
                        .to_string(),
                });
            (k.clone(), shown)
        })
        .collect();
    let mut rows = Vec::with_capacity(keyed.len());
    for (_, note, fm) in &keyed {
        let ctx = Ctx {
            vault,
            note,
            fm,
            formulas: &formulas,
            depth: 0,
        };
        let cells = keys
            .iter()
            .map(|k| ctx.column(k).map(|v| v.display()))
            .collect::<Result<Vec<_>>>()?;
        let image = match &v.image {
            Some(prop) => image_of(vault, note, &ctx.column(prop)?),
            None => None,
        };
        rows.push(Row {
            path: note.path.clone(),
            cells,
            image,
        });
    }
    Ok(Table {
        view: v.name.clone(),
        kind: v.kind.clone(),
        columns,
        rows,
        total,
        card_size: v.card_size,
        image_aspect: v.image_aspect,
        image_fit: v.image_fit.clone(),
    })
}

/// 表を Markdown の表にする（プレビューや埋め込みで使う）。名前の列はノートへのリンクにする。
/// カードのビューは、画像を先頭の列にした表にする。
pub fn to_markdown(table: &Table) -> String {
    let esc = |s: &str| s.replace('|', "\\|").replace('\n', " ");
    let cards = table.kind == "cards" && table.rows.iter().any(|r| r.image.is_some());
    let mut out = String::new();
    let mut header: Vec<String> = table.columns.iter().map(|(_, n)| esc(n)).collect();
    if cards {
        header.insert(0, "画像".to_string());
    }
    out.push_str(&format!("| {} |\n", header.join(" | ")));
    out.push_str(&format!("|{}\n", " --- |".repeat(header.len())));
    for row in &table.rows {
        let mut cells: Vec<String> = row
            .cells
            .iter()
            .zip(&table.columns)
            .map(|(c, (k, _))| {
                if k == "file.name" {
                    // 表の中では `|` を区切りと読まれないよう、パスと名前の間は `\|` にする
                    format!("[[{}\\|{}]]", row.path.trim_end_matches(".md"), esc(c))
                } else {
                    esc(c)
                }
            })
            .collect();
        if cards {
            cells.insert(
                0,
                match &row.image {
                    Some(url) if url.contains("://") => format!("![]({url})"),
                    Some(path) => format!("![[{path}\\|120]]"),
                    None => String::new(),
                },
            );
        }
        out.push_str(&format!("| {} |\n", cells.join(" | ")));
    }
    if table.rows.is_empty() {
        out.push_str("\n*当てはまるノートはありません*\n");
    } else if table.total > table.rows.len() {
        out.push_str(&format!(
            "\n*{}件中{}件を表示*\n",
            table.total,
            table.rows.len()
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let w = |p: &str, t: &str| {
            let abs = dir.path().join(p);
            fs::create_dir_all(abs.parent().unwrap()).unwrap();
            fs::write(abs, t).unwrap();
        };
        w(
            "books/a.md",
            "---\ntags: [book]\nauthor: 太宰\nstatus: done\npages: 120\n---\n[[c]]",
        );
        w(
            "books/b.md",
            "---\ntags: [book/novel]\nauthor: 漱石\nstatus: reading\npages: 300\n---\n",
        );
        w(
            "books/c.md",
            "---\ntags: [book]\nauthor: 鴎外\npages: 80\n---\n",
        );
        w("notes/d.md", "---\nstatus: reading\n---\n#memo");
        let v = Vault::open(dir.path()).unwrap();
        (dir, v)
    }

    #[test]
    fn filters_sorts_and_limits() {
        let (_d, v) = vault();
        let base = parse(
            r#"
filters:
  and:
    - file.hasTag("book")
    - file.inFolder("books")
properties:
  author:
    displayName: 著者
views:
  - type: table
    name: 全部
    order: [file.name, author, pages]
    sort:
      - property: pages
        direction: DESC
  - type: table
    name: 未読
    filters:
      or:
        - 'status != "done"'
        - not:
            - file.hasProperty("status")
    limit: 1
  - type: table
    name: 厚い本
    filters: 'pages >= 100'
"#,
        )
        .unwrap();
        let t = run(&v, &base, None).unwrap();
        assert_eq!(t.view, "全部");
        assert_eq!(
            t.columns,
            [
                ("file.name".to_string(), "名前".to_string()),
                ("author".to_string(), "著者".to_string()),
                ("pages".to_string(), "pages".to_string())
            ]
        );
        let names: Vec<&str> = t.rows.iter().map(|r| r.cells[0].as_str()).collect();
        // 数値として降順（文字列なら 80 が先頭になる）
        assert_eq!(names, ["b", "a", "c"]);

        let t = run(&v, &base, Some("未読")).unwrap();
        assert_eq!(t.total, 2);
        assert_eq!(t.rows.len(), 1);
        assert_eq!(t.rows[0].path, "books/b.md");

        let t = run(&v, &base, Some("厚い本")).unwrap();
        let paths: Vec<&str> = t.rows.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(paths, ["books/a.md", "books/b.md"]);
        assert!(run(&v, &base, Some("無い")).is_err());
    }

    #[test]
    fn expressions() {
        let (_d, v) = vault();
        let check = |expr: &str| -> Vec<String> {
            let base = parse(&format!("filters: '{expr}'")).unwrap();
            run(&v, &base, None)
                .unwrap()
                .rows
                .into_iter()
                .map(|r| r.path)
                .collect()
        };
        assert_eq!(check("author.contains(\"漱\")"), ["books/b.md"]);
        assert_eq!(check("file.hasLink(\"c\")"), ["books/a.md"]);
        assert_eq!(check("!file.hasTag(\"book\")"), ["notes/d.md"]);
        assert_eq!(check("file.hasTag(\"memo\")"), ["notes/d.md"]);
        assert_eq!(check("file.name == \"c\""), ["books/c.md"]);
        assert_eq!(check("status"), ["books/a.md", "books/b.md", "notes/d.md"]);
        let bad = parse("filters: 'pages + 1 ~ 2'").unwrap();
        assert!(run(&v, &bad, None).is_err());
    }

    #[test]
    fn formulas_and_cards() {
        let (d, v) = vault();
        fs::write(d.path().join("books/a.png"), "png").unwrap();
        fs::write(
            d.path().join("books/a.md"),
            "---\ntags: [book]\nauthor: 太宰\nstatus: done\npages: 120\ncover: \"[[a.png]]\"\n---\n",
        )
        .unwrap();
        let v2 = Vault::open(d.path()).unwrap();
        drop(v);
        let base = parse(
            r#"
filters: file.hasTag("book")
formulas:
  half: pages / 2
  label: 'if(pages > 100, "厚い", "薄い") + "(" + formula.half + ")"'
properties:
  formula.label:
    displayName: 厚さ
views:
  - type: table
    name: 表
    order: [file.name, formula.label]
    sort:
      - property: formula.half
        direction: DESC
  - type: cards
    name: カード
    image: note.cover
    cardSize: 180
    imageAspectRatio: 1.5
"#,
        )
        .unwrap();
        let t = run(&v2, &base, None).unwrap();
        assert_eq!(t.columns[1].1, "厚さ");
        let cells: Vec<&str> = t.rows.iter().map(|r| r.cells[1].as_str()).collect();
        assert_eq!(cells, ["厚い(150)", "厚い(60)", "薄い(40)"]);

        let t = run(&v2, &base, Some("カード")).unwrap();
        assert_eq!(t.kind, "cards");
        assert_eq!(t.card_size, Some(180.0));
        assert_eq!(t.image_aspect, Some(1.5));
        assert_eq!(t.rows[0].image.as_deref(), Some("books/a.png"));
        assert_eq!(t.rows[1].image, None);
        // 列の指定が無ければ、計算列も並ぶ
        assert!(t.columns.iter().any(|(k, _)| k == "formula.half"));
        let md = to_markdown(&t);
        assert!(md.starts_with("| 画像 |"), "{md}");
        assert!(md.contains("![[books/a.png\\|120]]"), "{md}");

        let bad = parse("formulas:\n  x: 'nope('").unwrap();
        assert!(run(&v2, &bad, None).is_err());
    }

    #[test]
    fn markdown_table() {
        let (_d, v) = vault();
        let base = parse("filters: 'file.inFolder(\"notes\")'\nviews:\n  - name: x\n    order: [file.name, status]").unwrap();
        let md = to_markdown(&run(&v, &base, None).unwrap());
        assert_eq!(
            md,
            "| 名前 | status |\n| --- | --- |\n| [[notes/d\\|d]] | reading |\n"
        );
    }
}
