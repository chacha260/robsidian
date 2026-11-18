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
    /// グループ分け（プロパティ, 降順か）
    pub group_by: Option<(String, bool)>,
    /// 集計（列 → 集計の種類 `Sum` `Average` など）
    pub summaries: Vec<(String, String)>,
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
            group_by: None,
            summaries: Vec::new(),
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
    /// グループ（`groupBy` が無ければ、名前の無い1つ）。行は `rows` の中の範囲
    pub groups: Vec<Group>,
    /// 集計の行があるか
    pub has_summary: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// グループの名前（`groupBy` の値。値が無ければ「（なし）」）。グループ分けしないときは空
    pub label: String,
    pub rows: std::ops::Range<usize>,
    /// 列ごとの集計（集計しない列は空）
    pub summary: Vec<String>,
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
                group_by: match &v["groupBy"] {
                    Yaml::Hash(_) => text_of(&v["groupBy"]["property"]).map(|p| {
                        let desc = v["groupBy"]["direction"]
                            .as_str()
                            .is_some_and(|d| d.eq_ignore_ascii_case("desc"));
                        (p, desc)
                    }),
                    other => text_of(other).map(|p| (p, false)),
                },
                summaries: match &v["summaries"] {
                    Yaml::Hash(h) => h
                        .iter()
                        .filter_map(|(k, f)| Some((text_of(k)?, text_of(f)?)))
                        .collect(),
                    _ => Vec::new(),
                },
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
    // 行ごとに、列の値・グループの値・画像を計算する
    let mut built: Vec<(Val, Vec<Val>, Row)> = Vec::with_capacity(keyed.len());
    for (_, note, fm) in &keyed {
        let ctx = Ctx {
            vault,
            note,
            fm,
            formulas: &formulas,
            depth: 0,
        };
        let vals = keys
            .iter()
            .map(|k| ctx.column(k))
            .collect::<Result<Vec<_>>>()?;
        let image = match &v.image {
            Some(prop) => image_of(vault, note, &ctx.column(prop)?),
            None => None,
        };
        let group = match &v.group_by {
            Some((prop, _)) => ctx.column(prop)?,
            None => Val::Null,
        };
        let row = Row {
            path: note.path.clone(),
            cells: vals.iter().map(Val::display).collect(),
            image,
        };
        built.push((group, vals, row));
    }
    // グループ分け: グループの値で並べ替える（同じグループの中は、上の並べ替えのまま）
    if let Some((_, desc)) = &v.group_by {
        built.sort_by(|(a, _, _), (b, _, _)| {
            let ord = match (a, b) {
                (Val::Null, Val::Null) => Ordering::Equal,
                (Val::Null, _) => return Ordering::Greater,
                (_, Val::Null) => return Ordering::Less,
                _ => a.compare(b).unwrap_or(Ordering::Equal),
            };
            if *desc {
                ord.reverse()
            } else {
                ord
            }
        });
    }
    let mut groups: Vec<Group> = Vec::new();
    let mut start = 0;
    for i in 1..=built.len() {
        let boundary = i == built.len()
            || (v.group_by.is_some() && built[i].0.display() != built[start].0.display());
        if !boundary {
            continue;
        }
        let label = match &v.group_by {
            None => String::new(),
            Some(_) => {
                let l = built[start].0.display();
                if l.is_empty() {
                    "（なし）".to_string()
                } else {
                    l
                }
            }
        };
        let summary = keys
            .iter()
            .enumerate()
            .map(|(col, key)| {
                let func = v
                    .summaries
                    .iter()
                    .find(|(k, _)| {
                        k == key
                            || k.strip_prefix("note.") == Some(key.as_str())
                            || key.strip_prefix("note.") == Some(k.as_str())
                    })
                    .map(|(_, f)| f);
                match func {
                    Some(f) => {
                        let vals: Vec<&Val> = built[start..i].iter().map(|b| &b.1[col]).collect();
                        summarize(f, &vals)
                    }
                    None => Ok(String::new()),
                }
            })
            .collect::<Result<Vec<_>>>()?;
        groups.push(Group {
            label,
            rows: start..i,
            summary,
        });
        start = i;
    }
    let rows: Vec<Row> = built.into_iter().map(|(_, _, r)| r).collect();
    let has_summary = !v.summaries.is_empty();
    Ok(Table {
        view: v.name.clone(),
        kind: v.kind.clone(),
        columns,
        rows,
        total,
        card_size: v.card_size,
        image_aspect: v.image_aspect,
        image_fit: v.image_fit.clone(),
        groups,
        has_summary,
    })
}

/// 集計（Obsidian の集計の名前。大文字小文字は問わない）。結果は「平均 12.5」のような文字列
pub fn summarize(func: &str, vals: &[&Val]) -> Result<String> {
    let nums: Vec<f64> = vals
        .iter()
        .filter_map(|v| match v {
            Val::Num(n) => Some(*n),
            Val::Str(s) => s.trim().parse().ok(),
            _ => None,
        })
        .collect();
    let dates: Vec<chrono::NaiveDateTime> = vals
        .iter()
        .filter_map(|v| match v {
            Val::Date(d, _) => Some(*d),
            _ => None,
        })
        .collect();
    let num = |n: f64| {
        // 小数は2桁まで
        Val::Num((n * 100.0).round() / 100.0).display()
    };
    let filled = |v: &&&Val| v.truthy() || matches!(v, Val::Num(_) | Val::Bool(_));
    let date =
        |d: &chrono::NaiveDateTime| Val::Date(*d, d.time() != chrono::NaiveTime::MIN).display();
    let mut sorted = nums.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let (label, value) = match func.to_lowercase().as_str() {
        "sum" => ("合計", num(nums.iter().sum())),
        "average" | "avg" | "mean" => (
            "平均",
            if nums.is_empty() {
                String::new()
            } else {
                num(nums.iter().sum::<f64>() / nums.len() as f64)
            },
        ),
        "min" => match (sorted.first(), dates.iter().min()) {
            (Some(n), _) => ("最小", num(*n)),
            (None, Some(d)) => ("最小", date(d)),
            _ => ("最小", String::new()),
        },
        "max" => match (sorted.last(), dates.iter().max()) {
            (Some(n), _) => ("最大", num(*n)),
            (None, Some(d)) => ("最大", date(d)),
            _ => ("最大", String::new()),
        },
        "median" => (
            "中央値",
            match sorted.len() {
                0 => String::new(),
                n if n % 2 == 1 => num(sorted[n / 2]),
                n => num((sorted[n / 2 - 1] + sorted[n / 2]) / 2.0),
            },
        ),
        "range" => match (
            sorted.first(),
            sorted.last(),
            dates.iter().min(),
            dates.iter().max(),
        ) {
            (Some(a), Some(b), _, _) => ("範囲", num(b - a)),
            (_, _, Some(a), Some(b)) => ("範囲", format!("{}日", (*b - *a).num_days())),
            _ => ("範囲", String::new()),
        },
        "stddev" => (
            "標準偏差",
            if nums.is_empty() {
                String::new()
            } else {
                let mean = nums.iter().sum::<f64>() / nums.len() as f64;
                let var = nums.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / nums.len() as f64;
                num(var.sqrt())
            },
        ),
        "earliest" => ("最も早い", dates.iter().min().map(date).unwrap_or_default()),
        "latest" => ("最も遅い", dates.iter().max().map(date).unwrap_or_default()),
        "checked" => (
            "チェック",
            vals.iter()
                .filter(|v| matches!(v, Val::Bool(true)))
                .count()
                .to_string(),
        ),
        "unchecked" => (
            "未チェック",
            vals.iter()
                .filter(|v| !matches!(v, Val::Bool(true)))
                .count()
                .to_string(),
        ),
        "empty" => ("空", vals.iter().filter(|v| !filled(v)).count().to_string()),
        "filled" => ("入力あり", vals.iter().filter(filled).count().to_string()),
        "unique" => {
            let set: BTreeSet<String> = vals.iter().filter(filled).map(|v| v.display()).collect();
            ("種類", set.len().to_string())
        }
        "count" => ("件数", vals.len().to_string()),
        other => bail!("未対応の集計です: {other}"),
    };
    Ok(if value.is_empty() {
        String::new()
    } else {
        format!("{label} {value}")
    })
}

/// 表を Markdown の表にする（プレビューや埋め込みで使う）。名前の列はノートへのリンクにする。
/// カードのビューは、画像を先頭の列にした表にする。グループ分けしていれば、グループごとに見出しと表を出す。
/// 集計があれば、表の最後に集計の行を足す。
pub fn to_markdown(table: &Table) -> String {
    let esc = |s: &str| s.replace('|', "\\|").replace('\n', " ");
    let cards = table.kind == "cards" && table.rows.iter().any(|r| r.image.is_some());
    let mut header: Vec<String> = table.columns.iter().map(|(_, n)| esc(n)).collect();
    if cards {
        header.insert(0, "画像".to_string());
    }
    let head = format!(
        "| {} |\n|{}\n",
        header.join(" | "),
        " --- |".repeat(header.len())
    );
    let mut out = String::new();
    for group in &table.groups {
        if !group.label.is_empty() {
            out.push_str(&format!(
                "\n**{}**（{}件）\n\n",
                esc(&group.label),
                group.rows.len()
            ));
        }
        out.push_str(&head);
        for row in &table.rows[group.rows.clone()] {
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
        if table.has_summary {
            let mut cells: Vec<String> = group
                .summary
                .iter()
                .map(|c| {
                    if c.is_empty() {
                        String::new()
                    } else {
                        format!("*{}*", esc(c))
                    }
                })
                .collect();
            if cells.first().is_some_and(String::is_empty) {
                cells[0] = "*集計*".to_string();
            }
            if cards {
                cells.insert(0, String::new());
            }
            out.push_str(&format!("| {} |\n", cells.join(" | ")));
        }
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
    fn groups_and_summaries() {
        let (_d, v) = vault();
        let base = parse(
            r#"
filters: file.hasTag("book")
views:
  - type: table
    name: 状態ごと
    order: [file.name, pages]
    groupBy:
      property: note.status
      direction: ASC
    summaries:
      pages: Sum
  - type: table
    name: 全体
    order: [file.name, pages, author]
    summaries:
      note.pages: Average
      author: Unique
"#,
        )
        .unwrap();
        let t = run(&v, &base, None).unwrap();
        let labels: Vec<&str> = t.groups.iter().map(|g| g.label.as_str()).collect();
        // 値の無いもの（c は status が無い）は最後
        assert_eq!(labels, ["done", "reading", "（なし）"]);
        assert_eq!(t.groups[0].summary, ["", "合計 120"]);
        assert_eq!(t.groups[2].summary, ["", "合計 80"]);
        let md = to_markdown(&t);
        assert!(md.contains("**reading**（1件）"), "{md}");
        assert!(md.contains("| *集計* | *合計 300* |"), "{md}");

        let t = run(&v, &base, Some("全体")).unwrap();
        assert_eq!(t.groups.len(), 1);
        assert_eq!(t.groups[0].label, "");
        assert_eq!(t.groups[0].summary, ["", "平均 166.67", "種類 3"]);

        let vals = [Val::Num(1.0), Val::Num(4.0), Val::Null, Val::Bool(true)];
        let refs: Vec<&Val> = vals.iter().collect();
        assert_eq!(summarize("median", &refs).unwrap(), "中央値 2.5");
        assert_eq!(summarize("Range", &refs).unwrap(), "範囲 3");
        assert_eq!(summarize("checked", &refs).unwrap(), "チェック 1");
        assert_eq!(summarize("empty", &refs).unwrap(), "空 1");
        assert!(summarize("nope", &refs).is_err());
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
