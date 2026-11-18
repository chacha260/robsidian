//! Bases（Obsidian の `.base`）: ノートのプロパティを表にして、絞り込み・並べ替えをする。
//!
//! Obsidian の書き方のうち、よく使う部分だけを読む。
//!
//! ```yaml
//! filters:
//!   and:
//!     - file.hasTag("book")
//!     - 'status != "done"'
//! properties:
//!   status:
//!     displayName: 状態
//! views:
//!   - type: table
//!     name: 読書中
//!     order: [file.name, author, status]
//!     sort:
//!       - property: file.mtime
//!         direction: DESC
//!     limit: 20
//! ```
//!
//! 条件式は、比較（`==` `!=` `>` `<` `>=` `<=`）、`file.hasTag()` `file.inFolder()` `file.hasLink()`
//! `file.hasProperty()`、`プロパティ.contains()`、先頭の `!` による否定に対応する。
//! それ以外（式の計算 `formulas`、カード表示など）は未対応で、未対応の式はエラーとして見せる。

use crate::props::{self, Value as PropValue};
use crate::{Note, Vault};
use anyhow::{anyhow, bail, Result};
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
    pub kind: String,
    pub name: String,
    pub filters: Option<Filter>,
    pub order: Vec<String>,
    /// (プロパティ, 降順か)
    pub sort: Vec<(String, bool)>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Base {
    pub filters: Option<Filter>,
    pub views: Vec<View>,
    /// プロパティ → 表示名
    pub display: BTreeMap<String, String>,
}

/// 表にした結果
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Table {
    pub view: String,
    /// (プロパティ, 表示名)
    pub columns: Vec<(String, String)>,
    pub rows: Vec<Row>,
    /// 当てはまったノートの数（`limit` で切る前）
    pub total: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub path: String,
    pub cells: Vec<String>,
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
            });
        }
    }
    if base.views.is_empty() {
        base.views.push(View {
            kind: "table".into(),
            name: "表".into(),
            filters: None,
            order: Vec::new(),
            sort: Vec::new(),
            limit: None,
        });
    }
    Ok(base)
}

/// 比較に使う値
#[derive(Debug, Clone, PartialEq)]
enum Val {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    List(Vec<String>),
}

impl Val {
    fn from_text(s: &str) -> Val {
        let t = s.trim().trim_matches(['"', '\'']);
        match t {
            "true" => Val::Bool(true),
            "false" => Val::Bool(false),
            "" => Val::Null,
            _ => t
                .parse::<f64>()
                .map_or_else(|_| Val::Str(t.to_string()), Val::Num),
        }
    }

    fn display(&self) -> String {
        match self {
            Val::Null => String::new(),
            Val::Bool(b) => b.to_string(),
            Val::Num(n) if n.fract() == 0.0 && n.abs() < 1e15 => format!("{}", *n as i64),
            Val::Num(n) => n.to_string(),
            Val::Str(s) => s.clone(),
            Val::List(l) => l.join(", "),
        }
    }

    fn truthy(&self) -> bool {
        match self {
            Val::Null => false,
            Val::Bool(b) => *b,
            Val::Num(n) => *n != 0.0,
            Val::Str(s) => !s.is_empty(),
            Val::List(l) => !l.is_empty(),
        }
    }

    fn cmp(&self, other: &Val) -> Option<Ordering> {
        match (self, other) {
            (Val::Num(a), Val::Num(b)) => a.partial_cmp(b),
            (Val::Null, Val::Null) => Some(Ordering::Equal),
            (Val::Null, _) | (_, Val::Null) => None,
            (a, b) => Some(a.display().to_lowercase().cmp(&b.display().to_lowercase())),
        }
    }
}

fn frontmatter(note: &Note) -> Vec<props::Prop> {
    props::read(&note.text).unwrap_or_default()
}

/// ノートのプロパティの値。`file.` で始まるものはファイルの情報、`note.` は省略できる。
fn property(vault: &Vault, note: &Note, fm: &[props::Prop], key: &str) -> Val {
    match key {
        "file.name" => return Val::Str(note.name.clone()),
        "file.basename" => return Val::Str(note.name.clone()),
        "file.path" => return Val::Str(note.path.clone()),
        "file.folder" => return Val::Str(crate::vault::parent_dir(&note.path).to_string()),
        "file.ext" => return Val::Str("md".into()),
        "file.size" => return Val::Num(note.text.len() as f64),
        "file.mtime" | "file.ctime" => return Val::Num(note.mtime as f64),
        "file.tags" => return Val::List(note.parsed.tags.clone()),
        "file.links" => {
            return Val::List(
                vault
                    .outgoing(&note.path)
                    .into_iter()
                    .filter_map(|(_, dst)| dst.map(str::to_string))
                    .collect(),
            )
        }
        _ => {}
    }
    let key = key.strip_prefix("note.").unwrap_or(key);
    match fm.iter().find(|p| p.key == key).map(|p| &p.value) {
        Some(PropValue::Text(t)) => Val::from_text(t),
        Some(PropValue::List(l)) => Val::List(l.clone()),
        Some(PropValue::Raw(r)) => Val::Str(r.trim().to_string()),
        None => Val::Null,
    }
}

/// 関数の引数（`"a", "b"`）を読む
fn args(s: &str) -> Vec<String> {
    s.split(',')
        .map(|a| a.trim().trim_matches(['"', '\'']).to_string())
        .filter(|a| !a.is_empty())
        .collect()
}

/// 条件式を1つ評価する。
fn eval(vault: &Vault, note: &Note, fm: &[props::Prop], expr: &str) -> Result<bool> {
    let e = expr.trim();
    if let Some(rest) = e.strip_prefix('!') {
        return Ok(!eval(vault, note, fm, rest)?);
    }
    if let Some(inner) = e.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
        return eval(vault, note, fm, inner);
    }
    // 関数呼び出し: name(args)
    if let Some(open) = e.find('(') {
        if e.ends_with(')') && !e[..open].contains([' ', '=', '<', '>']) {
            let name = &e[..open];
            let a = args(&e[open + 1..e.len() - 1]);
            return match name {
                "file.hasTag" => Ok(a.iter().any(|t| {
                    let t = t.trim_start_matches('#').to_lowercase();
                    note.parsed.tags.iter().any(|nt| {
                        let nt = nt.to_lowercase();
                        nt == t || nt.starts_with(&format!("{t}/"))
                    })
                })),
                "file.inFolder" => Ok(a.iter().any(|f| {
                    let f = f.trim_matches('/');
                    f.is_empty() || note.path.starts_with(&format!("{f}/"))
                })),
                "file.hasLink" => Ok(a.iter().any(|target| {
                    let target_path = vault.resolve(target, &note.path).map(str::to_string);
                    vault
                        .outgoing(&note.path)
                        .iter()
                        .any(|(_, dst)| dst.is_some() && dst.map(str::to_string) == target_path)
                })),
                "file.hasProperty" => Ok(a.iter().any(|p| fm.iter().any(|f| f.key == *p))),
                _ => {
                    // プロパティ.contains("x")
                    if let Some(prop) = name.strip_suffix(".contains") {
                        let v = property(vault, note, fm, prop);
                        let needle = a.first().map(|s| s.to_lowercase()).unwrap_or_default();
                        Ok(match v {
                            Val::List(l) => l.iter().any(|x| x.to_lowercase() == needle),
                            other => other.display().to_lowercase().contains(&needle),
                        })
                    } else {
                        bail!("未対応の関数です: {name}()")
                    }
                }
            };
        }
    }
    // 比較: 左 演算子 右（長い演算子から探す）
    for op in ["==", "!=", ">=", "<=", ">", "<"] {
        if let Some(i) = find_op(e, op) {
            let left = operand(vault, note, fm, &e[..i]);
            let right = operand(vault, note, fm, &e[i + op.len()..]);
            let ord = left.cmp(&right);
            return Ok(match op {
                "==" => ord == Some(Ordering::Equal),
                "!=" => ord != Some(Ordering::Equal),
                ">" => ord == Some(Ordering::Greater),
                "<" => ord == Some(Ordering::Less),
                ">=" => matches!(ord, Some(Ordering::Greater | Ordering::Equal)),
                _ => matches!(ord, Some(Ordering::Less | Ordering::Equal)),
            });
        }
    }
    match e {
        "true" => Ok(true),
        "false" => Ok(false),
        // プロパティだけなら、値があるか
        _ if e
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-')) =>
        {
            Ok(property(vault, note, fm, e).truthy())
        }
        _ => bail!("未対応の条件です: {e}"),
    }
}

/// 引用符の外にある演算子の位置
fn find_op(e: &str, op: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (i, c) in e.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if e[i..].starts_with(op) => {
                // `>=` を `>` と読まないよう、次の文字が `=` なら飛ばす（`==` `!=` `>=` `<=` を先に探している）
                if matches!(op, ">" | "<") && e[i + 1..].starts_with('=') {
                    continue;
                }
                return Some(i);
            }
            None => {}
        }
    }
    None
}

fn operand(vault: &Vault, note: &Note, fm: &[props::Prop], s: &str) -> Val {
    let t = s.trim();
    if t.starts_with('"') || t.starts_with('\'') {
        return Val::Str(t.trim_matches(['"', '\'']).to_string());
    }
    if let Ok(n) = t.parse::<f64>() {
        return Val::Num(n);
    }
    match t {
        "true" => Val::Bool(true),
        "false" => Val::Bool(false),
        "null" => Val::Null,
        _ => property(vault, note, fm, t),
    }
}

fn matches(vault: &Vault, note: &Note, fm: &[props::Prop], f: &Filter) -> Result<bool> {
    Ok(match f {
        Filter::And(list) => {
            for f in list {
                if !matches(vault, note, fm, f)? {
                    return Ok(false);
                }
            }
            true
        }
        Filter::Or(list) => {
            for f in list {
                if matches(vault, note, fm, f)? {
                    return Ok(true);
                }
            }
            list.is_empty()
        }
        Filter::Not(list) => {
            for f in list {
                if matches(vault, note, fm, f)? {
                    return Ok(false);
                }
            }
            true
        }
        Filter::Expr(e) => eval(vault, note, fm, e)?,
    })
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
    let mut hits: Vec<(&Note, Vec<props::Prop>)> = Vec::new();
    for note in vault.notes() {
        let fm = frontmatter(note);
        let ok = match (&base.filters, &v.filters) {
            (None, None) => true,
            (a, b) => {
                a.as_ref()
                    .map_or(Ok(true), |f| matches(vault, note, &fm, f))?
                    && b.as_ref()
                        .map_or(Ok(true), |f| matches(vault, note, &fm, f))?
            }
        };
        if ok {
            hits.push((note, fm));
        }
    }
    // 並べ替え（指定が無ければ名前順）
    let sort = if v.sort.is_empty() {
        vec![("file.name".to_string(), false)]
    } else {
        v.sort.clone()
    };
    hits.sort_by(|(a, fa), (b, fb)| {
        for (prop, desc) in &sort {
            let (x, y) = (property(vault, a, fa, prop), property(vault, b, fb, prop));
            // 値の無いものは常に後ろ
            let ord = match (&x, &y) {
                (Val::Null, Val::Null) => Ordering::Equal,
                (Val::Null, _) => return Ordering::Greater,
                (_, Val::Null) => return Ordering::Less,
                _ => x.cmp(&y).unwrap_or(Ordering::Equal),
            };
            let ord = if *desc { ord.reverse() } else { ord };
            if ord != Ordering::Equal {
                return ord;
            }
        }
        a.path.cmp(&b.path)
    });
    let total = hits.len();
    if let Some(n) = v.limit {
        hits.truncate(n);
    }
    // 列（指定が無ければ、名前と、当てはまったノートの frontmatter のキー）
    let keys: Vec<String> = if v.order.is_empty() {
        let mut seen = BTreeSet::new();
        let mut keys = vec!["file.name".to_string()];
        for (_, fm) in &hits {
            for p in fm {
                if seen.insert(p.key.clone()) {
                    keys.push(p.key.clone());
                }
            }
        }
        keys.truncate(8);
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
                .cloned()
                .unwrap_or_else(|| match k.as_str() {
                    "file.name" => "名前".to_string(),
                    "file.path" => "パス".to_string(),
                    "file.folder" => "フォルダ".to_string(),
                    "file.mtime" => "更新".to_string(),
                    "file.tags" => "タグ".to_string(),
                    other => other.strip_prefix("note.").unwrap_or(other).to_string(),
                });
            (k.clone(), shown)
        })
        .collect();
    let rows = hits
        .iter()
        .map(|(note, fm)| Row {
            path: note.path.clone(),
            cells: keys
                .iter()
                .map(|k| match k.as_str() {
                    "file.mtime" | "file.ctime" => {
                        chrono::DateTime::from_timestamp(note.mtime as i64, 0)
                            .map(|t| {
                                t.with_timezone(&chrono::Local)
                                    .format("%Y-%m-%d %H:%M")
                                    .to_string()
                            })
                            .unwrap_or_default()
                    }
                    _ => property(vault, note, fm, k).display(),
                })
                .collect(),
        })
        .collect();
    Ok(Table {
        view: v.name.clone(),
        columns,
        rows,
        total,
    })
}

/// 表を Markdown の表にする（プレビューや埋め込みで使う）。名前の列はノートへのリンクにする。
pub fn to_markdown(table: &Table) -> String {
    let esc = |s: &str| s.replace('|', "\\|").replace('\n', " ");
    let mut out = String::new();
    out.push_str(&format!(
        "| {} |\n",
        table
            .columns
            .iter()
            .map(|(_, n)| esc(n))
            .collect::<Vec<_>>()
            .join(" | ")
    ));
    out.push_str(&format!("|{}\n", " --- |".repeat(table.columns.len())));
    for row in &table.rows {
        let cells: Vec<String> = row
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
