//! Bases の式: 絞り込みの条件（`filters`）と計算列（`formulas`）で使う、小さな式の言語。
//!
//! ```text
//! pages >= 100 && !file.hasTag("done")
//! if(price, (price * 1.1).toFixed(0) + "円", "")
//! file.mtime > now() - "7d"
//! note["読み終えた日"].format("YYYY/MM/DD")
//! ```
//!
//! - 値: 数値・文字列（`"…"` か `'…'`）・`true` / `false` / `null`・リスト（`[a, b]`）・日付
//! - 演算子: `+ - * / %`、比較 `== != > < >= <=`、`&& || !`、かっこ
//! - 参照: 名前だけならノートのプロパティ（`note.x`・`note["空白 入り"]` とも書ける）、
//!   `file.name` などファイルの情報、`formula.x` は計算列
//! - 関数とメソッドは `call_global` と `method` を参照。知らないものはエラーにする（黙って無視しない）

use crate::props::{self, Value as PropValue};
use crate::{Note, Vault};
use anyhow::{anyhow, bail, Result};
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, Timelike};
use std::cmp::Ordering;
use std::collections::BTreeMap;

// ---------------------------------------------------------------- 字句

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    /// 記号（`(` `==` `&&` など）
    Op(&'static str),
}

const OPS: [&str; 22] = [
    "==", "!=", ">=", "<=", "&&", "||", "(", ")", "[", "]", ",", ".", "!", "+", "-", "*", "/", "%",
    ">", "<", "?", ":",
];

fn lex(src: &str) -> Result<Vec<Tok>> {
    let mut out = Vec::new();
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '"' || c == '\'' {
            let mut s = String::new();
            i += 1;
            loop {
                match chars.get(i) {
                    None => bail!("文字列が閉じていません: {src}"),
                    Some(&q) if q == c => break,
                    Some('\\') => {
                        let e = chars.get(i + 1).copied().unwrap_or('\\');
                        s.push(match e {
                            'n' => '\n',
                            't' => '\t',
                            other => other,
                        });
                        i += 1;
                    }
                    Some(&ch) => s.push(ch),
                }
                i += 1;
            }
            i += 1;
            out.push(Tok::Str(s));
        } else if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                // `1.toFixed` のような書き方は無いので、`.` の後が数字のときだけ小数点
                if chars[i] == '.' && !chars.get(i + 1).is_some_and(char::is_ascii_digit) {
                    break;
                }
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            out.push(Tok::Num(
                text.parse()
                    .map_err(|_| anyhow!("数値が読めません: {text}"))?,
            ));
        } else if c.is_alphanumeric() || c == '_' || c == '$' {
            let start = i;
            // `a-b` は引き算として読む（`-` や空白を含むプロパティ名は `note["a-b"]` と書く）
            while i < chars.len() && (chars[i].is_alphanumeric() || matches!(chars[i], '_' | '$')) {
                i += 1;
            }
            out.push(Tok::Ident(chars[start..i].iter().collect()));
        } else {
            let rest: String = chars[i..chars.len().min(i + 2)].iter().collect();
            let op = OPS
                .iter()
                .find(|op| rest.starts_with(**op))
                .ok_or_else(|| anyhow!("使えない記号です: {c}（{src}）"))?;
            out.push(Tok::Op(op));
            i += op.chars().count();
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- 構文

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Num(f64),
    Str(String),
    Bool(bool),
    Null,
    Ident(String),
    List(Vec<Expr>),
    Member(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    Unary(&'static str, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    /// `a ? b : c`
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

fn binding(op: &str) -> Option<u8> {
    Some(match op {
        "||" => 1,
        "&&" => 2,
        "==" | "!=" => 3,
        ">" | "<" | ">=" | "<=" => 4,
        "+" | "-" => 5,
        "*" | "/" | "%" => 6,
        _ => return None,
    })
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn eat(&mut self, op: &str) -> bool {
        if matches!(self.peek(), Some(Tok::Op(o)) if *o == op) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, op: &str) -> Result<()> {
        if self.eat(op) {
            Ok(())
        } else {
            bail!("`{op}` がありません")
        }
    }

    fn expr(&mut self, min: u8) -> Result<Expr> {
        let mut left = self.unary()?;
        while let Some(Tok::Op(op)) = self.peek().cloned() {
            if op == "?" && min == 0 {
                self.pos += 1;
                let yes = self.expr(0)?;
                self.expect(":")?;
                let no = self.expr(0)?;
                left = Expr::Cond(Box::new(left), Box::new(yes), Box::new(no));
                continue;
            }
            let Some(power) = binding(op).filter(|p| *p > min) else {
                break;
            };
            self.pos += 1;
            let right = self.expr(power)?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr> {
        if self.eat("!") {
            return Ok(Expr::Unary("!", Box::new(self.unary()?)));
        }
        if self.eat("-") {
            return Ok(Expr::Unary("-", Box::new(self.unary()?)));
        }
        let mut e = self.primary()?;
        loop {
            if self.eat(".") {
                match self.peek().cloned() {
                    Some(Tok::Ident(name)) => {
                        self.pos += 1;
                        e = Expr::Member(Box::new(e), name);
                    }
                    _ => bail!("`.` の後に名前がありません"),
                }
            } else if self.eat("(") {
                let args = self.list(")")?;
                e = Expr::Call(Box::new(e), args);
            } else if self.eat("[") {
                let index = self.expr(0)?;
                self.expect("]")?;
                e = Expr::Index(Box::new(e), Box::new(index));
            } else {
                break;
            }
        }
        Ok(e)
    }

    fn list(&mut self, close: &str) -> Result<Vec<Expr>> {
        let mut items = Vec::new();
        if self.eat(close) {
            return Ok(items);
        }
        loop {
            items.push(self.expr(0)?);
            if self.eat(close) {
                return Ok(items);
            }
            self.expect(",")?;
        }
    }

    fn primary(&mut self) -> Result<Expr> {
        let tok = self
            .toks
            .get(self.pos)
            .cloned()
            .ok_or_else(|| anyhow!("式が途中で終わっています"))?;
        self.pos += 1;
        Ok(match tok {
            Tok::Num(n) => Expr::Num(n),
            Tok::Str(s) => Expr::Str(s),
            Tok::Ident(name) => match name.as_str() {
                "true" => Expr::Bool(true),
                "false" => Expr::Bool(false),
                "null" => Expr::Null,
                _ => Expr::Ident(name),
            },
            Tok::Op("(") => {
                let e = self.expr(0)?;
                self.expect(")")?;
                e
            }
            Tok::Op("[") => Expr::List(self.list("]")?),
            Tok::Op(op) => bail!("ここに `{op}` は置けません"),
        })
    }
}

/// 式を読む
pub fn parse(src: &str) -> Result<Expr> {
    let toks = lex(src)?;
    let mut p = Parser { toks, pos: 0 };
    let e = p.expr(0)?;
    if p.pos < p.toks.len() {
        bail!("式の終わりに余分なものがあります: {src}");
    }
    Ok(e)
}

// ---------------------------------------------------------------- 値

#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    /// 日付（時刻を持つか）
    Date(NaiveDateTime, bool),
    List(Vec<Val>),
}

impl Val {
    /// プロパティの文字列を値にする（数値・真偽値・日付として読めればそれで）
    pub fn from_text(s: &str) -> Val {
        let t = s.trim().trim_matches(['"', '\'']);
        match t {
            "true" => Val::Bool(true),
            "false" => Val::Bool(false),
            "" => Val::Null,
            _ => {
                if let Ok(n) = t.parse::<f64>() {
                    return Val::Num(n);
                }
                parse_date(t).unwrap_or_else(|| Val::Str(t.to_string()))
            }
        }
    }

    pub fn display(&self) -> String {
        match self {
            Val::Null => String::new(),
            Val::Bool(b) => b.to_string(),
            Val::Num(n) if n.fract() == 0.0 && n.abs() < 1e15 => format!("{}", *n as i64),
            Val::Num(n) => {
                // 0.1 + 0.2 のような誤差を見せない
                let s = format!("{n:.10}");
                s.trim_end_matches('0').trim_end_matches('.').to_string()
            }
            Val::Str(s) => s.clone(),
            Val::Date(d, true) => d.format("%Y-%m-%d %H:%M").to_string(),
            Val::Date(d, false) => d.format("%Y-%m-%d").to_string(),
            Val::List(l) => l.iter().map(Val::display).collect::<Vec<_>>().join(", "),
        }
    }

    pub fn truthy(&self) -> bool {
        match self {
            Val::Null => false,
            Val::Bool(b) => *b,
            Val::Num(n) => *n != 0.0 && !n.is_nan(),
            Val::Str(s) => !s.is_empty(),
            Val::Date(..) => true,
            Val::List(l) => !l.is_empty(),
        }
    }

    /// 比べる。数値どうしは数値、日付どうしは日付（文字列は日付として読めれば日付）、それ以外は文字列で
    pub fn compare(&self, other: &Val) -> Option<Ordering> {
        match (self, other) {
            (Val::Num(a), Val::Num(b)) => a.partial_cmp(b),
            (Val::Date(a, _), Val::Date(b, _)) => Some(a.cmp(b)),
            (Val::Date(..), Val::Str(s)) => parse_date(s).and_then(|d| self.compare(&d)),
            (Val::Str(s), Val::Date(..)) => parse_date(s).and_then(|d| d.compare(other)),
            (Val::Null, Val::Null) => Some(Ordering::Equal),
            (Val::Null, _) | (_, Val::Null) => None,
            (a, b) => Some(a.display().to_lowercase().cmp(&b.display().to_lowercase())),
        }
    }

    fn num(&self) -> Option<f64> {
        match self {
            Val::Num(n) => Some(*n),
            Val::Bool(b) => Some(f64::from(u8::from(*b))),
            Val::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }

    fn type_name(&self) -> &'static str {
        match self {
            Val::Null => "null",
            Val::Bool(_) => "真偽値",
            Val::Num(_) => "数値",
            Val::Str(_) => "文字列",
            Val::Date(..) => "日付",
            Val::List(_) => "リスト",
        }
    }
}

fn parse_date(s: &str) -> Option<Val> {
    let s = s.trim();
    for f in [
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(d) = NaiveDateTime::parse_from_str(s, f) {
            return Some(Val::Date(d, true));
        }
    }
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|d| Val::Date(d, false))
}

/// `"7d"` `"2 weeks"` `"-1M"` のような期間。月と年は日数ではなく暦で足す
enum Span {
    Fixed(Duration),
    Months(i32),
}

fn parse_span(s: &str) -> Option<Span> {
    let t = s.trim();
    let (sign, t) = match t.strip_prefix('-') {
        Some(rest) => (-1, rest.trim()),
        None => (1, t.trim_start_matches('+').trim()),
    };
    let split = t.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
    let n: f64 = t[..split].trim().parse().ok()?;
    let unit = t[split..].trim();
    let n = n * f64::from(sign);
    let secs = |k: f64| Some(Span::Fixed(Duration::milliseconds((n * k * 1000.0) as i64)));
    match unit {
        "y" | "year" | "years" => Some(Span::Months((n * 12.0) as i32)),
        "M" | "month" | "months" => Some(Span::Months(n as i32)),
        "w" | "week" | "weeks" => secs(7.0 * 86400.0),
        "d" | "day" | "days" => secs(86400.0),
        "h" | "hour" | "hours" => secs(3600.0),
        "m" | "minute" | "minutes" => secs(60.0),
        "s" | "second" | "seconds" => secs(1.0),
        _ => None,
    }
}

fn shift(d: NaiveDateTime, span: &Span, sign: i32) -> Option<NaiveDateTime> {
    match span {
        Span::Fixed(dur) => d.checked_add_signed(*dur * sign),
        Span::Months(m) => {
            let m = m * sign;
            if m >= 0 {
                d.checked_add_months(chrono::Months::new(m as u32))
            } else {
                d.checked_sub_months(chrono::Months::new((-m) as u32))
            }
        }
    }
}

/// Moment.js 風の書式（`YYYY-MM-DD HH:mm`）を chrono の書式にする
fn moment_format(d: &NaiveDateTime, fmt: &str) -> String {
    const TOKENS: [(&str, &str); 14] = [
        ("YYYY", "%Y"),
        ("YY", "%y"),
        ("MMMM", "%B"),
        ("MMM", "%b"),
        ("MM", "%m"),
        ("M", "%-m"),
        ("DD", "%d"),
        ("D", "%-d"),
        ("dddd", "%A"),
        ("ddd", "%a"),
        ("HH", "%H"),
        ("H", "%-H"),
        ("mm", "%M"),
        ("ss", "%S"),
    ];
    let mut out = String::new();
    let mut rest = fmt;
    'outer: while !rest.is_empty() {
        // `[文字]` はそのまま
        if let Some(inner) = rest.strip_prefix('[') {
            if let Some(end) = inner.find(']') {
                out.push_str(&inner[..end].replace('%', "%%"));
                rest = &inner[end + 1..];
                continue;
            }
        }
        for (tok, f) in TOKENS {
            if let Some(r) = rest.strip_prefix(tok) {
                out.push_str(f);
                rest = r;
                continue 'outer;
            }
        }
        let c = rest.chars().next().expect("空でない");
        if c == '%' {
            out.push_str("%%");
        } else {
            out.push(c);
        }
        rest = &rest[c.len_utf8()..];
    }
    d.format(&out).to_string()
}

// ---------------------------------------------------------------- 評価

/// 1つのノートについて式を計算するときの材料
pub struct Ctx<'a> {
    pub vault: &'a Vault,
    pub note: &'a Note,
    pub fm: &'a [props::Prop],
    /// 計算列（名前 → 式）
    pub formulas: &'a BTreeMap<String, Expr>,
    /// 計算列の入れ子の深さ（互いに参照し合う式で止まらないように）
    pub depth: u8,
}

fn local(secs: u64) -> Val {
    chrono::DateTime::from_timestamp(secs as i64, 0)
        .map(|t| Val::Date(t.with_timezone(&Local).naive_local(), true))
        .unwrap_or(Val::Null)
}

impl Ctx<'_> {
    /// ノートのプロパティ
    pub fn note_prop(&self, key: &str) -> Val {
        match self.fm.iter().find(|p| p.key == key).map(|p| &p.value) {
            Some(PropValue::Text(t)) => Val::from_text(t),
            Some(PropValue::List(l)) => Val::List(l.iter().map(|x| Val::from_text(x)).collect()),
            Some(PropValue::Raw(r)) => Val::Str(r.trim().to_string()),
            None => Val::Null,
        }
    }

    /// ファイルの情報（`file.name` など）
    fn file_prop(&self, key: &str) -> Result<Val> {
        let note = self.note;
        Ok(match key {
            "name" | "basename" => Val::Str(note.name.clone()),
            "path" => Val::Str(note.path.clone()),
            "folder" => Val::Str(crate::vault::parent_dir(&note.path).to_string()),
            "ext" => Val::Str("md".into()),
            "size" => Val::Num(note.text.len() as f64),
            "mtime" | "ctime" => local(note.mtime),
            "tags" => Val::List(
                note.parsed
                    .tags
                    .iter()
                    .map(|t| Val::Str(t.clone()))
                    .collect(),
            ),
            "links" => Val::List(
                self.vault
                    .outgoing(&note.path)
                    .into_iter()
                    .filter_map(|(_, dst)| dst.map(|d| Val::Str(d.to_string())))
                    .collect(),
            ),
            "backlinks" => {
                // 1つのノートから何行もリンクしていれば、そのぶん並ぶので1つにまとめる
                let mut seen: Vec<Val> = Vec::new();
                for b in self.vault.backlinks(&note.path) {
                    let v = Val::Str(b.source.to_string());
                    if !seen.contains(&v) {
                        seen.push(v);
                    }
                }
                Val::List(seen)
            }
            _ => bail!("未対応のファイルの情報です: file.{key}"),
        })
    }

    /// 列や並べ替えに書く名前（`file.name`・`note.x`・`formula.x`・`x`）の値
    pub fn column(&self, key: &str) -> Result<Val> {
        if let Some(k) = key.strip_prefix("file.") {
            return self.file_prop(k);
        }
        if let Some(k) = key.strip_prefix("formula.") {
            return self.formula(k);
        }
        Ok(self.note_prop(key.strip_prefix("note.").unwrap_or(key)))
    }

    fn formula(&self, name: &str) -> Result<Val> {
        let e = self
            .formulas
            .get(name)
            .ok_or_else(|| anyhow!("計算列がありません: formula.{name}"))?;
        if self.depth > 16 {
            bail!("計算列が互いに参照し合っています: formula.{name}");
        }
        let inner = Ctx {
            depth: self.depth + 1,
            ..*self
        };
        inner.eval(e)
    }

    pub fn eval(&self, e: &Expr) -> Result<Val> {
        Ok(match e {
            Expr::Num(n) => Val::Num(*n),
            Expr::Str(s) => Val::Str(s.clone()),
            Expr::Bool(b) => Val::Bool(*b),
            Expr::Null => Val::Null,
            Expr::List(items) => {
                Val::List(items.iter().map(|i| self.eval(i)).collect::<Result<_>>()?)
            }
            Expr::Ident(name) => match name.as_str() {
                "file" | "note" | "formula" => bail!("`{name}.名前` の形で書いてください"),
                "this" => bail!("`this` には未対応です"),
                _ => self.note_prop(name),
            },
            Expr::Member(obj, name) => match obj.as_ref() {
                Expr::Ident(ns) if ns == "file" => self.file_prop(name)?,
                Expr::Ident(ns) if ns == "note" => self.note_prop(name),
                Expr::Ident(ns) if ns == "formula" => self.formula(name)?,
                _ => {
                    let v = self.eval(obj)?;
                    field(&v, name)?
                }
            },
            Expr::Index(obj, index) => {
                let i = self.eval(index)?;
                match obj.as_ref() {
                    Expr::Ident(ns) if ns == "note" => self.note_prop(&i.display()),
                    Expr::Ident(ns) if ns == "file" => self.file_prop(&i.display())?,
                    Expr::Ident(ns) if ns == "formula" => self.formula(&i.display())?,
                    _ => match (self.eval(obj)?, i.num()) {
                        (Val::List(l), Some(n)) => {
                            let n = if n < 0.0 { l.len() as f64 + n } else { n };
                            l.get(n as usize).cloned().unwrap_or(Val::Null)
                        }
                        (Val::Str(s), Some(n)) => s
                            .chars()
                            .nth(n as usize)
                            .map_or(Val::Null, |c| Val::Str(c.to_string())),
                        _ => Val::Null,
                    },
                }
            }
            Expr::Call(callee, args) => self.call(callee, args)?,
            Expr::Unary(op, x) => {
                let v = self.eval(x)?;
                match *op {
                    "!" => Val::Bool(!v.truthy()),
                    _ => v.num().map_or(Val::Null, |n| Val::Num(-n)),
                }
            }
            Expr::Cond(c, yes, no) => {
                if self.eval(c)?.truthy() {
                    self.eval(yes)?
                } else {
                    self.eval(no)?
                }
            }
            Expr::Binary(op, a, b) => match *op {
                "&&" => Val::Bool(self.eval(a)?.truthy() && self.eval(b)?.truthy()),
                "||" => Val::Bool(self.eval(a)?.truthy() || self.eval(b)?.truthy()),
                _ => binary(op, self.eval(a)?, self.eval(b)?),
            },
        })
    }

    fn call(&self, callee: &Expr, args: &[Expr]) -> Result<Val> {
        match callee {
            // ファイルについての関数
            Expr::Member(obj, name) if matches!(obj.as_ref(), Expr::Ident(ns) if ns == "file") => {
                let a: Vec<Val> = args.iter().map(|x| self.eval(x)).collect::<Result<_>>()?;
                let texts: Vec<String> = a.iter().flat_map(flatten).map(|v| v.display()).collect();
                let note = self.note;
                Ok(Val::Bool(match name.as_str() {
                    "hasTag" => texts.iter().any(|t| {
                        let t = t.trim_start_matches('#').to_lowercase();
                        note.parsed.tags.iter().any(|nt| {
                            let nt = nt.to_lowercase();
                            nt == t || nt.starts_with(&format!("{t}/"))
                        })
                    }),
                    "inFolder" => texts.iter().any(|f| {
                        let f = f.trim_matches('/');
                        f.is_empty() || note.path.starts_with(&format!("{f}/"))
                    }),
                    "hasLink" => texts.iter().any(|target| {
                        let target = target.trim_start_matches("[[").trim_end_matches("]]");
                        let want = self.vault.resolve(target, &note.path).map(str::to_string);
                        want.is_some()
                            && self
                                .vault
                                .outgoing(&note.path)
                                .iter()
                                .any(|(_, dst)| dst.map(str::to_string) == want)
                    }),
                    "hasProperty" => texts.iter().any(|p| self.fm.iter().any(|f| f.key == *p)),
                    other => bail!("未対応の関数です: file.{other}()"),
                }))
            }
            Expr::Member(obj, name) => {
                let v = self.eval(obj)?;
                let a: Vec<Val> = args.iter().map(|x| self.eval(x)).collect::<Result<_>>()?;
                method(&v, name, &a)
            }
            Expr::Ident(name) => self.call_global(name, args),
            _ => bail!("呼び出せない式です"),
        }
    }

    fn call_global(&self, name: &str, args: &[Expr]) -> Result<Val> {
        // if は使わない側を計算しない
        if name == "if" {
            let cond = args
                .first()
                .ok_or_else(|| anyhow!("if() に条件がありません"))?;
            let pick = if self.eval(cond)?.truthy() { 1 } else { 2 };
            return args.get(pick).map_or(Ok(Val::Null), |e| self.eval(e));
        }
        let a: Vec<Val> = args.iter().map(|x| self.eval(x)).collect::<Result<_>>()?;
        let first = a.first().cloned().unwrap_or(Val::Null);
        Ok(match name {
            "now" => Val::Date(Local::now().naive_local(), true),
            "today" => Val::Date(
                Local::now()
                    .date_naive()
                    .and_hms_opt(0, 0, 0)
                    .expect("0時は必ずある"),
                false,
            ),
            "date" => match first {
                Val::Date(..) => first,
                other => parse_date(&other.display()).unwrap_or(Val::Null),
            },
            "number" => match first {
                Val::Date(d, _) => Val::Num(d.and_utc().timestamp_millis() as f64),
                other => other.num().map_or(Val::Null, Val::Num),
            },
            "string" => Val::Str(first.display()),
            "list" => match first {
                Val::List(_) => first,
                Val::Null => Val::List(Vec::new()),
                other => Val::List(vec![other]),
            },
            "link" => Val::Str(
                a.get(1)
                    .map(Val::display)
                    .unwrap_or_else(|| first.display()),
            ),
            "max" | "min" => {
                let nums: Vec<f64> = a.iter().flat_map(flatten).filter_map(|v| v.num()).collect();
                let pick = if name == "max" {
                    nums.into_iter().reduce(f64::max)
                } else {
                    nums.into_iter().reduce(f64::min)
                };
                pick.map_or(Val::Null, Val::Num)
            }
            "duration" => match parse_span(&first.display()) {
                Some(Span::Fixed(d)) => Val::Num(d.num_milliseconds() as f64),
                _ => Val::Null,
            },
            _ => bail!("未対応の関数です: {name}()"),
        })
    }
}

/// リストなら中身を、それ以外はそのものを並べる
fn flatten(v: &Val) -> Vec<Val> {
    match v {
        Val::List(l) => l.clone(),
        other => vec![other.clone()],
    }
}

fn binary(op: &str, a: Val, b: Val) -> Val {
    match op {
        "==" => Val::Bool(a.compare(&b) == Some(Ordering::Equal)),
        "!=" => Val::Bool(a.compare(&b) != Some(Ordering::Equal)),
        ">" | "<" | ">=" | "<=" => {
            let ord = a.compare(&b);
            Val::Bool(match op {
                ">" => ord == Some(Ordering::Greater),
                "<" => ord == Some(Ordering::Less),
                ">=" => matches!(ord, Some(Ordering::Greater | Ordering::Equal)),
                _ => matches!(ord, Some(Ordering::Less | Ordering::Equal)),
            })
        }
        "+" | "-" => match (&a, &b) {
            // 日付 ± 期間
            (Val::Date(d, t), Val::Str(s)) => parse_span(s)
                .and_then(|span| shift(*d, &span, if op == "+" { 1 } else { -1 }))
                .map_or(Val::Null, |d| Val::Date(d, *t)),
            (Val::Date(d, t), Val::Num(ms)) => d
                .checked_add_signed(Duration::milliseconds(
                    (*ms * if op == "+" { 1.0 } else { -1.0 }) as i64,
                ))
                .map_or(Val::Null, |d| Val::Date(d, *t)),
            // 日付 - 日付 はミリ秒
            (Val::Date(x, _), Val::Date(y, _)) if op == "-" => {
                Val::Num((*x - *y).num_milliseconds() as f64)
            }
            (Val::Num(x), Val::Num(y)) => Val::Num(if op == "+" { x + y } else { x - y }),
            (Val::List(x), Val::List(y)) if op == "+" => {
                Val::List(x.iter().chain(y).cloned().collect())
            }
            _ if op == "+" && (matches!(a, Val::Str(_)) || matches!(b, Val::Str(_))) => {
                Val::Str(a.display() + &b.display())
            }
            _ => match (a.num(), b.num()) {
                (Some(x), Some(y)) => Val::Num(if op == "+" { x + y } else { x - y }),
                _ => Val::Null,
            },
        },
        _ => match (a.num(), b.num()) {
            (Some(x), Some(y)) => Val::Num(match op {
                "*" => x * y,
                "/" => x / y,
                _ => x % y,
            }),
            _ => Val::Null,
        },
    }
}

/// メソッドの付かない参照（`.length` `.year` など）
fn field(v: &Val, name: &str) -> Result<Val> {
    Ok(match (v, name) {
        (Val::Null, _) => Val::Null,
        (Val::Str(s), "length") => Val::Num(s.chars().count() as f64),
        (Val::List(l), "length") => Val::Num(l.len() as f64),
        (Val::Date(d, _), _) => Val::Num(f64::from(match name {
            "year" => d.year(),
            "month" => d.month() as i32,
            "day" => d.day() as i32,
            "hour" => d.hour() as i32,
            "minute" => d.minute() as i32,
            "second" => d.second() as i32,
            _ => bail!("日付に `.{name}` はありません"),
        })),
        _ => bail!("{}に `.{name}` はありません", v.type_name()),
    })
}

fn method(v: &Val, name: &str, a: &[Val]) -> Result<Val> {
    let arg = |i: usize| a.get(i).cloned().unwrap_or(Val::Null);
    let text = |i: usize| arg(i).display();
    let int = |i: usize, default: i64| arg(i).num().map_or(default, |n| n as i64);
    // どの値にもあるもの
    match name {
        "isEmpty" => {
            return Ok(Val::Bool(
                !v.truthy() && !matches!(v, Val::Num(_) | Val::Bool(_)),
            ))
        }
        "toString" => return Ok(Val::Str(v.display())),
        "isTruthy" => return Ok(Val::Bool(v.truthy())),
        _ => {}
    }
    Ok(match v {
        Val::Null => Val::Null,
        Val::Str(s) => match name {
            "contains" => Val::Bool(s.to_lowercase().contains(&text(0).to_lowercase())),
            "containsAny" => Val::Bool(a.iter().any(|x| s.contains(&x.display()))),
            "containsAll" => Val::Bool(a.iter().all(|x| s.contains(&x.display()))),
            "startsWith" => Val::Bool(s.starts_with(&text(0))),
            "endsWith" => Val::Bool(s.ends_with(&text(0))),
            "lower" => Val::Str(s.to_lowercase()),
            "upper" => Val::Str(s.to_uppercase()),
            "title" => Val::Str(
                s.split(' ')
                    .map(|w| {
                        let mut c = w.chars();
                        c.next()
                            .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            "trim" => Val::Str(s.trim().to_string()),
            "replace" => Val::Str(s.replace(&text(0), &text(1))),
            "repeat" => Val::Str(s.repeat(int(0, 1).max(0) as usize)),
            "slice" => {
                let chars: Vec<char> = s.chars().collect();
                let (from, to) = slice_range(chars.len(), int(0, 0), a.get(1).and_then(Val::num));
                Val::Str(chars[from..to].iter().collect())
            }
            "split" => Val::List(s.split(&text(0)).map(|p| Val::Str(p.to_string())).collect()),
            _ => bail!("文字列に `.{name}()` はありません"),
        },
        Val::Num(n) => match name {
            "toFixed" => Val::Str(format!("{n:.*}", int(0, 0).clamp(0, 20) as usize)),
            "round" => {
                let p = 10f64.powi(int(0, 0).clamp(0, 15) as i32);
                Val::Num((n * p).round() / p)
            }
            "abs" => Val::Num(n.abs()),
            "ceil" => Val::Num(n.ceil()),
            "floor" => Val::Num(n.floor()),
            _ => bail!("数値に `.{name}()` はありません"),
        },
        Val::Bool(_) => bail!("真偽値に `.{name}()` はありません"),
        Val::List(l) => match name {
            "contains" => {
                let want = text(0).to_lowercase();
                Val::Bool(l.iter().any(|x| x.display().to_lowercase() == want))
            }
            "containsAny" => Val::Bool(a.iter().flat_map(flatten).any(|w| l.contains(&w))),
            "containsAll" => Val::Bool(a.iter().flat_map(flatten).all(|w| l.contains(&w))),
            "join" => Val::Str(l.iter().map(Val::display).collect::<Vec<_>>().join(
                &if a.is_empty() {
                    ", ".to_string()
                } else {
                    text(0)
                },
            )),
            "reverse" => Val::List(l.iter().rev().cloned().collect()),
            "sort" => {
                let mut l = l.clone();
                l.sort_by(|x, y| x.compare(y).unwrap_or(Ordering::Equal));
                Val::List(l)
            }
            "unique" => {
                let mut out: Vec<Val> = Vec::new();
                for x in l {
                    if !out.contains(x) {
                        out.push(x.clone());
                    }
                }
                Val::List(out)
            }
            "flat" => Val::List(l.iter().flat_map(flatten).collect()),
            "slice" => {
                let (from, to) = slice_range(l.len(), int(0, 0), a.get(1).and_then(Val::num));
                Val::List(l[from..to].to_vec())
            }
            _ => bail!("リストに `.{name}()` はありません"),
        },
        Val::Date(d, t) => match name {
            "format" => Val::Str(moment_format(
                d,
                &if a.is_empty() {
                    "YYYY-MM-DD".to_string()
                } else {
                    text(0)
                },
            )),
            "date" => Val::Date(d.date().and_hms_opt(0, 0, 0).expect("0時は必ずある"), false),
            "time" => Val::Str(d.format("%H:%M:%S").to_string()),
            "relative" => {
                let days = (Local::now().naive_local() - *d).num_days();
                Val::Str(match days {
                    0 => "今日".to_string(),
                    1 => "昨日".to_string(),
                    -1 => "明日".to_string(),
                    n if n > 0 => format!("{n}日前"),
                    n => format!("{}日後", -n),
                })
            }
            _ => {
                let _ = t;
                bail!("日付に `.{name}()` はありません")
            }
        },
    })
}

/// JavaScript の `slice` と同じく、負の位置は後ろから数える
fn slice_range(len: usize, from: i64, to: Option<f64>) -> (usize, usize) {
    let at = |i: i64| -> usize {
        if i < 0 {
            (len as i64 + i).max(0) as usize
        } else {
            (i as usize).min(len)
        }
    };
    let from = at(from);
    let to = to.map_or(len, |t| at(t as i64));
    (from, to.max(from))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calc(src: &str) -> Val {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("n.md"),
            "---\nprice: 1200\ntags: [a, b]\ndue: 2026-01-31\nname with space: x\n---\n#memo",
        )
        .unwrap();
        let v = Vault::open(dir.path()).unwrap();
        let note = v.note("n.md").unwrap();
        let fm = props::read(&note.text).unwrap_or_default();
        let mut formulas = BTreeMap::new();
        formulas.insert("tax".to_string(), parse("price * 0.1").unwrap());
        formulas.insert("loop".to_string(), parse("formula.loop + 1").unwrap());
        let ctx = Ctx {
            vault: &v,
            note,
            fm: &fm,
            formulas: &formulas,
            depth: 0,
        };
        match parse(src).and_then(|e| ctx.eval(&e)) {
            Ok(v) => v,
            Err(e) => Val::Str(format!("ERR {e}")),
        }
    }

    #[test]
    fn arithmetic_and_logic() {
        assert_eq!(calc("1 + 2 * 3"), Val::Num(7.0));
        assert_eq!(calc("(1 + 2) * 3 % 4"), Val::Num(1.0));
        assert_eq!(calc("-price / 4"), Val::Num(-300.0));
        assert_eq!(calc("price >= 1000 && !false"), Val::Bool(true));
        assert_eq!(calc("1 > 2 || 2 > 3"), Val::Bool(false));
        assert_eq!(calc("\"a\" + 1"), Val::Str("a1".into()));
        assert_eq!(calc("price > 1 ? 'big' : 'small'"), Val::Str("big".into()));
        assert_eq!(calc("0.1 + 0.2").display(), "0.3");
    }

    #[test]
    fn properties_and_formulas() {
        assert_eq!(calc("note.price"), Val::Num(1200.0));
        assert_eq!(calc("note[\"name with space\"]"), Val::Str("x".into()));
        assert_eq!(calc("formula.tax"), Val::Num(120.0));
        assert_eq!(
            calc("(formula.tax + price).toFixed(1)"),
            Val::Str("1320.0".into())
        );
        assert!(calc("formula.loop").display().contains("参照し合って"));
        assert_eq!(calc("missing"), Val::Null);
        assert_eq!(calc("file.name + '.' + file.ext"), Val::Str("n.md".into()));
        assert_eq!(calc("file.hasTag('memo')"), Val::Bool(true));
        assert_eq!(calc("tags.contains('B')"), Val::Bool(true));
        assert_eq!(calc("tags.length"), Val::Num(2.0));
        assert_eq!(calc("tags[1]"), Val::Str("b".into()));
        assert_eq!(calc("tags.join(' / ')"), Val::Str("a / b".into()));
        assert_eq!(calc("['x', 'y'].reverse()[0]"), Val::Str("y".into()));
    }

    #[test]
    fn functions_and_methods() {
        assert_eq!(calc("if(price > 2000, 'high')"), Val::Null);
        assert_eq!(
            calc("if(price > 1000, 'high', 'low')"),
            Val::Str("high".into())
        );
        assert_eq!(calc("max(3, [7, 1], price)"), Val::Num(1200.0));
        assert_eq!(calc("min(3, 7)"), Val::Num(3.0));
        assert_eq!(calc("number('42') + 1"), Val::Num(43.0));
        assert_eq!(calc("'Hello'.lower().startsWith('he')"), Val::Bool(true));
        assert_eq!(calc("'a-b-c'.split('-').length"), Val::Num(3.0));
        assert_eq!(calc("'abcdef'.slice(1, -1)"), Val::Str("bcde".into()));
        assert_eq!(
            calc("'hello world'.title()"),
            Val::Str("Hello World".into())
        );
        assert_eq!(calc("(2.346).round(2)"), Val::Num(2.35));
        assert_eq!(calc("''.isEmpty()"), Val::Bool(true));
        assert!(calc("price.nope()").display().starts_with("ERR"));
        assert!(calc("nope()").display().starts_with("ERR"));
        assert!(calc("1 +").display().starts_with("ERR"));
        assert!(calc("1 ~ 2").display().starts_with("ERR"));
    }

    #[test]
    fn dates() {
        assert_eq!(calc("due.format('YYYY/M/D')"), Val::Str("2026/1/31".into()));
        assert_eq!(calc("due.month"), Val::Num(1.0));
        assert_eq!(calc("(due + '1M').format()"), Val::Str("2026-02-28".into()));
        assert_eq!(
            calc("(due - '2w').format('MM-DD')"),
            Val::Str("01-17".into())
        );
        assert_eq!(calc("due > '2026-01-01'"), Val::Bool(true));
        assert_eq!(
            calc("(date('2026-02-01') - due) / duration('1d')"),
            Val::Num(1.0)
        );
        assert_eq!(calc("due.format('[年]YYYY')"), Val::Str("年2026".into()));
        assert_eq!(calc("file.mtime > now() - '1d'"), Val::Bool(true));
        assert_eq!(
            calc("today() >= due"),
            Val::Bool(Local::now().date_naive() >= NaiveDate::from_ymd_opt(2026, 1, 31).unwrap())
        );
    }
}
